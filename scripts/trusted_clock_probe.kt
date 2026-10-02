
data class RequestTiming(val uncertaintyMs:Long, val midpointPhysicalMs:Long, val midpointElapsedRealtimeMs:Long)
data class ServerClockSample(val offsetMs:Long, val uncertaintyMs:Long, val serverTimeMs:Long,
    val midpointPhysicalMs:Long, val midpointElapsedRealtimeMs:Long) {
    fun fields() = mapOf("offsetMs" to offsetMs, "uncertaintyMs" to uncertaintyMs, "serverTimeMs" to serverTimeMs,
        "midpointPhysicalMs" to midpointPhysicalMs, "midpointElapsedRealtimeMs" to midpointElapsedRealtimeMs)
}
data class SyncResponse(val serverTime:String, val serverHlcWallMs:Long)
class SyncProtocolException(message:String): RuntimeException(message)
data class LocalStateEntity(val serverClockOffsetMs:Long?, val serverClockUncertaintyMs:Long?,
    val serverClockSamplePhysicalMs:Long?, val serverClockSampleElapsedRealtimeMs:Long?, val serverClockBootId:String?,
    val hlcWallMs:Long) {
    fun fields() = mapOf("serverClockOffsetMs" to serverClockOffsetMs, "serverClockUncertaintyMs" to serverClockUncertaintyMs,
        "serverClockSamplePhysicalMs" to serverClockSamplePhysicalMs,
        "serverClockSampleElapsedRealtimeMs" to serverClockSampleElapsedRealtimeMs,
        "serverClockBootId" to serverClockBootId, "retainedWallMs" to hlcWallMs)
}
data class AnchorInput(val serverTimeMs:Long, val elapsedRealtimeMs:Long)
fun encode(value:Any?):String = when(value) {
    null -> "null"
    is String -> "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\""
    is Map<*,*> -> value.entries.joinToString(",", "{", "}") { encode(it.key) + ":" + encode(it.value) }
    else -> value.toString()
}
internal fun anchorFields(clock:TrustedClock):Map<String,Long>? {
    val field=clock.javaClass.getDeclaredField("anchor"); field.isAccessible=true
    val anchor=field.get(clock) ?: return null
    fun number(name:String):Long {
        val member=anchor.javaClass.getDeclaredField(name); member.isAccessible=true
        return member.getLong(anchor)
    }
    return mapOf("serverTimeMs" to number("serverTimeMs"), "elapsedRealtimeMs" to number("elapsedRealtimeMs"))
}
fun runProbe(action:String, local:LocalStateEntity, prior:ServerClockSample?, anchor:AnchorInput?,
    wall:Long, elapsed:Long, boot:String?, response:SyncResponse?, timing:LongArray?, trustedAnchor:Long?):String {
    val clock=TrustedClock({wall},{elapsed},{boot})
    if(anchor != null) clock.install(ServerClockSample(0,0,anchor.serverTimeMs,0,anchor.elapsedRealtimeMs))
    var stored=local
    var sample:ServerClockSample?=null
    var now:Long?=null
    return try {
        StoredClockValidation.validate(local)
        when(action) {
            "sample" -> sample=clock.sample(response!!,timing!![0],timing[1],timing[2],timing[3])
            "advance" -> sample=clock.advance(prior!!,response!!,timing!![0],timing[1],timing[2],timing[3])
            "restore" -> stored=clock.invalidateStaleElapsedAnchor(local) ?: local
            else -> now=clock.now(local,prior)
        }
        val state=stored.fields() + mapOf("anchor" to anchorFields(clock), "requestSample" to prior?.fields())
        val offset=sample?.offsetMs ?: stored.serverClockOffsetMs
        val delta=offset?.let { clock.responsePhysicalDelta(ServerClockSample(it,0,0,0,0)) }
        val physical=trustedAnchor?.let { value ->
            Instant.parse(PhysicalMapper.translatePhysicalInstant(Instant.ofEpochMilli(value).toString(), delta ?: 0L)).toEpochMilli()
        }
        encode(mapOf("state" to state, "sample" to sample?.fields(), "trustedNowMs" to now,
            "sampleStale" to (sample ?: prior)?.let { clock.isStale(it) },
            "physicalDeltaMs" to delta, "physicalAnchorMs" to physical))
    } catch(error:RuntimeException) { "{\"error\":true}" }
}
