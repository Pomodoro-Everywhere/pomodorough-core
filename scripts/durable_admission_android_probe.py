#!/usr/bin/env python3
"""Execute the unchanged Android coordinator and serialize every returned field.

Room annotations are test stubs. LocalStateEntity, coordinator, clock allocation,
wire models, projection/command policy dispatchers, and presentation are original
production source. This is method extraction, not an Android Room runtime.
"""
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess
import tempfile

import bootstrap_workspace_android_probe as build
import workspace_mutation_source_probe as fixture

ROOT = build.ROOT
ANDROID = build.ANDROID


def compile_probe(temporary):
    compiler = Path(os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc"))
    libraries = compiler.parent.parent / "lib"
    gradle = Path.home() / ".gradle/wrapper/dists"
    jars = [next(gradle.glob(f"**/{name}-1.6.2.jar")) for name in ("kotlinx-serialization-json-jvm", "kotlinx-serialization-core-jvm")]
    classpath = os.pathsep.join(map(str, [*jars, libraries / "kotlinx-coroutines-core-jvm.jar"]))
    pending = build.section(ANDROID / "TimerSyncConstruction.kt", "internal data class PendingSyncQueues(", "internal data class SentSyncIds(")
    retire = build.section(ANDROID / "local/CentralizedSyncDao.kt", "    suspend fun retireNeverSent(", "    @Transaction\n    suspend fun clearAccount(")
    program = (ROOT / "scripts/durable_admission_android.kt").read_text().replace("// INSERT_PENDING_MODEL", pending).replace("// INSERT_RETIRE_METHOD", retire)
    source = temporary / "Probe.kt"
    source.write_text(program)
    entity = temporary / "Entity.kt"
    entity.write_text(build.section(ANDROID / "local/Entities.kt", "package me.egigoka.pomodorough.data.local", "@Entity(tableName = \"replication_settings\")"))
    annotations = temporary / "Room.kt"
    annotations.write_text('package androidx.room\nannotation class Entity(val tableName: String, val indices: Array<Index> = [])\nannotation class Index(val value: Array<String>, val unique: Boolean = false)\nannotation class PrimaryKey\nannotation class ForeignKey\n')
    jar = temporary / "probe.jar"
    env = dict(os.environ, JAVA_HOME=os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    sources = [source, entity, annotations, *[ANDROID / name for name in ("Models.kt", "CoreProjectionDispatcher.kt", "SyncWireBounds.kt", "CoreTimerPolicyDispatchers.kt", "TimerMutationCoordinator.kt", "TimerTaskRetarget.kt", "TimerSyncValidation.kt")], ANDROID.parent / "domain/TimerPresentation.kt"]
    result = subprocess.run([str(compiler), *map(str, sources), "-cp", classpath,
        "-Xplugin=" + str(libraries / "kotlinx-serialization-compiler-plugin.jar"), "-include-runtime", "-d", str(jar)], env=env, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr)
    return [str(Path(env["JAVA_HOME"]) / "bin/java"), "-cp", str(jar) + os.pathsep + classpath, "me.egigoka.pomodorough.data.ProbeKt"]


def inputs():
    task = fixture.core("task.identity.v1", {"title": "Café"})
    task = {key: task[key] for key in ("id", "title")}
    for null_head in (False, True):
        for kind in ("upsertTask", "addAndSelectTask", "deleteTask", "selectTask", "changeDuration", "setAutoStart", "start", "pause", "resume", "cancel", "clear", "cancelAndClear", "finish"):
            intent = {"kind": kind}
            intent.update({"title": "Durable task" if kind == "addAndSelectTask" else "Café"} if kind in ("upsertTask", "addAndSelectTask") else {"taskId": task["id"]} if kind in ("deleteTask", "selectTask") else
                {"phase": "focus", "delta": 1} if kind == "changeDuration" else {"enabled": True} if kind == "setAutoStart" else {})
            value = fixture.request("androidCoordinator", intent)
            if kind not in ("upsertTask", "addAndSelectTask", "deleteTask", "selectTask", "changeDuration", "setAutoStart"):
                value.pop("ownership"); value.pop("durability")
            value["workspace"]["base"]["tasks"] = [task]
            if kind not in ("start", "changeDuration", "upsertTask", "addAndSelectTask", "deleteTask", "selectTask", "setAutoStart"):
                value["workspace"]["base"]["canonicalTimer"] = deepcopy(fixture.TIMER)
            if kind == "resume": value["workspace"]["base"]["canonicalTimer"]["status"] = "paused"
            if kind == "clear": value["workspace"]["base"]["canonicalTimer"].update(status="completed", elapsedAtAnchorMs=60000)
            domain = {"upsertTask": "taskOperations", "addAndSelectTask": "taskOperations", "deleteTask": "taskOperations", "selectTask": "selectedTaskOperations", "changeDuration": "durationOperations", "setAutoStart": "autoStartOperations"}.get(kind, "commands")
            old = {"id": "old-operation", "deviceId": "device-local", "occurredAt": "2026-07-20T12:00:00Z", "hlcWallMs": 1784548800000, "hlcCounter": 1}
            old.update({"taskId": task["id"], "type": "upsert", "title": task["title"]} if domain == "taskOperations" else
                {"taskId": None} if domain == "selectedTaskOperations" else {"phase": "focus", "durationMs": 120000} if domain == "durationOperations" else
                {"enabled": False} if domain == "autoStartOperations" else {"type": "retarget", "timerId": "existing-timer", "deviceSequence": 7, "phase": "focus", "plannedDurationMs": 60000, "observedElapsedMs": 0, "taskId": None})
            # Timer cases retain an unrelated stale session. Fresh actions still
            # target the canonical timer. No hand-written mutation outcome.
            if domain == "commands": old["hlcWallMs"] = 1784548790000; old["occurredAt"] = "2026-07-20T11:59:50Z"
            value["workspace"]["local"][domain] = [old]
            value["allocation"]["hlc"]["counter"] = 1
            if null_head: value["workspace"]["canonicalHead"] = None
            new_task = fixture.core("task.identity.v1", {"title": "Durable task"}) if kind == "addAndSelectTask" else task
            yield kind, value, {key: new_task[key] for key in ("id", "title")}
    for null_head in (False, True):
        value = fixture.request("androidCoordinator", {"kind": "pause"})
        value.pop("ownership"); value.pop("durability")
        value["workspace"]["base"]["tasks"] = [task]
        value["workspace"]["local"]["commands"] = [{"id": "pending-only-start", "deviceId": "device-local",
            "deviceSequence": 7, "timerId": "pending-only-timer", "type": "start", "phase": "focus", "plannedDurationMs": 60000,
            "occurredAt": "2026-07-20T12:00:00Z", "hlcWallMs": 1784548800000, "hlcCounter": 1, "observedElapsedMs": 0}]
        value["allocation"]["hlc"]["counter"] = 1
        if null_head: value["workspace"]["canonicalHead"] = None
        yield "pause", value, task


def native_queues(queues):
    result = deepcopy(queues)
    for domain, operations in result.items():
        for item in operations:
            if domain != "autoStartOperations": item.pop("deviceId", None)
            if domain == "commands":
                if item["type"] == "retarget": item.setdefault("taskId", None)
                elif item.get("taskId") is None: item.pop("taskId", None)
            elif domain == "taskOperations": item.setdefault("title", None)
    return result


def normalize_optional(value):
    if isinstance(value, list): return [normalize_optional(item) for item in value]
    if isinstance(value, dict): return {key: normalize_optional(item) for key, item in value.items() if item is not None}
    return value


def local_values(value, settings):
    result = {key: json.loads(item) if key.endswith("Json") and item is not None else item for key, item in value.items()}
    result["settingsJson"] = settings
    return result


def assert_complete(kind, raw, returned, planned, task):
    queues = native_queues(planned["workspace"]["local"])
    assert returned["rawQueuesBefore"] == native_queues(raw["workspace"]["local"]), (kind, returned["rawQueuesBefore"], native_queues(raw["workspace"]["local"]))
    old = local_values(returned["rawLocalBefore"], returned["rawDecodedSettings"])
    claim = returned["claim"]
    assert claim["proofAfter"] == {domain: [] for domain in raw["workspace"]["local"]}
    assert claim["queuesAfter"] == returned["rawQueuesBefore"]
    assert json.loads(claim["savedRaw"]) == claim["restored"]
    assert claim["restored"]["outgoing"] == returned["rawQueuesBefore"]
    for domain, old_rows in claim["rowsBefore"].items():
        assert claim["rowsAfter"][domain] == [row | {"neverSent": False} for row in old_rows]
    assert old["deviceId"] == raw["allocation"]["deviceId"]
    assert old["deviceSequence"] == raw["allocation"]["deviceSequence"]
    assert old["hlcWallMs"] == raw["allocation"]["hlc"]["wallMs"]
    assert old["hlcCounter"] == raw["allocation"]["hlc"]["counter"]
    assert old["settingsJson"] == raw["nativeSettings"]
    assert normalize_optional(old["canonicalTimerJson"]) == normalize_optional(raw["workspace"]["base"]["canonicalTimer"])
    assert old["tasksJson"] == raw["workspace"]["base"]["tasks"]
    assert old["historyJson"] == raw["workspace"]["base"]["history"]
    assert old["canonicalAutoStartBreaks"] == raw["workspace"]["base"]["autoStartBreaks"]
    expected = deepcopy(old)
    expected.update(deviceSequence=planned["allocation"]["deviceSequence"], hlcWallMs=planned["allocation"]["hlc"]["wallMs"],
        hlcCounter=planned["allocation"]["hlc"]["counter"], lastUuidV7=planned["allocation"]["lastUuid"])
    if kind in ("upsertTask", "deleteTask", "addAndSelectTask"):
        known = {item["id"]: item for item in raw["workspace"]["base"]["tasks"]} | {task["id"]: task}
        expected["knownTasksJson"] = sorted(known.values(), key=lambda item: item["id"])
        assert returned["knownTasks"] == known
        assert returned["taskOperations"] == queues["taskOperations"]
        assert returned["selectedTaskOperations"] == queues["selectedTaskOperations"]
        if kind == "addAndSelectTask":
            assert returned["selectedOperation"] == queues["selectedTaskOperations"][-1]
            expected["selectedTaskId"] = task["id"]
        else: assert returned["selectedOperation"] is None
    elif kind == "selectTask": expected["selectedTaskId"] = raw["intent"]["taskId"]
    if kind in ("changeDuration", "setAutoStart"):
        settings = deepcopy(raw["nativeSettings"])
        if kind == "changeDuration":
            settings["durationsMs"][raw["intent"]["phase"]] = planned["operations"]["durationOperations"][0]["durationMs"]
            settings["focusMinutes"] = settings["durationsMs"]["focus"] // 60000
            # The old Android coordinator drops all same-phase rows. Core now
            # keeps claimed payloads. Record this required adapter correction.
            assert returned["operations"] == queues["durationOperations"][-1:]
        else:
            settings["autoStartBreaks"] = raw["intent"]["enabled"]
            assert returned["operations"] == queues["autoStartOperations"]
        expected["settingsJson"] = settings
        assert returned["settings"] == settings
    elif kind == "selectTask": assert returned["operations"] == queues["selectedTaskOperations"]
    elif "commands" in returned:
        settings = deepcopy(raw["nativeSettings"])
        settings["selectedPhase"] = planned["selection"]["phase"]
        assert returned["settings"] == settings
        assert returned["dependencies"] == {}
        if kind == "finish": expected["settingsJson"] = settings
        if kind == "start": expected["ownedTimerId"] = planned["commands"][0]["timerId"]
    assert local_values(returned["local"], returned["localDecodedSettings"]) == expected, (kind, returned["local"], expected)
    projected = fixture.core("projection.apply.v2", {"base": planned["workspace"]["base"],
        "pending": planned["workspace"]["local"], "now": raw["clock"]["physicalNow"]})
    for row in projected["history"]: row["pending"] = False
    assert normalize_optional(returned["projection"]) == normalize_optional(projected), (kind, returned["projection"], projected)
    common = {"outcome", "local", "projection", "rawLocalBefore", "rawQueuesBefore", "rawDecodedSettings", "localDecodedSettings", "claim"}
    extra = {"operation", "selectedOperation", "knownTasks", "taskOperations", "selectedTaskOperations"} if kind in ("upsertTask", "deleteTask", "addAndSelectTask") else {"operation", "operations"} if kind == "selectTask" else {"operation", "operations", "settings"} if kind in ("changeDuration", "setAutoStart") else {"commands", "commandPhysicalTimes", "dependencies", "settings"}
    assert set(returned) == common | extra


def main():
    bridge = Path(os.environ.get("CORE_PROBE", ROOT / "target/debug/examples/completion_policy_probe"))
    fixture.BRIDGE = bridge
    rows = list(inputs())
    requests = []
    for kind, value, task in rows:
        raw = deepcopy(value)
        raw["task"] = task
        raw["nativeSettings"] = {"selectedPhase": "focus", "focusMinutes": 1, "shortBreakMinutes": 2, "longBreakMinutes": 3,
                                 "autoStartBreaks": False, "durationsMs": value["workspace"]["base"]["durationsMs"]}
        requests.append(raw)
    with tempfile.TemporaryDirectory(prefix="durable-android-") as directory:
        command = compile_probe(Path(directory)) + [str(bridge)]
        result = subprocess.run(command, input="".join(json.dumps(row) + "\n" for row in requests), text=True, capture_output=True, check=True)
    actual = [json.loads(line) for line in result.stdout.splitlines()]
    assert len(actual) == len(rows)
    receipts = []
    for (kind, value, task), raw, returned in zip(rows, requests, actual, strict=True):
        assert "productionError" not in returned, (kind, returned)
        assert returned["outcome"] == "planned", (kind, returned)
        value["workspace"]["neverSent"] = returned["claim"]["proofAfter"]
        assert native_queues(value["workspace"]["local"]) == returned["claim"]["queuesAfter"]
        if "ownership" in value:
            owner = returned["rawLocalBefore"]["ownerUserId"]
            value["ownership"] = {"ownerId": owner, "expectedOwnerId": owner}
        operation = "workspace.intent.v1"
        if kind == "finish":
            value.pop("intent"); value.update(stage="finishCommit", requestedTimer=value["workspace"]["base"]["canonicalTimer"], ownership=None)
            value["identities"]["commandUuids"] = value["identities"]["commandUuids"][:1]
            operation = "workspace.completionMutation.v1"
        elif kind == "cancelAndClear": value["requestedTimer"] = value["workspace"]["base"]["canonicalTimer"]
        planned = fixture.core(operation, value)
        expected = planned.get("durableOperations", {"commands": planned.get("durableCommands",
            [{key: item for key, item in command.items() if key != "deviceId"} for command in planned["commands"]])})
        if "commands" in returned:
            commands = deepcopy(returned["commands"])
            assert returned["commandPhysicalTimes"] == {command["id"]: planned["observation"]["commandTimes"][command["id"]] for command in planned["commands"]}
            for item in commands:
                for field in ("physicalOccurredAt", "taskId"):
                    if item.get(field) is None: item.pop(field, None)
                item.pop("physicalOccurredAt", None)
            assert commands == expected["commands"], (kind, commands, expected)
        else:
            domain = {"upsertTask": "taskOperations", "addAndSelectTask": "taskOperations", "deleteTask": "taskOperations", "selectTask": "selectedTaskOperations", "changeDuration": "durationOperations", "setAutoStart": "autoStartOperations"}[kind]
            item = deepcopy(returned["operation"])
            if item.get("title") is None: item.pop("title", None)
            assert [item] == expected[domain], (kind, item, expected)
        local = returned["local"]
        assert {"deviceId": local["deviceId"], "deviceSequence": local["deviceSequence"], "hlc": {"wallMs": local["hlcWallMs"], "counter": local["hlcCounter"]}, "lastUuid": local["lastUuidV7"]} == planned["allocation"]
        assert_complete(kind, raw, returned, planned, task)
        receipts.append({"rawRequest": raw, "productionReturn": returned, "coreRequest": value, "core": planned,
            "compatibilityRequired": ["androidDurationCoalescingDropsClaimedRow"] if kind == "changeDuration" else []})
    evidence = os.environ.get("PROBE_EVIDENCE")
    if evidence: Path(evidence).write_text(json.dumps(receipts, indent=2) + "\n")
    print(f"{len(rows)} extracted Android complete plan returns compared; duration retention difference recorded")


if __name__ == "__main__": main()
