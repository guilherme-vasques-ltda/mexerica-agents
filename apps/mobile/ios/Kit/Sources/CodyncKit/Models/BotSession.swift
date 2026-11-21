import Foundation

/// A conversation in the Hub catalog. The principal session has the chat's id.
public struct BotSession: Decodable, Hashable, Sendable, Identifiable {
    public let id: String
    public let chatId: String
    public let principal: Bool
    public let parentSession: String?
    public let depth: Int
    public let title: String
    public let state: String
    public let revision: Int64

    private enum CodingKeys: String, CodingKey { case id, chatId, principal, parentSession, depth, title, state, revision }
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        chatId = try c.decode(String.self, forKey: .chatId)
        guard !id.isEmpty, id.utf8.count <= 256, !chatId.isEmpty, chatId.utf8.count <= 256 else {
            throw DecodingError.dataCorruptedError(forKey: .id, in: c, debugDescription: "Invalid session id")
        }
        principal = try c.decode(Bool.self, forKey: .principal)
        parentSession = try c.decodeIfPresent(String.self, forKey: .parentSession)
        if let parentSession, parentSession.utf8.count > 256 {
            throw DecodingError.dataCorruptedError(forKey: .parentSession, in: c, debugDescription: "Invalid parent session id")
        }
        depth = try c.decode(Int.self, forKey: .depth)
        title = BotSnapshot.limit(try c.decode(String.self, forKey: .title), 256)
        state = BotSnapshot.limit(try c.decode(String.self, forKey: .state), 64)
        revision = try c.decode(Int64.self, forKey: .revision)
    }
}

public struct BotSessions: Decodable, Sendable {
    public let sessions: [BotSession]
    /// Missing in older Hubs, which may ignore send.sessionId.
    public let sessionActions: [String]?
}
