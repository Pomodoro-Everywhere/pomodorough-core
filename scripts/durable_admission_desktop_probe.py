#!/usr/bin/env python3
"""Compare complete real SQLite queue returns with durable Core admission.

Run with desktop/.venv/bin/python. CORE_PROBE selects the baseline or fixed
native dispatcher. Each case claims an outgoing payload, loses its response,
closes SQLite, reopens it, then invokes an unchanged production Store method.
"""
import argparse
from contextlib import ExitStack
from copy import deepcopy
import json
import os
from pathlib import Path
import tempfile
from unittest.mock import patch

import desktop_known_task_source_probe as source


def controlled():
    stack = ExitStack()
    stack.enter_context(patch("pomodorough.storage.time.time", return_value=source.NOW / 1000))
    stack.enter_context(patch("pomodorough.storage.uuid.uuid4", return_value="12345678-1234-4234-8234-123456789012"))
    stack.enter_context(patch("pomodorough.storage.reserve_uuid7", side_effect=lambda wall, count, previous:
        source.uuid7.reserve_uuid7(wall, count, previous, entropy=lambda size: (1).to_bytes(size, "big"))))
    return stack


def request(store, intent):
    state = store.load(projection=True)
    value = deepcopy(source.FIXTURE["request"])
    queues = {domain: [store._with_device_id(item) for item in state[field]]
              for domain, field in source.QUEUES.items()}
    metadata = source.delivery_metadata(store, queues)
    value.update(compatibility="desktopStorage", intent=intent)
    value["workspace"].update(base={key: deepcopy(state["snapshot"][key]) for key in value["workspace"]["base"]},
        local=queues, neverSent=metadata["proof"], canonicalHead=metadata["head"], timerDependencies=metadata["dependencies"])
    value["allocation"] = source.allocation(store)
    first = source.uuid7.uuid7_parts(value["allocation"]["lastUuid"])[1] + 1 if value["allocation"]["lastUuid"] else 1
    value["identities"]["commandUuids"] = [source.uuid7.uuid7_from_parts(source.NOW, first + index) for index in range(3)]
    raw_times = store.get_meta("commandPhysicalTimes")
    value["observation"] = {"canonicalAnchorAt": None,
        "commandTimes": {key: source.utc_timestamp(ms) for key, ms in raw_times.items()}}
    assert {key: source.parse_timestamp_ms(at) for key, at in value["observation"]["commandTimes"].items()} == raw_times
    value["clock"] = {key: source.utc_timestamp(source.NOW) for key in ("occurredAt", "physicalNow", "observedAt")}
    if intent["kind"] in ("upsertTask", "addAndSelectTask", "deleteTask", "selectTask", "setDuration", "setAutoStart"):
        raw_owner = state["snapshot"].get("user")
        owner = raw_owner.get("id") if isinstance(raw_owner, dict) else None
        value.update(ownership={"ownerId": owner, "expectedOwnerId": owner},
            durability={"outgoingDurationOperationIds": [op["id"] for op in (metadata["outgoing"] or {}).get("durationOperations", [])]})
        if intent["kind"] == "deleteTask":
            value["knownTasks"] = deepcopy(state["snapshot"]["knownTasks"])
    else:
        value["identities"]["commandUuids"] = value["identities"]["commandUuids"][:2]
    return value, state, metadata, raw_times


def action(store, intent):
    kind = intent["kind"]
    state = store.load(projection=True)
    projected = store.projected_state(now_ms=source.NOW, state=state)
    settings = store.projected_settings(state, projected)
    if kind == "deleteTask":
        outcome, calls, _, commits = source.interact(store, state, intent["taskId"])
        return {"outcome": source.asdict(outcome), "calls": calls, "commits": commits,
                "effects": [type(effect).__name__ for effect in outcome.effects]}
    if kind == "addAndSelectTask":
        instance, _ = source.controller(store, state)
        calls = []
        with ExitStack() as stack:
            for method in ("queue_task_operation", "set_selected_task_id", "_insert_retarget_command"):
                stack.enter_context(patch.object(store, method, source.record_call(getattr(store, method), calls, method)))
            outcome = instance.add_task(intent["title"])
        return {"outcome": source.asdict(outcome), "calls": calls,
                "effects": [type(effect).__name__ for effect in outcome.effects]}
    if kind == "upsertTask":
        task = source.CASES["knownTasks"][0]
        return store.queue_task_operation("upsert" if kind == "upsertTask" else "delete", task, now_ms=source.NOW)
    if kind == "selectTask":
        return store.set_selected_task_id(intent["taskId"], now_ms=source.NOW)
    if kind == "setDuration":
        return store.queue_duration_operation(intent["phase"], intent["minutes"] * 60000, now_ms=source.NOW)
    if kind == "setAutoStart":
        return store.set_auto_start_breaks(intent["enabled"], now_ms=source.NOW)
    if kind in ("restart", "cancelAndClear"):
        method = store.queue_restart if kind == "restart" else store.queue_cancel_and_clear
        return method(projected.canonical_timer, settings["selectedPhase"], settings["durationsMs"], now_ms=source.NOW)
    return store.queue_command(kind, projected.canonical_timer, settings["selectedPhase"], settings["durationsMs"], now_ms=source.NOW)


def prepare(path, native, intent, null_head, pending_source=None):
    store = source.Store(path, shared_core=native)
    case = deepcopy(source.CASES["cases"][3])
    if intent["kind"] == "resume":
        case["status"] = "paused"
    elif intent["kind"] in ("clear", "restart"):
        case["status"] = "completed"
    elif intent["kind"] in ("start", "setDuration") or pending_source:
        case["status"] = "idle"
    if pending_source == "task":
        case["current"] = case["selected"] = False
    source.seed(store, case)
    with controlled():
        if pending_source == "task":
            action(store, {"kind": "upsertTask", "title": source.CASES["knownTasks"][0]["title"]})
        elif pending_source == "start":
            action(store, {"kind": "start"})
        elif intent["kind"] in ("clear", "restart"):
            action(store, {"kind": "setAutoStart", "enabled": True})
        elif intent["kind"] in ("pause", "resume", "cancel", "finish", "start", "cancelAndClear", "selectTask", "addAndSelectTask"):
            action(store, {"kind": "selectTask", "taskId": None})
        else:
            action(store, intent)
    claimed = store.sync_payload()
    assert any(claimed[domain] for domain in source.QUEUES)
    if null_head:
        store.set_meta("canonicalHead", None)
    rows = source.durable_rows(store)
    store.close()
    store = source.Store(path, shared_core=native)
    assert source.durable_rows(store) == rows and store.sync_payload() == claimed
    return store, rows, claimed


def core_request(value, store, intent):
    if intent["kind"] == "finish":
        value.pop("intent")
        value.update(stage="finishCommit", requestedTimer=store.projected_state(now_ms=source.NOW).canonical_timer,
                     ownership=None)
        value["identities"]["commandUuids"] = value["identities"]["commandUuids"][:1]
        return "workspace.completionMutation.v1"
    if intent["kind"] in ("restart", "cancelAndClear"):
        value["requestedTimer"] = store.projected_state(now_ms=source.NOW).canonical_timer
    return "workspace.intent.v1"


def compare_return(store, intent, returned, calls, planned):
    kind = intent["kind"]
    if kind in ("deleteTask", "addAndSelectTask"):
        assert returned["outcome"] == {"value": None, "effects": ({}, {}, {})}
        assert returned["effects"] == ["LoadState", "Render", "Synchronize"]
        methods = {call["method"]: call["returned"] for call in returned["calls"]}
        for method, domain in [("queue_task_operation", "taskOperations"),
            ("set_selected_task_id", "selectedTaskOperations"), ("_insert_retarget_command", "commands")]:
            assert planned["durableOperations"][domain] == ([methods[method]] if method in methods else [])
        return planned["durableOperations"]
    if kind in ("upsertTask", "deleteTask", "selectTask", "setDuration", "setAutoStart"):
        domain = {"upsertTask": "taskOperations", "deleteTask": "taskOperations", "selectTask": "selectedTaskOperations",
                  "setDuration": "durationOperations", "setAutoStart": "autoStartOperations"}[kind]
        assert planned["durableOperations"][domain] == [returned], (kind, returned, planned)
        if kind == "selectTask":
            assert planned["durableOperations"]["commands"] == calls
        return planned["durableOperations"]
    commands = returned if isinstance(returned, list) else [returned]
    assert commands == planned.get("durableCommands", [{key: item for key, item in command.items() if key != "deviceId"}
                                                        for command in planned["commands"]]), (kind, commands, planned)
    return {domain: commands if domain == "commands" else [] for domain in source.QUEUES}


def verify_metadata(store, before, input_value, planned, created):
    after = store.load()
    compatibility = []
    for domain, field in source.QUEUES.items():
        actual = [store._with_device_id(item) for item in after[field]]
        expected = planned["workspace"]["local"][domain]
        if domain == "durationOperations" and len(actual) < len(expected):
            # SQLite's phase-unique schema replaces claimed duration rows. Core
            # retains them. The immutable outgoing payload is still unchanged.
            assert actual == [store._with_device_id(item) for item in created[domain]]
            compatibility.append("desktopPhaseUniqueDurationQueue")
        else:
            assert actual == expected, (domain, actual, expected)
    assert source.allocation(store) == planned["allocation"]
    assert store.get_meta("commandPhysicalTimes") == {
        key: source.parse_timestamp_ms(at) for key, at in planned["observation"]["commandTimes"].items()}
    assert source.durable_rows(store)["outgoing"] == before["outgoing"]
    assert store.canonical_head() == (None if input_value["workspace"]["canonicalHead"] is None else
        tuple(input_value["workspace"]["canonicalHead"][key] for key in ("wallMs", "counter")))
    assert store.delivery_proof() == {key: planned["workspace"]["neverSent"].get(key, []) for key in source.QUEUES}
    return compatibility


def run_case(directory, native, intent, null_head, pending_source=None):
    name = intent["kind"] + ("NullHead" if null_head else "Claimed") + ("Pending" + pending_source.title() if pending_source else "")
    store, rows, claimed = prepare(directory / (name + ".sqlite3"), native, intent, null_head, pending_source)
    try:
        value, state, metadata, raw_times = request(store, intent)
        operation = core_request(value, store, intent)
        calls = []
        original = store._insert_retarget_command
        with controlled(), patch.object(store, "_insert_retarget_command", side_effect=lambda *args, **kwargs:
            calls.append(original(*args, **kwargs)) or calls[-1]):
            returned = action(store, intent)
        planned = native.dispatch(operation, value)
        assert planned["outcome"] == "planned", (name, planned)
        created = compare_return(store, intent, returned, calls, planned)
        compatibility = verify_metadata(store, rows, value, planned, created)
        assert store.sync_payload() == claimed
        assert planned == native.dispatch(operation, json.loads(json.dumps(value))), "complete restored output"
        outcomes = planned.get("commandOutcomes", planned.get("groupOutcomes"))
        assert outcomes is not None
        return {"case": name, "rawRequest": value, "rawPhysicalTimes": raw_times,
            "metadataBefore": metadata, "rowsBefore": rows, "rowsAfter": source.durable_rows(store),
            "productionReturn": returned, "retargetReturns": calls, "core": planned,
            "productionProjection": source.serialized_projection(store.projected_state(now_ms=source.NOW)),
            "compatibilityRequired": compatibility}
    finally:
        store.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--case")
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    native = source.NativeCore(Path(os.environ.get("CORE_PROBE", source.ROOT / "target/debug/examples/completion_policy_probe")))
    intents = [{"kind": "upsertTask", "title": source.CASES["knownTasks"][0]["title"]},
        {"kind": "addAndSelectTask", "title": "Durable task"},
        {"kind": "deleteTask", "taskId": source.CASES["knownTasks"][0]["id"]},
        {"kind": "selectTask", "taskId": source.CASES["knownTasks"][0]["id"]},
        {"kind": "setDuration", "phase": "short_break", "minutes": 3}, {"kind": "setAutoStart", "enabled": True}]
    intents += [{"kind": kind} for kind in ("start", "pause", "resume", "cancel", "clear", "restart", "cancelAndClear", "finish")]
    results = []
    with tempfile.TemporaryDirectory(prefix="durable-admission-") as directory:
        for null_head in (False, True):
            for intent in intents:
                if args.case is None or args.case == intent["kind"]:
                    results.append(run_case(Path(directory), native, intent, null_head))
        if args.case is None or args.case == "pendingStart":
            results.append(run_case(Path(directory), native, {"kind": "pause"}, True, "start"))
        if args.case is None or args.case == "pendingTask":
            results.append(run_case(Path(directory), native, {"kind": "selectTask", "taskId": source.CASES["knownTasks"][0]["id"]}, False, "task"))
    if args.evidence:
        args.evidence.write_text(json.dumps(results, indent=2) + "\n")
    print(f"{len(results)} real SQLite claim/lost-response/reopen complete queue-return cases passed")


if __name__ == "__main__":
    main()
