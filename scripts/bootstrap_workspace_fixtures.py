"""Raw persisted records for classification and actual production projection probes."""
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "fixtures/bootstrap-workspace-v1.json"
QUEUES = ("commands", "taskOperations", "durationOperations", "autoStartOperations", "selectedTaskOperations")
RUNTIME_TASK = {"id":"3add75a3-a242-8300-8b81-36cdaba27d6a", "title":"Runtime task"}


def queue_operation(fixture, queue):
    if queue == "commands":
        return copy.deepcopy(fixture["command"])
    fields = {"taskOperations": dict(taskId="legacy-task", type="delete"),
              "durationOperations": dict(phase="focus", durationMs=1_500_000),
              "autoStartOperations": dict(enabled=False), "selectedTaskOperations": dict(taskId=None)}
    return {**fixture["operationClock"], **fields[queue]}


def cases():
    fixture = json.loads(FIXTURE.read_text())
    for case in fixture["cases"]:
        value = copy.deepcopy(fixture["request"])
        if case["path"]:
            target = value
            for field in case["path"][:-1]: target = target[field]
            target[case["path"][-1]] = copy.deepcopy(case["value"])
        yield case["name"], value
    for queue in QUEUES:
        for delivery in ("unknown", "never_sent", "covered", "newer", "retired_proof"):
            value = copy.deepcopy(fixture["request"])
            operation = queue_operation(fixture, queue)
            workspace = value["local"]["workspace"]
            workspace["local"][queue] = [operation]
            if delivery == "never_sent": workspace["neverSent"][queue] = [operation["id"]]
            if delivery in ("covered", "newer"):
                workspace["canonicalHead"] = dict(wallMs=operation["hlcWallMs"] - int(delivery == "newer"), counter=0)
            yield f"{queue}/{delivery}", value
    yield from history_state_cases(fixture)
    yield from terminal_cases(fixture)
    value = copy.deepcopy(fixture["request"])
    value["local"]["preferences"].update(durationsMs=None, focusMinutes=30)
    yield "android_minute_fallback", value


def history_state_cases(fixture):
    for mask in range(16):
        value = copy.deepcopy(fixture["request"])
        base = value["local"]["workspace"]["base"]
        for target, field, bit, source in [(base, "history", 1, "history"),
            (value["remote"], "history", 2, "history"), (base, "tasks", 4, "task"),
            (value["remote"], "tasks", 8, "task")]:
            target[field] = [copy.deepcopy(fixture["crossProduct"][source])] if mask & bit else []
        yield f"history_state/{mask}", value


def terminal_cases(fixture):
    for side in ("local", "remote"):
        for status, kind in (("completed", "finish"), ("cancelled", "cancel"), ("superseded", "start")):
            value = copy.deepcopy(fixture["request"])
            timer, history = copy.deepcopy(fixture["timer"]), copy.deepcopy(fixture["history"])
            timer["status"] = history["status"] = status
            timer["lastIntent"]["type"] = kind
            if status != "completed":
                history.pop("completedAt")
                timer["elapsedAtAnchorMs"] = history["elapsedMs"] = 17_000
            if status == "superseded":
                timer["supersededByTimerId"] = history["supersededByTimerId"] = "newer-timer"
                timer["lastIntent"].update(commandId="original-start", occurredAt=timer["startedAt"])
                history["commandId"] = "newer-start"
            target = value["local"]["workspace"]["base"] if side == "local" else value["remote"]
            target.update(canonicalTimer=timer, history=[history])
            yield f"{side}/{status}_pair", value


def horizon_cases():
    shared = json.loads(FIXTURE.read_text())
    horizon = json.loads((ROOT / "fixtures/bootstrap-android-horizon-v1.json").read_text())
    for case in horizon["cases"]:
        value = copy.deepcopy(shared["request"])
        workspace = value["local"]["workspace"]
        workspace["now"] = case["now"]
        if not case.get("noTimer"): workspace["base"]["canonicalTimer"] = copy.deepcopy(horizon["timer"])
        if case.get("terminal"):
            workspace["base"].update(canonicalTimer=copy.deepcopy(shared["timer"]), history=[copy.deepcopy(shared["history"])])
        commands = []
        for index, clock in enumerate(case.get("commands", [])):
            commands.append({**shared["command"], "id":clock["id"], "type":"retarget", "taskId":None,
                "timerId":"missing-timer", "occurredAt":clock["at"], "hlcWallMs":clock["wall"], "deviceSequence":index+1})
            for field in ("type", "timerId"):
                if field in clock: commands[-1][field] = clock[field]
        if case.get("start"): commands.append(copy.deepcopy(shared["command"]))
        workspace["local"]["commands"] = commands
        if case.get("durationQueue"):
            workspace["local"]["durationOperations"] = [{**shared["operationClock"], "phase":"focus", "durationMs":1500000,
                "occurredAt":"2026-09-21T12:02:00Z"}]
        if case.get("remoteTask"): value["remote"]["tasks"] = [copy.deepcopy(RUNTIME_TASK)]
        yield "runtime_horizon/" + case["name"], value


def runtime_cases():
    shared = json.loads(FIXTURE.read_text())
    for queue in QUEUES:
        value = copy.deepcopy(shared["request"])
        operation = queue_operation(shared, queue)
        if queue == "taskOperations": operation.update(taskId=RUNTIME_TASK["id"], type="upsert", title=RUNTIME_TASK["title"])
        value["local"]["workspace"]["local"][queue] = [operation]
        yield "runtime_queue/" + queue, value
    for remote in (False, True):
        for delivery in ("unknown", "never_sent", "covered", "retired_proof"):
            value = copy.deepcopy(shared["request"])
            start = shared["command"]
            finish = {**start, "id":"runtime-finish", "deviceSequence":2, "hlcCounter":1,
                "type":"finish", "observedElapsedMs":60000, "occurredAt":"2026-09-21T12:00:10Z"}
            workspace = value["local"]["workspace"]
            workspace["local"]["commands"] = [copy.deepcopy(start), finish]
            if delivery == "never_sent": workspace["neverSent"] = {"commands":[start["id"], finish["id"]]}
            if delivery == "covered": workspace["canonicalHead"] = {"wallMs":start["hlcWallMs"], "counter":1}
            if remote: value["remote"]["tasks"] = [copy.deepcopy(RUNTIME_TASK)]
            yield f"runtime_finish/{delivery}/remote={remote}", value


def pwa_delivery_cases():
    for name, original in runtime_cases():
        if not name.startswith("runtime_finish/"): continue
        for stored, fresh in ((False, False), (True, False), (False, True)):
            value = copy.deepcopy(original)
            workspace = value["local"]["workspace"]
            value["local"]["projectionPending"] = {queue: [] for queue in QUEUES}
            if stored:
                value["local"]["projectionPending"]["commands"] = copy.deepcopy(workspace["local"]["commands"])
            if fresh:
                workspace["neverSent"]["commands"] = [command["id"] for command in workspace["local"]["commands"]]
            yield f"runtime_pwa/{name}/stored={stored}/fresh={fresh}", value


def expected_legacy_rejections(profile):
    if profile == "appleWorkspace": return set()
    common = {"local_tasks", "local_cancelled_history", "local_superseded_history",
        "local_completed_history", "local_duplicate_history", "local_legacy_unclassified_history",
        "local_null_empty_and_identityless_history", "local_history_identity_namespaces"}
    if profile == "androidRepository":
        common.update({"local_selection", "local_empty_selection", "remote_cancelled_history",
                       "remote_completed_history", "remote_legacy_identityless_history"})
        masks = [mask for mask in range(16) if mask & 7]
    else:
        common.add("local_base_selection")
        masks = [mask for mask in range(16) if mask & 5]
    return common | {f"history_state/{mask}" for mask in masks}
