// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

/// `b64url(SHA-256(hostSignPub)[0..16])`: derived from the host's signing key, never assigned by the cloud.
public typealias ComputerID = String

/// A computer this device may talk to. Holds no secrets: only pinned public keys and addresses.
public struct Computer: Codable, Hashable, Sendable, Identifiable {
    public var id: ComputerID
    public var name: String
    /// Pinned host signing key: the QR's `sk`, or the account flow's key once its SAS matched.
    public var signKey: String
    /// Pinned mailbox key: only from the QR's `bk` or the E2E `hello`; nil = no mailbox yet.
    public var boxKey: String?
    /// Direct candidates (`http://ip:port`), tried before the relay.
    public var urls: [String]
    /// The cloud relay base URL; nil = the host has the cloud off.
    public var cloud: URL?
    /// laptop · macmini · … · linux, from `hello`; picks the icon.
    public var device: String?
    /// The badge color picked on this device (an `AvatarPalette` id).
    public var color: String?
    /// How this device reaches the computer, picked per computer; nil = `.automatic`.
    public var route: ConnectionRoute?

    public init(id: ComputerID, name: String, signKey: String, boxKey: String? = nil, urls: [String] = [],
                cloud: URL? = nil, device: String? = nil, color: String? = nil) {
        self.id = id
        self.name = name
        self.signKey = signKey
        self.boxKey = boxKey
        self.urls = urls
        self.cloud = cloud
        self.device = device
        self.color = color
    }

    /// `true` when `id` really is the computer ID of `signKey`.
    public var isConsistent: Bool {
        Data(base64URL: signKey).map { $0.count == 32 && RelayCrypto.computerId(signKey: $0) == id } ?? false
    }
}

/// The order routes are tried in. Every paired computer has both routes; this only says which goes first
/// and whether the other one is a fallback.
public enum ConnectionRoute: String, Codable, CaseIterable, Sendable {
    /// Wi-Fi or Tailscale when it answers within 1.5 s, Cloudflare when it doesn't.
    case automatic
    /// Account default: the Hub may listen only on loopback; never dial advertised LAN addresses.
    case relayOnly
    /// Cloudflare, falling back to Wi-Fi or Tailscale when Cloudflare can't be reached.
    case cloudflareFirst
    /// Wi-Fi and Tailscale only: nothing goes through Cloudflare, even when they fail.
    case directOnly

    public init(from decoder: any Decoder) throws {
        self = Self(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .automatic
    }
}

/// A bot on a computer in an account context: bot IDs are only unique per computer.
/// (Multiple computers are future work; with one computer this is just the bot's address.)
public struct BotReference: Codable, Hashable, Sendable {
    /// `SharedStore.Context.accountID` (raw Clerk user ID; nil = the local context).
    public var accountId: String?
    public var computerId: ComputerID
    public var botId: String

    public init(accountId: String?, computerId: ComputerID, botId: String) {
        self.accountId = accountId
        self.computerId = computerId
        self.botId = botId
    }
}

/// Bounded representation persisted for widgets and the notification extension.
public struct BotSnapshot: Codable, Hashable, Sendable {
    public static let maxBots = 100
    public private(set) var computerId: ComputerID
    public private(set) var bot: Bot

    private enum Keys: String, CodingKey { case computerId, bot }
    private enum BotKeys: String, CodingKey {
        case id, kind, members, name, avatarColor, avatarShape, avatarConfig, mexeriquinha
        case hidden, createdAt, status, activity, startedAt, hasPendingQuestions, lastAt
    }

    /// Limit UTF-8 bytes without splitting a character (including combining sequences).
    static func limit(_ text: String, _ bytes: Int) -> String {
        var result = ""
        var used = 0
        for character in text {
            let size = String(character).utf8.count
            guard size <= bytes - used else { break }
            result.append(character)
            used += size
        }
        return result
    }

    public init(computerId: ComputerID, bot: Bot) {
        self.computerId = Self.limit(computerId, 256)
        var b = bot
        b.id = Self.limit(b.id, 256)
        b.kind = Self.limit(b.kind, 64)
        b.members = b.members.prefix(16).map { Self.limit($0, 64) }
        b.name = Self.limit(b.name, 128)
        if b.name.isEmpty { b.name = "Bot" }
        b.avatarColor = Bot.idCurto(b.avatarColor, padrao: "blue")
        b.avatarShape = Bot.idCurto(b.avatarShape, padrao: "blob")
        b.avatarConfig = b.avatarConfig?.ifValid
        b.mexeriquinha = b.mexeriquinha?.ifValid
        b.status = Self.limit(b.status, 64)
        b.activity = Self.limit(b.activity, 2048)
        // Drop all fields not read by the extensions, including large collections.
        b.title = ""; b.description = ""; b.backend = "custom"; b.command = nil; b.cwd = ""
        b.managedWorkspace = false; b.permission = "auto"; b.model = nil; b.modelOptions = [:]
        b.fastMode = false; b.fastModeSupport = "unknown"; b.fastModeIssue = nil
        b.pinned = false; b.position = 0; b.notify = nil; b.connectors = []; b.skills = []
        b.computer = false; b.manageTeam = nil; b.createdBy = nil; b.sharedMemory = nil
        b.harnessMemory = nil; b.memoryRev = nil; b.autoName = false; b.rev = 0
        b.workingChat = nil; b.unread = 0; b.lastMessage = nil
        self.bot = b
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        self.init(computerId: try c.decode(String.self, forKey: .computerId),
                  bot: try c.decode(Bot.self, forKey: .bot))
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        try c.encode(computerId, forKey: .computerId)
        var b = c.nestedContainer(keyedBy: BotKeys.self, forKey: .bot)
        try b.encode(bot.id, forKey: .id); try b.encode(bot.kind, forKey: .kind)
        try b.encode(bot.members, forKey: .members); try b.encode(bot.name, forKey: .name)
        try b.encode(bot.avatarColor, forKey: .avatarColor); try b.encode(bot.avatarShape, forKey: .avatarShape)
        try b.encodeIfPresent(bot.avatarConfig, forKey: .avatarConfig)
        try b.encodeIfPresent(bot.mexeriquinha, forKey: .mexeriquinha)
        try b.encode(bot.hidden, forKey: .hidden); try b.encode(bot.createdAt, forKey: .createdAt)
        try b.encode(bot.status, forKey: .status); try b.encode(bot.activity, forKey: .activity)
        try b.encodeIfPresent(bot.startedAt, forKey: .startedAt)
        try b.encode(bot.hasPendingQuestions, forKey: .hasPendingQuestions); try b.encode(bot.lastAt, forKey: .lastAt)
    }
}
