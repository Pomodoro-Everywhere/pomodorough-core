#!/usr/bin/env python3
"""Compare extracted production mutation decisions with native Core intent plans."""

import ast
from datetime import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT.parent
BRIDGE = ROOT / "target/debug/examples/completion_policy_probe"
FIXTURE = json.loads((ROOT / "fixtures/workspace-intent-v1.json").read_text())["request"]
TIMER = json.loads((ROOT / "fixtures/workspace-intent-v1.json").read_text())["timer"]
MUTATION_CASES = json.loads((ROOT / "fixtures/workspace-intent-mutations-v1.json").read_text())


def core(operation, value):
    result = subprocess.run([str(BRIDGE)], input=json.dumps({"operation": operation, "input": value}) + "\n",
                            capture_output=True, text=True, check=True)
    return json.loads(result.stdout)


def request(profile, intent):
    value = json.loads(json.dumps(FIXTURE))
    value["compatibility"] = profile
    value["intent"] = intent
    value["ownership"] = {"ownerId": "account-a", "expectedOwnerId": "account-a"}
    value["durability"] = {"outgoingDurationOperationIds": [], "localTabId": "tab-a"}
    if profile == "appleWorkspace" and intent["kind"] == "setDuration" or intent["kind"] == "changeDuration":
        value["localDurationsMs"] = dict(value["workspace"]["base"]["durationsMs"])
    return value


def section(path, begin, end):
    source = path.read_text()
    start = source.index(begin)
    return source[start:source.index(end, start)]


def source_function(path, name, scope):
    tree = ast.parse(path.read_text())
    function = next(node for node in ast.walk(tree) if isinstance(node, ast.FunctionDef) and node.name == name)
    function.decorator_list = []
    exec(compile(ast.Module(body=[function], type_ignores=[]), str(path), "exec"), scope)
    return scope[name]


def desktop():
    storage = SUITE / "desktop/src/pomodorough/storage.py"
    source = SUITE / "desktop/src/pomodorough/core.py"
    scope = {"Any": object, "datetime": datetime, "re": re}
    regex = next(node for node in ast.parse(source.read_text()).body if isinstance(node, ast.Assign)
                 and any(isinstance(target, ast.Name) and target.id == "_RFC3339_OFFSET" for target in node.targets))
    exec(compile(ast.Module(body=[regex], type_ignores=[]), str(source), "exec"), scope)
    parser = source_function(source, "parse_timestamp_ms", scope)
    scope["parse_timestamp_ms"] = parser
    elapsed = source_function(source, "elapsed_ms", scope)
    scope["elapsed_ms"] = elapsed
    observed = source_function(storage, "_retarget_observed_ms", scope)
    for status in ("running", "paused"):
        value = request("desktopStorage", {"kind": "selectTask", "taskId": None})
        value["workspace"]["base"]["canonicalTimer"] = {**TIMER, "status": status, "taskId": "legacy-task"}
        planned = core("workspace.intent.v1", value)["commands"][0]
        assert planned["observedElapsedMs"] == observed(TIMER | {"status": status}, 1784548810000, 60000)
    return 2


def apple(tmp):
    source = SUITE / "apple/Sources/SynchronizedWorkspaceMutationController.swift"
    expression = section(source, "        let minutes = min(180, max(1, requestedMinutes))", "\n",)
    program = "for requestedMinutes in [0, 1, 40, 180, 181] {\n" + expression + "\nprint(minutes)\n}\n"
    path = tmp / "duration.swift"
    path.write_text(program)
    values = subprocess.run(["swift", str(path)], capture_output=True, text=True, check=True).stdout.splitlines()
    for minutes, expected in zip((0, 1, 40, 180, 181), values, strict=True):
        value = request("appleWorkspace", {"kind": "setDuration", "phase": "focus", "minutes": minutes})
        actual = core("workspace.intent.v1", value)["projection"]["durationsMs"]["focus"]
        assert actual == int(expected) * 60000
    return len(values)


def android(tmp):
    source = SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data/TimerMutationCoordinator.kt"
    method = section(source, "    private fun changedDurationMs(", "    private fun selectedTaskForStart(")
    method = method.replace("private fun changedDurationMs", "fun changedDurationMs", 1)
    program = "object DurationLimits { const val MinuteMs = 60000L; const val MaxMs = 10800000L }\n"
    program += "class TimerSettings(val duration: Long) { fun durationMsFor(phase: String) = duration }\nclass Probe {\n" + method + "}\n"
    cases = [(60000, 60000, delta) for delta in (-2, 0, 1, 180)]
    cases += [(case["localMs"], case["projectedMs"], case["delta"])
              for case in MUTATION_CASES["androidDurationDivergence"]]
    rows = ", ".join(f"Pair({local}L, {delta})" for local, _, delta in cases)
    program += f'fun main() {{ for ((local, delta) in listOf({rows})) println(Probe().changedDurationMs(TimerSettings(local), "focus", delta)) }}\n'
    path = tmp / "duration.kt"
    path.write_text(program)
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    java = os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home")
    jar = tmp / "duration.jar"
    subprocess.run([compiler, str(path), "-include-runtime", "-d", str(jar)],
                   env=dict(os.environ, JAVA_HOME=java), capture_output=True, check=True)
    output = subprocess.run([str(Path(java) / "bin/java"), "-jar", str(jar)],
                            capture_output=True, text=True, check=True).stdout.splitlines()
    for (local, projected, delta), expected in zip(cases, output, strict=True):
        value = request("androidCoordinator", {"kind": "changeDuration", "phase": "focus", "delta": delta})
        value["localDurationsMs"]["focus"] = local
        value["workspace"]["base"]["durationsMs"]["focus"] = projected
        planned = core("workspace.intent.v1", value)
        operations = planned["operations"]["durationOperations"]
        if expected == "null":
            assert operations == [] and planned["outcome"] == "noop", (local, projected, delta)
        else:
            assert len(operations) == 1 and operations[0]["durationMs"] == int(expected), (local, projected, delta, expected, planned)
    return len(output)


def android_add(tmp):
    source = SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data/TimerRepository.kt"
    method = section(source, "    override suspend fun addTask(title: String): Boolean {",
                     "    override suspend fun deleteTask(taskId: String) {")
    method = method.replace("override suspend fun addTask", "suspend fun addTask", 1)
    identity = core("task.identity.v1", {"title": "Café"})
    program = "import kotlin.coroutines.*\n"
    program += "data class FocusTask(val id: String, val title: String)\n"
    program += "object TaskOperationType { const val Upsert = \"upsert\" }\n"
    program += "class Probe(val known: Boolean, val active: Boolean) {\n"
    program += f'val canonical = FocusTask("{identity["id"]}", "{identity["title"]}")\n'
    program += "val tasks = if (known) listOf(canonical) else emptyList()\nvar route = \"none\"\n"
    program += "suspend fun initialize() {}\nfun taskFromSharedCore(title: String) = canonical\n"
    program += "suspend fun selectTask(taskId: String?) { route = \"select\" }\n"
    program += "suspend fun issueTaskOperation(type: String, task: FocusTask, select: Boolean, identityValidated: Boolean): Boolean { route = \"upsert\"; return !active }\n"
    program += method + "}\n"
    program += "fun <T> runSync(block: suspend () -> T): T { var result: Result<T>? = null; block.startCoroutine(object: Continuation<T> { override val context = EmptyCoroutineContext; override fun resumeWith(value: Result<T>) { result = value } }); return result!!.getOrThrow() }\n"
    program += "fun main() {\n"
    for case in MUTATION_CASES["androidAdd"]:
        known = str(case["existing"]).lower()
        active = str(case["status"] in ("running", "paused")).lower()
        program += f'run {{ val probe = Probe({known}, {active}); val saved = runSync {{ probe.addTask("Cafe\\u0301") }}; println("${{probe.route}}|$saved") }}\n'
    program += "}\n"
    path = tmp / "add-task.kt"
    path.write_text(program)
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    java = os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home")
    jar = tmp / "add-task.jar"
    subprocess.run([compiler, str(path), "-include-runtime", "-d", str(jar)],
                   env=dict(os.environ, JAVA_HOME=java), capture_output=True, text=True, check=True)
    lines = subprocess.run([str(Path(java) / "bin/java"), "-jar", str(jar)],
                           capture_output=True, text=True, check=True).stdout.splitlines()
    for case, line in zip(MUTATION_CASES["androidAdd"], lines, strict=True):
        route, saved = line.split("|")
        expected_route = "select" if case["existing"] else "upsert"
        assert route == expected_route, (case, line)
        value = request("androidCoordinator", {"kind": "addAndSelectTask", "title": case["title"]})
        if case["existing"]:
            value["workspace"]["base"]["tasks"] = [{"id": identity["id"], "title": identity["title"]}]
        if case["selected"]:
            value["workspace"]["base"]["selectedTaskId"] = identity["id"]
        if case["status"] != "idle":
            timer = dict(TIMER, phase=case["phase"], status=case["status"])
            if case["phase"] != "focus":
                timer["plannedDurationMs"] = 120000
            if case["timerTask"]:
                timer["taskId"] = identity["id"]
            value["workspace"]["base"]["canonicalTimer"] = timer
        planned = core("workspace.intent.v1", value)["operations"]
        assert [len(planned[domain]) for domain in ("taskOperations", "selectedTaskOperations", "commands")] == [case[key] for key in ("tasks", "selection", "commands")], case
        if case["existing"]:
            assert saved == "true" and case["tasks"] == 0, (case, line)
        elif case["status"] != "idle":
            assert saved == "false" and case["tasks"] == 0, (case, line)
        else:
            assert saved == "true" and case["tasks"] == 1, (case, line)
    return len(lines)


def pwa(tmp):
    source = SUITE / "server/web/app-storage.js"
    method = section(source, "    buildRetargetCommand(timer, taskId, allocation) {", "    async persistRetargetOperation(")
    program = "const input = JSON.parse(process.argv[2]);\nclass Probe {\n" + method + "}\n"
    program += "const probe = new Probe(); probe.state = {deviceId: 'device-local'};\n"
    program += "probe.syncCore = {retargetRequestFields: (taskId) => ({taskId})};\n"
    program += "probe.use = {trustedNow: () => 1784548810000, elapsedFor: () => 15000};\n"
    program += "console.log(JSON.stringify(probe.buildRetargetCommand(input.timer, input.taskId, input.allocation)));\n"
    path = tmp / "retarget.cjs"
    path.write_text(program)
    value = request("pwaStorage", {"kind": "selectTask", "taskId": None})
    identity = core("task.identity.v1", {"title": "Café"})
    value["workspace"]["base"]["tasks"] = [{"id": identity["id"], "title": identity["title"]}]
    value["workspace"]["base"]["selectedTaskId"] = identity["id"]
    value["workspace"]["base"]["canonicalTimer"] = {**TIMER, "taskId": identity["id"]}
    result = core("workspace.intent.v1", value)["commands"][0]
    supplied = {"timer": TIMER, "taskId": None, "allocation": {"id": result["id"],
                "deviceSequence": result["deviceSequence"], "wallMs": result["hlcWallMs"],
                "counter": result["hlcCounter"]}}
    actual = json.loads(subprocess.run(["node", str(path), json.dumps(supplied)],
                        capture_output=True, text=True, check=True).stdout)
    for field in ("id", "deviceId", "deviceSequence", "timerId", "type", "phase", "plannedDurationMs",
                  "hlcWallMs", "hlcCounter", "observedElapsedMs", "taskId"):
        assert actual[field] == result[field], (field, actual[field], result[field])
    return 1


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory)
        print({"desktop": desktop(), "apple": apple(temporary),
               "androidDuration": android(temporary), "androidAdd": android_add(temporary),
               "pwa": pwa(temporary)})
