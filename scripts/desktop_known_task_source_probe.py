#!/usr/bin/env python3
"""Run real Desktop SQLite Store and Qt controller against native intent plans.

Run with Desktop's Python environment. Only time and UUID entropy are controlled.
The injected dispatcher executes production Rust, never a projection stub.
"""

import argparse
from copy import deepcopy
from dataclasses import asdict
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT.parent / "desktop"
sys.path.insert(0, str(DESKTOP / "src"))

from pomodorough import uuid7  # noqa: E402
from pomodorough.core import elapsed_ms  # noqa: E402
from pomodorough.shared_core import SharedCoreOperationError  # noqa: E402
from pomodorough.storage import Store, parse_timestamp_ms, utc_timestamp  # noqa: E402
from pomodorough.timer_interaction_controller import (  # noqa: E402
    TimerInteractionContext, TimerInteractionController, TimerInteractionPorts,
)

FIXTURE = json.loads((ROOT / "fixtures/workspace-intent-v1.json").read_text())
CASES = json.loads((ROOT / "fixtures/workspace-intent-desktop-known-tasks-v1.json").read_text())
NOW = 1784548810000
QUEUES = {"commands": "pending", "taskOperations": "pendingTasks",
          "durationOperations": "pendingDurations", "autoStartOperations": "pendingAutoStarts",
          "selectedTaskOperations": "pendingSelectedTasks"}


class NativeCore:
    def __init__(self, bridge):
        self.bridge = bridge

    def dispatch(self, operation, value):
        result = subprocess.run([str(self.bridge)], input=json.dumps({"operation": operation, "input": value}) + "\n",
                                capture_output=True, text=True, env=dict(os.environ, RUST_BACKTRACE="0"))
        if result.returncode:
            raise SharedCoreOperationError(operation, result.stderr.splitlines()[2])
        output = json.loads(result.stdout)
        if "error" in output:
            raise SharedCoreOperationError(operation, output["error"])
        return output


def seed(store, case):
    value = deepcopy(FIXTURE["request"])
    known = deepcopy(case.get("knownTasks", CASES["knownTasks"]))
    task = CASES["knownTasks"][0]
    base = value["workspace"]["base"]
    base["history"] = deepcopy(case.get("history", []))
    if case["current"]:
        base["tasks"] = [deepcopy(task)]
    if case["selected"]:
        base["selectedTaskId"] = task["id"]
    if case["status"] != "idle":
        timer = deepcopy(FIXTURE["timer"])
        timer.update(status=case["status"], phase=case["phase"], taskId=task["id"])
        if case["phase"] != "focus":
            timer.update(plannedDurationMs=120000, taskId=None)
        if case["status"] == "completed":
            timer["elapsedAtAnchorMs"] = 60000
        base["canonicalTimer"] = timer
    snapshot = deepcopy(base) | {"knownTasks": known, "user": None, "revision": 0}
    settings = store._normalize_settings(store.get_meta("settings") | {
        "durationsMs": base["durationsMs"], "selectedTaskId": base["selectedTaskId"]})
    for key, item in {"snapshot": snapshot, "settings": settings, "deviceId": "device-local",
                      "deviceSequence": 7, "hlc": value["allocation"]["hlc"], "lastUuidV7": None,
                      "selectedPhaseVersion": 5, "canonicalHead": value["workspace"]["canonicalHead"]}.items():
        store.set_meta(key, item)


def raw_request(store, case):
    with store._immediate_transaction():
        return raw_request_locked(store, case)


def raw_request_locked(store, case):
    state = store.load(projection=True)
    value = deepcopy(FIXTURE["request"])
    value.update(compatibility="desktopStorage", intent={"kind": "deleteTask", "taskId": case.get("taskId", CASES["knownTasks"][0]["id"])},
                 ownership={"ownerId": None, "expectedOwnerId": None},
                 knownTasks=deepcopy(state["snapshot"]["knownTasks"]))
    base = {key: deepcopy(state["snapshot"][key]) for key in value["workspace"]["base"]}
    queues = {domain: [store._with_device_id(item) for item in state[field]] for domain, field in QUEUES.items()}
    delivery = delivery_metadata(store, queues)
    value["workspace"].update(base=base, local=queues, neverSent=delivery["proof"],
                              canonicalHead=delivery["head"], timerDependencies=delivery["dependencies"])
    value["durability"] = {"outgoingDurationOperationIds": [op["id"] for op in
                            (delivery["outgoing"] or {}).get("durationOperations", [])]}
    value["allocation"] = allocation(store)
    previous = value["allocation"]["lastUuid"]
    first = uuid7.uuid7_parts(previous)[1] + 1 if previous else 1
    value["identities"]["commandUuids"] = [uuid7.uuid7_from_parts(NOW, first + index) for index in range(3)]
    value["clock"] = {key: utc_timestamp(NOW) for key in ("occurredAt", "physicalNow", "observedAt")}
    raw_times = store.get_meta("commandPhysicalTimes")
    value["observation"] = {"canonicalAnchorAt": None,
                            "commandTimes": {key: utc_timestamp(ms) for key, ms in raw_times.items()}}
    assert {key: parse_timestamp_ms(at) for key, at in value["observation"]["commandTimes"].items()} == raw_times
    # Preserve native raw observations; do not reconstruct them from Core's result.
    assert state["projectionSnapshot"]["canonicalTimer"] == base["canonicalTimer"]
    assert state["projectionPending"] == state["pending"]
    return value, state


def delivery_metadata(store, queues):
    head = store.get_meta("canonicalHead")
    actual_head = store.canonical_head()
    assert head == ({"wallMs": actual_head[0], "counter": actual_head[1]} if actual_head else None)
    proof = store.get_meta("deliveryProof")
    assert proof == store.delivery_proof()
    outgoing = store.get_meta("pendingSync")
    assert outgoing == store.pending_sync()
    for domain in QUEUES:
        claimed = {op["id"] for op in (outgoing or {}).get(domain, [])}
        assert claimed.isdisjoint(proof[domain])
    dependencies = store._canonical_storage._core_timer_dependencies(queues)
    rows = store.connection.execute("SELECT id, depends_on_command_id FROM pending_commands "
        "WHERE depends_on_command_id IS NOT NULL ORDER BY device_sequence").fetchall()
    assert [(item["operationId"], item["dependsOnOperationId"]) for item in dependencies] == [tuple(row) for row in rows]
    return {"head": head, "proof": proof, "outgoing": outgoing, "dependencies": dependencies}


def durable_rows(store):
    rows = {}
    for table in ["pending_commands", "pending_task_operations", "pending_duration_operations",
                  "pending_auto_start_operations", "pending_selected_task_operations"]:
        rows[table] = [dict(row) for row in store.connection.execute(f"SELECT * FROM {table} ORDER BY rowid")]
    rows["outgoing"] = store.connection.execute("SELECT value FROM meta WHERE key = 'pendingSync'").fetchone()[0]
    return rows


def allocation(store):
    return {"deviceId": store.device_id, "deviceSequence": store.get_meta("deviceSequence"),
            "hlc": store.get_meta("hlc"), "lastUuid": store.get_meta("lastUuidV7")}


def controller(store, state, blocked=False):
    projected = store.projected_state(now_ms=NOW, state=state)
    settings = store.projected_settings(state, projected)
    known = {task["id"]: task for task in state["snapshot"]["knownTasks"]}
    known.update({task["id"]: task for task in projected.tasks})
    context = TimerInteractionContext(store, None, False, projected.canonical_timer, settings,
                                      None, projected.tasks, known, NOW, "centralized", False)
    ports = TimerInteractionPorts(context=lambda: context, apply_outcome=lambda _: None,
        mutation_blocked=lambda: blocked, issue_command=lambda *_: None,
        maybe_auto_start_break=lambda **_: False, notice=lambda _: None, task_input_text=lambda: "",
        clear_task_input=lambda: None, task_item_data=lambda _: None,
        invalidate_task_selector=lambda: None, render_task_selector=lambda *_: None,
        refresh_duration_spins=lambda _: None, refresh_auto_breaks=lambda _: None,
        stop_sound_timer=lambda: None, stop_completion_sound=lambda: None, set_stop_sound_control=lambda _: None)
    return TimerInteractionController(ports), context


def record_call(original, calls, name):
    def invoke(*args, **kwargs):
        returned = original(*args, **kwargs)
        calls.append({"method": name, "returned": deepcopy(returned)})
        return returned
    return invoke


def interact(store, state, task_id, blocked=False):
    instance, context = controller(store, state, blocked)
    calls, transactions = [], []
    store.connection.set_trace_callback(lambda sql: transactions.append(sql) if sql == "COMMIT" else None)
    names = ("queue_task_operation", "set_selected_task_id", "_insert_retarget_command")
    with patch("pomodorough.storage.time.time", return_value=NOW / 1000), patch(
        "pomodorough.storage.reserve_uuid7", side_effect=lambda wall, count, previous:
            uuid7.reserve_uuid7(wall, count, previous, entropy=lambda size: (1).to_bytes(size, "big"))):
        with patch.object(store, names[0], record_call(store.queue_task_operation, calls, names[0])), patch.object(
            store, names[1], record_call(store.set_selected_task_id, calls, names[1])), patch.object(
            store, names[2], record_call(store._insert_retarget_command, calls, names[2])):
            outcome = instance.delete_task(task_id)
    store.connection.set_trace_callback(None)
    return outcome, calls, context, len(transactions)


def serialized_projection(projected):
    value = asdict(projected)
    names = {"canonical_timer": "canonicalTimer", "durations_ms": "durationsMs",
             "auto_start_breaks": "autoStartBreaks", "selected_task_id": "selectedTaskId",
             "timer_outcomes": "timerOutcomes", "winning_operation_ids": "winningOperationIds"}
    value = {names.get(key, key): item for key, item in value.items()}
    winners = value["winningOperationIds"]
    winners["autoStart"] = winners.pop("auto_start")
    winners["selectedTask"] = winners.pop("selected_task")
    return value


def assert_complete(store, input_value, state, planned, outcome, calls, context):
    after = store.load(projection=True)
    durable = {domain: after[field][len(state[field]):] for domain, field in QUEUES.items()}
    assert durable == planned["durableOperations"], (durable, planned["durableOperations"])
    returned = {call["method"]: call["returned"] for call in calls}
    for method, domain in [("queue_task_operation", "taskOperations"), ("set_selected_task_id", "selectedTaskOperations"),
                           ("_insert_retarget_command", "commands")]:
        assert ([returned[method]] if method in returned else []) == durable[domain], (method, returned)
    assert allocation(store) == planned["allocation"]
    assert store.get_meta("commandPhysicalTimes") == {
        key: parse_timestamp_ms(at) for key, at in planned["observation"]["commandTimes"].items()}
    assert store.delivery_proof() == ({key: planned["workspace"]["neverSent"].get(key, []) for key in QUEUES})
    for domain, field in QUEUES.items():
        assert [store._with_device_id(item) for item in after[field]] == planned["workspace"]["local"][domain]
    metadata = delivery_metadata(store, planned["workspace"]["local"])
    assert metadata["head"] == planned["workspace"]["canonicalHead"]
    assert metadata["dependencies"] == planned["workspace"]["timerDependencies"]
    assert {key: after["snapshot"][key] for key in input_value["workspace"]["base"]} == planned["workspace"]["base"]
    assert str(store.get_meta("selectedPhaseVersion")) == planned["selection"]["generation"]
    actual_projection = serialized_projection(store.projected_state(now_ms=NOW, state=after))
    expected_projection = deepcopy(planned["projection"])
    # Legacy projection.apply.v2 omits native lastIntent.deviceId; workspace.project.v1 retains it.
    for timer in [expected_projection["canonicalTimer"], *expected_projection["history"]]:
        if timer and timer.get("lastIntent"):
            timer["lastIntent"].pop("deviceId", None)
    if input_value["workspace"]["canonicalHead"] is None:
        # Desktop's legacy reader replays without a head. Core's safe display remains canonical-only.
        assert not any(input_value["workspace"]["local"].values())
        before_projection = serialized_projection(store.projected_state(now_ms=NOW, state=state))
        assert expected_projection == before_projection, (expected_projection, before_projection)
    else:
        assert actual_projection == expected_projection, (actual_projection, expected_projection)
    assert_timer_reading(planned["projection"]["canonicalTimer"], planned["timerObservation"])
    effects = ["LoadState", "Render", "Synchronize"] if planned["outcome"] == "planned" else []
    assert outcome.value is None and [type(effect).__name__ for effect in outcome.effects] == effects
    assert asdict(outcome) == {"value": None, "effects": tuple({} for _ in effects)}
    assert context.settings["selectedTaskId"] == (None if durable["selectedTaskOperations"] else planned["projection"]["selectedTaskId"])
    assert_plan_metadata(input_value, planned, outcome, durable)


def assert_plan_metadata(input_value, planned, outcome, durable):
    assert set(planned) == {"schemaVersion", "outcome", "reason", "workspace", "selection", "allocation",
        "observation", "commands", "atomicCommandIds", "operations", "durableOperations", "atomicOperationIds",
        "retiredDurationOperationIds", "groupOutcomes", "ownershipWrites", "projection", "timerObservation", "effectsAfterCommit"}
    assert planned["schemaVersion"] == 1
    changed = bool(outcome.effects)
    assert planned["outcome"] == ("planned" if changed else "noop")
    assert planned["reason"] == ("" if changed else "unchangedOrIneligible")
    assert planned["selection"] == input_value["selection"]
    assert planned["observation"]["canonicalAnchorAt"] is None
    assert planned["effectsAfterCommit"] == ([{"kind": "launchSync"}] if changed else [])
    assert planned["atomicCommandIds"] == [command["id"] for command in durable["commands"]]
    assert planned["ownershipWrites"] == planned["retiredDurationOperationIds"] == []
    assert planned["commands"] == planned["operations"]["commands"]
    for domain, operations in planned["operations"].items():
        assert planned["atomicOperationIds"][domain] == [op["id"] for op in operations]
        assert planned["groupOutcomes"][domain] == [
            {"id": op["id"], "outcome": display_outcome(domain, op, planned["projection"])} for op in operations]


def display_outcome(domain, operation, projection):
    if domain == "commands":
        applied = projection["timerOutcomes"].get(operation["id"], {}).get("outcome") == "applied"
    elif domain == "taskOperations":
        applied = projection["winningOperationIds"]["tasks"].get(operation["taskId"]) == operation["id"]
    else:
        applied = projection["winningOperationIds"]["selectedTask"] == operation["id"]
    return "applied" if applied else "queued"


def assert_timer_reading(timer, reading):
    if timer is None:
        assert reading is None
        return
    elapsed = elapsed_ms(timer, NOW)
    remaining = timer["plannedDurationMs"] - elapsed
    actual = {"timerId": timer["id"], "elapsedMs": elapsed, "remainingMs": remaining,
              "deadlineAt": NOW + remaining if timer["status"] == "running" else None}
    decoded = deepcopy(reading)
    if decoded["deadlineAt"] is not None:
        decoded["deadlineAt"] = parse_timestamp_ms(decoded["deadlineAt"])
    assert decoded == actual, (decoded, actual)


def run_case(directory, case, native, legacy_input=False):
    store = Store(directory / (case["name"] + ".sqlite3"), shared_core=native)
    try:
        seed(store, case)
        return run_step(store, case, native, legacy_input)
    finally:
        store.close()


def run_step(store, case, native, legacy_input=False):
    value, state = raw_request(store, case)
    before = list(store.connection.iterdump())
    rows = durable_rows(store)
    metadata = delivery_metadata(store, value["workspace"]["local"])
    outcome, calls, context, commits = interact(store, state, value["intent"]["taskId"])
    if legacy_input:
        value.pop("knownTasks")
    try:
        planned = native.dispatch("workspace.intent.v1", value)
    except SharedCoreOperationError as error:
        raise AssertionError(f'{case["name"]}: actual Desktop returns {calls}, outcome={asdict(outcome)}; Core rejects: {error}') from error
    assert_complete(store, value, state, planned, outcome, calls, context)
    after_rows = durable_rows(store)
    assert after_rows["outgoing"] == rows["outgoing"]
    for table in rows.keys() - {"outgoing"}:
        assert after_rows[table][:len(rows[table])] == rows[table], (table, rows, after_rows)
    assert_queued_case(case["name"], planned)
    assert commits == (2 if case["selected"] else int(case["taskOperations"] != 0)), (case, commits)
    if planned["outcome"] == "noop":
        assert list(store.connection.iterdump()) == before and calls == []
    return {"case": case["name"], "rawRequest": value, "nativeDeliveryMetadata": metadata,
            "nativeRowsBefore": rows, "nativeRowsAfter": after_rows, "nativeReturns": calls,
            "nativeRawPhysicalTimes": store.get_meta("commandPhysicalTimes"),
            "controllerOutcome": asdict(outcome), "controllerEffectTypes": [type(effect).__name__ for effect in outcome.effects],
                "nativeCommits": commits, "nativeProjection": serialized_projection(store.projected_state(now_ms=NOW)), "core": planned}


def assert_queued_case(name, planned):
    case = next((case for case in CASES["queuedCases"] if case["name"] == name), None)
    if case is None:
        return
    for domain, suffix, counter, outcome in zip(case["domains"], case["uuidSuffixes"],
                                              case["hlcCounters"], case["outcomes"], strict=True):
        operation = planned["operations"][domain][0]
        assert operation["id"].endswith(suffix) and operation["hlcCounter"] == counter
        assert planned["groupOutcomes"][domain][0]["outcome"] == outcome
    if "deviceSequence" in case:
        assert planned["commands"][0]["deviceSequence"] == case["deviceSequence"]
        assert planned["commands"][0]["observedElapsedMs"] == case["observedElapsedMs"]


def restart_sequence(directory, native):
    case = CASES["cases"][0]
    path = directory / "restart.sqlite3"
    store = Store(path, shared_core=native)
    try:
        seed(store, case)
        first = run_step(store, case, native)
        before = store.load()
    finally:
        store.close()
    store = Store(path, shared_core=native)
    try:
        assert store.load() == before
        second = run_step(store, case, native)
        assert second["core"]["workspace"]["local"]["taskOperations"][0] == first["core"]["operations"]["taskOperations"][0]
        assert len(second["core"]["workspace"]["local"]["taskOperations"]) == 2
        return [first | {"case": "beforeRestart"}, second | {"case": "afterRestart"}]
    finally:
        store.close()


def corrupt_identity(directory, native):
    case = deepcopy(CASES["cases"][0])
    case["knownTasks"] = deepcopy(CASES["knownTasks"])
    case["knownTasks"][0]["title"] = "Unrelated title"
    store = Store(directory / "corrupt.sqlite3", shared_core=native)
    try:
        seed(store, case)
        value, state = raw_request(store, case)
        before = list(store.connection.iterdump())
        outcome, calls, _, commits = interact(store, state, value["intent"]["taskId"])
        assert asdict(outcome) == {"value": None, "effects": ({"message": "Task identity does not match its name."},)}
        assert [type(effect).__name__ for effect in outcome.effects] == ["EmitNotice"]
        assert calls == [] and commits == 0 and list(store.connection.iterdump()) == before
        try:
            native.dispatch("workspace.intent.v1", value)
        except SharedCoreOperationError as error:
            assert "known task identity" in str(error)
        else:
            raise AssertionError("Core accepted corrupt cache identity")
        return {"case": "corruptIdentity", "controllerOutcome": asdict(outcome), "nativeCommits": commits}
    finally:
        store.close()


def claim_restart(directory, native, selected):
    case = deepcopy(CASES["cases"][3] if selected else CASES["cases"][0])
    case["name"] = "selectedClaimRestart" if selected else "cacheClaimRestart"
    path = directory / (case["name"] + ".sqlite3")
    store = Store(path, shared_core=native)
    try:
        seed(store, case)
        with patch("pomodorough.storage.time.time", return_value=NOW / 1000), patch(
            "pomodorough.storage.reserve_uuid7", side_effect=lambda wall, count, previous:
                uuid7.reserve_uuid7(wall, count, previous, entropy=lambda size: (1).to_bytes(size, "big"))):
            if selected:
                store.set_selected_task_id(CASES["knownTasks"][0]["id"], now_ms=NOW)
            else:
                _, state = raw_request(store, case)
                outcome, _, _, _ = interact(store, state, CASES["knownTasks"][0]["id"])
                assert [type(effect).__name__ for effect in outcome.effects] == ["LoadState", "Render", "Synchronize"]
        claimed = store.sync_payload()
        original = durable_rows(store)
        assert any(claimed[domain] for domain in QUEUES)
    finally:
        store.close()
    store = Store(path, shared_core=native)
    try:
        assert durable_rows(store) == original
        assert store.sync_payload() == claimed
        result = run_step(store, case, native)
        assert store.sync_payload() == claimed and durable_rows(store)["outgoing"] == original["outgoing"]
        operations = result["core"]["operations"]
        if selected:
            assert [operations[domain][0]["id"] for domain in ("taskOperations", "selectedTaskOperations", "commands")] == [uuid7.uuid7_from_parts(NOW, index) for index in (3, 4, 5)]
            assert [operations[domain][0]["hlcCounter"] for domain in ("taskOperations", "selectedTaskOperations", "commands")] == [2, 3, 4]
            assert operations["commands"][0]["deviceSequence"] == 9
            assert operations["commands"][0]["observedElapsedMs"] == 15000
        else:
            assert operations["taskOperations"][0]["id"] == uuid7.uuid7_from_parts(NOW, 2)
        return result
    finally:
        store.close()


def retained_dependency(directory, native):
    case = deepcopy(CASES["cases"][1]) | {"name": "retainedDependency"}
    store = Store(directory / "retained-dependency.sqlite3", shared_core=native)
    try:
        seed(store, case)
        with patch("pomodorough.storage.reserve_uuid7", side_effect=lambda wall, count, previous:
            uuid7.reserve_uuid7(wall, count, previous, entropy=lambda size: (1).to_bytes(size, "big"))):
            store.set_selected_task_id(None, now_ms=NOW)
            store.set_selected_task_id(None, now_ms=NOW)
        commands = store.load()["pending"]
        # Persist a valid held causal pair, then read it through the production dependency adapter.
        with store._immediate_transaction():
            store.connection.execute("UPDATE pending_commands SET depends_on_command_id = ? WHERE id = ?",
                                     (commands[0]["id"], commands[1]["id"]))
        claimed = store.sync_payload()
        result = run_step(store, case, native)
        expected = [{"operationId": commands[1]["id"], "dependsOnOperationId": commands[0]["id"]}]
        assert result["rawRequest"]["workspace"]["timerDependencies"] == expected
        assert result["core"]["workspace"]["timerDependencies"] == expected
        assert store.sync_payload() == claimed
        return result
    finally:
        store.close()


def null_head(directory, native):
    case = deepcopy(CASES["cases"][0]) | {"name": "nullHead"}
    store = Store(directory / "null-head.sqlite3", shared_core=native)
    try:
        seed(store, case)
        store.set_meta("canonicalHead", None)
        result = run_step(store, case, native)
        assert result["rawRequest"]["workspace"]["canonicalHead"] is None
        assert result["core"]["groupOutcomes"]["taskOperations"][0]["outcome"] == "queued"
        return result
    finally:
        store.close()


def claimed_cancel_rejection(directory, native):
    case = deepcopy(CASES["cases"][3]) | {"name": "claimedCancelRejectsRetarget"}
    store = Store(directory / "claimed-cancel.sqlite3", shared_core=native)
    try:
        seed(store, case)
        state = store.load(projection=True)
        timer = store.projected_state(now_ms=NOW, state=state).canonical_timer
        with patch("pomodorough.storage.reserve_uuid7", side_effect=lambda wall, count, previous:
            uuid7.reserve_uuid7(wall, count, previous, entropy=lambda size: (1).to_bytes(size, "big"))):
            store.queue_command("cancel", timer, "focus", state["settings"]["durationsMs"], now_ms=NOW)
        claimed = store.sync_payload()
        value, state = raw_request(store, case)
        before = durable_rows(store)
        assert store.projected_state(now_ms=NOW, state=state).canonical_timer["status"] == "running"
        outcome, calls, _, commits = interact(store, state, value["intent"]["taskId"])
        assert asdict(outcome) == {"value": None, "effects": (
            {"message": "Shared core rejected the timer command projection."},)}
        assert [type(effect).__name__ for effect in outcome.effects] == ["EmitNotice"]
        assert len(calls) == 1 and calls[0]["method"] == "queue_task_operation" and commits == 1
        after = durable_rows(store)
        assert after["pending_commands"] == before["pending_commands"]
        assert after["pending_selected_task_operations"] == before["pending_selected_task_operations"]
        assert after["outgoing"] == before["outgoing"] and store.sync_payload() == claimed
        actual = allocation(store)
        assert actual["lastUuid"] == calls[0]["returned"]["id"] and actual["deviceSequence"] == 8
        assert store.delivery_proof()["commands"] == []
        assert store.delivery_proof()["selectedTaskOperations"] == []
        assert store.delivery_proof()["taskOperations"] == [calls[0]["returned"]["id"]]
        try:
            native.dispatch("workspace.intent.v1", value)
        except SharedCoreOperationError as error:
            assert "workspace group failed retained-ledger admission" in str(error)
            return {"case": case["name"], "rawRequest": value, "nativeRowsBefore": before,
                    "nativeRowsAfter": after, "nativeReturns": calls, "controllerOutcome": asdict(outcome),
                    "nativeCommits": commits, "nativeAllocation": actual, "nativeProof": store.delivery_proof(),
                    "coreError": str(error), "intentionalAtomicUpgrade": True}
        raise AssertionError("Core admitted retarget after a retained Cancel")
    finally:
        store.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--case")
    parser.add_argument("--legacy-input", action="store_true", help="Reproduce the original projected-task-only refusal.")
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    native = NativeCore(Path(os.environ.get("CORE_PROBE", ROOT / "target/debug/examples/completion_policy_probe")))
    with tempfile.TemporaryDirectory(prefix="desktop-known-task-") as temporary:
        results = [run_case(Path(temporary), case, native, args.legacy_input) for case in CASES["cases"]
                   if args.case is None or case["name"] == args.case]
        if args.case is None:
            results.extend(restart_sequence(Path(temporary), native))
            results.append(corrupt_identity(Path(temporary), native))
        for name, run in [("cacheClaimRestart", lambda: claim_restart(Path(temporary), native, False)),
                          ("selectedClaimRestart", lambda: claim_restart(Path(temporary), native, True)),
                          ("nullHead", lambda: null_head(Path(temporary), native)),
                          ("retainedDependency", lambda: retained_dependency(Path(temporary), native)),
                          ("claimedCancelRejectsRetarget", lambda: claimed_cancel_rejection(Path(temporary), native))]:
            if args.case is None or args.case == name:
                results.append(run())
    assert results, "no selected cases"
    if args.evidence:
        args.evidence.write_text(json.dumps(results, indent=2) + "\n")
    print(f"{len(results)} real Desktop Store/Qt controller complete-output cases passed")
    for source in ["storage.py", "timer_interaction_controller.py", "uuid7.py", "shared_core.py"]:
        path = DESKTOP / "src/pomodorough" / source
        print(f"{path.relative_to(ROOT.parent)} sha256={hashlib.sha256(path.read_bytes()).hexdigest()}")


if __name__ == "__main__":
    main()
