import Foundation
import OSLog

enum AppError: Error { case invalidLocalClock }
enum ReplicationMode: String, Codable, Sendable { case centralized, iroh }
enum SentryCapture { static func captureOnce(key: String, error: Error) {} }
enum SharedCoreError: Error { case invalidResponse(String) }
struct IrohAutoStartOperation { let id: String; let enabled: Bool; let occurredAt: Date; let hlcWallMs: Int64; let hlcCounter: Int64 }
struct IrohSelectedTaskOperation { let id: String; let taskId: String?; let occurredAt: Date; let hlcWallMs: Int64; let hlcCounter: Int64 }

let bridge = CommandLine.arguments[1]
func dispatch(_ operation: String, _ input: Any) throws -> [String: Any] {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: bridge)
    let writer = Pipe(), reader = Pipe()
    process.standardInput = writer; process.standardOutput = reader
    try process.run()
    writer.fileHandleForWriting.write(try JSONSerialization.data(withJSONObject: ["operation": operation, "input": input]))
    writer.fileHandleForWriting.write(Data([10])); try writer.fileHandleForWriting.close()
    let bytes = reader.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    guard process.terminationStatus == 0 else { throw AppError.invalidLocalClock }
    let value = try JSONSerialization.jsonObject(with: bytes) as! [String: Any]
    if let error = value["error"] as? String { throw SharedCoreError.invalidResponse(error) }
    return value
}

let encoder: JSONEncoder = {
    let value = JSONEncoder(); value.dateEncodingStrategy = .iso8601; value.outputFormatting = [.sortedKeys]; return value
}()
let decoder: JSONDecoder = { let value = JSONDecoder(); value.dateDecodingStrategy = .iso8601; return value }()
func encoded<T: Encodable>(_ value: T) throws -> Any { try JSONSerialization.jsonObject(with: encoder.encode(value)) }
func decode<T: Decodable>(_ type: T.Type, _ value: Any) throws -> T {
    try decoder.decode(type, from: JSONSerialization.data(withJSONObject: value))
}
struct TrustedClock: Codable, Equatable, Sendable {
    func recordingTrustedMilliseconds(_ value: Int64) -> Self { self }
}

struct PersistedTimerState: Codable, Equatable, Sendable {
    var settings = TimerSettings()
    var selectedPhaseGeneration: Int64 = 5
    var hasExplicitPhaseSelection = true
    var selectedTaskID: UUID?
    var deviceId = "device-local"
    var nextSequence: Int64 = 8
    var sequenceExhausted = false
    var hlcWallMs: Int64 = 1784548800000
    var hlcCounter: Int64 = 0
    var lastUuidV7: UUID?
    var revision: Int64 = 0
    var canonicalTimer: CanonicalTimer?
    var history: [HistoryItem] = []
    var tasks: [FocusTask] = []
    var knownTasks: [FocusTask] = []
    var baseAutoStart = false
    var baseSelection: String?
    var canonicalHeadWallMs: Int64?
    var canonicalHeadCounter: Int64?
    var pendingCommands: [TimerCommand] = []
    var pendingTaskOperations: [TaskOperation] = []
    var pendingDurationOperations: [DurationOperation] = []
    var pendingAutoStartOperations: [AutoStartOperation] = []
    var pendingSelectedTaskOperations: [SelectedTaskOperation] = []
    var neverSentCommandIDs = Set<String>()
    var neverSentTaskOperationIDs = Set<String>()
    var neverSentDurationOperationIDs = Set<String>()
    var neverSentAutoStartOperationIDs = Set<String>()
    var neverSentSelectedTaskOperationIDs = Set<String>()
    var localCommandDates: [String: Date] = [:]
    var localTimerOwners: [String: String] = [:]
    var trustedClockState = TrustedClock()
    var hasValidGeneratorState: Bool { WireBounds.isValidClock(wallMs: hlcWallMs, counter: hlcCounter) }
    var hasValidPendingWireOperations: Bool {
        pendingCommands.allSatisfy(\.isValid) && pendingTaskOperations.allSatisfy(\.isValid)
        && pendingDurationOperations.allSatisfy(\.isValid) && pendingAutoStartOperations.allSatisfy(\.isValid)
        && pendingSelectedTaskOperations.allSatisfy(\.isValid)
    }
    func trustedOccurrenceDate(for date: Date, uptime: TimeInterval) throws -> Date { date }
    mutating func mergeKnownTasks(_ tasks: [FocusTask]) { knownTasks = tasks }
    mutating func advanceClock(at date: Date) throws { try advanceClock(at: date) { try ProbeClock().tickHLC($0) } }
    mutating func reserveUuidV7() throws -> [UUID] {
        let ids = try UUIDv7.reserve(timestampMs: hlcWallMs, previous: lastUuidV7,
            entropy: { Array(repeating: 0, count: 9) + [1] })
        lastUuidV7 = ids.last; return ids
    }
    // INSERT_STATE_METHODS
}

struct ProbeClock {
    func tickHLC(_ input: CoreHLCTickInput) throws -> CoreHLCTickOutput {
        try decode(CoreHLCTickOutput.self, dispatch("hlc.tick.v1", encoded(input)))
    }
}

@MainActor final class TimerSessionController {
    struct CommandRequest { let type: CommandType; let timerID: String; let taskID: String?; let phase: TimerPhase
        let duration: TimeInterval; let elapsed: TimeInterval; let occurredAt: Date; let localDate: Date }
    struct CommandTransition { let state: PersistedTimerState; let command: TimerCommand }
    enum AlarmAction: Equatable, Sendable {
        case schedule(timerID: String, phase: TimerPhase, duration: TimeInterval)
        case pause(timerID: String)
        case resume(timerID: String, phase: TimerPhase, duration: TimeInterval)
        case cancel(timerID: String)
    }
    struct AlarmPlan: Equatable, Sendable { let actions: [AlarmAction] }
    func loadCore() throws -> ProbeClock { ProbeClock() }
    // INSERT_COMMAND_METHODS
    // INSERT_GENERATION_METHOD
    func alarmPlan(for action: AlarmAction) -> AlarmPlan { AlarmPlan(actions: [action]) }
    func project(_ state: PersistedTimerState, replicationMode: ReplicationMode, physicalNow: Date) throws -> CoreProjectionOutput {
        let pending = CoreProjectionPending(commands: state.safeProjectionCommands().map { CoreTimerCommand($0, deviceId: state.deviceId) },
            taskOperations: state.safeProjectionTaskOperations().map { CoreTaskOperation($0, deviceId: state.deviceId) },
            durationOperations: state.safeProjectionDurationOperations().map { CoreDurationOperation($0, deviceId: state.deviceId) },
            autoStartOperations: state.safeProjectionAutoStartOperations().map(CoreAutoStartOperation.init),
            selectedTaskOperations: state.safeProjectionSelectedTaskOperations().map(CoreSelectedTaskOperation.init))
        let base = CoreProjectionBase(canonicalTimer: state.canonicalTimer, history: state.history, tasks: state.tasks,
            durationsMs: state.settings.durationsMs, autoStartBreaks: state.baseAutoStart, selectedTaskId: state.baseSelection)
        return try decode(CoreProjectionOutput.self, dispatch("projection.apply.v2", encoded(CoreProjectionInput(base: base, pending: pending, now: physicalNow))))
    }
}

@MainActor struct AccountSynchronization {
    struct SyncBatch: Codable { let commands: [TimerCommand]; let taskOperations: [TaskOperation]; let durationOperations: [DurationOperation]
        let autoStartOperations: [AutoStartOperation]; let selectedTaskOperations: [SelectedTaskOperation] }
    struct SyncRequest: Codable { let deviceId: String; let lastRevision: Int64; let commands: [TimerCommand]; let taskOperations: [TaskOperation]
        let durationOperations: [DurationOperation]; let autoStartOperations: [AutoStartOperation]; let selectedTaskOperations: [SelectedTaskOperation] }
    struct SyncPlan: Codable { let batch: SyncBatch; let request: SyncRequest; let legacyDependencyReviewRequired: Bool }
    func uploadableCommands(in state: PersistedTimerState, limit: Int) -> [TimerCommand] { Array(state.pendingCommands.prefix(limit)) }
    // INSERT_SYNC_METHODS
}
enum LegacyBreakDependencyRecovery { static func requiresReview(_ state: PersistedTimerState) -> Bool { false } }

@MainActor func snapshot(_ state: PersistedTimerState) throws -> SynchronizedWorkspaceMutationController.Snapshot {
    let projection = try TimerSessionController().project(state, replicationMode: .centralized, physicalNow: Date(timeIntervalSince1970: 1784548810))
    return .init(state: state, canonicalTimer: projection.canonicalTimer, tasks: projection.tasks,
        projectedAutoStartBreaks: projection.autoStartBreaks,
        projectedSelectedTaskID: projection.selectedTaskId.flatMap(UUID.init(uuidString:)), replicationMode: .centralized,
        localDate: Date(timeIntervalSince1970: 1784548810), trustedClockUptime: 10, isWorkspaceMutationBlocked: false)
}

@MainActor func intent(_ raw: [String: Any], task: FocusTask) -> SynchronizedWorkspaceMutationController.Intent {
    let now = Date(timeIntervalSince1970: 1784548810)
    switch raw["kind"] as! String {
    case "upsertTask": return .task(.upsert, task)
    case "deleteTask": return .task(.delete, task)
    case "selectTask": return .selectTask(raw["taskId"] is NSNull ? nil : task.id)
    case "setDuration": return .setDurationMinutes(raw["minutes"] as! Int, for: .shortBreak)
    case "setAutoStart": return .setAutoStartBreaks(raw["enabled"] as! Bool)
    case "start": return .startTimer
    case "pause": return .pauseTimer(at: now)
    case "resume": return .resumeTimer(at: now)
    case "cancel": return .cancelTimer(at: now)
    case "clear": return .clearTimer
    default: fatalError("Unknown probe intent")
    }
}

func reflected(_ value: Any) -> Any {
    if let item = value as? Date { return item.timeIntervalSince1970 }
    if let item = value as? String { return item }
    if let item = value as? Bool { return item }
    if let item = value as? Int64 { return item }
    if let item = value as? Double { return item }
    if let item = value as? UUID { return item.uuidString.lowercased() }
    let mirror = Mirror(reflecting: value)
    if mirror.displayStyle == .optional { return mirror.children.first.map { reflected($0.value) } ?? NSNull() }
    if mirror.displayStyle == .collection || mirror.displayStyle == .set { return mirror.children.map { reflected($0.value) } }
    if mirror.displayStyle == .enum {
        return mirror.children.first.map { [$0.label!: reflected($0.value)] } ?? String(describing: value)
    }
    return Dictionary(uniqueKeysWithValues: mirror.children.enumerated().map { index, child in
        (child.label ?? String(index), reflected(child.value)) })
}

func wireQueues(_ state: PersistedTimerState) throws -> Any {
    try encoded(CoreProjectionPending(commands: state.pendingCommands.map { CoreTimerCommand($0, deviceId: state.deviceId) },
        taskOperations: state.pendingTaskOperations.map { CoreTaskOperation($0, deviceId: state.deviceId) },
        durationOperations: state.pendingDurationOperations.map { CoreDurationOperation($0, deviceId: state.deviceId) },
        autoStartOperations: state.pendingAutoStartOperations.map(CoreAutoStartOperation.init),
        selectedTaskOperations: state.pendingSelectedTaskOperations.map(CoreSelectedTaskOperation.init)))
}

@MainActor func effects(_ values: [SynchronizedWorkspaceMutationController.Effect]) throws -> [[String: Any]] {
    try values.map { effect in
        switch effect {
        case .persist: return ["kind": "persist"]
        case .persistAtomically(let previous, let rebuilds):
            return ["kind": "persistAtomically", "previous": try encoded(previous), "rebuildsOnRollback": rebuilds]
        case .launchSync: return ["kind": "launchSync"]
        case .setExplicitPhaseSelection(let explicit): return ["kind": "setExplicitPhaseSelection", "explicit": explicit]
        case .clearCompletionAlert(let id): return ["kind": "clearCompletionAlert", "timerId": id]
        case .alarm(let plan): return ["kind": "alarm", "actions": plan.actions.map { action -> [String: Any] in
            switch action {
            case .schedule(let id, let phase, let duration): return ["kind": "scheduleAlarm", "timerId": id, "phase": phase.rawValue, "durationMs": duration * 1000]
            case .pause(let id): return ["kind": "pauseAlarm", "timerId": id]
            case .resume(let id, let phase, let duration): return ["kind": "resumeAlarm", "timerId": id, "phase": phase.rawValue, "durationMs": duration * 1000]
            case .cancel(let id): return ["kind": "cancelAlarm", "timerId": id]
            }
        }]
        }
    }
}

@MainActor func run(_ raw: [String: Any], directory: URL) throws -> [String: Any] {
    var state = try decode(PersistedTimerState.self, raw["state"]!)
    let task = try decode(FocusTask.self, raw["task"]!)
    let controller = SynchronizedWorkspaceMutationController(timerSessionController: TimerSessionController(),
        timerIDProvider: { "timer-12345678-1234-4234-8234-123456789012" })
    let initial = try controller.plan(intent(raw["seedIntent"] as! [String: Any], task: task), from: snapshot(state))!
    state = initial.state
    let claim = AccountSynchronization().prepareSyncPlan(state: state)
    state = claim.retired
    if raw["nullHead"] as! Bool { state.canonicalHeadWallMs = nil; state.canonicalHeadCounter = nil }
    let outgoing = try encoder.encode(claim.plan)
    let saved = try encoder.encode(state)
    let file = directory.appendingPathComponent("state.json")
    try saved.write(to: file, options: .atomic)
    state = try decoder.decode(PersistedTimerState.self, from: Data(contentsOf: file))
    precondition(state == claim.retired || raw["nullHead"] as! Bool)
    var result: [String: Any] = ["rawStateBefore": try encoded(state), "outgoing": try JSONSerialization.jsonObject(with: outgoing),
        "rawSavedBytes": String(data: saved, encoding: .utf8)!, "rawDates": state.localCommandDates.mapValues(\.timeIntervalSince1970),
        "wireQueuesBefore": try wireQueues(state)]
    do {
        if let next = try controller.plan(intent(raw["intent"] as! [String: Any], task: task), from: snapshot(state)) {
            result["state"] = try encoded(next.state); result["requirements"] = reflected(next.requirements)
            result["effects"] = try effects(next.effects)
            result["projection"] = try next.projection.map { try encoded($0) } ?? NSNull()
            result["wireQueuesAfter"] = try wireQueues(next.state)
        } else { result["returned"] = "nil" }
    } catch { result["error"] = String(describing: error) }
    result["savedStateAfter"] = try JSONSerialization.jsonObject(with: Data(contentsOf: file))
    let outgoingAfter = try encoder.encode(claim.plan)
    precondition(outgoingAfter == outgoing)
    return result
}

@MainActor func main() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    while let line = readLine() {
        let raw = try JSONSerialization.jsonObject(with: Data(line.utf8)) as! [String: Any]
        let result = try run(raw, directory: directory)
        print(String(data: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), encoding: .utf8)!)
    }
}
try MainActor.assumeIsolated { try main() }
