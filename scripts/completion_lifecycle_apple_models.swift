import Foundation

typealias TimerPhase = String

enum CommandType: String, Codable { case start, finish, retarget }
enum ReplicationMode { case iroh, centralized }
enum AppError: Error { case invalidLocalClock }

struct CanonicalTimer { let id: String }
struct Settings: Codable {
    var selectedPhase: String
    var durationsMs: [String: Int64]
}

struct TimerCommand: Codable {
    let id: String
    let deviceSequence: Int64
    let timerId: String
    let taskId: String?
    let type: CommandType
    let phase: String
    let plannedDurationMs: Int64
    let occurredAt: Date
    let hlcWallMs: Int64
    let hlcCounter: Int64
    let observedElapsedMs: Int64
    var isValid: Bool { !id.isEmpty && plannedDurationMs > 0 }
}

struct ProvisionalPhaseAdvance: Codable {
    let sourceTimerId: String
    let finishCommandId: String
    let previousPhase: String
    let advancedPhase: String
    let generation: Int64
}

struct PersistedTimerState: Codable {
    var settings: Settings
    var selectedPhaseGeneration: Int64
    var hasExplicitPhaseSelection: Bool
    var commandUuids: [UUID]
    var deviceId = "device-local"
    var deviceSequence: Int64 = 7
    var hlcWallMs: Int64 = 1784548800000
    var hlcCounter: Int64 = 0
    var pendingCommands: [TimerCommand] = []
    var neverSentCommands: Set<String> = []
    var localCommandDates: [String: Date] = [:]
    var localTimerOwners: [String: String] = [:]
    var provisionalPhaseAdvances: [ProvisionalPhaseAdvance] = []

    mutating func advanceClock(at date: Date, tick: (Int64) throws -> Int64) throws {
        hlcWallMs = try tick(Int64(date.timeIntervalSince1970 * 1000))
        hlcCounter += 1
    }

    mutating func reserveDeviceSequence() throws -> Int64 {
        deviceSequence += 1
        return deviceSequence
    }

    mutating func reserveUuidV7() throws -> [UUID] {
        guard !commandUuids.isEmpty else { throw AppError.invalidLocalClock }
        return [commandUuids.removeFirst()]
    }

    mutating func recordNeverSentCommand(id: String) {
        neverSentCommands.insert(id)
    }
}

struct CommandRequest {
    let type: CommandType
    let timerID: String
    let taskID: String?
    let phase: String
    let duration: Double
    let elapsed: Double
    let occurredAt: Date
    let localDate: Date
}

struct CommandTransition { let state: PersistedTimerState; let command: TimerCommand }
struct AutomaticBreak { let timerID: String; let phase: String; let duration: Double }
struct AutomaticBreakTransition {
    let state: PersistedTimerState
    let automaticBreak: AutomaticBreak
    let command: TimerCommand
}
struct Decision {
    let completedAt: Date?
    let selectedPhase: String?
    let generatedBreakPhase: String?
}
enum IrohCompletionPlan {
    case persist(PersistedTimerState, timerID: String)
    case automaticBreak(state: PersistedTimerState, timer: CanonicalTimer, completedAt: Date?, nextPhase: String)
}
struct ProbeClock {
    func tickHLC(_ physical: Int64) throws -> Int64 { physical }
}

struct ReturnedSelection: Codable {
    let phase: String
    let generation: String
    let explicit: Bool

    init(_ state: PersistedTimerState) {
        phase = state.settings.selectedPhase
        generation = String(state.selectedPhaseGeneration)
        explicit = state.hasExplicitPhaseSelection
    }
}

struct ReturnedOutcome: Codable {
    let kind: String
    let phase: String?
    let selection: ReturnedSelection

    enum CodingKeys: String, CodingKey { case kind, phase, selection }

    func encode(to encoder: Encoder) throws {
        var output = encoder.container(keyedBy: CodingKeys.self)
        try output.encode(kind, forKey: .kind)
        try output.encode(phase, forKey: .phase)
        try output.encode(selection, forKey: .selection)
    }
}

func emit(_ kind: String, phase: String? = nil, state: PersistedTimerState) throws {
    let value = ReturnedOutcome(kind: kind, phase: phase, selection: ReturnedSelection(state))
    let encoded = try JSONEncoder().encode(value)
    print(String(decoding: encoded, as: UTF8.self))
}

func restored(_ state: PersistedTimerState) throws -> PersistedTimerState {
    try JSONDecoder().decode(PersistedTimerState.self, from: JSONEncoder().encode(state))
}
