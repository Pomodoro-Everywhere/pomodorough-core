import java.time.Instant
import java.time.ZoneId

// Test-only wire models. Decision methods are extracted from production at runtime.
data class TimerCommand(val id: String, val timerId: String, val type: String,
    val phase: String, val deviceSequence: Long, val occurredAt: String,
    val physicalOccurredAt: String? = null)
data class Intent(val commandId: String)
data class CanonicalTimer(val id: String, val phase: String, val status: String,
    val plannedDurationMs: Long, val anchorAt: String, val lastIntent: Intent? = null,
    val taskId: String? = null)
data class HistoryItem(val id: String, val timerId: String, val commandId: String?,
    val phase: String, val status: String, val plannedDurationMs: Long,
    val completedAt: String? = null, val endedAt: String? = null, val taskId: String? = null)
data class Acknowledgement(val commandId: String, val outcome: String)
data class SyncResponse(val canonicalTimer: CanonicalTimer?, val history: List<HistoryItem>,
    val acknowledgements: List<Acknowledgement>)
data class TimerProjection(val timer: CanonicalTimer?)
data class Settings(val autoStartBreaks: Boolean = false)
data class Local(val deviceId: String = "probe", val ownedTimerId: String? = null)
data class CentralizedSyncSnapshot(val selectedPhaseGeneration: Long,
    val settings: Settings = Settings(), val local: Local = Local())
object CommandType { const val Finish = "finish" }
object TimerStatus { const val Completed = "completed" }
data class CoreFinishAppliedInput(val commandId: String, val timerId: String,
    val phase: String, val occurredAt: String, val history: List<HistoryItem>,
    val autoStartBreaks: Boolean, val localDeviceId: String, val ownedTimerId: String?,
    val reference: Instant, val zoneId: ZoneId)
data class Decision(val selectedPhase: String)

fun quoted(value: String?): String = value?.let {
    "\"" + it.replace("\\", "\\\\").replace("\"", "\\\"")
        .replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t") + "\""
} ?: "null"

fun historyJson(item: HistoryItem): String = "{" + listOf(
    "id" to item.id, "timerId" to item.timerId, "commandId" to item.commandId,
    "phase" to item.phase, "status" to item.status, "completedAt" to item.completedAt,
    "endedAt" to item.endedAt, "taskId" to item.taskId
).joinToString(",") { (key, value) -> quoted(key) + ":" + quoted(value) } +
    ",\"plannedDurationMs\":" + item.plannedDurationMs + "}"

class Dispatcher(private val bridge: String) {
    fun finishApplied(input: CoreFinishAppliedInput): Decision {
        val day = input.reference.atZone(input.zoneId).toLocalDate()
        val source = listOf("commandId" to input.commandId, "timerId" to input.timerId,
            "phase" to input.phase, "occurredAt" to input.occurredAt)
            .joinToString(",") { (key, value) -> quoted(key) + ":" + quoted(value) }
        val history = input.history.joinToString(",", "[", "]", transform = ::historyJson)
        val request = """{"operation":"timer.completionPlan.v1","input":{"kind":"finishApplied",
            "source":{$source},"history":$history,"autoStartBreaks":false,
            "localDeviceId":"probe","ownership":null,
            "dayStart":"${day.atStartOfDay(input.zoneId).toInstant()}",
            "dayEnd":"${day.plusDays(1).atStartOfDay(input.zoneId).toInstant()}"}}"""
        val process = ProcessBuilder(bridge).redirectError(ProcessBuilder.Redirect.INHERIT).start()
        process.outputStream.bufferedWriter().use { it.write(request.replace("\n", " ") + "\n") }
        val output = process.inputStream.bufferedReader().readText()
        check(process.waitFor() == 0) { output }
        Regex("\"error\":\"([^\"]+)\"").find(output)?.let {
            error("error:" + it.groupValues[1])
        }
        return Decision(Regex("\"selectedPhase\":\"([^\"]+)\"").find(output)!!.groupValues[1])
    }
}
