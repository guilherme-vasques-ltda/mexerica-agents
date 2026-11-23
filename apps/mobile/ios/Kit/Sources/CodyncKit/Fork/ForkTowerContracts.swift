import Foundation

// MARK: - Fail-closed TowerObservation<T>

public enum TowerObservation<Value: Sendable>: Sendable {
    case available(value: Value, source: String? = nil, coverage: String? = nil)
    case unavailable

    public var isAvailable: Bool {
        if case .available = self { return true }
        return false
    }

    public var value: Value? {
        if case .available(let value, _, _) = self { return value }
        return nil
    }
}

extension TowerObservation: Equatable where Value: Equatable {}
extension TowerObservation: Hashable where Value: Hashable {}

extension TowerObservation: Codable where Value: Codable {
    private enum CodingKeys: String, CodingKey {
        case availability
        case source
        case coverage
        case value
    }

    public init(from decoder: Decoder) throws {
        do {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            let availability = try container.decodeIfPresent(String.self, forKey: .availability)
            switch availability {
            case "available":
                let value = try container.decode(Value.self, forKey: .value)
                let source = (try? container.decodeIfPresent(String.self, forKey: .source)) ?? nil
                let coverage = (try? container.decodeIfPresent(String.self, forKey: .coverage)) ?? nil
                self = .available(value: value, source: source, coverage: coverage)
            default:
                // Fail-closed: "unavailable" or any unrecognized availability never fabricates data.
                self = .unavailable
            }
        } catch {
            // Fail-closed: malformed TowerObservation wrapper or value payload degrades to `.unavailable`.
            self = .unavailable
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .available(let value, let source, let coverage):
            try container.encode("available", forKey: .availability)
            try container.encodeIfPresent(source, forKey: .source)
            try container.encodeIfPresent(coverage, forKey: .coverage)
            try container.encode(value, forKey: .value)
        case .unavailable:
            try container.encode("unavailable", forKey: .availability)
        }
    }
}

extension KeyedDecodingContainer {
    /// Decodes a TowerObservation<T> fail-closed: missing or null keys (e.g. older Hub) become `.unavailable`.
    public func decodeObservationIfPresent<Value: Codable & Sendable>(
        _ type: Value.Type,
        forKey key: Key
    ) throws -> TowerObservation<Value> {
        guard contains(key) else { return .unavailable }
        if try decodeNil(forKey: key) { return .unavailable }
        return try decode(TowerObservation<Value>.self, forKey: key)
    }
}

/// Additive fields introduced on TowerSnapshot in MIRROR_SCHEMA 71+.
/// Older Hubs omit `attention` and `doctor`, which decode fail-closed as `.unavailable`.
public struct TowerSnapshotObservations: Codable, Equatable, Sendable {
    public var attention: TowerObservation<AttentionQueue>
    public var doctor: TowerObservation<HubDoctorReport>

    public init(
        attention: TowerObservation<AttentionQueue> = .unavailable,
        doctor: TowerObservation<HubDoctorReport> = .unavailable
    ) {
        self.attention = attention
        self.doctor = doctor
    }

    private enum CodingKeys: String, CodingKey {
        case attention
        case doctor
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.attention = try container.decodeObservationIfPresent(AttentionQueue.self, forKey: .attention)
        self.doctor = try container.decodeObservationIfPresent(HubDoctorReport.self, forKey: .doctor)
    }
}

// MARK: - RPC: donnaAttention

public struct DonnaAttentionRequest: Codable, Equatable, Sendable {
    public var spaceId: String
    public var includeResolvedToday: Bool

    public init(spaceId: String, includeResolvedToday: Bool = true) {
        self.spaceId = spaceId
        self.includeResolvedToday = includeResolvedToday
    }

    private enum CodingKeys: String, CodingKey {
        case spaceId
        case includeResolvedToday
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.spaceId = try container.decode(String.self, forKey: .spaceId)
        self.includeResolvedToday = try container.decodeIfPresent(Bool.self, forKey: .includeResolvedToday) ?? true
    }
}

public enum AttentionKind: String, Codable, Hashable, Sendable {
    case permission
    case delegatedPermission = "delegated_permission"
    case ownerQuestion = "owner_question"
    case taskProposal = "task_proposal"
    case taskReview = "task_review"
    case routineReview = "routine_review"
    case doctorAlert = "doctor_alert"
}

public enum AttentionSeverity: String, Codable, Hashable, Sendable, Comparable {
    case warning
    case actionRequired = "action_required"
    case blocking

    public var rank: Int {
        switch self {
        case .blocking: return 3
        case .actionRequired: return 2
        case .warning: return 1
        }
    }

    public static func < (lhs: AttentionSeverity, rhs: AttentionSeverity) -> Bool {
        lhs.rank < rhs.rank
    }
}

public enum AttentionActionKind: String, Codable, Hashable, Sendable {
    case respondPermission = "respond_permission"
    case answerQuestion = "answer_question"
    case decideProposal = "decide_proposal"
    case reviewTask = "review_task"
    case reviewRoutine = "review_routine"
    case openConversation = "open_conversation"
    case openDoctor = "open_doctor"
}

public enum AttentionApproverRole: String, Codable, Hashable, Sendable {
    case owner
}

public enum AttentionResolutionKind: String, Codable, Hashable, Sendable {
    case approved
    case rejected
    case answered
    case accepted
    case reopened
    case kept
    case cancelled
    case recovered
}

public enum CoverageGapCode: String, Codable, Hashable, Sendable {
    case sourceUnavailable = "source_unavailable"
    case hubOutdated = "hub_outdated"
}

public struct AttentionConversationRef: Codable, Hashable, Sendable {
    public var conversationId: String
    public var revision: Int

    public init(conversationId: String, revision: Int) {
        self.conversationId = conversationId
        self.revision = revision
    }
}

public struct AttentionTarget: Codable, Hashable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public var botId: String?
    public var requesterBotId: String?
    public var projectId: String?
    public var taskId: String?
    public var proposalId: String?
    public var routineId: String?
    public var entryId: String?
    public var conversation: AttentionConversationRef?
    public var sessionId: String?
    public var doctorCheckId: String?

    public init(
        botId: String? = nil,
        requesterBotId: String? = nil,
        projectId: String? = nil,
        taskId: String? = nil,
        proposalId: String? = nil,
        routineId: String? = nil,
        entryId: String? = nil,
        conversation: AttentionConversationRef? = nil,
        sessionId: String? = nil,
        doctorCheckId: String? = nil
    ) {
        self.botId = botId
        self.requesterBotId = requesterBotId
        self.projectId = projectId
        self.taskId = taskId
        self.proposalId = proposalId
        self.routineId = routineId
        self.entryId = entryId
        self.conversation = conversation
        self.sessionId = sessionId
        self.doctorCheckId = doctorCheckId
    }

    public var description: String {
        "AttentionTarget(botId: \(botId ?? "nil"), taskId: \(taskId ?? "nil"), entryId: \(entryId != nil ? "<redacted>" : "nil"), sessionId: \(sessionId != nil ? "<redacted>" : "nil"))"
    }

    public var debugDescription: String { description }
}

public struct AttentionItem: Codable, Hashable, Sendable, Identifiable, CustomStringConvertible, CustomDebugStringConvertible {
    public var id: String
    public var dedupeKey: String
    public var kind: AttentionKind
    public var severity: AttentionSeverity
    public var spaceId: String
    public var titleKey: String
    public var summary: String?
    public var actionKind: AttentionActionKind
    public var approverRole: AttentionApproverRole
    public var createdAt: Int64
    public var target: AttentionTarget

    public init(
        id: String,
        dedupeKey: String,
        kind: AttentionKind,
        severity: AttentionSeverity,
        spaceId: String,
        titleKey: String,
        summary: String?,
        actionKind: AttentionActionKind,
        approverRole: AttentionApproverRole = .owner,
        createdAt: Int64,
        target: AttentionTarget
    ) {
        self.id = id
        self.dedupeKey = dedupeKey
        self.kind = kind
        self.severity = severity
        self.spaceId = spaceId
        self.titleKey = titleKey
        self.summary = summary
        self.actionKind = actionKind
        self.approverRole = approverRole
        self.createdAt = createdAt
        self.target = target
    }

    private enum CodingKeys: String, CodingKey {
        case id
        case dedupeKey
        case kind
        case severity
        case spaceId
        case titleKey
        case summary
        case actionKind
        case approverRole
        case createdAt
        case target
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.id = try container.decode(String.self, forKey: .id)
        self.dedupeKey = try container.decode(String.self, forKey: .dedupeKey)
        self.kind = try container.decode(AttentionKind.self, forKey: .kind)
        self.severity = try container.decode(AttentionSeverity.self, forKey: .severity)
        self.spaceId = try container.decode(String.self, forKey: .spaceId)
        self.titleKey = try container.decode(String.self, forKey: .titleKey)
        self.summary = try container.decodeIfPresent(String.self, forKey: .summary)
        self.actionKind = try container.decode(AttentionActionKind.self, forKey: .actionKind)
        self.approverRole = try container.decodeIfPresent(AttentionApproverRole.self, forKey: .approverRole) ?? .owner
        self.createdAt = try container.decode(Int64.self, forKey: .createdAt)
        self.target = try container.decode(AttentionTarget.self, forKey: .target)
    }

    public var description: String {
        "AttentionItem(id: \(id), kind: \(kind.rawValue), severity: \(severity.rawValue), spaceId: \(spaceId), titleKey: \(titleKey), summary: <redacted>)"
    }

    public var debugDescription: String { description }
}

public struct ResolvedAttentionItem: Codable, Hashable, Sendable, Identifiable, CustomStringConvertible, CustomDebugStringConvertible {
    public var id: String
    public var kind: AttentionKind
    public var titleKey: String
    public var summary: String?
    public var resolvedAt: Int64
    public var resolutionKind: AttentionResolutionKind
    public var target: AttentionTarget

    public init(
        id: String,
        kind: AttentionKind,
        titleKey: String,
        summary: String?,
        resolvedAt: Int64,
        resolutionKind: AttentionResolutionKind,
        target: AttentionTarget
    ) {
        self.id = id
        self.kind = kind
        self.titleKey = titleKey
        self.summary = summary
        self.resolvedAt = resolvedAt
        self.resolutionKind = resolutionKind
        self.target = target
    }

    public var description: String {
        "ResolvedAttentionItem(id: \(id), kind: \(kind.rawValue), resolutionKind: \(resolutionKind.rawValue), summary: <redacted>)"
    }

    public var debugDescription: String { description }
}

public struct CoverageGap: Codable, Hashable, Sendable {
    public var source: String
    public var code: CoverageGapCode

    public init(source: String, code: CoverageGapCode) {
        self.source = source
        self.code = code
    }
}

public struct AttentionQueue: Codable, Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public var version: Int
    public var hubId: String
    public var spaceId: String
    public var collectedAt: Int64
    public var items: [AttentionItem]
    public var resolvedToday: [ResolvedAttentionItem]
    public var coverageGaps: [CoverageGap]

    public init(
        version: Int,
        hubId: String,
        spaceId: String,
        collectedAt: Int64,
        items: [AttentionItem],
        resolvedToday: [ResolvedAttentionItem],
        coverageGaps: [CoverageGap]
    ) {
        self.version = version
        self.hubId = hubId
        self.spaceId = spaceId
        self.collectedAt = collectedAt
        self.items = items
        self.resolvedToday = resolvedToday
        self.coverageGaps = coverageGaps
    }

    /// Defensive deduplication by `dedupeKey` (delegated_permission suppresses duplicate permission)
    /// and stable sort by severity (blocking > action_required > warning) then recency (createdAt desc).
    public var normalizedItems: [AttentionItem] {
        var byKey: [String: AttentionItem] = [:]
        var keyOrder: [String] = []
        for item in items {
            if let existing = byKey[item.dedupeKey] {
                if Self.shouldReplace(existing: existing, candidate: item) {
                    byKey[item.dedupeKey] = item
                }
            } else {
                byKey[item.dedupeKey] = item
                keyOrder.append(item.dedupeKey)
            }
        }
        let deduped = keyOrder.compactMap { byKey[$0] }
        return deduped.sorted { lhs, rhs in
            if lhs.severity != rhs.severity {
                return lhs.severity > rhs.severity
            }
            if lhs.createdAt != rhs.createdAt {
                return lhs.createdAt > rhs.createdAt
            }
            return lhs.id < rhs.id
        }
    }

    /// Count strictly reflects confirmed Hub items after defensive deduplication.
    public var pendingCount: Int {
        normalizedItems.count
    }

    public var hasCoverageGaps: Bool {
        !coverageGaps.isEmpty
    }

    /// UI contract: resolvedToday renders collapsed by default.
    public var isResolvedTodayCollapsedByDefault: Bool {
        true
    }

    private static func shouldReplace(existing: AttentionItem, candidate: AttentionItem) -> Bool {
        if existing.kind != candidate.kind {
            if candidate.kind == .delegatedPermission && existing.kind == .permission {
                return true
            }
            if existing.kind == .delegatedPermission && candidate.kind == .permission {
                return false
            }
        }
        if candidate.severity != existing.severity {
            return candidate.severity > existing.severity
        }
        return candidate.createdAt > existing.createdAt
    }

    public var description: String {
        "AttentionQueue(version: \(version), hubId: \(hubId), spaceId: \(spaceId), pendingCount: \(pendingCount), resolvedToday: \(resolvedToday.count), coverageGaps: \(coverageGaps.count))"
    }

    public var debugDescription: String { description }
}

// MARK: - RPC: donnaHubDoctor

public struct DonnaHubDoctorRequest: Codable, Equatable, Sendable {
    public var spaceId: String

    public init(spaceId: String) {
        self.spaceId = spaceId
    }
}

public enum DoctorOverallStatus: String, Codable, Hashable, Sendable {
    case healthy
    case waitingUser = "waiting_user"
    case degraded
    case error
}

public enum DoctorCheckStatus: String, Codable, Hashable, Sendable {
    case ok
    case warning
    case error
    case unavailable
}

public enum DoctorArea: String, Codable, Hashable, Sendable {
    case hub
    case aiEngine = "ai_engine"
    case approvals
    case channels
    case mcps
    case routines
    case delegation
}

public enum DoctorActionKind: String, Codable, Hashable, Sendable {
    case none
    case startHub = "start_hub"
    case connectAI = "connect_ai"
    case reviewWaiting = "review_waiting"
    case checkChannels = "check_channels"
    case checkMCPs = "check_mcps"
    case checkRoutines = "check_routines"
    case openTorre = "open_torre"
}

public struct DoctorCheck: Codable, Hashable, Sendable, Identifiable, CustomStringConvertible, CustomDebugStringConvertible {
    public var id: String
    public var area: DoctorArea
    public var status: DoctorCheckStatus
    public var code: String
    public var count: Int?
    public var actionKind: DoctorActionKind

    public init(
        id: String,
        area: DoctorArea,
        status: DoctorCheckStatus,
        code: String,
        count: Int?,
        actionKind: DoctorActionKind
    ) {
        self.id = id
        self.area = area
        self.status = status
        self.code = code
        self.count = count
        self.actionKind = actionKind
    }

    public var description: String {
        "DoctorCheck(id: \(id), area: \(area.rawValue), status: \(status.rawValue), code: \(code), actionKind: \(actionKind.rawValue))"
    }

    public var debugDescription: String { description }
}

public struct HubDoctorReport: Codable, Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public var version: Int
    public var hubId: String
    public var spaceId: String
    public var collectedAt: Int64
    public var overall: DoctorOverallStatus
    public var checks: [DoctorCheck]

    public init(
        version: Int,
        hubId: String,
        spaceId: String,
        collectedAt: Int64,
        overall: DoctorOverallStatus,
        checks: [DoctorCheck]
    ) {
        self.version = version
        self.hubId = hubId
        self.spaceId = spaceId
        self.collectedAt = collectedAt
        self.overall = overall
        self.checks = checks
    }

    public var description: String {
        "HubDoctorReport(version: \(version), hubId: \(hubId), spaceId: \(spaceId), overall: \(overall.rawValue), checks: \(checks.count))"
    }

    public var debugDescription: String { description }
}

public enum DoctorTransportDiagnosis: Equatable, Sendable {
    case hubOffline
    case aiDisconnected(code: String)
    case waitingUserApproval(pendingCount: Int?)
    case degraded(codes: [String])
    case healthy
    case unavailable

    public var actionKind: DoctorActionKind {
        switch self {
        case .hubOffline: return .startHub
        case .aiDisconnected: return .connectAI
        case .waitingUserApproval: return .reviewWaiting
        case .degraded: return .openTorre
        case .healthy, .unavailable: return .none
        }
    }

    public static func diagnose(
        _ observation: TowerObservation<HubDoctorReport>,
        hubReachable: Bool
    ) -> DoctorTransportDiagnosis {
        guard hubReachable else {
            return .hubOffline
        }
        guard case .available(let report, _, _) = observation else {
            return .unavailable
        }

        if let aiCheck = report.checks.first(where: {
            $0.area == .aiEngine && ($0.status == .error || $0.status == .warning || $0.actionKind == .connectAI)
        }) {
            return .aiDisconnected(code: aiCheck.code)
        }

        if report.overall == .waitingUser {
            let approvalCount = report.checks.first(where: { $0.area == .approvals })?.count
            return .waitingUserApproval(pendingCount: approvalCount)
        }

        if let waitingCheck = report.checks.first(where: {
            $0.area == .approvals && $0.actionKind == .reviewWaiting && ($0.count ?? 0) > 0
        }) {
            return .waitingUserApproval(pendingCount: waitingCheck.count)
        }

        if report.overall == .error || report.overall == .degraded {
            let failingCodes = report.checks
                .filter { $0.status == .error || $0.status == .warning || $0.status == .unavailable }
                .map(\.code)
            return .degraded(codes: failingCodes)
        }

        return .healthy
    }
}

// MARK: - RPC: donnaTowerView

public struct DonnaTowerViewRequest: Codable, Equatable, Sendable {
    public var spaceId: String
    public var perspective: TowerPerspective
    public var directorBotId: String?
    public var botId: String?

    public init(
        spaceId: String,
        perspective: TowerPerspective,
        directorBotId: String? = nil,
        botId: String? = nil
    ) {
        self.spaceId = spaceId
        self.perspective = perspective
        self.directorBotId = directorBotId
        self.botId = botId
    }
}

public enum TowerPerspective: String, Codable, Hashable, Sendable {
    case overview
    case director
    case bot
}

public enum WorkOriginKind: String, Codable, Hashable, Sendable {
    case ownerDirect = "owner_direct"
    case delegated
    case routine
    case unknown
}

public enum WorkBucket: String, Codable, Hashable, Sendable {
    case needsYou = "needs_you"
    case observedWorking = "observed_working"
    case waitingOrBlocked = "waiting_or_blocked"
    case backlog
    case done
}

public enum WorkRequesterLabel: String, Codable, Hashable, Sendable {
    case owner
    case bot
    case routine
    case unknown
}

public enum WorkBlockerKind: String, Codable, Hashable, Sendable {
    case permissionPause = "permission_pause"
    case ownerDecision = "owner_decision"
    case dependency
    case manual
    case none
}

public enum ObservedExecutionStatus: String, Codable, Hashable, Sendable {
    case idle
    case working
    case needsInput
    case waiting
    case error
    case unavailable
}

public struct WorkOrigin: Codable, Hashable, Sendable {
    public var kind: WorkOriginKind
    public var rootBotId: String?
    public var delegatedByBotId: String?
    public var routineId: String?
    public var admittedAt: Int64?

    public init(
        kind: WorkOriginKind,
        rootBotId: String? = nil,
        delegatedByBotId: String? = nil,
        routineId: String? = nil,
        admittedAt: Int64? = nil
    ) {
        self.kind = kind
        self.rootBotId = rootBotId
        self.delegatedByBotId = delegatedByBotId
        self.routineId = routineId
        self.admittedAt = admittedAt
    }
}

public struct WorkBlocker: Codable, Hashable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public var kind: WorkBlockerKind
    public var reason: String?
    public var attentionItemId: String?

    public init(kind: WorkBlockerKind, reason: String? = nil, attentionItemId: String? = nil) {
        self.kind = kind
        self.reason = reason
        self.attentionItemId = attentionItemId
    }

    public var description: String {
        "WorkBlocker(kind: \(kind.rawValue), attentionItemId: \(attentionItemId ?? "nil"), reason: <redacted>)"
    }

    public var debugDescription: String { description }
}

public struct WorkDelivery: Codable, Hashable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public var hasEvidence: Bool
    public var evidenceSummary: String?
    public var acceptedByOwner: Bool

    public init(hasEvidence: Bool, evidenceSummary: String? = nil, acceptedByOwner: Bool) {
        self.hasEvidence = hasEvidence
        self.evidenceSummary = evidenceSummary
        self.acceptedByOwner = acceptedByOwner
    }

    public var description: String {
        "WorkDelivery(hasEvidence: \(hasEvidence), acceptedByOwner: \(acceptedByOwner), evidenceSummary: <redacted>)"
    }

    public var debugDescription: String { description }
}

public struct ObservedExecution: Codable, Hashable, Sendable {
    public var active: Bool
    public var status: ObservedExecutionStatus
    public var observedAt: Int64?
    public var machineLabel: String?

    public init(
        active: Bool,
        status: ObservedExecutionStatus,
        observedAt: Int64? = nil,
        machineLabel: String? = nil
    ) {
        self.active = active
        self.status = status
        self.observedAt = observedAt
        self.machineLabel = machineLabel
    }
}

public struct WorkCapabilities: Codable, Hashable, Sendable {
    public var canDirectorInterrupt: Bool
    public var canOwnerAccept: Bool

    public init(canDirectorInterrupt: Bool, canOwnerAccept: Bool) {
        self.canDirectorInterrupt = canDirectorInterrupt
        self.canOwnerAccept = canOwnerAccept
    }
}

public struct WorkCard: Codable, Hashable, Sendable, Identifiable, CustomStringConvertible, CustomDebugStringConvertible {
    public var taskId: String
    public var projectId: String
    public var projectName: String
    public var spaceId: String
    public var parentTaskId: String?
    public var title: String
    public var state: String
    public var bucket: WorkBucket
    public var origin: WorkOrigin
    public var requesterLabel: WorkRequesterLabel
    public var requesterBotId: String?
    public var approverRole: AttentionApproverRole
    public var directorBotId: String?
    public var assigneeBotId: String?
    public var nextAction: String?
    public var blocker: WorkBlocker
    public var delivery: WorkDelivery
    public var declaredUpdatedAt: Int64
    public var observedExecution: ObservedExecution
    public var capabilities: WorkCapabilities

    public var id: String { taskId }

    public init(
        taskId: String,
        projectId: String,
        projectName: String,
        spaceId: String,
        parentTaskId: String? = nil,
        title: String,
        state: String,
        bucket: WorkBucket,
        origin: WorkOrigin,
        requesterLabel: WorkRequesterLabel,
        requesterBotId: String? = nil,
        approverRole: AttentionApproverRole = .owner,
        directorBotId: String? = nil,
        assigneeBotId: String? = nil,
        nextAction: String? = nil,
        blocker: WorkBlocker,
        delivery: WorkDelivery,
        declaredUpdatedAt: Int64,
        observedExecution: ObservedExecution,
        capabilities: WorkCapabilities
    ) {
        self.taskId = taskId
        self.projectId = projectId
        self.projectName = projectName
        self.spaceId = spaceId
        self.parentTaskId = parentTaskId
        self.title = title
        self.state = state
        self.bucket = bucket
        self.origin = origin
        self.requesterLabel = requesterLabel
        self.requesterBotId = requesterBotId
        self.approverRole = approverRole
        self.directorBotId = directorBotId
        self.assigneeBotId = assigneeBotId
        self.nextAction = nextAction
        self.blocker = blocker
        self.delivery = delivery
        self.declaredUpdatedAt = declaredUpdatedAt
        self.observedExecution = observedExecution
        self.capabilities = capabilities
    }

    private enum CodingKeys: String, CodingKey {
        case taskId
        case projectId
        case projectName
        case spaceId
        case parentTaskId
        case title
        case state
        case bucket
        case origin
        case requesterLabel
        case requesterBotId
        case approverRole
        case directorBotId
        case assigneeBotId
        case nextAction
        case blocker
        case delivery
        case declaredUpdatedAt
        case observedExecution
        case capabilities
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.taskId = try container.decode(String.self, forKey: .taskId)
        self.projectId = try container.decode(String.self, forKey: .projectId)
        self.projectName = try container.decode(String.self, forKey: .projectName)
        self.spaceId = try container.decode(String.self, forKey: .spaceId)
        self.parentTaskId = try container.decodeIfPresent(String.self, forKey: .parentTaskId)
        self.title = try container.decode(String.self, forKey: .title)
        self.state = try container.decode(String.self, forKey: .state)
        self.bucket = try container.decode(WorkBucket.self, forKey: .bucket)
        self.origin = try container.decode(WorkOrigin.self, forKey: .origin)
        self.requesterLabel = try container.decode(WorkRequesterLabel.self, forKey: .requesterLabel)
        self.requesterBotId = try container.decodeIfPresent(String.self, forKey: .requesterBotId)
        self.approverRole = try container.decodeIfPresent(AttentionApproverRole.self, forKey: .approverRole) ?? .owner
        self.directorBotId = try container.decodeIfPresent(String.self, forKey: .directorBotId)
        self.assigneeBotId = try container.decodeIfPresent(String.self, forKey: .assigneeBotId)
        self.nextAction = try container.decodeIfPresent(String.self, forKey: .nextAction)
        self.blocker = try container.decode(WorkBlocker.self, forKey: .blocker)
        self.delivery = try container.decode(WorkDelivery.self, forKey: .delivery)
        self.declaredUpdatedAt = try container.decode(Int64.self, forKey: .declaredUpdatedAt)
        self.observedExecution = try container.decode(ObservedExecution.self, forKey: .observedExecution)
        self.capabilities = try container.decode(WorkCapabilities.self, forKey: .capabilities)
    }

    public var description: String {
        "WorkCard(taskId: \(taskId), spaceId: \(spaceId), state: \(state), bucket: \(bucket.rawValue), title: <redacted>, nextAction: <redacted>)"
    }

    public var debugDescription: String { description }
}

public struct TowerViewCounts: Codable, Equatable, Sendable {
    public var needsYou: Int
    public var observedWorking: Int
    public var waitingOrBlocked: Int
    public var totalVisible: Int

    public init(needsYou: Int, observedWorking: Int, waitingOrBlocked: Int, totalVisible: Int) {
        self.needsYou = needsYou
        self.observedWorking = observedWorking
        self.waitingOrBlocked = waitingOrBlocked
        self.totalVisible = totalVisible
    }
}

public struct TowerViewSnapshot: Codable, Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public var version: Int
    public var hubId: String
    public var spaceId: String
    public var perspective: TowerPerspective
    public var directorBotId: String?
    public var botId: String?
    public var collectedAt: Int64
    public var cards: [WorkCard]
    public var counts: TowerViewCounts

    public init(
        version: Int,
        hubId: String,
        spaceId: String,
        perspective: TowerPerspective,
        directorBotId: String? = nil,
        botId: String? = nil,
        collectedAt: Int64,
        cards: [WorkCard],
        counts: TowerViewCounts
    ) {
        self.version = version
        self.hubId = hubId
        self.spaceId = spaceId
        self.perspective = perspective
        self.directorBotId = directorBotId
        self.botId = botId
        self.collectedAt = collectedAt
        self.cards = cards
        self.counts = counts
    }

    public var description: String {
        "TowerViewSnapshot(version: \(version), hubId: \(hubId), spaceId: \(spaceId), perspective: \(perspective.rawValue), visibleCards: \(cards.count))"
    }

    public var debugDescription: String { description }
}

// MARK: - Leak-free Invalidation Events & Context Guard

public enum TowerInvalidationEventType: String, Codable, Hashable, Sendable {
    case donnaAttentionChanged
    case donnaDoctorChanged
    case donnaControlTowerChanged
    case donnaGovernanceChanged
}

public struct TowerInvalidationEvent: Codable, Equatable, Sendable {
    public var type: TowerInvalidationEventType
    public var hubId: String
    public var spaceId: String

    public init(type: TowerInvalidationEventType, hubId: String, spaceId: String) {
        self.type = type
        self.hubId = hubId
        self.spaceId = spaceId
    }
}

public enum TowerInvalidationParser {
    public static func parse(_ data: Data) -> TowerInvalidationEvent? {
        guard let event = try? JSONDecoder().decode(TowerInvalidationEvent.self, from: data) else {
            return nil
        }
        guard !event.hubId.isEmpty, !event.spaceId.isEmpty else {
            return nil
        }
        return event
    }
}

public struct TowerInvalidationGuard: Sendable {
    public private(set) var activeHubId: String?
    public private(set) var activeSpaceId: String?
    public private(set) var generation: UInt64

    public init(activeHubId: String? = nil, activeSpaceId: String? = nil, generation: UInt64 = 0) {
        self.activeHubId = activeHubId
        self.activeSpaceId = activeSpaceId
        self.generation = generation
    }

    @discardableResult
    public mutating func bindContext(hubId: String, spaceId: String) -> UInt64 {
        generation &+= 1
        activeHubId = hubId
        activeSpaceId = spaceId
        return generation
    }

    public mutating func resetForContextChange() {
        generation &+= 1
        activeHubId = nil
        activeSpaceId = nil
    }

    public func shouldAccept(_ event: TowerInvalidationEvent, capturedGeneration: UInt64) -> Bool {
        guard let activeHubId, let activeSpaceId else {
            return false
        }
        guard capturedGeneration == generation else {
            return false
        }
        return event.hubId == activeHubId && event.spaceId == activeSpaceId
    }
}
