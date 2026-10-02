#!/usr/bin/env python3
"""Compare Core follow-up dependencies with unchanged production source methods."""

import copy
from datetime import datetime
import json
import os
from pathlib import Path
import subprocess
import tempfile

import completion_mutation_source_probe as baseline


def core_followup(profile):
    request = baseline.generated_request(profile, 3)
    first = baseline.core(request)
    next_request = copy.deepcopy(request)
    next_request["stage"] = "finishCommit"
    next_request["workspace"] = first["workspace"]
    next_request["requestedTimer"] = first["projection"]["canonicalTimer"]
    next_request["selection"] = first["selection"]
    next_request["allocation"] = first["allocation"]
    next_request["observation"] = first["observation"]
    next_request["ownership"]["timerId"] = first["commands"][1]["timerId"]
    stamp = "2026-07-20T12:01:01Z"
    next_request["clock"].update(occurredAt=stamp, physicalNow=stamp, observedAt=stamp)
    wall = int(datetime.fromisoformat(stamp.replace("Z", "+00:00")).timestamp() * 1000)
    digits = f"{wall:012x}"
    next_request["identities"] = {
        "commandUuids": [f"{digits[:8]}-{digits[8:]}-7000-8000-000000000001"],
        "timerUuid": None,
    }
    planned = baseline.core(next_request)
    parent = planned["workspace"]["timerDependencies"][1]["dependsOnOperationId"]
    assert parent == first["commands"][1]["id"]
    return first, next_request, planned


def pwa_source(first, request, planned):
    source = baseline.between(baseline.SOURCES["pwa"],
                              "  function finishTimerCommand(", "  function generatedBreakCommand(")
    program = source + "\nconst item=JSON.parse(require('fs').readFileSync(0,'utf8'));\n"
    program += "process.stdout.write(JSON.stringify(finishTimerCommand(item.timer,item.input,item.id,item.position)));\n"
    command = planned["commands"][0]
    timer = {**request["requestedTimer"], "dependsOnCommandId": first["commands"][0]["id"]}
    payload = {"timer": timer, "id": command["id"],
               "input": {"deviceId": request["allocation"]["deviceId"],
                         "observedElapsedMs": command["observedElapsedMs"]},
               "position": {"highestSequence": command["deviceSequence"] - 1,
                            "occurredAt": command["occurredAt"], "wallMs": command["hlcWallMs"],
                            "firstCounter": command["hlcCounter"]}}
    run = subprocess.run(["node", "-e", program], input=json.dumps(payload),
                         text=True, capture_output=True, check=True)
    actual = json.loads(run.stdout)
    assert actual["dependsOnCommandId"] == first["commands"][0]["id"]
    assert planned["commands"][0]["dependsOnCommandId"] == first["commands"][1]["id"]
    return actual["dependsOnCommandId"]


def android_source(first, planned, temporary):
    source = baseline.SOURCES["android"]
    dependency = baseline.between(source, "    private fun dependencyForTimer(", "    private fun validTransition(")
    completion = baseline.between(source, "    private fun completionDependencies(",
                                  "    private fun completionProjection(")
    program = """data class TimerCommand(val id:String,val timerId:String,val type:String)
data class Queues(val commands:List<TimerCommand>)
data class TimerMutationState(val queues:Queues,val dependencies:Map<String,String>)
object CommandType { const val Start="start" }
class Probe {
""" + dependency + completion + "\nfun call(state:TimerMutationState,finish:TimerCommand)=completionDependencies(state,finish,listOf(finish))\n}\nfun main(){\n"
    focus, start = first["commands"]
    later = planned["commands"][0]
    program += f'''val state=TimerMutationState(Queues(listOf(
 TimerCommand({json.dumps(focus["id"])},{json.dumps(focus["timerId"])},"finish"),
 TimerCommand({json.dumps(start["id"])},{json.dumps(start["timerId"])},"start"))),
 mapOf({json.dumps(start["id"])} to {json.dumps(focus["id"])}))
print(Probe().call(state,TimerCommand({json.dumps(later["id"])},{json.dumps(later["timerId"])},"finish"))[{json.dumps(later["id"])}])
}}
'''
    path = temporary / "android_followup.kt"
    path.write_text(program)
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    java = Path(os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    jar = temporary / "android_followup.jar"
    subprocess.run([compiler, str(path), "-include-runtime", "-d", str(jar)],
                   env=dict(os.environ, JAVA_HOME=str(java)), capture_output=True, check=True)
    run = subprocess.run([str(java / "bin/java"), "-jar", str(jar)],
                         text=True, capture_output=True, check=True)
    assert run.stdout == focus["id"]
    return run.stdout


def apple_source(first, planned, temporary):
    source = baseline.SUITE / "apple/Sources/AccountSynchronization.swift"
    dependency = baseline.between(source, "    private func coreTimerDependencies(",
                                  "    private func resolveProvisionalPhaseAdvances(")
    program = """import Foundation
enum CommandType { case start, finish }
struct Command { let id:String;let timerId:String;let type:CommandType;let occurredAt:Date }
typealias TimerCommand = Command
struct ProvisionalBreak { let focusTimerId:String;let finishCommandId:String;let breakTimerId:String;let startCommandId:String }
struct PersistedTimerState { let pendingCommands:[Command];let provisionalBreaks:[ProvisionalBreak];let pendingTimerDependencies:[CoreTimerDependency]=[] }
struct CoreTimerDependency {
 let operationId:String;let dependsOnOperationId:String
 init(operationId:String,dependsOnOperationId:String,generatedBreak:Bool=false,
 sourceDayStart:Date?=nil,sourceDayEnd:Date?=nil) {
 self.operationId=operationId;self.dependsOnOperationId=dependsOnOperationId
 }
}
struct Probe {
""" + dependency + "\nfunc call(_ state:PersistedTimerState)->[CoreTimerDependency] { coreTimerDependencies(in:state) }\n}\n"
    focus, start = first["commands"]
    later = planned["commands"][0]
    program += f'''let date=Date(timeIntervalSince1970:1784548860)
let state=PersistedTimerState(pendingCommands:[
 Command(id:{json.dumps(focus["id"])},timerId:{json.dumps(focus["timerId"])},type:.finish,occurredAt:date),
 Command(id:{json.dumps(start["id"])},timerId:{json.dumps(start["timerId"])},type:.start,occurredAt:date),
 Command(id:{json.dumps(later["id"])},timerId:{json.dumps(later["timerId"])},type:.finish,occurredAt:date)],
 provisionalBreaks:[ProvisionalBreak(focusTimerId:{json.dumps(focus["timerId"])},
 finishCommandId:{json.dumps(focus["id"])},breakTimerId:{json.dumps(start["timerId"])},
 startCommandId:{json.dumps(start["id"])})])
print(Probe().call(state).last!.dependsOnOperationId)
'''
    path = temporary / "apple_followup.swift"
    path.write_text(program)
    run = subprocess.run(["swift", str(path)], text=True, capture_output=True)
    assert run.returncode == 0, run.stderr
    assert run.stdout.strip() == start["id"]
    return run.stdout.strip()


def main():
    if not baseline.BRIDGE.is_file():
        raise RuntimeError("build native completion_policy_probe example first")
    with tempfile.TemporaryDirectory(prefix="completion-followup-") as directory:
        temporary = Path(directory)
        for profile, source in (("pwaStorage", pwa_source),
                                ("androidCoordinator", android_source),
                                ("appleWorkspace", apple_source)):
            first, request, planned = core_followup(profile)
            actual = source(first, request, planned) if profile == "pwaStorage" else source(first, planned, temporary)
            expected = planned["workspace"]["timerDependencies"][1]["dependsOnOperationId"]
            status = "parity" if actual == expected else "source divergence"
            print(f"{profile}: {status}; source parent={actual}, Core parent={expected}")


if __name__ == "__main__":
    main()
