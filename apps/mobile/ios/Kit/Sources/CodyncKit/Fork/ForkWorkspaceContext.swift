import Foundation

public struct ForkWorkspaceContext: Decodable, Equatable, Sendable {
    public enum Kind: String, Codable, Sendable { case personal, team }
    public enum Role: String, Codable, Sendable { case owner, admin, member }

    public let schema: Int
    public let hubId: String
    public let kind: Kind
    public let workspaceId: String
    public let userId: String
    public let role: Role
    public let workspaceEpoch: UInt64
    public let capabilities: [String]

    private enum CodingKeys: String, CodingKey {
        case schema, hubId, kind, workspaceId, userId, role, workspaceEpoch, capabilities
    }

    public init(from decoder: any Decoder) throws {
        let data = try decoder.container(keyedBy: CodingKeys.self)
        schema = try data.decode(Int.self, forKey: .schema)
        hubId = try data.decode(String.self, forKey: .hubId)
        kind = try data.decode(Kind.self, forKey: .kind)
        workspaceId = try data.decode(String.self, forKey: .workspaceId)
        userId = try data.decode(String.self, forKey: .userId)
        role = try data.decode(Role.self, forKey: .role)
        workspaceEpoch = try data.decode(UInt64.self, forKey: .workspaceEpoch)
        capabilities = try data.decode([String].self, forKey: .capabilities)
        let validBinding = switch kind {
        case .personal: workspaceId == "personal:\(userId)" && role == .owner
        case .team: workspaceId.hasPrefix("org:") && !workspaceId.dropFirst(4).isEmpty
        }
        guard schema == 1, !hubId.isEmpty, !userId.isEmpty, validBinding else {
            throw DecodingError.dataCorruptedError(forKey: .workspaceId, in: data, debugDescription: "Invalid workspace context")
        }
    }

    public func cacheScopeID(spaceId: String) -> String {
        ForkWorkspaceID.partition(["hub-v1", hubId, workspaceId, userId, String(workspaceEpoch), spaceId])
    }
}

public enum ForkWorkspaceID: Hashable, Sendable {
    case personal
    case team(orgId: String)

    public func workspaceId(userId: String) -> String {
        switch self {
        case .personal: "personal:\(userId)"
        case .team(let orgId): "org:\(orgId)"
        }
    }

    public func storageScopeID(userId: String) -> String {
        Self.partition(["workspace-v1", userId, workspaceId(userId: userId)])
    }

    fileprivate static func partition(_ parts: [String]) -> String {
        parts.map { "\($0.utf8.count):\($0)" }.joined()
    }
}

public struct ForkWorkspaceMemberships: Equatable, Sendable {
    public struct Team: Identifiable, Equatable, Sendable {
        public let orgId: String
        public let name: String
        public let role: ForkWorkspaceContext.Role
        public var id: ForkWorkspaceID { .team(orgId: orgId) }

        public init?(orgId: String, name: String, clerkRole: String) {
            guard !orgId.isEmpty else { return nil }
            switch clerkRole {
            case "org:admin": role = .admin
            case "org:member": role = .member
            default: return nil
            }
            self.orgId = orgId
            self.name = name
        }
    }

    public let hasPersonal: Bool
    public let teams: [Team]
    public var ids: [ForkWorkspaceID] { (hasPersonal ? [.personal] : []) + teams.map(\.id) }

    public init(hasPersonal: Bool, teams: [Team]) {
        self.hasPersonal = hasPersonal
        self.teams = Dictionary(grouping: teams, by: \.orgId).values.compactMap { candidates in
            candidates.count == 1 ? candidates.first : nil
        }.sorted { $0.orgId < $1.orgId }
    }
}

public enum ForkWorkspaceIssue: String, Error, Sendable {
    case authInvalid = "auth_invalid"
    case deviceUnpaired = "device_unpaired"
    case deviceRevoked = "device_revoked"
    case leaseExpired = "lease_expired"
    case workspaceForbidden = "workspace_forbidden"
    case roleInsufficient = "role_insufficient"
    case crossWorkspaceReference = "cross_workspace_reference"
    case workspaceEpochMismatch = "workspace_epoch_mismatch"
    case workspaceRevoked = "workspace_revoked"
    case roleChanged = "role_changed"
    case hubOffline = "hub_offline"
}

public struct ForkWorkspaceSelection: Sendable {
    public struct Token: Equatable, Sendable {
        public let userId: String
        public let workspace: ForkWorkspaceID
        public let generation: UInt64
        fileprivate let incarnation: UUID
        public var storageScopeID: String { workspace.storageScopeID(userId: userId) }
    }

    public let userId: String
    public private(set) var memberships: ForkWorkspaceMemberships?
    public private(set) var active: ForkWorkspaceID?
    public private(set) var generation: UInt64 = 0
    private let incarnation = UUID()
    private var forbidden = Set<ForkWorkspaceID>()

    public init(userId: String) { self.userId = userId }

    public var choices: [ForkWorkspaceID] { memberships?.ids.filter { !forbidden.contains($0) } ?? [] }
    public var token: Token? { active.map { Token(userId: userId, workspace: $0, generation: generation, incarnation: incarnation) } }

    @discardableResult
    public mutating func update(_ next: ForkWorkspaceMemberships) -> Set<ForkWorkspaceID> {
        let previous = memberships
        let removed = Set(previous?.ids ?? []).subtracting(next.ids)
        let roles = Dictionary(uniqueKeysWithValues: next.teams.map { ($0.id, $0.role) })
        let changed = previous?.teams.compactMap { team in roles[team.id] == team.role ? nil : team.id } ?? []
        let purge = removed.union(changed)
        forbidden.subtract(removed)
        memberships = next
        if let active, choices.contains(active) {
            if purge.contains(active) { generation += 1 }
        } else {
            activate(choices.first)
        }
        return purge
    }

    public mutating func select(_ workspace: ForkWorkspaceID) {
        guard choices.contains(workspace) else { return }
        activate(workspace)
    }

    public mutating func suspend() { activate(nil) }

    public func accepts(_ token: Token) -> Bool { self.token == token }

    public mutating func invalidate(_ issue: ForkWorkspaceIssue, from token: Token) -> ForkWorkspaceID? {
        guard accepts(token) else { return nil }
        switch issue {
        case .workspaceForbidden, .workspaceRevoked, .deviceRevoked:
            forbidden.insert(token.workspace)
            activate(choices.first)
        case .workspaceEpochMismatch, .roleChanged:
            generation += 1
        default:
            return nil
        }
        return token.workspace
    }

    private mutating func activate(_ workspace: ForkWorkspaceID?) {
        guard active != workspace else { return }
        active = workspace
        generation += 1
    }
}
