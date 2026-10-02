#!/usr/bin/env python3
"""Compare bounded finish decisions with extracted client production methods.

This exercises command builders on Desktop, Android, and PWA, and Apple's
provisional phase-advance method. It does not execute platform transactions.
"""
import ast
import copy
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import types

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT.parent
FIXTURE = json.loads((ROOT / "fixtures/workspace-intent-v1.json").read_text())
CASES = json.loads((ROOT / "fixtures/completion-mutation-v1.json").read_text())
BRIDGE = Path(os.environ.get("COMPLETION_PROBE_BIN", ROOT / "target/debug/examples/completion_policy_probe"))
SOURCES = {
    "apple": SUITE / "apple/Sources/TimerSessionController.swift",
    "apple_timer": SUITE / "apple/Sources/TimerOperationModels.swift",
    "android": SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data/TimerMutationCoordinator.kt",
    "android_repository": SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data/TimerRepository.kt",
    "presentation": SUITE / "android/app/src/main/java/me/egigoka/pomodorough/domain/TimerPresentation.kt",
    "desktop": SUITE / "desktop/src/pomodorough/storage.py",
    "desktop_core": SUITE / "desktop/src/pomodorough/core.py",
    "desktop_model": SUITE / "desktop/src/pomodorough/storage_model.py",
    "pwa": SUITE / "server/web/sync-storage.js",
    "pwa_actions": SUITE / "server/web/app-actions.js",
}


def request(profile, status):
    value = copy.deepcopy(FIXTURE["request"])
    value.pop("intent")
    value.update(stage="finishCommit", compatibility=profile, requestedTimer=copy.deepcopy(FIXTURE["timer"]), ownership=None)
    value["identities"].update(commandUuids=value["identities"]["commandUuids"][:1], timerUuid=None)
    value["selection"]["explicit"] = False
    value["requestedTimer"]["status"] = status
    value["workspace"]["base"]["canonicalTimer"] = copy.deepcopy(value["requestedTimer"])
    return value


def automatic(case):
    value = request(case["profile"], "running")
    value["stage"] = "automaticFinishCommit"
    value["clock"].update(occurredAt="2026-07-20T12:01:00Z",
                          physicalNow="2026-07-20T12:01:00Z", observedAt="2026-07-20T12:01:00Z")
    value["identities"]["commandUuids"] = ["019f7f66-a060-7000-8000-000000000001"]
    owner = case["ownerDeviceId"]
    value["ownership"] = {"timerId": "existing-timer", "deviceId": owner} if owner else None
    if case["profile"] == "pwaStorage":
        value["localTabId"] = "tab-local"
        value["leaseNowMs"] = case["leaseNowMs"]
        if owner:
            value["ownership"]["tabId"] = case["ownerTabId"]
            if case["leaseExpiresAtMs"] is None:
                value["ownership"].pop("leaseExpiresAtMs", None)
            else:
                value["ownership"]["leaseExpiresAtMs"] = case["leaseExpiresAtMs"]
    return value


def owner_cases(profile):
    return [case for case in CASES["automaticCases"] if case["profile"] == profile]


def snapshot_request(case):
    value = request(case["profile"], "running")
    for change in case["changes"]:
        target = value
        *parents, key = change["path"].split(".")
        for parent in parents:
            target = target[parent]
        target[key] = change["value"]
    return value


def active_cases(profile):
    for status in CASES["states"]:
        for at in ("2026-07-20T12:00:10Z", "2026-07-20T12:00:59.999Z",
                   "2026-07-20T12:01:00Z", "2026-07-20T12:01:00.001Z"):
            value = request(profile, status)
            value["clock"].update(occurredAt=at, physicalNow=at, observedAt=at)
            ms = int(datetime.fromisoformat(at.replace("Z", "+00:00")).timestamp() * 1000)
            stamp = f"{ms:012x}"
            value["identities"]["commandUuids"] = [f"{stamp[:8]}-{stamp[8:]}-7000-8000-000000000001"]
            yield status, value


def core(value):
    return dispatch("workspace.completionMutation.v1", value)


def dispatch(operation, value):
    payload = json.dumps({"operation": operation, "input": value}) + "\n"
    output = subprocess.run([str(BRIDGE)], input=payload, text=True, capture_output=True, check=True)
    result = json.loads(output.stdout)
    assert "error" not in result, result
    return result


def between(path, start, end):
    content = path.read_text()
    begin = content.index(start)
    return content[begin:content.index(end, begin)]


def method(path, name, namespace):
    tree = ast.parse(path.read_text())
    node = next(node for node in ast.walk(tree) if isinstance(node, ast.FunctionDef) and node.name == name)
    exec(compile(ast.Module(body=[node], type_ignores=[]), str(path), "exec"), namespace)
    return namespace[name]


def desktop():
    namespace = {"Any": object, "datetime": datetime, "timezone": timezone,
                 "re": re, "COMMAND_TYPES": {"finish"}, "PHASES": {"focus", "short_break", "long_break"}}
    tree = ast.parse(SOURCES["desktop_core"].read_text())
    regex = next(node for node in tree.body if isinstance(node, ast.Assign)
                 and any(isinstance(target, ast.Name) and target.id == "_RFC3339_OFFSET" for target in node.targets))
    exec(compile(ast.Module(body=[regex], type_ignores=[]), str(SOURCES["desktop_core"]), "exec"), namespace)
    namespace["parse_timestamp_ms"] = method(SOURCES["desktop_core"], "parse_timestamp_ms", namespace)
    namespace["elapsed_ms"] = method(SOURCES["desktop_core"], "elapsed_ms", namespace)
    namespace["utc_timestamp"] = method(SOURCES["desktop_model"], "utc_timestamp", namespace)
    prepare = method(SOURCES["desktop"], "_prepare_timer_command", namespace)
    fingerprint = method(SOURCES["desktop"], "_timer_fingerprint", namespace)
    owner = types.SimpleNamespace(connection=types.SimpleNamespace(execute=lambda *_: []))
    count = 0
    for status, value in active_cases("desktopStorage"):
        result = core(value)
        command = result["commands"][0]
        timer_now = int(datetime.fromisoformat(value["clock"]["physicalNow"].replace("Z", "+00:00")).timestamp() * 1000)
        source, dependency = prepare(owner, "finish", value["requestedTimer"], "focus", {}, None,
            timer_now, timer_now, command["deviceSequence"],
            (command["hlcWallMs"], command["hlcCounter"]), command["id"], None)
        assert dependency is None
        assert source == result["durableCommands"][0], (status, source, result["durableCommands"][0])
        assert result["completionRecords"]["phaseAdvance"]["selectedPhaseVersion"] == value["selection"]["generation"]
        count += 1
    for name in ("desktop-task-changed", "desktop-retarget-stale",
                 "no-task-same-snapshot", "retarget-current-snapshot"):
        case = next(case for case in CASES["snapshotCases"] if case["name"] == name)
        value = snapshot_request(case)
        source_match = fingerprint(value["requestedTimer"]) == fingerprint(value["workspace"]["base"]["canonicalTimer"])
        planned = core(value)["outcome"] == "planned"
        assert source_match == planned == (case["outcome"] == "planned"), name
        count += 1
    return count


def pwa():
    builder = between(SOURCES["pwa"], "  function finishTimerCommand(", "  function generatedBreakCommand(")
    program = builder + "\nconst cases = JSON.parse(require('fs').readFileSync(0, 'utf8'));\n"
    program += "process.stdout.write(JSON.stringify(cases.map(({timer,input,id,position}) => finishTimerCommand(timer,input,id,position))));\n"
    cases = []
    expected = []
    for status, value in active_cases("pwaStorage"):
        result = core(value)
        command = result["commands"][0]
        cases.append({"timer": value["requestedTimer"], "input": {"deviceId": value["allocation"]["deviceId"],
            "observedElapsedMs": command["observedElapsedMs"]}, "id": command["id"],
            "position": {"highestSequence": command["deviceSequence"] - 1,
                         "occurredAt": command["occurredAt"], "wallMs": command["hlcWallMs"],
                         "firstCounter": command["hlcCounter"]}})
        expected.append(command)
        assert result["ownershipWrites"] == [{"kind": "removeTimerOwner"}]
    output = subprocess.run(["node", "-e", program], input=json.dumps(cases), text=True,
                            capture_output=True, check=True)
    assert json.loads(output.stdout) == expected
    return len(cases)


def pwa_owner():
    source = SOURCES["pwa"]
    program = 'const TIMER_OWNER_KEY = "timerOwner";\n'
    program += between(source, "  function timerOwnerValue(", "  function retainedCommands(")
    program += between(source, "  function finishTimerOwnership(", "  function completionOwnership(")
    program += "\nconst cases = JSON.parse(require('fs').readFileSync(0, 'utf8'));\n"
    program += "process.stdout.write(JSON.stringify(cases.map(({owner,input,timer,commands}) => {\n"
    program += " const writes = []; const metaStore = {put: row => writes.push(row)};\n"
    program += " const results = {timerOwner: {value: owner}, snapshot: {value: {canonicalTimer: timer}}};\n"
    program += " const decision = finishTimerOwnership(metaStore,results,input,commands,timer);\n"
    program += " return {owned: decision.ownerGranted, reason: decision.denied?.reason ?? '',\n"
    program += "   ...(Object.hasOwn(decision.denied ?? {},'retryAtMs') ? {retryAtMs: decision.denied.retryAtMs} : {})};\n})));\n"
    cases = []
    for case in owner_cases("pwaStorage"):
        request_value = automatic(case)
        result = core(request_value)
        assert (result["outcome"] == "planned") == (case["expected"] == "planned"), case
        owner = request_value["ownership"]
        cases.append({"owner": owner, "timer": request_value["requestedTimer"], "commands": [], "input": {
            "timerId": "existing-timer", "deviceId": "device-local", "tabId": "tab-local",
            "nowMs": case["leaseNowMs"], "localNowMs": case["leaseNowMs"], "leaseMs": 30000,
            "manual": False, "requireOwner": True}})
    output = subprocess.run(["node", "-e", program], input=json.dumps(cases),
                            text=True, capture_output=True, check=True)
    for case, actual in zip(owner_cases("pwaStorage"), json.loads(output.stdout), strict=True):
        expected = {"owned": case["expected"] == "planned",
                    "reason": "" if case["expected"] == "planned" else "not_owner"}
        if "retryAtMs" in case:
            expected["retryAtMs"] = case["retryAtMs"]
        assert actual == expected, (case, actual)
    return len(cases)


def pwa_parity_input(case):
    value = automatic(owner_cases("pwaStorage")[0])
    name = case["name"]
    value["stage"] = case["stage"]
    if name == "missing-canonical-foreign-start":
        value["ownership"] = None
        workspace = value["workspace"]
        workspace["base"]["canonicalTimer"] = None
        workspace["canonicalHead"] = {"wallMs": 1784548799000, "counter": 0}
        workspace["local"]["commands"] = [{
            "id": "start-foreign", "deviceId": "device-foreign", "deviceSequence": 1,
            "timerId": "existing-timer", "type": "start", "phase": "focus",
            "plannedDurationMs": 60000, "observedElapsedMs": 0,
            "occurredAt": "2026-07-20T12:00:00Z", "hlcWallMs": 1784548800000,
            "hlcCounter": 0}]
        workspace["neverSent"] = {"commands": ["start-foreign"]}
        value["requestedTimer"] = dispatch("workspace.project.v1", {
            **workspace, "now": "1970-01-01T00:00:00Z"})["workspace"]["canonicalTimer"]
    elif name == "manual-foreign-timer-owner":
        value["ownership"]["timerId"] = "other-timer"
    elif name == "foreign-owner-denied":
        value["ownership"]["deviceId"] = "device-foreign"
    elif name == "peer-lease-live":
        value["ownership"]["tabId"] = "tab-peer"
    elif name == "same-tab-missing-lease":
        value["ownership"].pop("leaseExpiresAtMs")
    else:
        raise AssertionError(name)
    return value


def pwa_indexeddb():
    cases = CASES["pwaParityCases"]
    inputs = [{"name": case["name"], "input": pwa_parity_input(case)} for case in cases]
    output = subprocess.run(["node", str(ROOT / "scripts/completion_mutation_probe_pwa.cjs")],
                            input=json.dumps(inputs), text=True, capture_output=True, check=True)
    for case, source, item in zip(cases, json.loads(output.stdout), inputs, strict=True):
        name = case["name"]
        result = core(item["input"])
        expected = {"outcome": "planned" if source["transitioned"] else "noop",
                    "reason": source["reason"]}
        assert expected == {"outcome": case["expectedOutcome"], "reason": case["expectedReason"]}, (name, source)
        assert {"outcome": result["outcome"], "reason": result["reason"]} == expected, (name, result, source)
        assert ("retryAtMs" in source) == ("retryAtMs" in result) == case["retryAtMsPresent"], (name, result, source)
        if case["retryAtMsPresent"]:
            assert result["retryAtMs"] == source["retryAtMs"] == case["retryAtMs"], name
        assert source["commands"] == result["commands"], (name, source, result)
        assert source["finishCount"] == len(result["commands"]), name
        if source["transitioned"]:
            assert source["ownerAfter"] is None, name
            assert source["selectedPhase"] == result["selection"]["phase"], name
        else:
            assert source["sequence"] == item["input"]["allocation"]["deviceSequence"], name
            assert source["hlc"] == item["input"]["allocation"]["hlc"], name
            assert source["uuidV7"] is None, name
    return len(cases)


def pwa_generated_indexeddb():
    cases = json.loads((ROOT / "fixtures/completion-generated-break-v1.json").read_text())
    inputs = []
    for case in cases["focusCounts"]:
        value = automatic(owner_cases("pwaStorage")[0])
        value["workspace"]["base"]["autoStartBreaks"] = True
        value["identities"]["commandUuids"] = cases["commandUuids"]
        value["identities"]["timerUuid"] = cases["timerUuid"]
        value["leaseDurationMs"] = cases["leaseDurationMs"]
        for index in range(case["prior"]):
            value["workspace"]["base"]["history"].append({
                "id": f"past-{index}", "timerId": f"past-timer-{index}",
                "commandId": f"past-finish-{index}", "phase": "focus",
                "status": "completed", "plannedDurationMs": 60000,
                "completedAt": "2026-07-20T11:00:00Z"})
        inputs.append({"name": f"generated-{case['prior']}", "input": value})
    output = subprocess.run(["node", str(ROOT / "scripts/completion_mutation_probe_pwa.cjs")],
                            input=json.dumps(inputs), text=True, capture_output=True, check=True)
    for item, source in zip(inputs, json.loads(output.stdout), strict=True):
        expected = core(item["input"])
        name = item["name"]
        assert source["transitioned"] and source["finishCount"] == source["startCount"] == 1, name
        assert source["commands"] == expected["commands"], (name, source, expected)
        assert source["ownerAfter"] == {key: value for key, value in expected["ownershipWrites"][0].items()
                                        if key != "kind"}, name
        assert source["selectedPhase"] == expected["selection"]["phase"], name
        assert source["hlc"] == expected["allocation"]["hlc"], name
        assert source["sequence"] == expected["allocation"]["deviceSequence"], name
        assert source["uuidV7"] == expected["allocation"]["lastUuid"], name
    return len(inputs)


def pwa_action_retry():
    source = SOURCES["pwa_actions"]
    program = 'const TIMER_OWNER_LEASE_MS=30000,TIMER_OWNER_HEARTBEAT_MS=5000;\n'
    program += 'class TimerLifecycle {\n'
    program += between(source, "    finishTimerRequest(", "    recordLastCommand(")
    program += between(source, "    completionRetryDelay(", "    completionAlertTitle(")
    program += '}\n'
    program += 'const obj=new TimerLifecycle();obj.state={deviceId:"device-local",autoStartBreaks:false};\n'
    program += 'obj.host={crypto:{randomUUID:()=>"break-uuid"}};\n'
    program += 'obj.use={captureAccountContext:()=>({}),tabId:()=>"tab-local",elapsedFor:()=>60000,settingsValue:()=>({})};\n'
    program += 'const timer={id:"existing-timer",phase:"focus"},now=1784548860000;\n'
    program += 'const request=obj.finishTimerRequest(timer,true,now,now,"owner");\n'
    program += 'const denied=obj.completionRetryDelay({reason:"not_owner",retryAtMs:1784548870000},now);\n'
    program += 'process.stdout.write(JSON.stringify({manual:request.manual,requireOwner:request.requireOwner,tabId:request.tabId,leaseNowMs:request.localNowMs,denied,missing:obj.completionRetryDelay({reason:"not_owner"},now),nullLease:obj.completionRetryDelay({reason:"not_owner",retryAtMs:null},now),camelCase:obj.completionRetryDelay({reason:"notOwner"},now),none:obj.completionRetryDelay({reason:"stale"},now)}));\n'
    output = subprocess.run(["node", "-e", program], text=True, capture_output=True, check=True)
    actual = json.loads(output.stdout)
    assert actual == {"manual": False, "requireOwner": True, "tabId": "tab-local",
                      "leaseNowMs": 1784548860000, "denied": 10001,
                      "missing": 5001, "nullLease": 250, "camelCase": None, "none": None}, actual
    owner = core(automatic(owner_cases("pwaStorage")[1]))
    assert owner["retryAtMs"] - actual["leaseNowMs"] + 1 == actual["denied"]
    missing = core(automatic(next(case for case in owner_cases("pwaStorage") if case["name"] == "pwa-foreign-expired")))
    assert missing["reason"] == "not_owner" and "retryAtMs" not in missing
    assert next(case for case in CASES["pwaParityCases"] if case["name"] == "foreign-owner-denied")["retryDelayMs"] == actual["missing"]
    assert next(case for case in CASES["pwaParityCases"] if case["name"] == "peer-lease-live")["retryDelayMs"] == actual["denied"]
    return 1


def pwa_expiry_gate():
    source = SOURCES["pwa_actions"]
    program = 'class TimerLifecycle {\n'
    program += between(source, "    updateTimerCompletion(", "    heartbeatTimerOwnership(")
    program += '}\n'
    program += 'const cases=JSON.parse(require("fs").readFileSync(0,"utf8"));\n'
    program += 'process.stdout.write(JSON.stringify(cases.map(([status,remaining])=>{\n'
    program += 'const timer={id:"existing-timer"};let calls=0;const lifecycle=new TimerLifecycle();\n'
    program += 'lifecycle.host={clearTimeout:()=>{}};lifecycle.finishTimer=()=>{calls++;return Promise.resolve(true)};\n'
    program += 'lifecycle.startCompletionAlert=()=>{};lifecycle.updateTimerCompletion(timer,status,remaining,false);\n'
    program += 'return calls;})));\n'
    cases = [["running", 1], ["running", 0], ["running", -1], ["paused", -1]]
    output = subprocess.run(["node", "-e", program], input=json.dumps(cases),
                            text=True, capture_output=True, check=True)
    assert json.loads(output.stdout) == [0, 1, 1, 0]
    request_value = automatic(owner_cases("pwaStorage")[0])
    request_value["requestedTimer"]["status"] = "paused"
    request_value["workspace"]["base"]["canonicalTimer"]["status"] = "paused"
    assert core(request_value)["reason"] == "notExpired"
    return len(cases)


def pwa_concurrent_finish():
    source = SOURCES["pwa"]
    program = 'const BootstrapGateError=Error;const assertAccountOwnership=()=>{};const core={compareTimerCommands:()=>0};\n'
    program += 'const projectState=({snapshot})=>({canonicalTimer:snapshot.canonicalTimer});\n'
    program += between(source, "  function finishProjection(", "  function finishAppliedCompletion(")
    program += between(source, "  function applyFinishedTimer(", "  function finishTimer(database")
    program += 'const timer={id:"existing-timer",phase:"focus",status:"running",lastIntent:{type:"finish"}};\n'
    program += 'const result=applyFinishedTimer({}, {timerId:timer.id,phase:timer.phase,requestedTimer:timer,nowMs:1784548860000}, {snapshot:{value:{canonicalTimer:timer}},commands:[]});\n'
    program += 'process.stdout.write(JSON.stringify(result));\n'
    output = subprocess.run(["node", "-e", program], text=True, capture_output=True, check=True)
    assert json.loads(output.stdout) == {"transitioned": False, "reason": "stale", "commands": []}
    input_value = automatic(owner_cases("pwaStorage")[0])
    input_value["workspace"]["base"]["canonicalTimer"]["lastIntent"] = {
        "type": "finish", "commandId": "old-finish", "occurredAt": "2026-07-20T12:00:20Z"}
    input_value["requestedTimer"] = copy.deepcopy(input_value["workspace"]["base"]["canonicalTimer"])
    assert core(input_value)["reason"] == "staleTimer"
    return 1


def android_owner(temporary):
    repository = between(SOURCES["android_repository"], "    private fun canFinishTimer(", "    private suspend fun issueCommand(")
    elapsed = between(SOURCES["presentation"], "    fun elapsedAt(", "\n}")
    program = "import java.time.Instant\nobject TimerStatus { const val Running = \"running\" }\n"
    program += "data class CanonicalTimer(val id:String,val status:String,val plannedDurationMs:Long,val elapsedAtAnchorMs:Long,val anchorAt:String)\n"
    program += "data class Local(val ownedTimerId:String?)\nobject TimerPresentation {\n" + elapsed + "\n}\n"
    program += "class Probe(var local:Local) { val activeStatuses=setOf(\"running\",\"paused\")\n"
    program += repository + "\nfun call(timer:CanonicalTimer, expired:Boolean)=canFinishTimer(timer,expired)\n}\nfun main(){\n"
    cases = owner_cases("androidCoordinator")
    for case in cases:
        owner = '"existing-timer"' if case["ownerDeviceId"] == "device-local" else '"other-timer"' if case["ownerDeviceId"] else "null"
        program += f'println(Probe(Local({owner})).call(CanonicalTimer("existing-timer","running",60000L,5000L,"2026-07-20T12:00:00Z"),true))\n'
    source = temporary / "finish_owner.kt"
    source.write_text(program + "}\n")
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    java = Path(os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    jar = temporary / "finish_owner.jar"
    subprocess.run([compiler, str(source), "-include-runtime", "-d", str(jar)],
                   env=dict(os.environ, JAVA_HOME=str(java)), check=True, capture_output=True)
    actual = subprocess.run([str(java / "bin/java"), "-jar", str(jar)],
                            text=True, capture_output=True, check=True).stdout.splitlines()
    for case, owned in zip(cases, actual, strict=True):
        assert (owned == "true") == (core(automatic(case))["outcome"] == "planned"), case
    return len(cases)


def apple_owner(temporary):
    source = between(SOURCES["apple"], "    private func completionOwnership(", "    private func completionDayBounds(")
    program = 'struct CanonicalTimer { let id:String; let startedByDeviceId:String? }\n'
    program += 'struct PersistedTimerState { let deviceId:String; let localTimerOwners:[String:String] }\n'
    program += 'struct CoreCompletionOwnership { let timerId:String; let ownerDeviceId:String }\n'
    program += 'struct Probe {\n' + source + '\nfunc call(_ owner:String?) -> Bool {\n'
    program += 'let timer=CanonicalTimer(id:"existing-timer",startedByDeviceId:"device-local")\n'
    program += 'let state=PersistedTimerState(deviceId:"device-local",localTimerOwners:owner.map {["existing-timer":$0]} ?? [:])\n'
    program += 'return completionOwnership(for:timer,state:state)?.ownerDeviceId == state.deviceId\n}\n}\n'
    for case in owner_cases("appleWorkspace"):
        owner = '"' + case["ownerDeviceId"] + '"' if case["ownerDeviceId"] else 'nil'
        program += f'print(Probe().call({owner}))\n'
    source_file = temporary / "finish_owner.swift"
    source_file.write_text(program)
    actual = subprocess.run(["swift", str(source_file)], text=True,
                            capture_output=True, check=True).stdout.splitlines()
    for case, owned in zip(owner_cases("appleWorkspace"), actual, strict=True):
        assert (owned == "true") == (core(automatic(case))["outcome"] == "planned"), case
    return len(actual)


def apple_expiry(temporary):
    elapsed = between(SOURCES["apple_timer"], "    func elapsed(at date: Date)", "    func remaining(at date: Date)")
    program = 'import Foundation\nstruct CanonicalTimer {\n'
    program += 'enum Status { case running,paused };let status:Status;let elapsedAtAnchorMs:Int64;let plannedDurationMs:Int64;let anchorAt:Date\n'
    program += 'var plannedDuration:TimeInterval { TimeInterval(plannedDurationMs)/1000 }\n'
    program += elapsed + '\n}\n'
    program += 'let anchor=Date(timeIntervalSince1970:1784548800)\n'
    program += 'let running=CanonicalTimer(status:.running,elapsedAtAnchorMs:5000,plannedDurationMs:60000,anchorAt:anchor)\n'
    program += 'let paused=CanonicalTimer(status:.paused,elapsedAtAnchorMs:5000,plannedDurationMs:60000,anchorAt:anchor)\n'
    program += 'for (timer,delta) in [(running,54.999),(running,55.0),(running,55.001),(paused,60.0)] {\n'
    program += 'print(timer.elapsed(at:anchor.addingTimeInterval(delta)) >= timer.plannedDuration)\n}\n'
    source = temporary / "finish_expiry.swift"
    source.write_text(program)
    actual = subprocess.run(["swift", str(source)], text=True,
                            capture_output=True, check=True).stdout.splitlines()
    assert actual == ["false", "true", "true", "false"], actual
    assert core(automatic(owner_cases("appleWorkspace")[0]))["outcome"] == "planned"
    return len(actual)


def android(temporary):
    finish = between(SOURCES["android"], "    private fun finishCommand(", "    private fun generatedBreakCommand(")
    elapsed = between(SOURCES["presentation"], "    fun elapsedAt(", "\n}")
    program = "import java.time.Instant\nobject CommandType { const val Finish = \"finish\" }\n"
    program += "object TimerStatus { const val Running = \"running\" }\n"
    program += "data class CanonicalTimer(val id:String,val phase:String,val status:String,val plannedDurationMs:Long,val elapsedAtAnchorMs:Long,val anchorAt:String)\n"
    program += "data class Stamp(val deviceSequence:Long?,val occurredAt:String,val wallMs:Long,val counter:Long)\n"
    program += "data class Reservation(val stamps:List<Stamp>,val uuids:List<String>)\n"
    program += "data class TimerFinishMutationInput(val current:CanonicalTimer,val reservation:Reservation,val physicalNowMs:Long)\n"
    program += "data class TimerCommand(val id:String,val deviceSequence:Long,val timerId:String,val type:String,val phase:String,val plannedDurationMs:Long,val occurredAt:String,val hlcWallMs:Long,val hlcCounter:Long,val observedElapsedMs:Long,val physicalOccurredAt:String)\n"
    program += "object TimerPresentation {\n" + elapsed + "\n}\nclass Probe {\n" + finish + "\n"
    program += "fun call(input:TimerFinishMutationInput):TimerCommand = finishCommand(input,Instant.ofEpochMilli(input.physicalNowMs).toString())\n}\nfun main() {\n"
    expected = []
    for status, value in active_cases("androidCoordinator"):
        result = core(value)
        command = result["commands"][0]
        timer = value["requestedTimer"]
        fields = [json.dumps(timer[key]) for key in ("id", "phase", "status", "anchorAt")]
        source = f'CanonicalTimer({fields[0]},{fields[1]},{fields[2]},{timer["plannedDurationMs"]}L,{timer["elapsedAtAnchorMs"]}L,{fields[3]})'
        stamp = f'Stamp({command["deviceSequence"]}L,{json.dumps(command["occurredAt"])},{command["hlcWallMs"]}L,{command["hlcCounter"]}L)'
        physical_ms = int(datetime.fromisoformat(value["clock"]["physicalNow"].replace("Z", "+00:00")).timestamp() * 1000)
        input_source = f'TimerFinishMutationInput({source},Reservation(listOf({stamp}),listOf({json.dumps(command["id"])})),{physical_ms}L)'
        program += f'val c{len(expected)}=Probe().call({input_source})\n'
        program += f'println(listOf(c{len(expected)}.id,c{len(expected)}.type,c{len(expected)}.occurredAt,c{len(expected)}.observedElapsedMs,c{len(expected)}.physicalOccurredAt).joinToString("|"))\n'
        expected.append("|".join(map(str, [command["id"], "finish", command["occurredAt"],
                                          command["observedElapsedMs"], value["clock"]["physicalNow"]])))
    source = temporary / "finish.kt"
    source.write_text(program + "}\n")
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    java = Path(os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    jar = temporary / "finish.jar"
    subprocess.run([compiler, str(source), "-include-runtime", "-d", str(jar)],
                   env=dict(os.environ, JAVA_HOME=str(java)), check=True, capture_output=True)
    actual = subprocess.run([str(java / "bin/java"), "-jar", str(jar)],
                            text=True, capture_output=True, check=True).stdout.splitlines()
    assert actual == expected, (actual, expected)
    return len(expected)


def generated_request(profile, prior):
    cases = json.loads((ROOT / "fixtures/completion-generated-break-v1.json").read_text())
    value = automatic({"profile": profile, "ownerDeviceId": "device-local",
                       "ownerTabId": "tab-local", "leaseNowMs": 1784548860000,
                       "leaseExpiresAtMs": 1784548870000})
    value["workspace"]["base"]["autoStartBreaks"] = True
    value["identities"]["commandUuids"] = cases["commandUuids"]
    value["identities"]["timerUuid"] = cases["timerUuid"]
    if profile == "pwaStorage":
        value["leaseDurationMs"] = cases["leaseDurationMs"]
    for index in range(prior):
        value["workspace"]["base"]["history"].append({
            "id": f"past-{index}", "timerId": f"past-timer-{index}",
            "commandId": f"past-finish-{index}", "phase": "focus",
            "status": "completed", "plannedDurationMs": 60000,
            "completedAt": "2026-07-20T11:00:00Z"})
    return value


def android_generated(temporary):
    method_source = between(SOURCES["android"], "    private fun generatedBreakCommand(", "    private fun completionDecision(")
    program = '''data class Stamp(val deviceSequence:Long?,val occurredAt:String,val wallMs:Long,val counter:Long)
data class Reservation(val stamps:List<Stamp>,val uuids:List<String>)
data class Settings(val shortMs:Long,val longMs:Long) { fun durationMsFor(phase:String)=if(phase=="long_break") longMs else shortMs }
data class State(val settings:Settings)
data class Input(val state:State,val reservation:Reservation)
data class TimerCommand(val id:String,val deviceSequence:Long,val timerId:String,val type:String,val phase:String,val plannedDurationMs:Long,val occurredAt:String,val hlcWallMs:Long,val hlcCounter:Long,val observedElapsedMs:Long,val physicalOccurredAt:String)
object CommandType { const val Start="start" }
class Probe(val breakId:String) {
fun timerId()=breakId
''' + method_source.replace("TimerFinishMutationInput", "Input") + '\nfun call(input:Input,phase:String)=generatedBreakCommand(input,phase,"2026-07-20T12:01:00Z")\n}\nfun main(){\n'
    expected = []
    for prior in (2, 3, 6, 7):
        result = core(generated_request("androidCoordinator", prior))
        start = result["commands"][1]
        stamp = f'Stamp({start["deviceSequence"]}L,{json.dumps(start["occurredAt"])},{start["hlcWallMs"]}L,{start["hlcCounter"]}L)'
        settings = result["projection"]["durationsMs"]
        command_ids = result["commands"]
        program += f'val x{prior}=Probe({json.dumps(start["timerId"])}).call(Input(State(Settings({settings["short_break"]}L,{settings["long_break"]}L)),Reservation(listOf({stamp}),listOf({json.dumps(command_ids[0]["id"])},{json.dumps(start["id"])}))),{json.dumps(start["phase"])})\n'
        program += f'println(listOf(x{prior}.id,x{prior}.timerId,x{prior}.phase,x{prior}.plannedDurationMs,x{prior}.deviceSequence,x{prior}.hlcCounter).joinToString("|"))\n'
        expected.append("|".join(str(start[field]) for field in ("id", "timerId", "phase", "plannedDurationMs", "deviceSequence", "hlcCounter")))
    source = temporary / "generated_break.kt"
    source.write_text(program + "}\n")
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    java = Path(os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    jar = temporary / "generated_break.jar"
    subprocess.run([compiler, str(source), "-include-runtime", "-d", str(jar)],
                   env=dict(os.environ, JAVA_HOME=str(java)), check=True, capture_output=True)
    actual = subprocess.run([str(java / "bin/java"), "-jar", str(jar)],
                            text=True, capture_output=True, check=True).stdout.splitlines()
    assert actual == expected, (actual, expected)
    return len(expected)


def android_generated_dependencies(temporary):
    method_source = between(SOURCES["android"], "    private fun completionDependencies(", "    private fun completionProjection(")
    program = '''data class TimerCommand(val id:String,val timerId:String,val type:String)
data class TimerMutationState(val previous:String?)
object CommandType { const val Start="start" }
class Probe {
fun dependencyForTimer(state:TimerMutationState,timerId:String)=state.previous
''' + method_source + '''
fun call(parent:String?,finish:TimerCommand,start:TimerCommand)=completionDependencies(
 TimerMutationState(parent),finish,listOf(finish,start))
}
fun main(){
'''
    result = core(generated_request("androidCoordinator", 3))
    finish, start = result["commands"]
    program += f'''val finish=TimerCommand({json.dumps(finish["id"])},{json.dumps(finish["timerId"])},"finish")
val start=TimerCommand({json.dumps(start["id"])},{json.dumps(start["timerId"])},"start")
println(Probe().call(null,finish,start).entries.joinToString("|"){{ it.key+":"+it.value }})
println(Probe().call("ancestor",finish,start).entries.joinToString("|"){{ it.key+":"+it.value }})
}}
'''
    source = temporary / "generated_dependencies.kt"
    source.write_text(program)
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    java = Path(os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    jar = temporary / "generated_dependencies.jar"
    subprocess.run([compiler, str(source), "-include-runtime", "-d", str(jar)],
                   env=dict(os.environ, JAVA_HOME=str(java)), check=True, capture_output=True)
    actual = subprocess.run([str(java / "bin/java"), "-jar", str(jar)],
                            text=True, capture_output=True, check=True).stdout.splitlines()
    expected = [f'{start["id"]}:{finish["id"]}',
                f'{finish["id"]}:ancestor|{start["id"]}:{finish["id"]}']
    assert actual == expected, (actual, expected)
    edge = result["workspace"]["timerDependencies"][0]
    assert edge["operationId"] == start["id"] and edge["dependsOnOperationId"] == finish["id"]
    return len(actual)


def apple_generated(temporary):
    method_source = between(SOURCES["apple"], "    func prepareCentralizedAutomaticBreak(", "    func prepareIrohAutomaticBreak(")
    program = '''struct Timer { let id:String }
struct Command { let id:String;let occurredAt:Int }
struct ProvisionalBreak { let focusTimerId:String;let finishCommandId:String;let breakTimerId:String;let startCommandId:String }
struct State { var provisionalBreaks:[ProvisionalBreak]=[] }
struct CommandRequest { let type:String;let timerID:String;let taskID:String?;let phase:String;let duration:Double;let elapsed:Double;let occurredAt:Int;let localDate:Int }
struct Transition { let state:State;let command:Command }
struct Finish { let state:State;let command:Command;let occurredAt:Int;let localDate:Int }
struct AutomaticBreak { let timerID:String;let phase:String;let duration:Double }
struct AutomaticBreakTransition { let state:State;let automaticBreak:AutomaticBreak;let command:Command }
typealias CanonicalTimer = Timer
typealias FinishTransition = Finish
struct Probe {
let startId:String
func makeCommand(_ request:CommandRequest,state:State) throws -> Transition {
precondition(request.type=="start" && request.taskID==nil && request.elapsed==0 && request.occurredAt==1784548860)
return Transition(state:state,command:Command(id:startId,occurredAt:request.occurredAt))
}
''' + method_source.replace('type: .start', 'type: "start"') + '\n}\n'
    for prior in (2, 3, 6, 7):
        result = core(generated_request("appleWorkspace", prior))
        start = result["commands"][1]
        phase = start["phase"]
        program += f'''let start{prior}=try Probe(startId:{json.dumps(start["id"])}).prepareCentralizedAutomaticBreak(
 AutomaticBreak(timerID:{json.dumps(start["timerId"])},phase:{json.dumps(phase)},duration:{start["plannedDurationMs"]}.0/1000),
 after:Timer(id:"existing-timer"),
 finish:Finish(state:State(),command:Command(id:{json.dumps(result["commands"][0]["id"])},occurredAt:1784548860),occurredAt:1784548860,localDate:1784548860))
let provisional{prior}=start{prior}.state.provisionalBreaks.first!
print(provisional{prior}.focusTimerId,provisional{prior}.finishCommandId,provisional{prior}.breakTimerId,provisional{prior}.startCommandId)
'''
    source = temporary / "generated_break.swift"
    source.write_text(program)
    run = subprocess.run(["swift", str(source)], text=True, capture_output=True)
    assert run.returncode == 0, run.stderr
    actual = run.stdout.splitlines()
    expected = []
    for prior in (2, 3, 6, 7):
        result = core(generated_request("appleWorkspace", prior))
        record = result["completionRecords"]["provisionalBreak"]
        expected.append(" ".join(record[key] for key in ("focusTimerId", "finishCommandId", "breakTimerId", "startCommandId")))
    assert actual == expected, (actual, expected)
    return len(actual)


def apple(temporary):
    method_source = between(SOURCES["apple"], "    private func recordPhaseAdvance(", "    private func loadCore()")
    program = 'enum TimerPhase: String { case focus, shortBreak = "short_break", longBreak = "long_break" }\n'
    program += 'enum ReplicationMode { case centralized, iroh }\n'
    program += 'struct CanonicalTimer { let id: String }\n'
    program += 'struct Settings { var selectedPhase: TimerPhase }\n'
    program += 'struct ProvisionalPhaseAdvance { let sourceTimerId:String;let finishCommandId:String;let previousPhase:TimerPhase;let advancedPhase:TimerPhase;let generation:Int64 }\n'
    program += 'struct PersistedTimerState { var settings:Settings;var hasExplicitPhaseSelection:Bool;var selectedPhaseGeneration:Int64;var provisionalPhaseAdvances:[ProvisionalPhaseAdvance]=[] }\n'
    program += 'struct Probe {\n' + method_source + '\nfunc call(_ state:inout PersistedTimerState) { recordPhaseAdvance(to:.shortBreak,afterFinishing:CanonicalTimer(id:"existing-timer"),commandID:"command-019f7f65-dd10-7000-8000-000000000001",replicationMode:.centralized,in:&state) }\n}\n'
    program += 'var state=PersistedTimerState(settings:Settings(selectedPhase:.focus),hasExplicitPhaseSelection:false,selectedPhaseGeneration:5)\n'
    program += 'Probe().call(&state)\nprint(state.settings.selectedPhase.rawValue, state.selectedPhaseGeneration, state.provisionalPhaseAdvances.first!.finishCommandId)\n'
    source = temporary / "finish.swift"
    source.write_text(program)
    output = subprocess.run(["swift", str(source)], text=True, capture_output=True, check=True).stdout.strip()
    result = core(request("appleWorkspace", "running"))
    assert output == " ".join([result["selection"]["phase"], result["selection"]["generation"],
                               result["completionRecords"]["phaseAdvance"]["finishCommandId"]])
    return 1


def main():
    if not BRIDGE.is_file():
        raise RuntimeError("build native completion_policy_probe example first")
    with tempfile.TemporaryDirectory(prefix="finish-source-probe-") as directory:
        temporary = Path(directory)
        for name, count in [("apple", apple(temporary)), ("android", android(temporary)),
                            ("Apple generated", apple_generated(temporary)),
                            ("Android generated", android_generated(temporary)),
                            ("Android dependency map", android_generated_dependencies(temporary)),
                            ("desktop", desktop()), ("pwa", pwa()),
                            ("apple owner", apple_owner(temporary)),
                            ("android owner", android_owner(temporary)), ("pwa owner", pwa_owner()),
                            ("pwa actions", pwa_action_retry()), ("pwa expiry gate", pwa_expiry_gate()),
                            ("pwa concurrent finish", pwa_concurrent_finish()),
                             ("apple expiry", apple_expiry(temporary)), ("pwa IndexedDB", pwa_indexeddb()),
                             ("PWA generated IndexedDB", pwa_generated_indexeddb())]:
            print(f"{name}: {count} extracted production-method cases passed")


if __name__ == "__main__":
    main()
