
enum AppError: Error { case invalidLocalClock, invalidResponse }
extension TrustedClockState: Codable {}
extension TrustedClockState {
    func sourceOccurrenceMilliseconds(for date: Date, uptime: Double) throws -> Int64 {
        let candidate = try trustedCandidateMilliseconds(for: date, uptime: uptime)
        return try monotonicTrustedMilliseconds(after: candidate)
    }
}
struct ProbeInput: Decodable {
    struct Reading: Decodable { let wallSeconds: Double?; let uptimeSeconds: Double? }
    struct Server: Decodable {
        let serverTimeMs: Int64; let requestWallMs: Int64
        let requestUptimeSeconds: Double; let responseUptimeSeconds: Double
    }
    let action: String; let state: TrustedClockState; let reading: Reading
    let server: Server?; let trustedAnchorMs: Int64?; let trustedAnchorSeconds: Double?
}
func stateFields(_ state: TrustedClockState) -> [String: Any] {
    ["offsetMs": state.offsetMs as Any? ?? NSNull(),
     "uncertaintyMs": state.uncertaintyMs as Any? ?? NSNull(),
     "anchorMs": state.anchorMs as Any? ?? NSNull(),
     "anchorUptime": state.anchorUptime as Any? ?? NSNull(),
     "lastEmittedMs": state.lastEmittedMs as Any? ?? NSNull()]
}
func observe(_ input: ProbeInput) throws -> [String: Any] {
    var state = input.state
    var now: Int64?
    var trustedSeconds: Double?
    var occurrenceWall: Int64?
    if input.action == "sample" {
        let sample = input.server!
        try validateSampleUptime(sample.requestUptimeSeconds, sample.responseUptimeSeconds)
        state = try state.resampled(serverTimeMs: sample.serverTimeMs,
            requestWallMs: sample.requestWallMs, requestUptime: sample.requestUptimeSeconds,
            responseUptime: sample.responseUptimeSeconds).state
    } else {
        let localDate = Date(timeIntervalSince1970: input.reading.wallSeconds ?? 0)
        let transition = try state.occurrenceTransition(for: localDate,
            uptime: input.reading.uptimeSeconds!)
        now = try state.sourceOccurrenceMilliseconds(for: localDate, uptime: input.reading.uptimeSeconds!)
        state = transition.state
        trustedSeconds = transition.trustedDate.timeIntervalSince1970
        occurrenceWall = WireBounds.physicalMilliseconds(for: transition.trustedDate)
        if input.action == "advance" {
            state = state.recordingTrustedMilliseconds(WireBounds.physicalMilliseconds(for: transition.trustedDate)!)
        }
    }
    let rawAnchor = input.trustedAnchorSeconds.map { Date(timeIntervalSince1970: $0) }
        ?? input.trustedAnchorMs.flatMap { WireBounds.date(milliseconds: $0) }
    let physicalDate = try rawAnchor.map { try state.physicalDate(forTrustedDate: $0) }
    let physical = physicalDate.flatMap { Int64(exactly: ($0.timeIntervalSince1970 * 1000).rounded()) }
    return ["state":stateFields(state), "trustedNowMs":now as Any? ?? NSNull(),
        "occurrenceWallMs":occurrenceWall as Any? ?? NSNull(),
        "trustedDateSeconds":trustedSeconds as Any? ?? NSNull(),
        "physicalDeltaMs": state.offsetMs.map { -$0 } as Any? ?? NSNull(),
        "physicalAnchorMs":physical as Any? ?? NSNull(),
        "physicalAnchorSeconds":physicalDate.map { $0.timeIntervalSince1970 } as Any? ?? NSNull()]
}
while let line = readLine() {
    do {
        let input = try JSONDecoder().decode(ProbeInput.self, from: Data(line.utf8))
        let result = try observe(input)
        print(String(data: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), encoding: .utf8)!)
    } catch { print("{\"error\":true}") }
}
