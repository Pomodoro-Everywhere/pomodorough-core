import Foundation

// Model dependencies only. The two decision bodies come from current sources.
// INSERT_DURATION_MODELS
struct Settings { let durationsMs: DurationValues }
struct PersistedTimerState {
    let pendingCommands: [Any]
    let pendingTaskOperations: [Any]
    let pendingDurationOperations: [Any]
    let pendingAutoStartOperations: [Any]
    let pendingSelectedTaskOperations: [Any]
    let canonicalTimer: Any?
    let history: [Any]
    let tasks: [Any]
    let settings: Settings
}
struct BootstrapResponse {
    let canonicalTimer: Any?
    let history: [Any]
    let tasks: [Any]
    let durationsMs: DurationValues
    let autoStartBreaks: Bool
}
struct Probe {
    let timerState: PersistedTimerState
    let autoStartBreaks: Bool
    // INSERT_LOCAL_METHOD
    // INSERT_REMOTE_METHOD

    func run(_ remote: BootstrapResponse) -> [String: Bool] {
        ["hasLocalState": hasLocalBootstrapState,
         "hasRemoteState": Self.hasRemoteBootstrapState(remote)]
    }
}

func optional(_ value: Any?) -> Any? { value is NSNull ? nil : value }
func durations(_ value: Any?) -> DurationValues {
    let map = value as! [String: NSNumber]
    return DurationValues(focus: map["focus"]!.int64Value,
        shortBreak: map["short_break"]!.int64Value, longBreak: map["long_break"]!.int64Value)
}
func run(_ value: [String: Any]) -> [String: Bool] {
    let local = value["local"] as! [String: Any]
    let workspace = local["workspace"] as! [String: Any]
    let base = workspace["base"] as! [String: Any]
    let queues = workspace["local"] as! [String: Any]
    let preferences = local["preferences"] as! [String: Any]
    let remote = value["remote"] as! [String: Any]
    let state = PersistedTimerState(
        pendingCommands: queues["commands"] as! [Any], pendingTaskOperations: queues["taskOperations"] as! [Any],
        pendingDurationOperations: queues["durationOperations"] as! [Any],
        pendingAutoStartOperations: queues["autoStartOperations"] as! [Any],
        pendingSelectedTaskOperations: queues["selectedTaskOperations"] as! [Any],
        canonicalTimer: optional(base["canonicalTimer"]), history: base["history"] as! [Any],
        tasks: base["tasks"] as! [Any], settings: Settings(durationsMs: durations(preferences["durationsMs"])))
    let response = BootstrapResponse(canonicalTimer: optional(remote["canonicalTimer"]),
        history: remote["history"] as! [Any], tasks: remote["tasks"] as! [Any],
        durationsMs: durations(remote["durationsMs"]), autoStartBreaks: remote["autoStartBreaks"] as! Bool)
    return Probe(timerState: state, autoStartBreaks: preferences["autoStartBreaks"] as! Bool).run(response)
}
while let line = readLine() {
    let value = try! JSONSerialization.jsonObject(with: Data(line.utf8)) as! [String: Any]
    let result = run(value)
    let raw = String(data: try! JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), encoding: .utf8)!
    print(raw)
}
