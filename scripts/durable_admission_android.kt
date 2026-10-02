package me.egigoka.pomodorough.data

import java.time.Instant
import java.time.ZoneId
import java.util.UUID
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.*
import me.egigoka.pomodorough.data.local.*
import kotlin.coroutines.*

// INSERT_PENDING_MODEL
private val json = Json { ignoreUnknownKeys=true; explicitNulls=false }
private val complete = Json { encodeDefaults=true; explicitNulls=true }

private fun dispatch(bridge: String, operation: String, raw: String): JsonElement {
    val process = ProcessBuilder(bridge).start()
    process.outputStream.bufferedWriter().use { it.write("{\"operation\":\"$operation\",\"input\":$raw}\n") }
    val output = process.inputStream.bufferedReader().readText()
    check(process.waitFor() == 0) { process.errorStream.bufferedReader().readText() }
    val value = json.parseToJsonElement(output).jsonObject
    value["error"]?.let { error(it.jsonPrimitive.content) }
    return value
}

private fun reflected(value: Any): JsonObject = buildJsonObject {
    for (field in value.javaClass.declaredFields.filterNot { it.isSynthetic || java.lang.reflect.Modifier.isStatic(it.modifiers) }) {
        field.isAccessible = true
        when (val item = field.get(value)) {
            null -> put(field.name, JsonNull)
            is String -> put(field.name, item)
            is Boolean -> put(field.name, item)
            is Number -> put(field.name, item)
            else -> error("Unsupported entity field ${field.name}")
        }
    }
}

private fun queues(raw: JsonObject) = PendingSyncQueues(
    json.decodeFromJsonElement(raw.getValue("commands")), json.decodeFromJsonElement(raw.getValue("taskOperations")),
    json.decodeFromJsonElement(raw.getValue("durationOperations")), json.decodeFromJsonElement(raw.getValue("autoStartOperations")),
    raw.getValue("selectedTaskOperations").jsonArray.map { json.decodeFromJsonElement<SelectedTaskOperation>(
        JsonObject(it.jsonObject.filterKeys { key -> key != "deviceId" })) })

private fun encodeQueues(value: PendingSyncQueues): JsonObject = buildJsonObject {
    put("commands", complete.encodeToJsonElement(value.commands)); put("taskOperations", complete.encodeToJsonElement(value.taskOperations))
    put("durationOperations", complete.encodeToJsonElement(value.durationOperations)); put("autoStartOperations", complete.encodeToJsonElement(value.autoStartOperations))
    put("selectedTaskOperations", complete.encodeToJsonElement(value.selectedTaskOperations))
}

private class Claim(val queues: PendingSyncQueues) {
    var commands = queues.commands.map { PendingCommandEntity.from(it) }
    var tasks = queues.taskOperations.map { PendingTaskOperationEntity.from(it) }
    var durations = queues.durationOperations.map { PendingDurationOperationEntity.from(it) }
    var autoStarts = queues.autoStartOperations.map { PendingAutoStartOperationEntity.from(it) }
    var selections = queues.selectedTaskOperations.map { PendingSelectedTaskOperationEntity.from(it) }
    suspend fun clearCommandNeverSent(ids: List<String>) { commands = commands.map { if (it.id in ids) it.copy(neverSent=false) else it } }
    suspend fun clearTaskNeverSent(ids: List<String>) { tasks = tasks.map { if (it.id in ids) it.copy(neverSent=false) else it } }
    suspend fun clearDurationNeverSent(ids: List<String>) { durations = durations.map { if (it.id in ids) it.copy(neverSent=false) else it } }
    suspend fun clearAutoStartNeverSent(ids: List<String>) { autoStarts = autoStarts.map { if (it.id in ids) it.copy(neverSent=false) else it } }
    suspend fun clearSelectedTaskNeverSent(ids: List<String>) { selections = selections.map { if (it.id in ids) it.copy(neverSent=false) else it } }
    // INSERT_RETIRE_METHOD
    fun rows(): JsonObject = buildJsonObject {
        for ((domain, items) in mapOf("commands" to commands, "taskOperations" to tasks, "durationOperations" to durations,
            "autoStartOperations" to autoStarts, "selectedTaskOperations" to selections)) put(domain, JsonArray(items.map(::reflected)))
    }
    fun proof(): JsonObject = buildJsonObject {
        put("commands", json.encodeToJsonElement(commands.filter { it.neverSent }.map { it.id }))
        put("taskOperations", json.encodeToJsonElement(tasks.filter { it.neverSent }.map { it.id }))
        put("durationOperations", json.encodeToJsonElement(durations.filter { it.neverSent }.map { it.id }))
        put("autoStartOperations", json.encodeToJsonElement(autoStarts.filter { it.neverSent }.map { it.id }))
        put("selectedTaskOperations", json.encodeToJsonElement(selections.filter { it.neverSent }.map { it.id }))
    }
    fun decoded() = PendingSyncQueues(commands.map { it.toModel() }, tasks.map { it.toModel() }, durations.map { it.toModel() },
        autoStarts.map { it.toModel() }, selections.map { it.toModel() })
}

private fun claim(pending: PendingSyncQueues): JsonObject {
    val store = Claim(pending)
    val before = store.rows()
    var failure: Throwable? = null
    val action: suspend () -> Unit = { store.retireNeverSent(pending.commands.map { it.id }, pending.taskOperations.map { it.id },
        pending.durationOperations.map { it.id }, pending.autoStartOperations.map { it.id }, pending.selectedTaskOperations.map { it.id }) }
    action.startCoroutine(object: Continuation<Unit> {
        override val context = EmptyCoroutineContext
        override fun resumeWith(result: Result<Unit>) { failure = result.exceptionOrNull() }
    })
    failure?.let { throw it }
    val saved = buildJsonObject { put("rows", store.rows()); put("proof", store.proof()); put("outgoing", encodeQueues(pending)) }
    val file = kotlin.io.path.createTempFile("room-extraction-", ".json").toFile()
    file.writeText(saved.toString())
    val restored = json.parseToJsonElement(file.readText()).jsonObject
    check(restored == saved)
    file.delete()
    return buildJsonObject { put("rowsBefore", before); put("rowsAfter", store.rows()); put("proofAfter", store.proof())
        put("queuesAfter", encodeQueues(store.decoded())); put("savedRaw", saved.toString()); put("restored", restored) }
}

private fun reserve(value: JsonObject, state: TimerMutationState, bridge: String, count: Int, commands: Boolean): TimerMutationReservation {
    val now = Instant.parse(value.getValue("clock").jsonObject.getValue("occurredAt").jsonPrimitive.content).toEpochMilli()
    val clocks = CoreHlcDispatcher { operation, raw -> dispatch(bridge, operation, raw) }.reserve(
        now, CoreHlc(state.local.hlcWallMs, state.local.hlcCounter), count)
    val stamps = SyncWireBounds.mutationStamps(now, clocks, state.local.deviceSequence, commands)
    val ids = value.getValue("identities").jsonObject.getValue("commandUuids").jsonArray.take(count)
        .map { UUID.fromString(it.jsonPrimitive.content) }
    return TimerMutationReservation(stamps, ids, ids.last().toString())
}

private fun run(value: JsonObject, bridge: String): JsonObject {
    val workspace = value.getValue("workspace").jsonObject
    val baseRaw = workspace.getValue("base").jsonObject
    val claimed = claim(queues(workspace.getValue("local").jsonObject))
    val pending = queues(claimed.getValue("queuesAfter").jsonObject)
    val base = CoreProjectionBase(json.decodeFromJsonElement(baseRaw.getValue("canonicalTimer")),
        json.decodeFromJsonElement(baseRaw.getValue("history")), json.decodeFromJsonElement(baseRaw.getValue("tasks")),
        json.decodeFromJsonElement(baseRaw.getValue("durationsMs")), baseRaw.getValue("autoStartBreaks").jsonPrimitive.boolean,
        baseRaw["selectedTaskId"]?.jsonPrimitive?.contentOrNull)
    val projection = CoreProjectionDispatcher { operation, raw -> dispatch(bridge, operation, raw) }
    val device = value.getValue("allocation").jsonObject.getValue("deviceId").jsonPrimitive.content
    val now = Instant.parse(value.getValue("clock").jsonObject.getValue("physicalNow").jsonPrimitive.content)
    val before = projection.apply(base, CoreProjectionPending(pending.commands.map { DeviceOperation(device, it) },
        pending.taskOperations.map { DeviceOperation(device, it) }, pending.durationOperations.map { DeviceOperation(device, it) },
        pending.autoStartOperations.map { DeviceOperation(it.deviceId, it) }, pending.selectedTaskOperations.map { DeviceOperation(device, it) }), now)
    val settings = json.decodeFromJsonElement<TimerSettings>(value.getValue("nativeSettings"))
    val allocation = value.getValue("allocation").jsonObject
    val clock = allocation.getValue("hlc").jsonObject
    val local = LocalStateEntity(deviceId=device, settingsJson=complete.encodeToString(settings),
        canonicalTimerJson=base.canonicalTimer?.let { complete.encodeToString(it) },
        historyJson=complete.encodeToString(base.history), tasksJson=complete.encodeToString(base.tasks),
        knownTasksJson=complete.encodeToString(before.tasks), canonicalAutoStartBreaks=base.autoStartBreaks,
        deviceSequence=allocation.getValue("deviceSequence").jsonPrimitive.long,
        hlcWallMs=clock.getValue("wallMs").jsonPrimitive.long, hlcCounter=clock.getValue("counter").jsonPrimitive.long,
        selectedTaskId=before.selectedTaskId, ownedTimerId=before.canonicalTimer?.id,
        lastUuidV7=allocation["lastUuid"]?.jsonPrimitive?.contentOrNull)
    val state = TimerMutationState(local, settings, TimerProjection(before.canonicalTimer, before.history), base, pending,
        emptyMap(), before.tasks.associateBy { it.id }, before.tasks, before.selectedTaskId)
    val coordinator = TimerMutationCoordinator(json, projection, CoreCompletionDispatcher { operation, raw -> dispatch(bridge, operation, raw) },
        ZoneId.of("UTC"), timerId={value.getValue("identities").jsonObject.getValue("timerUuid").jsonPrimitive.content})
    return JsonObject(transition(value, state, coordinator, bridge, now.toEpochMilli()) + ("claim" to claimed))
}

private fun transition(value: JsonObject, state: TimerMutationState, coordinator: TimerMutationCoordinator, bridge: String, now: Long): JsonObject {
    val intent = value.getValue("intent").jsonObject
    val kind = intent.getValue("kind").jsonPrimitive.content
    val next = when (kind) {
        "upsertTask", "deleteTask", "addAndSelectTask" -> {
            val task = json.decodeFromJsonElement<FocusTask>(value.getValue("task"))
            val select = kind == "addAndSelectTask"
            coordinator.task(TaskMutationInput(state, if (kind != "deleteTask") "upsert" else "delete", task, select,
                reserve(value, state, bridge, if (select) 2 else 1, false)))
        }
        "selectTask" -> coordinator.selectedTask(SelectedTaskMutationInput(state, intent["taskId"]?.jsonPrimitive?.contentOrNull, reserve(value, state, bridge, 1, false)))
        "setAutoStart" -> coordinator.autoStart(AutoStartMutationInput(state, intent.getValue("enabled").jsonPrimitive.boolean, reserve(value, state, bridge, 1, false)))
        "changeDuration" -> coordinator.duration(DurationMutationInput(state, intent.getValue("phase").jsonPrimitive.content, intent.getValue("delta").jsonPrimitive.int, reserve(value, state, bridge, 1, false)))
        "cancelAndClear" -> coordinator.cancel(TimerCancelMutationInput(state, state.projection.timer!!,
            coordinator.cancelAndClearTypes(state.projection.timer!!), reserve(value, state, bridge, 2, true), now))
        "finish" -> coordinator.finish(TimerFinishMutationInput(state, state.projection.timer!!, CoreCommandRequestDecision(true, false), reserve(value, state, bridge, 1, true), now))
        else -> coordinator.command(TimerCommandMutationInput(state, kind, null, reserve(value, state, bridge, 1, true), now))
    }
    return encode(next, state)
}

private fun encode(next: TimerMutationTransition<*>, state: TimerMutationState): JsonObject = buildJsonObject {
    if (next is TimerMutationTransition.Ignored) { put("outcome", "ignored"); return@buildJsonObject }
    put("outcome", "planned")
    val plan = (next as TimerMutationTransition.Planned<*>).plan
    when (plan) {
        is TimerCommandMutationPlan -> {
            put("commands", complete.encodeToJsonElement(plan.commands)); put("dependencies", complete.encodeToJsonElement(plan.dependencies))
            put("commandPhysicalTimes", complete.encodeToJsonElement(plan.commands.associate { it.id to it.physicalOccurredAt }))
            put("local", reflected(plan.local)); put("settings", complete.encodeToJsonElement(plan.settings)); put("projection", complete.encodeToJsonElement(plan.projection))
        }
        is TaskMutationPlan -> {
            put("operation", complete.encodeToJsonElement(plan.operation)); put("selectedOperation", complete.encodeToJsonElement(plan.selectedOperation))
            put("local", reflected(plan.local)); put("knownTasks", complete.encodeToJsonElement(plan.knownTasks)); put("taskOperations", complete.encodeToJsonElement(plan.taskOperations))
            put("selectedTaskOperations", complete.encodeToJsonElement(plan.selectedTaskOperations)); put("projection", complete.encodeToJsonElement(plan.projection))
        }
        is DurationMutationPlan -> {
            put("operation", complete.encodeToJsonElement(plan.operation)); put("local", reflected(plan.local)); put("settings", complete.encodeToJsonElement(plan.settings))
            put("operations", complete.encodeToJsonElement(plan.operations)); put("projection", complete.encodeToJsonElement(plan.projection))
        }
        is AutoStartMutationPlan -> {
            put("operation", complete.encodeToJsonElement(plan.operation)); put("local", reflected(plan.local)); put("settings", complete.encodeToJsonElement(plan.settings))
            put("operations", complete.encodeToJsonElement(plan.operations)); put("projection", complete.encodeToJsonElement(plan.projection))
        }
        is SelectedTaskMutationPlan -> {
            put("operation", complete.encodeToJsonElement(plan.operation)); put("local", reflected(plan.local)); put("operations", complete.encodeToJsonElement(plan.operations))
            put("projection", complete.encodeToJsonElement(plan.projection))
        }
        else -> error("Unencoded complete production plan")
    }
    put("rawLocalBefore", reflected(state.local))
    put("rawQueuesBefore", encodeQueues(state.queues))
    val local = when (plan) {
        is TimerCommandMutationPlan -> plan.local
        is TaskMutationPlan -> plan.local
        is DurationMutationPlan -> plan.local
        is AutoStartMutationPlan -> plan.local
        is SelectedTaskMutationPlan -> plan.local
        else -> error("Unknown plan")
    }
    put("localDecodedSettings", complete.encodeToJsonElement(json.decodeFromString<TimerSettings>(local.settingsJson)))
    put("rawDecodedSettings", complete.encodeToJsonElement(json.decodeFromString<TimerSettings>(state.local.settingsJson)))
}

fun main(args: Array<String>) {
    generateSequence(::readLine).forEach { line ->
        try { println(run(json.parseToJsonElement(line).jsonObject, args[0])) }
        catch (error: Exception) { println(buildJsonObject { put("productionError", error.message ?: error.toString()) }) }
    }
}
