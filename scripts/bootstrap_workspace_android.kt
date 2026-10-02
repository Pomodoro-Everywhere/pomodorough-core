package me.egigoka.pomodorough.data

import java.time.Instant
import kotlinx.serialization.encodeToString
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.*

internal data class Local(val selectedTaskId: String?)
// INSERT_PENDING_MODEL
internal class Probe(val projection: TimerProjection, val tasks: List<FocusTask>,
    val pending: List<TimerCommand>, val pendingTaskOperations: List<TaskOperation>,
    val pendingDurationOperations: List<DurationOperation>, val pendingAutoStartOperations: List<AutoStartOperation>,
    val pendingSelectedTaskOperations: List<SelectedTaskOperation>, val local: Local, val settings: TimerSettings) {
    // INSERT_METHODS
    fun classify(remote: JsonObject): JsonObject = buildJsonObject {
        val response = SyncResponse(emptyList(), 0,
            json.decodeFromJsonElement<CanonicalTimer?>(remote.getValue("canonicalTimer")),
            json.decodeFromJsonElement<List<HistoryItem>>(remote.getValue("history")),
            "2026-09-21T12:00:00Z", 1, 0, emptyList(),
            json.decodeFromJsonElement<DurationsMs>(remote.getValue("durationsMs")), emptyList(),
            json.decodeFromJsonElement<List<FocusTask>>(remote.getValue("tasks")),
            autoStartBreaks=remote.getValue("autoStartBreaks").jsonPrimitive.boolean,
            selectedTaskId=remote["selectedTaskId"]?.jsonPrimitive?.contentOrNull)
        put("hasLocalState", hasLocalSyncState())
        put("hasRemoteState", hasRemoteSyncState(response))
        put("localDisplayHistoryCount", visibleHistoryCount(projection.history))
        put("remoteDisplayHistoryCount", visibleHistoryCount(response.history))
    }
}

private val json = Json { ignoreUnknownKeys=true; explicitNulls=false }
private val completeJson = Json { encodeDefaults=true; explicitNulls=true }
private fun dispatch(bridge: String, operation: String, raw: String): JsonObject {
    val process = ProcessBuilder(bridge).start()
    process.outputStream.bufferedWriter().use { it.write("{\"operation\":\"$operation\",\"input\":$raw}\n") }
    val output = process.inputStream.bufferedReader().readText()
    check(process.waitFor() == 0) { process.errorStream.bufferedReader().readText() }
    val envelope = json.parseToJsonElement(output).jsonObject
    check(json.parseToJsonElement(envelope.getValue("inputRaw").jsonPrimitive.content) == envelope["inputDecoded"])
    check(envelope["inputDecoded"] == json.parseToJsonElement(raw))
    if (envelope.containsKey("error")) error(envelope.getValue("error").jsonPrimitive.content)
    check(json.parseToJsonElement(envelope.getValue("raw").jsonPrimitive.content) == envelope["decoded"])
    return envelope
}

private fun queues(value: JsonObject): PendingSyncQueues = PendingSyncQueues(
    json.decodeFromJsonElement(value.getValue("commands")),
    json.decodeFromJsonElement(value.getValue("taskOperations")),
    json.decodeFromJsonElement(value.getValue("durationOperations")),
    json.decodeFromJsonElement(value.getValue("autoStartOperations")),
    value.getValue("selectedTaskOperations").jsonArray.map {
        json.decodeFromJsonElement<SelectedTaskOperation>(JsonObject(it.jsonObject.filterKeys { key -> key != "deviceId" }))
    })

private fun run(value: JsonObject, bridge: String): JsonObject {
    val local = value.getValue("local").jsonObject
    val workspace = local.getValue("workspace").jsonObject
    val base = workspace.getValue("base").jsonObject
    val settings = json.decodeFromJsonElement<TimerSettings>(local.getValue("preferences"))
    val pending = queues(workspace.getValue("local").jsonObject)
    val observation = buildJsonObject {
        put("base", base)
        put("queues", workspace.getValue("local"))
        put("preferences", local.getValue("preferences"))
    }
    val request = SynchronizedProjectionRequestFactory.create(CoreProjectionBase(
        json.decodeFromJsonElement(base.getValue("canonicalTimer")),
        json.decodeFromJsonElement(base.getValue("history")),
        json.decodeFromJsonElement(base.getValue("tasks")), settings.effectiveDurationsMs(),
        base.getValue("autoStartBreaks").jsonPrimitive.boolean,
        local.getValue("preferences").jsonObject["selectedTaskId"]?.jsonPrimitive?.contentOrNull),
        pending, "device-a")
    val calls = mutableListOf<JsonObject>()
    val dispatcher = CoreProjectionDispatcher { operation, raw ->
        dispatch(bridge, operation, raw).also { calls.add(it) }.getValue("decoded")
    }
    val result = dispatcher.apply(request.base, request.pending, request.horizon)
    val projectionRaw = completeJson.encodeToString(result)
    val probe = Probe(TimerProjection(result.canonicalTimer, result.history), result.tasks,
        pending.commands, pending.taskOperations, pending.durationOperations, pending.autoStartOperations,
        pending.selectedTaskOperations, Local(request.base.selectedTaskId), settings)
    return buildJsonObject {
        for ((key, field) in probe.classify(value.getValue("remote").jsonObject)) put(key, field)
        put("projectionRaw", projectionRaw)
        put("projectionDecoded", json.parseToJsonElement(projectionRaw))
        put("projectionCall", calls.single())
        put("observationRaw", observation.toString())
        put("observationDecoded", observation)
        put("localHistory", json.encodeToJsonElement(result.history))
        put("horizon", request.horizon.toString())
    }
}

fun main(args: Array<String>) {
    generateSequence(::readLine).forEach { line ->
        try { println(run(json.parseToJsonElement(line).jsonObject, args[0])) }
        catch (error: Exception) { println(buildJsonObject { put("productionError", error.message ?: error.toString()) }) }
    }
}
