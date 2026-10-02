#!/usr/bin/env python3
"""Compare genuine production projections and classifiers with raw Core requests.

Android compiles the full production factory, dispatcher, and wire models.
Desktop executes production projection methods and its actual typed adapter.
PWA executes projectOwnerState, projectState, and bootstrap caller methods.
Legacy admission vectors that production projection rejects remain explicit.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

from bootstrap_workspace_fixtures import cases, horizon_cases, runtime_cases, pwa_delivery_cases, expected_legacy_rejections
from bootstrap_workspace_android_probe import android as android_runtime
from bootstrap_workspace_desktop_probe import desktop as desktop_runtime

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT.parent
BRIDGE = ROOT / "target/debug/examples/bootstrap_workspace_probe"
ANDROID = SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data"
PROFILES = ("appleWorkspace", "androidRepository", "desktopStorage", "pwaStorage")


def dispatch(operation, value, calls=None):
    raw = json.dumps(value)
    result = subprocess.run([str(BRIDGE)], input='{"operation":' + json.dumps(operation) + ',"input":' + raw + '}\n',
                            capture_output=True, text=True, check=True)
    envelope = json.loads(result.stdout)
    assert json.loads(envelope["inputRaw"]) == envelope["inputDecoded"] == json.loads(raw), "Native request decode equality"
    if "error" in envelope: raise ValueError(envelope["error"])
    assert json.loads(envelope["raw"]) == envelope["decoded"], "Native response decode equality"
    if calls is not None: calls.append(envelope)
    return envelope["decoded"]


def section(path, start, end):
    source = path.read_text()
    begin = source.index(start)
    return source[begin:source.index(end, begin)]


def apple(rows, temporary):
    program = (ROOT / "scripts/bootstrap_workspace_apple.swift").read_text()
    models = SUITE / "apple/Sources/TimerDomainModels.swift"
    phases = "enum TimerPhase { case focus, shortBreak, longBreak\n" + section(models, "    var defaultMinutes:", "struct DurationValues:")
    durations = section(models, "struct DurationValues:", "struct TimerSettings:")
    program = program.replace("// INSERT_DURATION_MODELS", phases + durations)
    local = section(SUITE / "apple/Sources/AppModel.swift", "    private var hasLocalBootstrapState:", "    private func submitBootstrapResolution(")
    remote = section(SUITE / "apple/Sources/AccountSynchronization.swift", "    private static func hasRemoteBootstrapState(", "    private func loadCore(")
    program = program.replace("// INSERT_LOCAL_METHOD", local).replace("// INSERT_REMOTE_METHOD", remote)
    source = temporary / "probe.swift"
    source.write_text(program)
    result = subprocess.run(["swift", str(source)], input="".join(json.dumps(value) + "\n" for _, value in rows),
                            text=True, capture_output=True, check=True)
    return [json.loads(line) for line in result.stdout.splitlines()]


def android(rows, temporary): return android_runtime(rows, temporary, BRIDGE)
def desktop(rows, temporary): return desktop_runtime(rows, temporary, dispatch)


def pwa(rows, temporary):
    result = subprocess.run(["node", str(ROOT / "scripts/bootstrap_workspace_pwa.cjs"),
        str(SUITE / "server/web/sync-core.js"), str(SUITE / "server/web/sync-storage.js"), str(BRIDGE)],
        input=json.dumps([value for _, value in rows]), text=True, capture_output=True, check=True)
    return json.loads(result.stdout)


def assert_result(profile, name, value, observed, calls):
    native = dispatch("bootstrap.workspacePlan.v1", value, calls)
    classification = native["classification"]
    assert observed["hasLocalState"] == classification["local"]["hasState"], "local presence"
    assert observed["hasRemoteState"] == classification["remote"]["hasState"], "remote presence"
    history = observed.get("localHistory", value["local"]["workspace"]["base"]["history"])
    plan_input = dict(localOwnerId=value["local"]["ownerId"], currentUserId=value["currentUserId"],
        localHistory=history, remoteHistory=value["remote"]["history"],
        hasLocalState=observed["hasLocalState"], hasRemoteState=observed["hasRemoteState"])
    actual_plan = dispatch("bootstrap.plan.v1", plan_input)
    assert native["plan"] == actual_plan, f"Core plan={native['plan']}; production plan={actual_plan}"
    for side in ("local", "remote"):
        if side + "DisplayHistoryCount" in observed:
            assert classification[side]["displayHistoryCount"] == observed[side + "DisplayHistoryCount"], f"{side} display count"
    if "plan" in observed and not (profile == "desktopStorage" and value["local"]["ownerId"]):
        assert observed["plan"] == native["plan"], "complete production plan"
    if "emittedInput" in observed:
        assert observed["plan"] == dispatch("bootstrap.plan.v1", observed["emittedInput"])
    if "completeReturn" in observed:
        assert observed["completeReturn"] == dict(expectedRevision=0, strategy=observed["strategy"],
            localHistory=any(row.get("status") == "completed" for row in history),
            remoteHistory=any(row.get("status") == "completed" for row in value["remote"]["history"]))
        assert observed["strategy"] == (None if observed["plan"]["mode"] == "choose" else observed["plan"]["strategy"])
    for prefix in ("projection", "observation"):
        if prefix + "Raw" in observed:
            assert json.loads(observed[prefix + "Raw"]) == observed[prefix + "Decoded"], prefix
    assert_projection_receipt(profile, value, observed)


def assert_projection_receipt(profile, value, observed):
    if "projectionCall" in observed:
        call = observed["projectionCall"]
        assert json.loads(call["inputRaw"]) == call["inputDecoded"]
        assert json.loads(call["raw"]) == call["decoded"]
        if profile == "androidRepository":
            native_projection = copy.deepcopy(observed["projectionDecoded"])
            for row in native_projection["history"]:
                assert row.pop("pending") is False, "unexpected native history marker"
            assert nullable_meaning(native_projection) == nullable_meaning(call["decoded"]), "complete typed Android projection decode"
        elif profile == "desktopStorage":
            decoded = observed["projectionDecoded"]
            names = dict(canonical_timer="canonicalTimer", durations_ms="durationsMs",
                         auto_start_breaks="autoStartBreaks", selected_task_id="selectedTaskId",
                         timer_outcomes="timerOutcomes", winning_operation_ids="winningOperationIds")
            complete = {names.get(key, key): field for key, field in decoded.items()}
            winners = complete["winningOperationIds"]
            complete["winningOperationIds"] = {dict(auto_start="autoStart", selected_task="selectedTask").get(key, key): field
                                               for key, field in winners.items()}
            assert complete == call["decoded"], "complete typed Desktop projection decode"
        else:
            state = observed["projectionDecoded"]
            for key in ("history", "tasks", "durationsMs", "autoStartBreaks", "selectedTaskId"):
                assert state[key] == call["decoded"][key], "complete PWA installed projection"
            timer = call["decoded"]["canonicalTimer"]
            if timer is None: assert state["timer"]["status"] == "idle" and state["timer"]["id"] is None
            else: assert all(state["timer"][key] == field for key, field in timer.items()), "PWA installed timer"
        assert_observation(profile, value, observed)


def nullable_meaning(value):
    if isinstance(value, dict): return {key: nullable_meaning(field) for key, field in value.items() if field is not None}
    if isinstance(value, list): return [nullable_meaning(field) for field in value]
    return value


def assert_observation(profile, value, observed):
    raw = observed["observationDecoded"]
    workspace = value["local"]["workspace"]
    base = raw["base"] if profile == "androidRepository" else raw.get("snapshot")
    if profile != "pwaStorage":
        assert base == workspace["base"], "raw canonical observation differs"
        preferences = raw["preferences"] if profile == "androidRepository" else raw["settings"]
        assert preferences == value["local"]["preferences"], "raw preferences observation differs"
    else:
        assert raw["baseTimer"] == workspace["base"]["canonicalTimer"]
        assert raw["baseHistory"] == workspace["base"]["history"]
        assert raw["baseTasks"] == workspace["base"]["tasks"]
        assert raw["deliveryProof"] == workspace["neverSent"]
        assert raw["canonicalHead"] == workspace["canonicalHead"]
    names = ["pending", "pendingTaskOperations", "pendingDurationOperations", "pendingAutoStartOperations", "pendingSelectedTaskOperations"]
    if profile == "desktopStorage": names = ["pending", "pendingTasks", "pendingDurations", "pendingAutoStarts", "pendingSelectedTasks"]
    for name, queue in zip(names,
            ("commands", "taskOperations", "durationOperations", "autoStartOperations", "selectedTaskOperations")):
        actual = raw["queues"][queue] if profile == "androidRepository" else raw[name]
        assert actual == workspace["local"][queue], "raw retained queue differs"


def run_profile(profile, probe, vectors, temporary):
    rows = [(name, copy.deepcopy(value)) for name, value in vectors if
            name != "android_minute_fallback" or profile == "androidRepository"]
    if profile != "appleWorkspace": rows.extend(list(horizon_cases()) + list(runtime_cases()))
    if profile == "pwaStorage": rows.extend(list(pwa_delivery_cases()))
    for _, value in rows: value["profile"] = profile
    observed = probe(rows, temporary)
    assert len(observed) == len(rows), (profile, len(observed), len(rows))
    passed, legacy_rejections, failures, receipts = [], [], [], []
    expected_rejections = expected_legacy_rejections(profile)
    for (name, value), result in zip(rows, observed):
        if "productionError" in result:
            assert name in expected_rejections, (profile, name, result)
            legacy_rejections.append(dict(name=name, error=result["productionError"]))
            # Keep the legacy endpoint admission assertion, without claiming projection parity.
            dispatch("bootstrap.workspacePlan.v1", value)
            continue
        assert name not in expected_rejections, (profile, name, "legacy projection unexpectedly accepted")
        calls = []
        try:
            assert_result(profile, name, value, result, calls)
            passed.append(name)
        except (AssertionError, ValueError) as error:
            failures.append(dict(profile=profile, name=name, error=str(error)))
        receipts.append(dict(name=name, core=calls[0] if calls else None, production=result))
    assert {row["name"] for row in legacy_rejections} == expected_rejections
    return dict(passed=passed, productionRejectedLegacy=legacy_rejections, failures=failures, receipts=receipts)


def main():
    global BRIDGE
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native", type=Path, default=BRIDGE)
    parser.add_argument("--profile", choices=PROFILES)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    BRIDGE = args.native.resolve()
    vectors = list(cases())
    reports = {}
    with tempfile.TemporaryDirectory(prefix="bootstrap-source-") as directory:
        for profile, probe in zip(PROFILES, (apple, android, desktop, pwa)):
            if args.profile and profile != args.profile: continue
            reports[profile] = run_profile(profile, probe, vectors, Path(directory))
    if args.report: args.report.write_text(json.dumps(reports, indent=2) + "\n")
    failures = [case for report in reports.values() for case in report["failures"]]
    print(json.dumps({profile: {key: len(rows) for key, rows in report.items()} for profile, report in reports.items()}, sort_keys=True))
    assert not failures, failures
    for path in source_paths():
        print(f"{path.relative_to(SUITE)} sha256={hashlib.sha256(path.read_bytes()).hexdigest()}")


def source_paths():
    return [SUITE / ("apple/Sources/" + name) for name in ("AppModel.swift", "AccountSynchronization.swift", "TimerDomainModels.swift")] + \
        [ANDROID / name for name in ("TimerRepository.kt", "Models.kt", "SynchronizedProjectionRequest.kt", "CoreProjectionDispatcher.kt", "SyncWireBounds.kt", "TimerSyncConstruction.kt")] + \
        [SUITE / ("desktop/src/pomodorough/" + name) for name in ("storage.py", "storage_sync.py", "shared_core.py", "core.py")] + \
        [SUITE / ("server/web/" + name) for name in ("sync-core.js", "sync-storage.js", "app-state.js", "app-bootstrap.js")]


if __name__ == "__main__":
    try: main()
    except subprocess.CalledProcessError as error:
        print(error.stdout or "")
        print(error.stderr or "")
        raise
