#!/usr/bin/env python3
"""Execute current Android/PWA production methods against native shared fixtures.

Only wire-model and finishApplied transport shims are supplied. No selection or
rollback algorithm is copied into this probe. Kotlin and Node run real methods.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ANDROID = ROOT.parent / "android/app/src/main/java/me/egigoka/pomodorough/data/CentralizedSyncCoordinator.kt"
PWA = ROOT.parent / "server/web/app-actions.js"
BRIDGE = ROOT / "target/debug/examples/completion_policy_probe"


def merge(target, patch):
    for key, value in patch.items():
        if isinstance(value, dict) and isinstance(target.get(key), dict):
            merge(target[key], value)
        else:
            target[key] = copy.deepcopy(value)


def requests():
    fixture = json.loads((ROOT / "fixtures/completion-sent-v1.json").read_text())
    for case in fixture["cases"]:
        request = copy.deepcopy(fixture["base"])
        if case.get("profile") == "pwaRejectedFinish":
            request["compatibility"] = case["profile"]
            request["sentContext"] = {"kind": "pwa", "rollbackHistory":
                                      [copy.deepcopy(fixture["history"])] if case.get("history") else []}
        context = request["sentContext"]
        context["commands"] = [copy.deepcopy(fixture["commands"][key]) for key in case["commands"]]
        for flag, owner, key, value in [
            ("canonical", request, "canonicalTimer", fixture["timer"]),
            ("history", request, "afterHistory", [fixture["history"]]),
            ("ackTimer", context, "acknowledgementTimer", fixture["timer"]),
            ("projected", context, "nextProjectionTimer", fixture["timer"]),
            ("ackHistory", context, "acknowledgementHistory", [fixture["history"]]),
        ]:
            if case.get(flag):
                owner[key] = copy.deepcopy(value)
        merge(request, case["patch"])
        yield case, request


def core(request):
    payload = {"operation": "timer.completionState.v1", "input": request}
    result = subprocess.run([str(BRIDGE)], input=json.dumps(payload) + "\n",
                            text=True, capture_output=True, check=True)
    output = json.loads(result.stdout)
    return "error:" + output["error"] if "error" in output else output["selection"]["phase"]


def literal(value):
    if value is None:
        return "null"
    return json.dumps(value, ensure_ascii=False).replace("$", "\\$")


def model(name, value, fields):
    if value is None:
        return "null"
    return name + "(" + ",".join(f"{key}={literal(value.get(key))}" for key in fields) + ")"


def history(rows):
    fields = "id timerId commandId phase status plannedDurationMs completedAt endedAt taskId".split()
    return "listOf(" + ",".join(model("HistoryItem", row, fields) for row in rows) + ")"


def timer(value):
    if value is None:
        return "null"
    fields = "id phase status plannedDurationMs anchorAt taskId".split()
    base = model("CanonicalTimer", value, fields)[:-1]
    intent = model("Intent", value.get("lastIntent"), ["commandId"])
    return base + ",lastIntent=" + intent + ")"


def kotlin_call(request):
    context = request["sentContext"]
    selected = request["selection"]
    captured = context["selectionAtSend"]
    fields = "id timerId type phase deviceSequence occurredAt physicalOccurredAt".split()
    commands = "listOf(" + ",".join(model("TimerCommand", c, fields) for c in context["commands"]) + ")"
    acks = "listOf(" + ",".join(model("Acknowledgement", a, ["commandId", "outcome"])
                                for a in request["acknowledgements"]) + ")"
    ack_response = f"SyncResponse({timer(context['acknowledgementTimer'])},{history(context['acknowledgementHistory'])},{acks})"
    canonical = f"SyncResponse({timer(request['canonicalTimer'])},{history(request['afterHistory'])},emptyList())"
    args = [f"CentralizedSyncSnapshot({selected['generation']}L)", literal(selected["phase"]),
            literal(captured["phase"] if captured else None),
            captured["generation"] + "L" if captured else "null", commands,
            ack_response, canonical, f"TimerProjection({timer(context['nextProjectionTimer'])})"]
    return "try { println(probe.run(" + ",".join(args) + ")) } catch (error: IllegalStateException) { println(error.message) }"


def android_probe(cases, temporary):
    source = ANDROID.read_text()
    start = source.index("    private fun reconciledSelectedPhase(")
    end = source.index("    private fun timerDependencies(", start)
    methods = source[start:end].replace("private fun reconciledSelectedPhase(", "fun run(", 1)
    models = (ROOT / "scripts/completion_probe_models.kt").read_text()
    wrapper = "\nclass Probe(val completionDispatcher: Dispatcher, val zoneId: ZoneId = ZoneId.of(\"UTC\")) {\n"
    main = "\nfun main(args: Array<String>) { val probe = Probe(Dispatcher(args[0]))\n"
    program = models + wrapper + methods + "}\n" + main
    program += "\n".join(kotlin_call(request) for _, request in cases) + "\n}\n"
    path = temporary / "Probe.kt"
    path.write_text(program)
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    env = dict(os.environ, JAVA_HOME=os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    jar = temporary / "probe.jar"
    subprocess.run([compiler, str(path), "-include-runtime", "-d", str(jar)], env=env, check=True, capture_output=True)
    result = subprocess.run([str(Path(env["JAVA_HOME"]) / "bin/java"), "-jar", str(jar), str(BRIDGE)],
                            text=True, capture_output=True, check=True)
    return result.stdout.splitlines()


def pwa_probe(cases, temporary):
    source = PWA.read_text()
    start = source.index("  class CompletionPlanPolicy {")
    end = source.index("  class ActionMutations {", start)
    methods = source[start:end]
    wrapper = (ROOT / "scripts/completion_probe_pwa.cjs").read_text()
    path = temporary / "probe.cjs"
    path.write_text(methods + wrapper)
    result = subprocess.run(["node", str(path), str(BRIDGE)], env=dict(os.environ, TZ="UTC"),
                            input=json.dumps([request for _, request in cases]),
                            text=True, capture_output=True, check=True)
    return json.loads(result.stdout)


def main():
    all_cases = list(requests())
    mismatches = []
    with tempfile.TemporaryDirectory(prefix="completion-parity-") as directory:
        for profile, probe in [("androidCapturedSend", android_probe), ("pwaRejectedFinish", pwa_probe)]:
            cases = [(case, request) for case, request in all_cases if request["compatibility"] == profile]
            actual = probe(cases, Path(directory))
            assert len(actual) == len(cases)
            for (case, request), phase in zip(cases, actual):
                expected = "error:" + case["error"] if "error" in case else case["phase"]
                assert phase == expected, (case["name"], "production", phase, expected)
                actual_core = core(request)
                if "error" in case or phase != actual_core:
                    print(f"{case['name']}: production={phase} core={actual_core}")
                if phase != actual_core:
                    mismatches.append((case["name"], phase, actual_core))
            print(f"{profile}: {len(cases)} production-source cases executed")
    assert not mismatches, mismatches
    for path in (ANDROID, PWA):
        print(f"{path.relative_to(ROOT.parent)} sha256={hashlib.sha256(path.read_bytes()).hexdigest()}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stdout or "")
        print(error.stderr or "")
        raise
