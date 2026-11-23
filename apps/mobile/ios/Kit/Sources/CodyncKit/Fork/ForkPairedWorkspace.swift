import Foundation

public struct ForkPairedWorkspace: Codable, Equatable, Sendable {
    let schema: Int
    public let hubId: String
    let kind: String
    let workspaceId: String
    let userId: String?
    let role: String
    public let workspaceEpoch: UInt64
    let capabilities: [String]

    var isSupported: Bool {
        schema == 1 && !hubId.isEmpty && kind == "personal" && role == "owner" &&
            (userId.map { !$0.isEmpty && workspaceId == "personal:\($0)" } ?? (workspaceId == "personal:unbound"))
    }

    public func continues(_ old: Self) -> Bool {
        hubId == old.hubId && workspaceEpoch >= old.workspaceEpoch &&
            (workspaceId == old.workspaceId || (old.workspaceId == "personal:unbound" && userId != nil))
    }
}

extension ChannelTransport {
    static func isPlaintextWorkspaceReject(_ wire: Wire) -> Bool {
        if wire.code == "revoked" { return true }
        switch wire.stableCode {
        case "workspace_revoked", "role_changed", "device_revoked", "workspace_forbidden": return true
        default: return false
        }
    }

    var workspaceVerificationEnded: Bool { reopeningWorkspace || checkingPlaintextRejection || refusedWorkspaceContext }

    func prepareWorkspace(channel: Int, route: HostRoute) async {
        do {
            let data = try await perform("workspaceContext", body: Data("{}".utf8), timeout: 10, workspaceProbe: channel)
            guard link?.channel == channel, link?.sealer != nil else { return }
            let context = try JSONDecoder().decode(ForkPairedWorkspace.self, from: data)
            guard context.isSupported, workspace.map({ context.continues($0) }) ?? true,
                  !reopeningWorkspace || context.workspaceEpoch != workspace?.workspaceEpoch else {
                throw HostError.unreachable
            }
            workspace = context
        } catch HostError.http(404, _) where workspace == nil && !reopeningWorkspace {
            guard link?.channel == channel, link?.sealer != nil else { return }
            legacyWorkspace = true
        } catch {
            guard link?.channel == channel, link?.sealer != nil else { return }
            refusedWorkspaceContext = true
            pendingOutcome = .stop(.failed(HostError.unreachable.localizedDescription))
            link?.sealer = nil
            link?.opener = nil
            link?.socket.close(code: 1000)
            return
        }
        reopeningWorkspace = false
        checkingPlaintextRejection = false
        verifiedChannel = channel
        setState(.ready(route))
        await refreshComputer(channel: channel)
    }

    public func pairedWorkspace(onLink channel: Int) throws -> ForkPairedWorkspace? {
        guard readyLink() == channel else { throw HostError.unreachable }
        return workspace
    }

    func retireWorkspace(_ issue: ForkWorkspaceIssue) {
        guard link?.sealer != nil else { return }
        let retry = issue == .workspaceEpochMismatch && !reopeningWorkspace
        reopeningWorkspace = true
        restartRequested = false
        pendingOutcome = retry ? .again : .stop(.unauthorized(Self.httpMessage(403)))
        link?.sealer = nil
        link?.opener = nil
        setState(.workspaceInvalidated)
        dropChannel(issue)
        dropMailbox()
        link?.socket.close(code: 1000)
    }

    func recheckPlaintextRejection() -> Outcome {
        if checkingPlaintextRejection { return .stop(.failed(HostError.unreachable.localizedDescription)) }
        checkingPlaintextRejection = true
        return .retry(HostError.unreachable.localizedDescription)
    }
}
