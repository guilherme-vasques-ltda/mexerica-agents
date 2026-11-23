import Foundation

public struct ForkWorkspaceFailure: Equatable, Sendable {
    public let issue: ForkWorkspaceIssue
    public let closesChannel: Bool

    public static func channelReject(_ data: Data, closeCode: Int) throws -> Self? {
        struct Reject: Decodable {
            let t: String
            let code: String?
            let stableCode: String?
        }
        let reject = try JSONDecoder().decode(Reject.self, from: data)
        guard reject.t == "reject" else { return nil }
        let issue: ForkWorkspaceIssue?
        if let stableCode = reject.stableCode {
            issue = ForkWorkspaceIssue(rawValue: stableCode)
        } else {
            switch reject.code {
            case "revoked": issue = .deviceRevoked
            case "unauthorized": issue = .deviceUnpaired
            case "leaseExpired": issue = .leaseExpired
            default: issue = nil
            }
        }
        switch closeCode {
        case 4003:
            let terminal = issue == .workspaceRevoked || issue == .roleChanged ? issue : nil
            return Self(issue: terminal ?? .deviceRevoked, closesChannel: true)
        case 4001:
            guard let issue else { return Self(issue: .deviceUnpaired, closesChannel: true) }
            switch issue {
            case .leaseExpired, .authInvalid, .deviceUnpaired, .workspaceRevoked, .roleChanged, .deviceRevoked:
                return Self(issue: issue, closesChannel: true)
            default: return Self(issue: .deviceUnpaired, closesChannel: true)
            }
        default: return nil
        }
    }

    public static func channelReply(_ data: Data) throws -> Self? {
        struct Reply: Decodable { let err: ReplyError? }
        struct ReplyError: Decodable { let status: Int; let code: String? }
        guard let err = try JSONDecoder().decode(Reply.self, from: data).err else { return nil }
        return http(status: err.status, code: err.code)
    }

    public static func http(status: Int, code: String?) -> Self? {
        guard let code, let issue = ForkWorkspaceIssue(rawValue: code) else { return nil }
        switch (status, issue) {
        case (401, .authInvalid), (401, .deviceUnpaired), (401, .deviceRevoked), (401, .leaseExpired),
             (403, .workspaceForbidden), (403, .roleInsufficient), (403, .crossWorkspaceReference),
             (409, .workspaceEpochMismatch):
            return Self(issue: issue, closesChannel: false)
        default: return nil
        }
    }
}
