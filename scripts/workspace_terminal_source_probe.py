#!/usr/bin/env python3
"""Compare complete terminal results with extracted Apple and Desktop mappers.

This uses the native dispatcher, not a packaged client or WASM artifact.
Only model dependencies and persistence inputs are supplied by the probe.
"""
import ast
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import types

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT.parent
BRIDGE = ROOT / "target/debug/examples/read_model_probe"
FIXTURE = json.loads((ROOT / "fixtures/workspace-terminal-v1.json").read_text())


def dispatch(operation, value):
    result = subprocess.run([str(BRIDGE)], input=json.dumps(dict(operation=operation, input=value)) + "\n",
                            text=True, capture_output=True, check=True)
    return json.loads(result.stdout)


def request(case):
    value = copy.deepcopy(FIXTURE["request"])
    timer = {**FIXTURE["timer"], **case.get("timerOverrides", {})}
    history = {**FIXTURE["history"], **case.get("historyOverrides", {})}
    value["base"].update(canonicalTimer=None if case.get("cleared") else timer,
                         history=[] if case.get("missingHistory") else [history])
    if case.get("sameTimeSibling"):
        value["base"]["history"].append({**FIXTURE["history"], "id": "aaa-sibling",
                                        "timerId": "aaa-sibling", "commandId": "finish-sibling"})
    if case.get("commands"):
        value["local"]["commands"] = [copy.deepcopy(FIXTURE["command"])]
        if case.get("safe"):
            value["neverSent"]["commands"] = [FIXTURE["command"]["id"]]
    if "retainedCommands" in case:
        value["local"]["commands"] = copy.deepcopy(case["retainedCommands"])
    return value


def method(path, name, namespace):
    tree = ast.parse(path.read_text())
    function = next(node for node in ast.walk(tree) if isinstance(node, ast.FunctionDef) and node.name == name)
    function.decorator_list = []
    module = ast.Module(body=[ast.ImportFrom(module="__future__", names=[ast.alias(name="annotations")], level=0),
                             function], type_ignores=[])
    exec(compile(ast.fix_missing_locations(module), str(path), "exec"), namespace)
    return namespace[name]


def wire(value):
    if isinstance(value, dict):
        return {key: wire(field) for key, field in value.items() if field is not None}
    if isinstance(value, list):
        return [wire(field) for field in value]
    if isinstance(value, str) and value.endswith(".000Z"):
        return value[:-5] + "Z"
    return value


def legacy_projection(value, projection_input):
    pending = dispatch("workspace.project.v1", value)["projectionPending"]
    raw = projection_input(value["base"], value["base"], pending, value["now"])
    return dispatch("projection.apply.v2", raw)


def desktop(vectors):
    storage = SUITE / "desktop/src/pomodorough/storage.py"
    controller = SUITE / "desktop/src/pomodorough/ui_controller.py"
    projection_input = method(storage, "_projection_input", {})
    presented = method(controller, "presented_timer", {"TERMINAL_STATUSES": {"completed", "cancelled", "superseded"}})
    history_differences = 0
    for value, expected in vectors:
        legacy = legacy_projection(value, projection_input)
        actual = presented(types.SimpleNamespace(canonical_timer=legacy["canonicalTimer"],
            timer_outcomes=legacy["timerOutcomes"]), value["base"], value["local"]["commands"])
        assert wire(actual) == wire(expected["canonicalTimer"]), (actual, expected)
        if wire(legacy["history"]) != wire(expected["history"]):
            assert not value["base"]["history"]
            assert value["base"]["canonicalTimer"]["lastIntent"]["type"] in {"start", "pause", "resume"}
            assert len(legacy["history"]) == 1
            old_history = copy.deepcopy(legacy["history"])
            assert old_history[0].pop("commandId") == value["base"]["canonicalTimer"]["lastIntent"]["commandId"]
            assert wire(old_history) == wire(expected["history"])
            history_differences += 1
    assert history_differences == 2
    return dict(terminalMatches=len(vectors), intentionalHistoryDifferences=history_differences)


def section(path, start, end):
    contents = path.read_text()
    return contents[contents.index(start):contents.index(end, contents.index(start))]


def apple_program():
    models = SUITE / "apple/Sources/TimerOperationModels.swift"
    history = SUITE / "apple/Sources/TaskHistoryModels.swift"
    mapper = SUITE / "apple/Sources/IrohRoomProjection.swift"
    return APPLE_MODELS + section(models, "struct TimerIntent:", "extension CanonicalTimer.Status") + \
        section(history, "struct HistoryItem:", "struct FocusTask:") + \
        "enum SourceProbe {\n" + section(mapper, "    private static func restoreIntentDevice(",
                                         "    private static func projectedState(") + APPLE_RUNNER


APPLE_MODELS = '''import Foundation
enum CommandType: String, Codable { case start, pause, resume, finish, cancel, clear, retarget }
enum TimerPhase: String, Codable { case focus, short_break, long_break }
enum WireBounds { static func physicalMilliseconds(for date:Date)->Int64? { Int64(date.timeIntervalSince1970*1000) } }
enum DurationValues { static func isValidWireDuration(_ value:Int64)->Bool { (60000...14400000).contains(value) } }
extension CanonicalTimer.Status { var localizedText:String { rawValue } }
struct TimerCommand:Codable {
 let id:String; let timerId:String; let type:CommandType; let observedElapsedMs:Int64; let occurredAt:Date; let deviceId:String
}
typealias CoreTimerCommand = TimerCommand
struct Outcome { enum Kind { case applied, ignored, rejected }; let outcome:Kind }
struct CoreProjectionOutput { let timerOutcomes:[String:Outcome] }
struct IrohGenesis { let canonicalTimer:CanonicalTimer? }
struct ProjectionOperations {
 let commands:[CoreTimerCommand]; let nativeCommands:[String:TimerCommand]
 let timerStarters:[String:String]; let deviceByCommand:[String:String]
}
struct Vector:Decodable {
 let timer:CanonicalTimer?; let history:[HistoryItem]; let projected:CanonicalTimer?
 let commands:[TimerCommand]; let outcomes:[String:String]
 let physicalAnchor:Date?
}
'''

APPLE_RUNNER = '''
 static func run(_ vector:Vector)->CanonicalTimer? {
  let operations=ProjectionOperations(commands:vector.commands,
   nativeCommands:Dictionary(uniqueKeysWithValues:vector.commands.map { ($0.id,$0) }),
   timerStarters:Dictionary(uniqueKeysWithValues:vector.commands.filter { $0.type == .start }.map { ($0.timerId,$0.deviceId) }),
   deviceByCommand:Dictionary(uniqueKeysWithValues:vector.commands.map { ($0.id,$0.deviceId) }))
  let outcomes=vector.outcomes.mapValues { Outcome(outcome:$0 == "applied" ? .applied : .ignored) }
  return restoreTerminalTimer(restoreIntentDevice(vector.projected, devices:operations.deviceByCommand), history:vector.history,
   output:CoreProjectionOutput(timerOutcomes:outcomes),
   genesis:IrohGenesis(canonicalTimer:vector.timer), operations:operations)
 }
}
struct SourcePhysical {
 let offset:TimeInterval
 func physicalDate(forTrustedDate date:Date) throws -> Date { date.addingTimeInterval(offset) }
 PHYSICAL_METHOD
}
let dates=ISO8601DateFormatter()
dates.formatOptions=[.withInternetDateTime]
let decoder=JSONDecoder()
decoder.dateDecodingStrategy = .custom { decoder in
 let value=try decoder.singleValueContainer().decode(String.self)
 if let date=dates.date(from:value) { return date }
 let fractional=ISO8601DateFormatter()
 fractional.formatOptions=[.withInternetDateTime,.withFractionalSeconds]
 return fractional.date(from:value)!
}
let encoder=JSONEncoder()
encoder.dateEncodingStrategy = .custom { date, encoder in
 var container=encoder.singleValueContainer(); try container.encode(dates.string(from:date))
}
while let line=readLine() {
 let vector=try! decoder.decode(Vector.self,from:Data(line.utf8))
 let timer:CanonicalTimer?
 if let physical=vector.physicalAnchor,let wire=vector.timer {
  timer=try! SourcePhysical(offset:physical.timeIntervalSince(wire.anchorAt)).physicalCanonicalTimer(wire)
 } else { timer=SourceProbe.run(vector) }
 print(String(data:try! encoder.encode(timer),encoding:.utf8)!)
}
'''.replace("PHYSICAL_METHOD", section(SUITE / "apple/Sources/PersistedTimerState.swift",
    "    func physicalCanonicalTimer(", "    mutating func advanceClock("))


def apple(vectors, physical_vectors, temporary):
    storage_input = method(SUITE / "desktop/src/pomodorough/storage.py", "_projection_input", {})
    rows = []
    expected_timers = []
    for value, expected in vectors:
        legacy = legacy_projection(value, storage_input)
        rows.append(dict(timer=value["base"]["canonicalTimer"], history=legacy["history"],
            projected=legacy["canonicalTimer"], commands=value["local"]["commands"],
            outcomes={key: outcome["outcome"] for key, outcome in legacy["timerOutcomes"].items()}))
        expected_timers.append(expected["canonicalTimer"])
    for kind, elapsed in [("finish", 60000), ("cancel", 17000)]:
        value = request({"missingHistory": True})
        value["base"]["canonicalTimer"].update(status="paused", elapsedAtAnchorMs=1000, lastIntent=None)
        command = {**FIXTURE["command"], "type": kind, "observedElapsedMs": elapsed}
        value["local"]["commands"] = [command]
        value["neverSent"]["commands"] = [command["id"]]
        projected = dispatch("workspace.project.v1", value)["workspace"]
        # Exercise the mapper's reconstruction branch with its nullable timer
        # parameter. History and outcomes come from the real native reduction.
        rows.append(dict(timer=value["base"]["canonicalTimer"], history=projected["history"],
            projected=None, commands=[command], outcomes={command["id"]: "applied"}))
        expected_timers.append(projected["canonicalTimer"])
    for value, observation, expected in physical_vectors:
        physical_anchor = observation["canonicalAnchorAt"] or observation["commandTimes"][value["base"]["canonicalTimer"]["lastIntent"]["commandId"]]
        rows.append(dict(timer=value["base"]["canonicalTimer"], history=value["base"]["history"], projected=None,
            commands=value["local"]["commands"], outcomes={}, physicalAnchor=physical_anchor))
        expected_timers.append(expected)
    program = temporary / "terminal.swift"
    program.write_text(apple_program())
    run = subprocess.run(["swift", str(program)], input="\n".join(map(json.dumps, rows)) + "\n",
                         text=True, capture_output=True, check=True)
    actual = list(map(json.loads, run.stdout.splitlines()))
    assert len(actual) == len(expected_timers)
    for timer, expected in zip(actual, expected_timers):
        assert wire(timer) == wire(expected), (timer, expected)
    return len(expected_timers)


def main():
    vectors = []
    for case in FIXTURE["cases"]:
        value = request(case)
        output = dispatch("workspace.project.v1", value)
        assert "error" not in output, (case["name"], output)
        vectors.append((value, output["workspace"]))
    for case in FIXTURE["rejections"]:
        assert dispatch("workspace.project.v1", request(case)) == {"error": FIXTURE["conflictError"]}, case["name"]
    originating, physical = originating_vectors()
    vectors.extend(originating)
    with tempfile.TemporaryDirectory(prefix="core-terminal-") as temporary:
        counts = dict(desktop=desktop(vectors), apple=apple(vectors, physical, Path(temporary)),
                      originating=len(originating), physicalObservations=len(physical))
    print(json.dumps(dict(passed=counts, rejected=len(FIXTURE["rejections"]))))
    for relative in ["apple/Sources/IrohRoomProjection.swift", "apple/Sources/SharedCoreModels.swift",
                     "desktop/src/pomodorough/storage.py", "desktop/src/pomodorough/ui_controller.py",
                     "android/app/src/main/java/me/egigoka/pomodorough/data/CoreProjectionDispatcher.kt"]:
        print(relative, hashlib.sha256((SUITE / relative).read_bytes()).hexdigest())


def originating_vectors():
    template = json.loads((ROOT / "fixtures/workspace-intent-v1.json").read_text())
    finish_input = copy.deepcopy(template["request"])
    finish_input.pop("intent")
    finish_input.update(stage="finishCommit", compatibility="desktopStorage", ownership=None)
    finish_input["identities"]["commandUuids"] = finish_input["identities"]["commandUuids"][:1]
    finish_input["requestedTimer"] = {**template["timer"], "taskId": FIXTURE["timer"]["taskId"]}
    finish_input["workspace"]["base"]["canonicalTimer"] = copy.deepcopy(finish_input["requestedTimer"])
    finish_input["clock"].update(physicalNow="2026-07-20T12:00:15Z", observedAt="2026-07-20T12:00:15Z")
    committed = dispatch("workspace.completionMutation.v1", finish_input)
    assert "error" not in committed and "taskId" not in committed["commands"][0], committed
    original = committed["workspace"]
    wire_output = dispatch("projection.apply.v2", dict(base=original["base"], pending=original["local"],
                                                     now=finish_input["clock"]["occurredAt"]))
    installed = copy.deepcopy(original)
    installed["base"].update(canonicalTimer=wire_output["canonicalTimer"], history=wire_output["history"])
    installed["base"]["canonicalTimer"]["lastIntent"]["deviceId"] = committed["commands"][0]["deviceId"]
    installed["neverSent"] = {}
    vectors = []
    for fields in FIXTURE["ignoredCommandFields"]:
        value = copy.deepcopy(installed)
        command = value["local"]["commands"][0]
        command.update({key: field for key, field in fields.items() if key != "omitTask"})
        if fields.get("omitTask"):
            command.pop("taskId", None)
        value["now"] = finish_input["clock"]["physicalNow"]
        output = dispatch("workspace.project.v1", value)
        assert "error" not in output and value["local"]["commands"][0]["id"] == committed["commands"][0]["id"], output
        vectors.append((value, output["workspace"]))
    return vectors, physical_vectors(installed, committed["observation"], template)


def physical_vectors(installed, saved, template):
    vectors = []
    for empty in (False, True):
        workspace = copy.deepcopy(installed)
        observation = copy.deepcopy(saved)
        if empty:
            workspace["local"]["commands"] = []
            observation = dict(canonicalAnchorAt="2026-07-20T12:00:15Z", commandTimes={})
        request_value = copy.deepcopy(template["request"])
        request_value.update(workspace=workspace, observation=observation, intent={"kind": "pause"}, compatibility="appleWorkspace")
        request_value["clock"].update(physicalNow="2026-07-20T12:00:15Z", observedAt="2026-07-20T12:00:15Z")
        output = dispatch("workspace.intent.v1", request_value)
        assert "error" not in output, output
        assert output["workspace"] == workspace and output["observation"] == observation
        assert output["projection"]["history"] == workspace["base"]["history"]
        vectors.append((workspace, observation, output["projection"]["canonicalTimer"]))
    return vectors


if __name__ == "__main__":
    main()
