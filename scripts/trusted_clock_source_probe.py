#!/usr/bin/env python3
"""Run real clock implementations against the shared raw-observation fixtures.

Swift and Kotlin compile production sources. Python extracts AST declarations;
Node loads sync-core and extracts the production TrustedClock class. Persistence,
wire models, and OS readings are supplied, but no clock algorithm is mocked.
"""
import ast
import copy
from contextlib import nullcontext
from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import math
import os
import re
from pathlib import Path
import subprocess
import tempfile
import types

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT.parent
BRIDGE = ROOT / "target/debug/examples/read_model_probe"
FIXTURE = json.loads((ROOT / "fixtures/clock-observe-v1.json").read_text())
CHECKER = json.loads((ROOT / "fixtures/clock-observe-checker-v1.json").read_text())
ORACLE = json.loads((ROOT / "fixtures/clock-observe-oracle-v1.json").read_text())


class NativeClockRequest(dict):
    """Keep raw source observations beside, never inside, the Core request."""
    def __init__(self, request, native_observations):
        super().__init__(request)
        self.native_observations = copy.deepcopy(native_observations)


def merge(target, patch):
    for key, value in patch.items():
        if isinstance(value, dict) and isinstance(target.get(key), dict):
            merge(target[key], value)
        else:
            target[key] = copy.deepcopy(value)


def request(case):
    profile = case["profile"]
    value = copy.deepcopy(FIXTURE["templates"][profile])
    if "saved" in case:
        saved = copy.deepcopy(FIXTURE[case["saved"]])
        if profile == "appleTrustedClock":
            value["state"] = saved
        else:
            value["state"]["sample" if profile == "desktopTrustedClock" else "clockOffset"] = saved
    if case.get("requestSample"):
        value["state"]["requestSample"] = copy.deepcopy(FIXTURE["androidSample"])
    if case.get("persistedAndroid"):
        value["state"].update(serverClockOffsetMs=100, serverClockUncertaintyMs=1,
            serverClockSamplePhysicalMs=1000000, serverClockSampleElapsedRealtimeMs=20000,
            serverClockBootId="boot-a", retainedWallMs=1000100)
    if case.get("runtimeDesktop"):
        value["state"]["anchor"] = copy.deepcopy(FIXTURE["desktopSample"])
    if case.get("runtimePwa"):
        value["state"]["runtime"] = dict(identity="100:1:1000000", monotonicMs=20.25, wallMs=1000100)
    if case.get("server"):
        value["server"] = copy.deepcopy(FIXTURE["servers"][profile])
    merge(value, case.get("patch", {}))
    if profile == "desktopTrustedClock":
        observations = copy.deepcopy(FIXTURE["nativeObservationTemplates"][profile])
        merge(observations, case.get("nativeObservations", {}))
        return NativeClockRequest(value, observations)
    return value


def core(value):
    raw = value if isinstance(value, str) else json.dumps(value)
    payload = '{"operation":"clock.observe.v1","input":' + raw + '}\n'
    run = subprocess.run([str(BRIDGE)], input=payload,
                         text=True, capture_output=True, check=True)
    return json.loads(run.stdout)


def section(path, start, end):
    source = path.read_text()
    return source[source.index(start):source.index(end, source.index(start))]


def declarations(path, names, namespace):
    tree = ast.parse(path.read_text())
    selected = [node for node in ast.walk(tree) if
        isinstance(node, (ast.FunctionDef, ast.ClassDef)) and node.name in names or
        isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id in names for target in node.targets)]
    module = ast.Module(body=selected, type_ignores=[])
    exec(compile(ast.fix_missing_locations(module), str(path), "exec"), namespace)


def desktop_source():
    namespace = dict(Any=object, MAX_SAFE_INTEGER=9007199254740991, MAX_SERVER_TIME_UNCERTAINTY_MS=30000,
        MAX_CLOCK_CONTINUITY_DRIFT_MS=1000, dataclass=dataclass, datetime=datetime, timezone=timezone, re=re)
    declarations(SUITE / "desktop/src/pomodorough/core.py", ["_RFC3339_OFFSET", "parse_timestamp_ms"], namespace)
    declarations(SUITE / "desktop/src/pomodorough/storage_model.py", ["utc_timestamp"], namespace)
    declarations(SUITE / "desktop/src/pomodorough/storage_canonical_installation.py", ["_response_clock_context"], namespace)
    path = SUITE / "desktop/src/pomodorough/storage.py"
    tree = ast.parse(path.read_text())
    names = {"_bounded_integer", "_physical_time_ms", "_signed_safe_integer", "_server_clock_sample",
        "_projected_trusted_time", "_restore_trusted_time_anchor", "_clock_sample_for_response",
        "_validated_response_timing", "_set_trusted_time_anchor", "_trusted_now_ms", "_physical_timestamp"}
    storage = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "Store")
    storage.body = [node for node in storage.body if isinstance(node, ast.FunctionDef) and node.name in names]
    timing = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "_ResponseTiming")
    exec(compile(ast.fix_missing_locations(ast.Module(body=[timing, storage], type_ignores=[])), str(path), "exec"), namespace)
    return namespace


def desktop(rows, temporary):
    namespace = desktop_source()
    output = []
    for value in rows:
        native = desktop_native_observations(value, namespace)
        state, reading = copy.deepcopy(value["state"]), value["reading"]
        source = namespace["Store"]()
        source._trusted_time_anchor = state.get("anchor")
        source.get_meta = lambda key, default=None: state.get("sample", default)
        source._set_meta = lambda key, sample: state.update(sample=sample)
        source._immediate_transaction = nullcontext
        namespace["time"] = types.SimpleNamespace(time=lambda: native["wallSeconds"],
            monotonic_ns=lambda: native["monotonicNs"])
        now = None
        response_sample, response_time = None, None
        try:
            if value["action"] == "sample":
                s = value["server"]
                sample, anchor, response_time = namespace["_response_clock_context"](
                    types.SimpleNamespace(_dependencies=source), {"serverTimeMs":s["serverTimeMs"]},
                    s.get("requestWallMs"), s.get("responseWallMs"), s.get("requestMonotonicMs"), s.get("responseMonotonicMs"))
                response_sample = sample
                if sample is not None: state["sample"] = sample
                if anchor is not None: source._set_trusted_time_anchor(anchor)
            elif value["action"] == "restore":
                source._restore_trusted_time_anchor()
            else:
                mode = state.get("mode", "monotonic")
                now = source._trusted_now_ms(use_server_clock=mode != "local", use_monotonic=mode == "monotonic")
            state.update(anchor=source._trusted_time_anchor, mode=state.get("mode", "monotonic"))
            try:
                sample = source._server_clock_sample(state["sample"])
            except ValueError:
                if state["mode"] != "local" or "trustedAnchorMs" in value:
                    raise
                sample = None
            physical = None
            if "trustedAnchorMs" in value:
                physical = namespace["parse_timestamp_ms"](source._physical_timestamp(native["trustedAnchorAt"]))
            output.append(dict(state=state, trustedNowMs=now, sample=response_sample, trustedResponseMs=response_time,
                physicalDeltaMs=-sample["offsetMs"] if sample else None, physicalAnchorMs=physical))
        except (ValueError, OverflowError):
            output.append(dict(error=True))
    return output


def desktop_native_observations(value, namespace):
    raw = getattr(value, "native_observations", None)
    assert isinstance(raw, dict), "Desktop source probe requires raw native observations"
    assert "wallSeconds" in raw and "monotonicNs" in raw, "Incomplete native observation fixture"
    wall = float(raw["wallSeconds"])
    monotonic = raw["monotonicNs"]
    assert math.isfinite(wall), "Non-finite native wall observation"
    assert isinstance(monotonic, int) and not isinstance(monotonic, bool), "Native monotonic_ns must be integral"
    decoded_wall = namespace["Store"]._physical_time_ms(int(wall * 1000))
    decoded_monotonic = namespace["Store"]._bounded_integer(monotonic // 1_000_000, "Monotonic clock")
    assert decoded_wall == value["reading"]["wallMs"], ("native wall decode mismatch", raw["wallSeconds"], decoded_wall, value["reading"]["wallMs"])
    assert decoded_monotonic == value["reading"]["monotonicMs"], ("native monotonic decode mismatch", monotonic, decoded_monotonic, value["reading"]["monotonicMs"])
    if "trustedAnchorMs" in value:
        assert isinstance(raw.get("trustedAnchorAt"), str), "Missing raw native trusted anchor"
        decoded_anchor = namespace["parse_timestamp_ms"](raw["trustedAnchorAt"])
        assert decoded_anchor == value["trustedAnchorMs"], ("native anchor decode mismatch", raw["trustedAnchorAt"], decoded_anchor, value["trustedAnchorMs"])
    return {**raw, "wallSeconds": wall, "monotonicNs": monotonic}


def fixture_row(case):
    value = json.loads(case["request"])
    if value["compatibility"] == "desktopTrustedClock":
        return NativeClockRequest(value, case["nativeObservations"])
    return value


def apple(rows, temporary):
    bounds = section(SUITE / "apple/Sources/WirePrimitives.swift", "enum WireBounds {", "enum UUIDv7 {")
    source = (SUITE / "apple/Sources/TrustedClockState.swift").read_text()
    runner = (ROOT / "scripts/trusted_clock_probe.swift").read_text()
    path = temporary / "clock.swift"
    guard = section(SUITE / "apple/Sources/PersistedTimerState.swift", "requestUptime.isFinite,",
        "        return (serverTimeMs, requestWallMs)")
    validation = "\nfunc validateSampleUptime(_ requestUptime:Double, _ responseUptime:Double) throws {\n guard " + guard + "}\n"
    path.write_text("import Foundation\n" + bounds + source + runner + validation)
    executable = temporary / "apple-clock"
    subprocess.run(["swiftc", str(path), "-o", str(executable)], capture_output=True, text=True, check=True)
    run = subprocess.run([str(executable)], input="\n".join(map(json.dumps, rows)) + "\n",
                         text=True, capture_output=True, check=True)
    return list(map(json.loads, run.stdout.splitlines()))


def pwa(rows, temporary):
    source = section(SUITE / "server/web/app-state.js", "  class TrustedClock {", "  class SharedTaskCore {")
    path = temporary / "pwa.cjs"
    path.write_text(source + (ROOT / "scripts/trusted_clock_probe.cjs").read_text())
    run = subprocess.run(["node", str(path), str(SUITE / "server/web/sync-core.js"), str(SUITE / "server/web/app-state.js")],
        input=json.dumps(rows), text=True, capture_output=True, check=True)
    return json.loads(run.stdout)


def kotlin_literal(value):
    if value is None:
        return "null"
    if isinstance(value, str):
        return json.dumps(value).replace("$", "\\$")
    return str(value) + "L"


def kotlin_model(name, value, fields):
    if value is None:
        return "null"
    return name + "(" + ",".join(f"{key}={kotlin_literal(value.get(key))}" for key in fields) + ")"


def android(rows, temporary):
    path = SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data/time/TrustedClock.kt"
    source = path.read_text()
    source = "\n".join(line for line in source.splitlines() if not line.startswith("package ") and not line.startswith("import me."))
    bounds = section(SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data/SyncWireBounds.kt",
        "    fun requirePhysicalSkew(", "    fun isClockTuple(")
    source += "\nobject SyncWireBounds { const val MaxSafeInteger=9007199254740991L; const val MaxClockSkewMs=300000L\n" + bounds + "}\n"
    validation = section(SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data/TimerSyncValidation.kt",
        "    private fun validateStoredClockSample(", "    private fun <T> validateQueued(")
    source += "\nobject StoredClockValidation { const val MaxServerClockUncertaintyMs=30000L\n" + validation + \
        "\nfun validate(local:LocalStateEntity) { validateStoredClockSample(local) }\n}\n"
    mapper = section(SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data/TimerRepository.kt",
        "    private fun translatePhysicalInstant(", "    private fun captureBootstrapResolutionAttempt(")
    source = "import java.util.concurrent.CancellationException\n" + source + \
        "\nobject PhysicalMapper {\n" + mapper.replace("private fun translatePhysicalInstant", "fun translatePhysicalInstant", 1) + "}\n"
    models = (ROOT / "scripts/trusted_clock_probe.kt").read_text()
    local_fields = "serverClockOffsetMs serverClockUncertaintyMs serverClockSamplePhysicalMs serverClockSampleElapsedRealtimeMs serverClockBootId".split()
    sample_fields = "offsetMs uncertaintyMs serverTimeMs midpointPhysicalMs midpointElapsedRealtimeMs".split()
    calls = []
    for row in rows:
        state, reading = row["state"], row["reading"]
        local = kotlin_model("LocalStateEntity", state, local_fields)[:-1] + ",hlcWallMs=" + kotlin_literal(state.get("retainedWallMs", 0)) + ")"
        sample = kotlin_model("ServerClockSample", state.get("requestSample"), sample_fields)
        anchor = kotlin_model("AnchorInput", state.get("anchor"), ["serverTimeMs", "elapsedRealtimeMs"])
        server = row.get("server")
        response = "null" if server is None else "SyncResponse(" + json.dumps(datetime.fromtimestamp(server["serverTimeMs"] / 1000, timezone.utc).isoformat()) + "," + kotlin_literal(server["serverHlcWallMs"]) + ")"
        timing = "null" if server is None else "longArrayOf(" + ",".join(kotlin_literal(server[key]) for key in
            ["requestWallMs", "requestMonotonicMs", "responseWallMs", "responseMonotonicMs"]) + ")"
        args = [json.dumps(row["action"]), local, sample, anchor, kotlin_literal(reading["wallMs"]),
            kotlin_literal(reading["monotonicMs"]), kotlin_literal(reading.get("bootId")), response, timing,
            kotlin_literal(row.get("trustedAnchorMs"))]
        calls.append("println(runProbe(" + ",".join(args) + "))")
    program = temporary / "clock.kt"
    program.write_text(source + models + "\nfun main() {\n" + "\n".join(calls) + "\n}\n")
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    env = dict(os.environ, JAVA_HOME=os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    jar = temporary / "clock.jar"
    compiled = subprocess.run([compiler, str(program), "-include-runtime", "-d", str(jar)], env=env, capture_output=True, text=True)
    assert compiled.returncode == 0, compiled.stderr
    run = subprocess.run([str(Path(env["JAVA_HOME"]) / "bin/java"), "-jar", str(jar)], capture_output=True, text=True, check=True)
    return list(map(json.loads, run.stdout.splitlines()))


def main():
    counts = {}
    with tempfile.TemporaryDirectory(prefix="trusted-clock-") as directory:
        for profile, probe in [("appleTrustedClock", apple), ("androidTrustedClock", android),
                               ("desktopTrustedClock", desktop), ("pwaTrustedClock", pwa)]:
            cases = [case for case in FIXTURE["cases"] if case["profile"] == profile and not case.get("coreOnly")]
            rows = [request(case) for case in cases]
            checker = [case for case in CHECKER["cases"] if not case.get("coreOnly")
                and json.loads(case["request"])["compatibility"] == profile]
            if profile == "desktopTrustedClock":
                checker += ORACLE["cases"]
            cases += checker
            rows += [fixture_row(case) for case in checker]
            actual = probe(rows, Path(directory))
            assert len(actual) == len(rows)
            for case, row, observed in zip(cases, rows, actual):
                result = core(case.get("request", row))
                if case.get("reject"):
                    assert "error" in result and "error" in observed, (case["name"], observed, result)
                else:
                    result.pop("schemaVersion")
                    result.pop("compatibility")
                    assert observed == result, (case["name"], observed, result)
            counts[profile] = dict(executed=len(rows), rejected=sum(bool(c.get("reject")) for c in cases))
            counts[profile]["chained"] = chained(probe, profile, Path(directory))
    api_rejections = 0
    for case in FIXTURE["cases"]:
        if case.get("coreOnly"):
            assert "error" in core(request(case)), case["name"]
            api_rejections += 1
    for case in CHECKER["cases"]:
        if case.get("coreOnly"):
            assert "error" in core(case["request"]), case["name"]
            api_rejections += 1
    print(json.dumps(dict(passed=counts, apiBoundaryRejections=api_rejections)))
    for relative in ["apple/Sources/TrustedClockState.swift", "apple/Sources/WirePrimitives.swift",
        "apple/Sources/PersistedTimerState.swift", "android/app/src/main/java/me/egigoka/pomodorough/data/time/TrustedClock.kt",
        "desktop/src/pomodorough/storage.py", "server/web/app-state.js", "server/web/sync-core.js"]:
        print(relative, hashlib.sha256((SUITE / relative).read_bytes()).hexdigest())


def chained(probe, profile, temporary):
    native_input = copy.deepcopy(FIXTURE["templates"][profile])
    native_input.update(action="sample", server=copy.deepcopy(FIXTURE["servers"][profile]))
    if profile == "desktopTrustedClock":
        native_input = NativeClockRequest(native_input, FIXTURE["nativeObservationTemplates"][profile])
    core_input = copy.deepcopy(native_input)
    for step in range(4):
        actual = probe([native_input], temporary)[0]
        result = core(core_input)
        result.pop("schemaVersion")
        result.pop("compatibility")
        assert actual == result, (profile, step, actual, result)
        native_input["state"] = actual["state"]
        core_input["state"] = result["state"]
        if step == 0 and profile == "androidTrustedClock":
            native_input["state"]["requestSample"] = actual["sample"]
            core_input["state"]["requestSample"] = result["sample"]
        for value in (native_input, core_input):
            value.pop("server", None)
            value["action"] = "advance" if step == 2 and profile == "appleTrustedClock" else "current"
            if profile == "appleTrustedClock":
                value["reading"].update(wallSeconds=1000.004 if step == 0 else 999, uptimeSeconds=20.004 + step * 0.001)
            else:
                value["reading"].update(wallMs=1000004 if step == 0 else 999000, monotonicMs=20004 + step)
        if profile == "desktopTrustedClock":
            native_input.native_observations = [
                {"wallSeconds":"1000.004","monotonicNs":20004000000},
                {"wallSeconds":"999.0","monotonicNs":20005000000},
                {"wallSeconds":"999.0","monotonicNs":20006000000},
                {"wallSeconds":"999.0","monotonicNs":20007000000},
            ][step]
    return 4


if __name__ == "__main__":
    main()
