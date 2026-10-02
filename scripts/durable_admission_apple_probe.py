#!/usr/bin/env python3
"""Run unchanged Apple mutation/claim methods with production wire types.

Test state supplies persistence, clocks, entropy and native dispatcher transport.
The original safe projection policy and requirement check run unchanged. Legacy
safe-winner refusals are reported explicitly as migration compatibility work.
"""
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess
import tempfile

import workspace_mutation_source_probe as common

ROOT = common.ROOT
APPLE = ROOT.parent / "apple/Sources"


def section(name, begin, end): return common.section(APPLE / name, begin, end)


def compile_probe(temporary):
    program = (ROOT / "scripts/durable_admission_apple.swift").read_text()
    state = section("PersistedTimerState.swift", "    mutating func reserveDeviceSequence()", "    mutating func reserveUuidV7(")
    state += section("PersistedTimerState.swift", "    mutating func advanceClock(\n", "    mutating func mergeClock(")
    program = program.replace("// INSERT_STATE_METHODS", state)
    commands = section("TimerSessionController.swift", "    func makeCommand(", "    func prepareFinish(")
    program = program.replace("// INSERT_COMMAND_METHODS", commands)
    generation = section("TimerSessionController.swift", "    static func nextPhaseGeneration(", "    static func derivedNextPhase(")
    program = program.replace("// INSERT_GENERATION_METHOD", generation)
    program = program.replace("// INSERT_SYNC_METHODS", section("AccountSynchronization.swift", "    func makeSyncPlan(", "    func sendSync("))
    wire = section("SharedCoreModels.swift", "struct CoreHLC:", "struct CoreCompletionOwnership:")
    wire += section("SharedCoreModels.swift", "struct CoreProjectionBase:", "    func validated(for input: CoreProjectionInput)") + "}\n"
    wire += "extension CoreProjectionOutput: Encodable {}\nextension CoreProjectionTimerOutcome: Encodable {}\nextension CoreProjectionWinningOperationIDs: Encodable {}\n"
    proof = section("ImmutableReconciliationPolicy.swift", "extension PersistedTimerState {", "    func neverSentProof()") + "}\n"
    proof += section("ImmutableReconciliationPolicy.swift", "extension PersistedTimerState {\n    func safeProjectionCommands()", "    func safeProjectionPending()") + "}\n"
    sources = []
    for name, contents in [("main.swift", program), ("Wire.swift", "import Foundation\n" + wire), ("Proof.swift", "import Foundation\n" + proof)]:
        path = temporary / name; path.write_text(contents); sources.append(path)
    sources += [APPLE / name for name in ("TimerDomainModels.swift", "TimerOperationModels.swift", "TaskHistoryModels.swift", "WirePrimitives.swift", "SynchronizedWorkspaceMutationController.swift")]
    binary = temporary / "apple-probe"
    result = subprocess.run(["swiftc", *map(str, sources), "-o", str(binary)], text=True, capture_output=True)
    if result.returncode: raise RuntimeError(result.stderr)
    return binary


def rows():
    task = common.core("task.identity.v1", {"title": "Café"})
    task = {key: task[key] for key in ("id", "title")}
    for null_head in (False, True):
        for kind in ("upsertTask", "deleteTask", "selectTask", "setDuration", "setAutoStart", "start", "pause", "resume", "cancel", "clear"):
            intent = {"kind": kind}
            intent.update({"title": "Café"} if kind == "upsertTask" else {"taskId": task["id"]} if kind in ("deleteTask", "selectTask") else
                {"phase": "short_break", "minutes": 4} if kind == "setDuration" else {"enabled": True} if kind == "setAutoStart" else {})
            state = {"settings": {"selectedPhase": "focus", "autoStartBreaks": False,
                "focusDurationMs": 60000, "shortBreakDurationMs": 120000, "longBreakDurationMs": 180000},
                "selectedPhaseGeneration": 5, "hasExplicitPhaseSelection": True, "deviceId": "device-local", "nextSequence": 8,
                "sequenceExhausted": False, "hlcWallMs": 1784548800000, "hlcCounter": 0, "revision": 0,
                "history": [], "tasks": [task], "knownTasks": [], "baseAutoStart": False,
                "canonicalHeadWallMs": 1784548800000, "canonicalHeadCounter": 0,
                "pendingCommands": [], "pendingTaskOperations": [], "pendingDurationOperations": [], "pendingAutoStartOperations": [], "pendingSelectedTaskOperations": [],
                "neverSentCommandIDs": [], "neverSentTaskOperationIDs": [], "neverSentDurationOperationIDs": [], "neverSentAutoStartOperationIDs": [], "neverSentSelectedTaskOperationIDs": [],
                "localCommandDates": {}, "localTimerOwners": {}, "trustedClockState": {}}
            if kind not in ("start", "setDuration"): state["canonicalTimer"] = deepcopy(common.TIMER)
            if kind == "resume": state["canonicalTimer"]["status"] = "paused"
            if kind == "clear": state["canonicalTimer"].update(status="completed", elapsedAtAnchorMs=60000)
            seed = ({"kind": "setAutoStart", "enabled": False} if kind == "setAutoStart" else {"kind": "setDuration", "minutes": 3} if kind == "setDuration" else
                {"kind": "selectTask", "taskId": task["id"]} if kind == "selectTask" else {"kind": "upsertTask", "title": "Café"} if kind in ("upsertTask", "deleteTask") else {"kind": "selectTask", "taskId": task["id"]})
            if kind == "setAutoStart": seed["enabled"] = True; intent["enabled"] = False
            yield {"intent": intent, "seedIntent": seed, "task": task, "state": state, "nullHead": null_head}


def main():
    bridge = Path(os.environ.get("CORE_PROBE", ROOT / "target/debug/examples/completion_policy_probe"))
    common.BRIDGE = bridge
    cases = list(rows())
    with tempfile.TemporaryDirectory(prefix="durable-apple-") as directory:
        binary = compile_probe(Path(directory))
        result = subprocess.run([str(binary), str(bridge)], input="".join(json.dumps(row) + "\n" for row in cases), capture_output=True, text=True)
        if result.returncode: raise RuntimeError(result.stderr)
    results = [json.loads(line) for line in result.stdout.splitlines()]
    assert len(results) == len(cases)
    receipts = []
    for case, returned in zip(cases, results, strict=True):
        assert returned["savedStateAfter"] == returned["rawStateBefore"]
        assert json.loads(returned["rawSavedBytes"]) == returned["rawStateBefore"]
        value = raw_request(case, returned)
        planned = common.core("workspace.intent.v1", value)
        assert_complete(case, returned, planned)
        receipts.append({"input": case, "rawRequest": value, "productionReturn": returned, "core": planned,
            "compatibilityRequired": ["appleLegacySafeWinnerAdmission"] if "error" in returned else []})
    evidence = os.environ.get("PROBE_EVIDENCE")
    if evidence: Path(evidence).write_text(json.dumps(receipts, indent=2) + "\n")
    print(f"{len(cases)} extracted Apple claim/reopen complete returns compared; safe-winner refusal differences recorded")


def raw_request(case, returned):
    state = returned["rawStateBefore"]
    value = common.request("appleWorkspace", case["intent"])
    base = value["workspace"]["base"]
    base.update(canonicalTimer=state.get("canonicalTimer"), history=state["history"], tasks=state["tasks"],
        autoStartBreaks=state["baseAutoStart"], selectedTaskId=state.get("baseSelection"),
        durationsMs={"focus": 60000, "short_break": 120000, "long_break": 180000})
    head = None if "canonicalHeadWallMs" not in state else {"wallMs": state["canonicalHeadWallMs"], "counter": state["canonicalHeadCounter"]}
    proof = {domain: state[field] for domain, field in [("commands", "neverSentCommandIDs"), ("taskOperations", "neverSentTaskOperationIDs"),
        ("durationOperations", "neverSentDurationOperationIDs"), ("autoStartOperations", "neverSentAutoStartOperationIDs"), ("selectedTaskOperations", "neverSentSelectedTaskOperationIDs")]}
    value["workspace"].update(local=returned["wireQueuesBefore"], canonicalHead=head, neverSent=proof)
    if case["intent"]["kind"] == "setDuration":
        value["localDurationsMs"] = {"focus": state["settings"]["focusDurationMs"], "short_break": state["settings"]["shortBreakDurationMs"], "long_break": state["settings"]["longBreakDurationMs"]}
    value["allocation"] = {"deviceId": state["deviceId"], "deviceSequence": state["nextSequence"] - 1,
        "hlc": {"wallMs": state["hlcWallMs"], "counter": state["hlcCounter"]}, "lastUuid": state["lastUuidV7"].lower()}
    first = int(state["lastUuidV7"][-12:], 16) + 1
    value["identities"]["commandUuids"] = [f"019f7f65-dd10-7000-8000-{index:012x}" for index in range(first, first + 3)]
    if case["intent"]["kind"] in ("start", "pause", "resume", "cancel", "clear"):
        value.pop("ownership"); value.pop("durability")
        value["identities"]["commandUuids"] = value["identities"]["commandUuids"][:2]
    else:
        value["durability"]["outgoingDurationOperationIds"] = [op["id"] for op in returned["outgoing"]["batch"]["durationOperations"]]
    value["observation"] = {"canonicalAnchorAt": None, "commandTimes": state["localCommandDates"]}
    from datetime import datetime
    assert {key: datetime.fromisoformat(at.replace("Z", "+00:00")).timestamp() for key, at in state["localCommandDates"].items()} == returned["rawDates"]
    return value


def assert_complete(case, returned, planned):
    before = returned["rawStateBefore"]
    if "error" in returned:
        assert returned["error"] == 'invalidResponse("new synchronized mutation did not win Core projection")'
        assert planned["outcome"] == "planned"
        outcomes = planned.get("commandOutcomes", [item for queue in planned.get("groupOutcomes", {}).values() for item in queue])
        assert any(item["outcome"] == "queued" for item in outcomes)
        assert "state" not in returned and "effects" not in returned
        return
    assert "returned" not in returned, (case, returned)
    after = returned["state"]
    assert after["hlcWallMs"] == planned["allocation"]["hlc"]["wallMs"] and after["hlcCounter"] == planned["allocation"]["hlc"]["counter"]
    assert after["lastUuidV7"].lower() == planned["allocation"]["lastUuid"]
    for domain, queue in returned["wireQueuesAfter"].items():
        expected = deepcopy(planned["workspace"]["local"][domain])
        if domain == "taskOperations":
            for op in expected:
                if op["type"] == "delete": op["title"] = ""
        assert queue == expected, (case["intent"], domain, queue, expected)
    effects = returned["effects"]
    assert effects[:2] == [{"kind": "persistAtomically", "previous": before, "rebuildsOnRollback": True}, {"kind": "launchSync"}]
    if returned["projection"] is None:
        assert case["intent"]["kind"] == "setAutoStart"
        assert all(not items for items in returned["requirements"].values())
        assert effects == effects[:2]
        assert planned["groupOutcomes"]["autoStartOperations"][0]["outcome"] == "queued"
    else:
        expected = deepcopy(planned["projection"])
        for timer in [expected["canonicalTimer"], *expected["history"]]:
            if timer and timer.get("lastIntent"): timer["lastIntent"].pop("deviceId", None)
        from durable_admission_android_probe import normalize_optional
        if not case["nullHead"]: assert normalize_optional(returned["projection"]) == normalize_optional(expected)
        else: assert all(item["outcome"] == "queued" for item in planned["commandOutcomes"])
        alarms = [action for effect in effects[2:] if effect["kind"] == "alarm" for action in effect["actions"]]
        assert alarms == [effect for effect in planned["effectsAfterCommit"] if effect["kind"] not in ("launchSync", "clearCompletionAlert")]
        if case["intent"]["kind"] == "start":
            assert effects[2:4] == [{"kind": "setExplicitPhaseSelection", "explicit": False}, {"kind": "persist"}]
            assert effects[4:] == [{"kind": "alarm", "actions": alarms}]
        elif case["intent"]["kind"] == "clear":
            assert effects[2:] == [{"kind": "clearCompletionAlert", "timerId": "existing-timer"}, {"kind": "alarm", "actions": alarms}]
        else: assert effects[2:] == [{"kind": "alarm", "actions": alarms}]
    assert after["selectedPhaseGeneration"] == int(planned["selection"]["generation"])
    assert after["settings"]["selectedPhase"] == planned["selection"]["phase"]
    for domain, field in [("commands", "neverSentCommandIDs"), ("taskOperations", "neverSentTaskOperationIDs"),
        ("durationOperations", "neverSentDurationOperationIDs"), ("autoStartOperations", "neverSentAutoStartOperationIDs"), ("selectedTaskOperations", "neverSentSelectedTaskOperationIDs")]:
        assert sorted(after[field]) == sorted(planned["workspace"]["neverSent"].get(domain, []))
    expected_state = deepcopy(before)
    expected_state.update(hlcWallMs=planned["allocation"]["hlc"]["wallMs"], hlcCounter=planned["allocation"]["hlc"]["counter"],
        lastUuidV7=planned["allocation"]["lastUuid"].upper(), nextSequence=planned["allocation"]["deviceSequence"] + 1)
    proof_fields = {"commands": "neverSentCommandIDs", "taskOperations": "neverSentTaskOperationIDs", "durationOperations": "neverSentDurationOperationIDs",
                   "autoStartOperations": "neverSentAutoStartOperationIDs", "selectedTaskOperations": "neverSentSelectedTaskOperationIDs"}
    native_fields = {"commands": "pendingCommands", "taskOperations": "pendingTaskOperations", "durationOperations": "pendingDurationOperations",
                    "autoStartOperations": "pendingAutoStartOperations", "selectedTaskOperations": "pendingSelectedTaskOperations"}
    for domain, field in native_fields.items():
        new = deepcopy(planned.get("durableOperations", {}).get(domain, planned["commands"] if domain == "commands" else []))
        for item in new:
            item.pop("deviceId", None) if domain in ("commands", "taskOperations", "durationOperations") else None
            if domain in ("autoStartOperations", "selectedTaskOperations"): item["id"] = item["id"].upper()
        expected_state[field] += new
        expected_state[proof_fields[domain]] = planned["workspace"]["neverSent"].get(domain, [])
    expected_state["localCommandDates"] = planned["observation"]["commandTimes"]
    if case["intent"]["kind"] == "start":
        expected_state["localTimerOwners"][planned["commands"][0]["timerId"]] = planned["allocation"]["deviceId"]
    from durable_admission_android_probe import normalize_optional
    expected_requirements = {"timerCommandIDs": [command["id"] for command in planned["commands"]], "taskOperationIDs": [],
        "durationOperationIDs": [], "autoStartOperationIDs": [], "selectedTaskOperationIDs": []}
    assert returned["requirements"] == expected_requirements
    for field in proof_fields.values():
        expected_state[field] = sorted(expected_state[field])
        after[field] = sorted(after[field])
    assert normalize_optional(after) == normalize_optional(expected_state), (case["intent"], after, expected_state)


if __name__ == "__main__": main()
