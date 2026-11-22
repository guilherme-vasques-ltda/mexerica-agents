// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import ActivityKit
import Foundation

/// Live Activity for a bot working on something the user just delegated.
/// Content state keys must match what codync-host pushes (host/src/remote/push.rs).
public struct BotActivityAttributes: ActivityAttributes {
    public struct ContentState: Codable, Hashable, Sendable {
        /// idle | working | needsInput | error
        public var status: String
        /// Local app updates may show the current step; remote pushes leave it empty (§6.7).
        public var activity: String
        public var startedAt: Date?

        public init(status: String, activity: String, startedAt: Date?) {
            self.status = status
            self.activity = activity
            self.startedAt = startedAt
        }
    }

    public var link: URL?
    public var computerId: ComputerID
    public var botId: String
    public var name: String
    public var avatarShape: String
    public var avatarColor: String
    /// The bot's avatar as the host sent it (see `BotAvatar.resolve`). Nil in an activity started by an older app.
    public var avatarConfig: LegacyAvatarConfig?
    public var mexeriquinha: MexeriquinhaConfig?

    /// The Mexeriquinha the Dynamic Island and the Lock Screen draw.
    public var avatar: BotAvatar { BotAvatar.resolve(avatarConfig: avatarConfig, mexeriquinha: mexeriquinha) }

    public init(bot: Bot, computerId: ComputerID, link: URL? = nil) {
        self.link = link
        self.computerId = computerId
        botId = bot.id
        name = bot.name
        avatarShape = bot.avatarShape
        avatarColor = bot.avatarColor
        avatarConfig = bot.avatarConfig
        mexeriquinha = bot.mexeriquinha
    }
}
