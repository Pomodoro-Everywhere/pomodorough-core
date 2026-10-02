#!/usr/bin/env python3
"""Execute unchanged production lifecycle methods against native Core vectors.

SQLite queue orchestration uses a real in-memory transaction. Swift and Kotlin
methods are compiled with deterministic persistence and clock adapters. Legacy
completion policy calls go to native timer.completionPlan.v1, independently of
the new staged operation. This does not build or load a new WASM artifact.
"""
import copy
from datetime import datetime
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import types

import completion_mutation_source_probe as source

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT.parent
CASES = json.loads((ROOT / "fixtures/completion-lifecycle-v1.json").read_text())
DESKTOP = SUITE / "desktop/src/pomodorough/storage.py"


def clock(value, at):
    value["clock"] = dict(occurredAt=at, observedAt=at, physicalNow=at)
    ms = int(datetime.fromisoformat(at.replace("Z", "+00:00")).timestamp() * 1000)
    stamp = f"{ms:012x}"
    value["identities"]["commandUuids"] = [f"{stamp[:8]}-{stamp[8:]}-7000-8000-000000000010"]


def expiry(profile, prior=0, owner="device-local", explicit=False, at="2026-07-20T12:00:55.000Z"):
    value = source.request(profile, "running")
    value.update(stage="expiryObservation", replicationMode="iroh", event=CASES["event"],
                 lifecycle=copy.deepcopy(CASES["lifecycle"]), centralizedSession=CASES["centralizedSession"])
    value["workspace"]["base"]["autoStartBreaks"] = True
    value["ownership"] = {"timerId": "existing-timer", "deviceId": owner}
    value["requestedTimer"]["startedByDeviceId"] = owner
    value["workspace"]["base"]["canonicalTimer"] = copy.deepcopy(value["requestedTimer"])
    for index in range(prior):
        value["workspace"]["base"]["history"].append(dict(id=f"past-{index}", timerId=f"past-timer-{index}",
            commandId=f"past-finish-{index}", phase="focus", status="completed", plannedDurationMs=60000,
            completedAt="2026-07-20T11:00:00Z"))
    value["previousWorkspace"] = copy.deepcopy(value["workspace"])
    value["previousObservation"] = copy.deepcopy(value["observation"])
    value["selection"]["explicit"] = explicit
    value["identities"]["timerUuid"] = "12345678-1234-4234-8234-123456789012"
    clock(value, at)
    return value


def legacy_expiry(value):
    raw = value["workspace"]
    projected = source.dispatch("projection.apply.v2", dict(base=raw["base"], pending=raw["local"],
        now=value["clock"]["observedAt"]))
    bounds = value["calendarIntervals"][0]
    owner = value["ownership"]
    ownership = {"timerId": owner["timerId"], "ownerDeviceId": owner["deviceId"]}
    if value["compatibility"] == "androidCoordinator" and owner["deviceId"] != value["allocation"]["deviceId"]:
        ownership = None
    plan = source.dispatch("timer.completionPlan.v1", dict(kind="expiry", beforeTimer=value["requestedTimer"],
        projectedTimer=projected["canonicalTimer"], history=projected["history"],
        selectedPhase=value["selection"]["phase"], autoStartBreaks=projected["autoStartBreaks"],
        localDeviceId=value["allocation"]["deviceId"],
        ownership=ownership,
        dayStart=bounds["start"], dayEnd=bounds["end"]))
    return projected, plan


def vectors(profile):
    for prior in (2, 3, 6, 7):
        for owner in ("device-local", "foreign"):
            for case in CASES["deadlineCases"]:
                yield expiry(profile, prior, owner, at=case["observedAt"])


def compile_swift(temporary, program, name):
    path = temporary / f"{name}.swift"
    path.write_text(program)
    run = subprocess.run(["swift", str(path)], text=True, capture_output=True)
    assert run.returncode == 0, run.stderr
    return run.stdout.splitlines()


def compile_kotlin(temporary, program):
    path = temporary / "expiry.kt"
    path.write_text(program)
    jar = temporary / "expiry.jar"
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    java = Path(os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    run = subprocess.run([compiler, str(path), "-include-runtime", "-d", str(jar)],
                         env=dict(os.environ, JAVA_HOME=str(java)), text=True, capture_output=True)
    assert run.returncode == 0, run.stderr
    return subprocess.run([str(java / "bin/java"), "-jar", str(jar)],
                          text=True, capture_output=True, check=True).stdout.splitlines()


def apple_start_admission(temporary):
    method = source.between(source.SOURCES["apple"], "    func prepareIrohAutomaticBreak(", "    func completionDecision(")
    program = '''import Foundation
typealias TimerPhase = String
struct PersistedTimerState {}
struct AutomaticBreak {let timerID:String;let phase:String;let duration:Double}
struct Command {let id:String}
struct CommandRequest {let type:String;let timerID:String;let taskID:String?;let phase:String;let duration:Double;let elapsed:Double;let occurredAt:Date;let localDate:Date}
struct Transition {let state:PersistedTimerState;let command:Command}
struct AutomaticBreakTransition {let state:PersistedTimerState;let automaticBreak:AutomaticBreak;let command:Command}
struct Probe {
 let commandID:String
 func makeAutomaticBreak(phase:String,state:PersistedTimerState)->AutomaticBreak {
  AutomaticBreak(timerID:"timer-12345678-1234-4234-8234-123456789012",phase:phase,duration:120)
 }
 func makeCommand(_ request:CommandRequest,state:PersistedTimerState) throws -> Transition {
  precondition(request.type=="start" && request.taskID==nil && request.elapsed==0)
  return Transition(state:state,command:Command(id:commandID))
 }
''' + method.replace("type: .start", 'type: "start"') + '\n}\n'
    expected = []
    for delta, at in ((-1, "2026-07-20T12:00:54.999Z"), (0, "2026-07-20T12:00:55.000Z"), (1, "2026-07-20T12:00:55.001Z")):
        value = expiry("appleWorkspace")
        clock(value, at)
        value["clock"].update(physicalNow="2026-07-20T12:00:55.001Z", observedAt="2026-07-20T12:00:55.000Z")
        result = source.core(value)
        command_id = result["commands"][0]["id"] if result["commands"] else "unused"
        program += f'''let p{len(expected)}=try Probe(commandID:{json.dumps(command_id)}).prepareIrohAutomaticBreak(
 completedAt:Date(timeIntervalSince1970:1784548855),nextPhase:"short_break",occurredAt:Date(timeIntervalSince1970:1784548855 + {delta}.0/1000),localDate:Date(),state:PersistedTimerState())
print(p{len(expected)}?.command.id ?? "none")
'''
        expected.append(command_id if result["commands"] else "none")
    assert compile_swift(temporary, program, "apple_admission") == expected
    return len(expected)


def apple_timer_controller():
    path = source.SOURCES["apple"]
    methods = [source.between(path, start, end) for start, end in (
        ("    func makeCommand(", "    private func validateRetargetRequest("),
        ("    func prepareIrohAutomaticBreak(", "    func completionDecision("),
        ("    private func recordPhaseAdvance(", "    private func loadCore("))]
    models = (ROOT / "scripts/completion_lifecycle_apple_models.swift").read_text()
    return models + '''
final class TimerSessionController {
 let decision:Decision?
 init(decision:Decision? = nil) { self.decision=decision }
 func loadCore() throws -> ProbeClock { ProbeClock() }
 func validateRetargetRequest(_ request:CommandRequest,state:PersistedTimerState) throws {
  throw AppError.invalidLocalClock
 }
 func completionDecision(for timer:CanonicalTimer,at date:Date,state:PersistedTimerState,
  replicationMode:ReplicationMode,physicalNow:Date,autoStartsBreak:Bool) throws -> Decision? { decision }
 func makeAutomaticBreak(phase:String,state:PersistedTimerState)->AutomaticBreak {
  AutomaticBreak(timerID:"timer-12345678-1234-4234-8234-123456789012",phase:phase,
   duration:Double(state.settings.durationsMs[phase]!)/1000)
 }
 func finishSelection(_ phase:String,commandID:String,state:PersistedTimerState)->PersistedTimerState {
  var updated=state
  recordPhaseAdvance(to:phase,afterFinishing:CanonicalTimer(id:"existing-timer"),
   commandID:commandID,replicationMode:.iroh,in:&updated)
  return updated
 }
''' + '\n'.join(methods) + '\n}\n'


def apple_state(value, command_uuids=None):
    selection = value["selection"]
    durations = value["workspace"]["base"]["durationsMs"]
    duration_source = ",".join(f'{json.dumps(phase)}:{duration}' for phase, duration in durations.items())
    uuids = command_uuids if command_uuids is not None else value["identities"]["commandUuids"]
    candidates = ",".join(f'UUID(uuidString:{json.dumps(uuid)})!' for uuid in uuids)
    return f'PersistedTimerState(settings:Settings(selectedPhase:{json.dumps(selection["phase"])},durationsMs:[{duration_source}]),selectedPhaseGeneration:{selection["generation"]},hasExplicitPhaseSelection:{str(selection["explicit"]).lower()},commandUuids:[{candidates}])'


def swift_optional(value):
    return json.dumps(value) if value is not None else "nil"


def epoch(stamp):
    return datetime.fromisoformat(stamp.replace("Z", "+00:00")).timestamp()


def apple_expiry(temporary):
    method = source.between(SUITE / "apple/Sources/AlarmEffectCoordinator.swift", "    func irohCompletionPlan(", "\n}")
    program = apple_timer_controller() + 'struct Probe { let timerSessionController:TimerSessionController\n' + method + '\n}\n'
    expected = []
    cases = list(vectors("appleWorkspace"))
    for phase, explicit in (("focus", True), ("long_break", True), ("long_break", False), ("short_break", True)):
        value = expiry("appleWorkspace", explicit=explicit)
        value["selection"]["phase"] = phase
        cases.append(value)
    for index, value in enumerate(cases):
        projected, legacy = legacy_expiry(value)
        result = source.core(value)
        completed = epoch(projected["canonicalTimer"]["anchorAt"])
        decision = "nil" if not legacy["expired"] else f'Decision(completedAt:Date(timeIntervalSince1970:{completed}),selectedPhase:{swift_optional(legacy["selectedPhase"])},generatedBreakPhase:{swift_optional(legacy["generatedBreakPhase"])})'
        program += f'''let state{index}={apple_state(value)}
let controller{index}=TimerSessionController(decision:{decision})
if let p = try Probe(timerSessionController:controller{index}).irohCompletionPlan(
 timer:CanonicalTimer(id:"existing-timer"),at:Date(),state:state{index},replicationMode:.iroh,physicalNow:Date(),autoStartsBreak:true) {{
 switch p {{case .persist(let s,_):try emit("persist",state:s)
 case .automaticBreak(let s,_,let at,let phase):
  let started = try controller{index}.prepareIrohAutomaticBreak(completedAt:at!,nextPhase:phase,
   occurredAt:Date(timeIntervalSince1970:{epoch(value["clock"]["occurredAt"])}),
   localDate:Date(timeIntervalSince1970:{epoch(value["clock"]["physicalNow"])}),state:s)!
  try emit("start",phase:started.command.phase,state:started.state)}}
 }} else {{ try emit("noop",state:state{index}) }}
'''
        outcome = "noop" if not legacy["expired"] else "start" if result["commands"] else "persist"
        expected.append(dict(kind=outcome, phase=result["commands"][0]["phase"] if result["commands"] else None,
            selection=result["selection"]))
    actual = [json.loads(line) for line in compile_swift(temporary, program, "apple_expiry")]
    for index, (observed, returned) in enumerate(zip(actual, expected, strict=True)):
        assert observed == returned, f"Apple expiry case {index}: source={observed}, Core={returned}"
    return len(expected)


def apple_manual_selection(temporary):
    program = apple_timer_controller()
    expected = []
    for index, (phase, explicit) in enumerate((("long_break", True), ("focus", True), ("focus", False), ("long_break", False))):
        value = source.request("appleWorkspace", "running")
        value.update(replicationMode="iroh", ownership={"timerId": "existing-timer", "deviceId": "device-local"})
        value["workspace"]["base"]["autoStartBreaks"] = True
        value["identities"]["timerUuid"] = "12345678-1234-4234-8234-123456789012"
        value["selection"].update(phase=phase, explicit=explicit)
        first = source.core(value)
        continuation = copy.deepcopy(value)
        continuation.pop("requestedTimer")
        for field in ("workspace", "allocation", "observation", "selection", "lifecycle"):
            continuation[field] = first[field]
        continuation.update(stage="deferredBreakOpportunity", previousWorkspace=first["workspace"],
            previousObservation=first["observation"], event={"kind": "opportunity"}, centralizedSession=CASES["centralizedSession"])
        clock(continuation, "2026-07-20T12:01:00.000Z")
        second = source.core(continuation)
        persisted = json.loads(json.dumps(second))
        restarted = copy.deepcopy(continuation)
        for field in ("workspace", "allocation", "observation", "selection", "lifecycle"):
            restarted[field] = persisted[field]
        restarted["identities"]["commandUuids"] = []
        retry = source.core(restarted)
        candidates = value["identities"]["commandUuids"][:1] + continuation["identities"]["commandUuids"]
        program += f'''let controllerM{index}=TimerSessionController()
let initialM{index}={apple_state(value, candidates)}
let finishM{index}=try controllerM{index}.makeCommand(CommandRequest(type:.finish,timerID:"existing-timer",taskID:nil,
 phase:"focus",duration:60,elapsed:15,occurredAt:Date(timeIntervalSince1970:1784548810),localDate:Date()),state:initialM{index})
let firstM{index}=controllerM{index}.finishSelection("short_break",commandID:finishM{index}.command.id,state:finishM{index}.state)
try emit("finish",phase:finishM{index}.command.phase,state:firstM{index})
let startM{index}=try controllerM{index}.makeCommand(CommandRequest(type:.start,timerID:"timer-12345678-1234-4234-8234-123456789012",taskID:nil,
 phase:"short_break",duration:120,elapsed:0,occurredAt:Date(timeIntervalSince1970:1784548860),localDate:Date()),state:firstM{index})
try emit("start",phase:startM{index}.command.phase,state:startM{index}.state)
try emit("restart",state:restored(startM{index}.state))
'''
        expected.extend((dict(kind="finish", phase=first["commands"][0]["phase"], selection=first["selection"]),
            dict(kind="start", phase=second["commands"][0]["phase"], selection=second["selection"]),
            dict(kind="restart", phase=None, selection=retry["selection"])))
    actual = [json.loads(line) for line in compile_swift(temporary, program, "apple_manual_selection")]
    for index, (observed, returned) in enumerate(zip(actual, expected, strict=True)):
        assert observed == returned, f"Apple manual selection step {index}: source={observed}, Core={returned}"
    return len(expected)


def android_expiry_adapter(method):
    return '''import kotlin.coroutines.*
import java.time.Instant
data class CanonicalTimer(val id:String)
data class Projection(val timer:CanonicalTimer?,val history:List<String> = emptyList())
data class Settings(val selectedPhase:String="focus",val autoStartBreaks:Boolean=true)
data class Local(val deviceId:String="device-local",val ownedTimerId:String?="existing-timer")
data class Decision(val expired:Boolean,val generatedBreakPhase:String?)
data class CoreExpiryInput(val beforeTimer:CanonicalTimer,val projectedTimer:CanonicalTimer?,val history:List<String>,
 val selectedPhase:String,val autoStartBreaks:Boolean,val localDeviceId:String,val ownedTimerId:String?,val reference:Instant,val zoneId:java.time.ZoneId)
class Completion(val decision:Decision) { fun expiry(input:CoreExpiryInput)=decision }
class Mutex { suspend fun <T> withLock(block:()->T):T=block() }
enum class ReplicationMode { IROH }
enum class CommandType { Start }
class Replication { suspend fun afterLocalMutation() {} }
class CancellationException:Exception()
object CrashReporter { fun report(error:Exception) { throw error } }
object R { object string { const val iroh_room_projection_could_not_be_refreshed=0 } }
class Context { fun getString(value:Int)="error" }
class Probe(val coreCompletion:Completion,owned:Boolean) {
 val replication:Replication?=Replication();val actionMutex=Mutex();var settings=Settings();val local=Local(ownedTimerId=if(owned) "existing-timer" else null)
 var generation=5L
 val projection=Projection(CanonicalTimer("existing-timer"));val appContext=Context();var conflict:String?=null
 var phase:String?=null
 suspend fun reloadWorkspace(mode:ReplicationMode) {}
 fun localWorkspaceAdmissionBlocked()=false
 fun currentTimeMillis()=1784548855000L
 fun commitTimerCommand(type:CommandType,phase:String):Boolean {
  this.phase=phase;if(settings.selectedPhase!=phase)generation+=1
  settings=settings.copy(selectedPhase=phase);return true
 }
 suspend fun afterLocalMutation() {}
 fun publish() {}
''' + method + '\nsuspend fun call()=finishExpiredIrohTimer(CanonicalTimer("existing-timer"))\n}\n'


def android_expiry(temporary):
    method = source.between(source.SOURCES["android_repository"], "    private suspend fun finishExpiredIrohTimer(", "    suspend fun rescheduleAlarmFromLocal(")
    program = android_expiry_adapter(method)
    program += 'fun main() {\n'
    expected = []
    for index, value in enumerate(vectors("androidCoordinator")):
        _, legacy = legacy_expiry(value)
        result = source.core(value)
        phase = json.dumps(legacy["generatedBreakPhase"]) if legacy["generatedBreakPhase"] else "null"
        owned = value["ownership"]["deviceId"] == value["allocation"]["deviceId"]
        program += f'''val p{index}=Probe(Completion(Decision({str(legacy["expired"]).lower()},{phase})),{str(owned).lower()})
 suspend {{ p{index}.call() }}.startCoroutine(object:Continuation<Boolean>{{
 override val context=EmptyCoroutineContext
 override fun resumeWith(result:Result<Boolean>){{println(result.getOrThrow().toString()+"|"+(p{index}.phase?:"none")+"|"+p{index}.settings.selectedPhase+"|"+p{index}.generation)}}
 }})
'''
        expected.append(f'{str(legacy["expired"]).lower()}|{result["commands"][0]["phase"] if result["commands"] else "none"}|{result["selection"]["phase"]}|{result["selection"]["generation"]}')
    assert compile_kotlin(temporary, program + "}\n") == expected
    return len(expected)


def desktop_expiry():
    path = SUITE / "desktop/src/pomodorough/storage_replication_projection.py"
    namespace = {"Any": object, "TimerCompletionPlanV1": object, "SharedCoreOperationError": ValueError,
        "_default_shared_core": lambda: None,
        "plan_timer_completion_v1": lambda _, value: decoded(source.dispatch("timer.completionPlan.v1", value))}
    plan = source.method(path, "plan", namespace)
    owner = source.method(path, "_ownership", namespace)
    probe = types.SimpleNamespace(_shared_core=lambda: None, _ownership=owner,
        _day_bounds=lambda _: ("2026-07-20T00:00:00Z", "2026-07-21T00:00:00Z"))
    count = 0
    for value in vectors("desktopStorage"):
        projected, _ = legacy_expiry(value)
        legacy = plan(probe, value["requestedTimer"], projected, {"selectedPhase": "focus"}, "device-local")
        result = source.core(value)
        assert bool(result["commands"]) == bool(legacy.generated_break_phase)
        if result["commands"]:
            assert result["commands"][0]["phase"] == legacy.generated_break_phase
        if legacy.selected_phase:
            assert result["selection"]["phase"] == legacy.selected_phase
        count += 1
    return count


def decoded(plan):
    return types.SimpleNamespace(**{
        "selected_phase": plan["selectedPhase"], "generated_phase": plan["generatedBreakPhase"],
        "generated_break_phase": plan["generatedBreakPhase"], "expired": plan["expired"],
        "generated_break_eligible": plan["generatedBreakEligible"], "source_already_accepted": plan["sourceAlreadyAccepted"]})


def deferred_request(prior=0):
    value = expiry("desktopStorage", prior)
    for field in ("previousWorkspace", "previousObservation", "event", "centralizedSession"):
        value.pop(field)
    value.update(stage="finishCommit", replicationMode="centralized")
    first = source.core(value)
    value.pop("requestedTimer")
    for field in ("workspace", "allocation", "observation", "selection", "lifecycle"):
        value[field] = copy.deepcopy(first[field])
    value.update(stage="deferredBreakOpportunity", event={"kind": "opportunity"}, centralizedSession=copy.deepcopy(CASES["centralizedSession"]))
    clock(value, "2026-07-20T12:01:00.000Z")
    return value, first


def methods(probe, names, namespace):
    for name in names:
        function = source.method(DESKTOP, name, namespace)
        setattr(probe, name, function if name in ("_auto_break_has_later_command", "_completion_source_timestamp") else types.MethodType(function, probe))


def sqlite_probe(value):
    connection = sqlite3.connect(":memory:")
    connection.row_factory = sqlite3.Row
    connection.executescript("CREATE TABLE pending_auto_breaks(finish_command_id TEXT,timer_id TEXT,finish_device_sequence INTEGER);"
        "CREATE TABLE pending_commands(id TEXT,device_sequence INTEGER,payload TEXT,depends_on_command_id TEXT);"
        "CREATE TABLE pending_auto_break_starts(source_finish_command_id TEXT,source_timer_id TEXT,start_command_id TEXT,selected_phase_version INTEGER);")
    for trigger in value["lifecycle"]["pendingBreaks"]:
        connection.execute("INSERT INTO pending_auto_breaks VALUES(?,?,?)", tuple(trigger[key] for key in ("finishCommandId", "timerId", "finishDeviceSequence")))
    for command in value["workspace"]["local"]["commands"]:
        connection.execute("INSERT INTO pending_commands VALUES(?,?,?,NULL)", (command["id"], command["deviceSequence"], json.dumps(command)))
    namespace = {"Any": object, "json": json, "utc_timestamp": lambda ms: datetime.fromtimestamp(ms / 1000, __import__("datetime").timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z"),
        "_AutoBreakTrigger": lambda *args: types.SimpleNamespace(finish_command_id=args[0], timer_id=args[1], finish_sequence=args[2]),
        "_AutoBreakContext": lambda settings, phase, accepted: types.SimpleNamespace(settings=settings, generated_phase=phase, source_already_accepted=accepted)}
    probe = types.SimpleNamespace(connection=connection)
    methods(probe, ("_process_auto_break_queue", "_next_auto_break_trigger", "_pending_commands_for_auto_break",
        "_auto_break_has_later_command", "_discard_auto_break", "_auto_break_context", "_completion_source_timestamp", "_queue_auto_break_start"), namespace)
    wire_adapters(probe, value)
    return probe


def wire_adapters(probe, value):
    base = value["workspace"]["base"]
    settings = dict(selectedPhase=value["selection"]["phase"], durationsMs=copy.deepcopy(base["durationsMs"]))
    meta = dict(settings=settings, snapshot=base, selectedPhaseVersion=int(value["selection"]["generation"]))
    probe.get_meta = lambda key, default: meta.get(key, default)
    probe._set_meta = lambda key, item: meta.update({key: item})
    probe._normalize_settings = copy.deepcopy
    probe._physical_pending_commands = lambda commands: commands
    probe._pending_auto_start_operations = lambda: value["workspace"]["local"]["autoStartOperations"]
    probe.load = lambda **_: dict(snapshot=base)
    probe._project_operation = lambda settings, **options: project_for_source(value, options)
    probe._completion_policy = types.SimpleNamespace(generated_break=lambda identity, canonical, optimistic, pending, required, at:
        decoded(source.dispatch("timer.completionPlan.v1", dict(kind="generatedBreak", source=identity,
            canonical={key: canonical[key] for key in ("canonicalTimer", "history")},
            optimistic=dict(canonicalTimer=optimistic.canonical_timer, history=optimistic.history),
            sourceFinishPending=pending, requireCanonical=required,
            dayStart=value["calendarIntervals"][0]["start"], dayEnd=value["calendarIntervals"][0]["end"]))))
    probe._reserve_generation = lambda *_, **__: generation(value)
    probe._reserve_uuid7_ids = lambda *_: value["identities"]["commandUuids"]
    probe._queue_command = lambda *args, **options: queue_source_start(probe, value, args, options)


def generation(value):
    ms = int(datetime.fromisoformat(value["clock"]["occurredAt"].replace("Z", "+00:00")).timestamp() * 1000)
    tick = source.dispatch("hlc.tick.v1", dict(local=value["allocation"]["hlc"], physicalNowMs=ms))
    return ms, [value["allocation"]["deviceSequence"] + 1], [(tick["wallMs"], tick["counter"])]


def project_for_source(value, options):
    base = copy.deepcopy(value["workspace"]["base"])
    if any(row["timerId"] == (base["canonicalTimer"] or {}).get("id") for row in base["history"]):
        base["canonicalTimer"] = None
    output = source.dispatch("projection.apply.v2", dict(base=base, pending=value["workspace"]["local"], now=options["now"]))
    return types.SimpleNamespace(canonical_timer=output["canonicalTimer"], history=output["history"])


def queue_source_start(probe, value, args, options):
    namespace = {"Any": object, "COMMAND_TYPES": {"start"}, "PHASES": {"focus", "short_break", "long_break"},
        "DURATION_MIN_MS": 60000, "PREFERENCE_DURATION_MAX_MS": 14400000,
        "uuid": types.SimpleNamespace(uuid4=lambda: value["identities"]["timerUuid"]),
        "utc_timestamp": lambda ms: datetime.fromtimestamp(ms / 1000, __import__("datetime").timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")}
    prepare = source.method(DESKTOP, "_prepare_timer_command", namespace)
    probe._duration_ms = source.method(DESKTOP, "_duration_ms", namespace)
    command, dependency = prepare(probe, *args[:5], options["trusted_ms"], options["trusted_ms"],
        options["sequence"], options["clock"], options["command_id"], args[6] if len(args) > 6 else None)
    probe.connection.execute("INSERT INTO pending_commands VALUES(?,?,?,?)", (command["id"], command["deviceSequence"], json.dumps(command), dependency))
    return command


def desktop_deferred():
    count = 0
    for prior in (2, 3, 6, 7):
        for accepted in (False, True):
            for required in (False, True):
                value, first = deferred_request(prior)
                if accepted:
                    value["workspace"]["base"].update(canonicalTimer=first["projection"]["canonicalTimer"], history=first["projection"]["history"])
                    value["workspace"]["local"]["commands"] = []
                    value["workspace"]["neverSent"]["commands"] = []
                    value["observation"]["commandTimes"] = {}
                value["centralizedSession"] = dict(userId="user" if required else None, authenticated=required)
                expected = source.core(value)
                probe = sqlite_probe(value)
                with probe.connection:
                    actual = probe._process_auto_break_queue(required, generation(value)[0], False)
                assert actual == expected["durableCommands"], (actual, expected)
                rows = probe.connection.execute("SELECT finish_command_id FROM pending_auto_breaks").fetchall()
                assert [row[0] for row in rows] == [row["finishCommandId"] for row in expected["lifecycle"]["pendingBreaks"]]
                provisional = probe.connection.execute("SELECT * FROM pending_auto_break_starts").fetchall()
                assert bool(provisional) == bool(expected["completionRecords"]["provisionalBreak"])
                probe.connection.close()
                count += 1
    return count


def apple_manual_adapter(method):
    return '''struct CanonicalTimer { let id:String }
struct Command { let id:String }
struct History { let commandId:String;let status:String }
struct State {}
enum CommandType { case start }
enum TimerSessionController {
 struct AutomaticBreak { let timerID:String;let phase:String;let duration:Double }
 struct FinishTransition { let state:State;let command:Command }
}
class Probe {
 let saveFinish:Bool;let saveStart:Bool
 var trace:[String]=[];var history:[History]=[]
 init(_ finish:Bool,_ start:Bool){saveFinish=finish;saveStart=start}
 func commitSynchronizedState(_ state:State,requiringTimerCommandIDs ids:[String])->Bool {
  trace.append("finish:"+ids[0]);if saveFinish {history=[History(commandId:ids[0],status:"completed")]};return saveFinish
 }
 func enqueue(_ type:CommandType,timerID:String,taskID:String?,phase:String,duration:Double,elapsed:Double)->Bool {
  precondition(taskID==nil && elapsed==0);trace.append("start:"+timerID+":"+phase+":"+String(Int(duration*1000)));return saveStart
 }
 func scheduleAutomaticBreak(_ next:TimerSessionController.AutomaticBreak,after timer:CanonicalTimer,cancelsAlarm:Bool){trace.append("alarm")}
''' + method + '''
 func call(_ next:TimerSessionController.AutomaticBreak,_ finish:String)->Bool {
  startIrohBreak(next,after:CanonicalTimer(id:"existing-timer"),preparation:TimerSessionController.FinishTransition(state:State(),command:Command(id:finish)),cancelsAlarm:true)
 }
}
'''


def apple_manual_boundaries(temporary):
    method = source.between(SUITE / "apple/Sources/AppModel.swift", "    private func startIrohBreak(", "    private func startCentralizedBreak(")
    program = apple_manual_adapter(method)
    value = source.request("appleWorkspace", "running")
    value["replicationMode"] = "iroh"
    value["workspace"]["base"]["autoStartBreaks"] = True
    value["ownership"] = {"timerId": "existing-timer", "deviceId": "device-local"}
    value["identities"]["timerUuid"] = "12345678-1234-4234-8234-123456789012"
    first = source.core(value)
    continuation = copy.deepcopy(value)
    continuation.pop("requestedTimer")
    for field in ("workspace", "allocation", "observation", "selection", "lifecycle"):
        continuation[field] = first[field]
    continuation.update(stage="deferredBreakOpportunity", previousWorkspace=first["workspace"],
        previousObservation=first["observation"], event={"kind": "opportunity"}, centralizedSession=CASES["centralizedSession"])
    clock(continuation, "2026-07-20T12:01:00.000Z")
    second = source.core(continuation)
    timer_id = second["commands"][0]["timerId"]
    finish_id = first["commands"][0]["id"]
    expected = []
    for save_finish, save_start in ((False, True), (True, False), (True, True)):
        program += f'''let p{len(expected)}=Probe({str(save_finish).lower()},{str(save_start).lower()})
print(p{len(expected)}.call(TimerSessionController.AutomaticBreak(timerID:{json.dumps(timer_id)},phase:"short_break",duration:120),{json.dumps(finish_id)}),p{len(expected)}.trace.joined(separator:"|"))
'''
        trace = [f"finish:{finish_id}"]
        if save_finish:
            trace.append(f"start:{timer_id}:short_break:120000")
        if save_finish and save_start:
            trace.append("alarm")
        expected.append(f'{str(save_finish and save_start).lower()} {"|".join(trace)}')
    assert compile_swift(temporary, program, "apple_manual") == expected
    return len(expected)


def desktop_acknowledgements():
    path = SUITE / "desktop/src/pomodorough/storage_canonical_acknowledgements.py"
    namespace = {"Any": object, "sqlite3": sqlite3}
    names = ("_reconcile_unmaterialized_auto_break_triggers", "_unmaterialized_auto_break_rows", "_auto_break_trigger_accepted")
    functions = {name: source.method(path, name, namespace) for name in names}
    count = 0
    for outcome in ("applied", "ignored", "rejected"):
        for exact in (False, True):
            for discarded in (False, True):
                value, first = deferred_request()
                value["workspace"]["base"].update(canonicalTimer=first["projection"]["canonicalTimer"],
                    history=first["projection"]["history"] if exact else [])
                value["workspace"]["local"]["commands"] = []
                value["workspace"]["neverSent"]["commands"] = []
                value["observation"]["commandTimes"] = {}
                finish_id = first["commands"][0]["id"]
                event = dict(kind="canonicalInstalled", acknowledgements=[dict(commandId=finish_id, outcome=outcome)],
                    discardedCommandIds=[finish_id] if discarded else [])
                value["event"] = event
                expected = source.core(value)
                probe = sqlite_probe(value)
                installation = types.SimpleNamespace(_dependencies=types.SimpleNamespace(connection=probe.connection))
                for name, function in functions.items():
                    setattr(installation, name, function if name == "_auto_break_trigger_accepted" else types.MethodType(function, installation))
                canonical = dict(value["workspace"]["base"], acknowledgements=event["acknowledgements"])
                with probe.connection:
                    installation._reconcile_unmaterialized_auto_break_triggers(canonical, set(event["discardedCommandIds"]))
                    actual = probe._process_auto_break_queue(False, generation(value)[0], False)
                assert actual == expected["durableCommands"], (event, exact, actual, expected)
                assert probe.connection.execute("SELECT count(*) FROM pending_auto_breaks").fetchone()[0] == 0
                probe.connection.close()
                count += 1
    return count


def main():
    with tempfile.TemporaryDirectory(prefix="completion-lifecycle-") as directory:
        temporary = Path(directory)
        results = dict(appleExpiry=apple_expiry(temporary), androidExpiry=android_expiry(temporary),
                       desktopExpiry=desktop_expiry(), desktopSQLite=desktop_deferred(),
                       appleManualBoundaries=apple_manual_boundaries(temporary),
                       desktopAcknowledgements=desktop_acknowledgements(),
                        appleStartAdmission=apple_start_admission(temporary),
                        appleManualSelection=apple_manual_selection(temporary))
    print(json.dumps(results, sort_keys=True))


if __name__ == "__main__":
    main()
