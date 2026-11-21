// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

// Wire types for codync-host (see host/src/api/). Field names match the
// host's camelCase JSON. Timestamps are epoch milliseconds.

public struct Bot: Codable, Identifiable, Hashable, Sendable {
    public var id: String
    /// `agent`, or `group`: several bots and the user in one chat (it has no agent, folder or harness).
    public var kind: String
    /// A group's bots.
    public var members: [String]
    public var name: String
    /// Função do agente informada pelo Hub.
    public var title: String
    public var description: String
    public var avatarColor: String
    public var avatarShape: String
    /// O avatar legado completo (pontos) que o Hub manda; reserva do avatar da Mexeriquinha. Nil de um Hub antigo ou se vier inválido.
    public var avatarConfig: LegacyAvatarConfig?
    /// O avatar da Mexeriquinha (Simples ou 3D). Nil quando o bot ainda não tem um. Ver `BotAvatar.resolve`.
    public var mexeriquinha: MexeriquinhaConfig?
    public var backend: String
    public var command: String?
    public var cwd: String
    /// Host-allocated personal workspace rather than a user-selected project.
    public var managedWorkspace: Bool = false
    /// `ask` or `auto`.
    public var permission: String
    public var model: String?
    /// The model's own settings (effort, fast…): agent option id → value.
    public var modelOptions: [String: String] = [:]
    /// The agent's Fast mode (Claude Code, Codex): faster replies for more usage.
    public var fastMode: Bool = false
    /// What the agent says about Fast mode: `unknown` (no session yet), `offered`, `unsupported`,
    /// or `refused` (it turned it off; `fastModeIssue` says why).
    public var fastModeSupport: String = "unknown"
    public var fastModeIssue: String?
    public var pinned: Bool
    /// Where the user dragged it, lowest first (after pinned bots); ties sort by recent activity.
    public var position: Int = 0
    public var hidden: Bool
    public var notify: Bool?
    /// Installed connector / skill ids this bot uses (see `Market.swift`).
    public var connectors: [String]
    public var skills: [String]
    /// The bot can see and operate the computer's desktop (the built-in `computer` tools).
    public var computer: Bool
    /// The bot can create bots (which can too) and change any bot's setup (the built-in `manage` tools). Nil from an older host.
    public var manageTeam: Bool?
    /// The bot that created this one; nil when the user did.
    public var createdBy: String?
    /// Uses the memory all the user's bots share ("About you"). Nil from an older host.
    public var sharedMemory: Bool?
    /// Keeps the harness's own memory (Claude Code's…) on beside Codync's. Nil where it has no switch.
    public var harnessMemory: Bool?
    /// Moves whenever memory changes, so an open memory screen fetches it again.
    public var memoryRev: Int64?
    /// Created without a name: the host names it after its first few conversations.
    public var autoName: Bool = false
    public var createdAt: Int64

    // Runtime, filled in by the host.
    public var rev: Int64
    /// idle | working | needsInput | error
    public var status: String
    public var activity: String
    public var startedAt: Int64?
    /// Where the running turn talks: the bot's own chat, or a group.
    public var workingChat: String?
    /// O bot tem pergunta ao dono (`ask_owner`) ainda sem resposta. O app não as mostra; só decidem o estado do avatar
    /// ("precisa de você"), então guardamos apenas o fato e nunca a lista nem o texto (entrada do Hub/relay não confiável).
    /// Falso de um Hub antigo ou se a lista vier fora do contrato. Ver `PendingQuestion.existe(em:)`.
    public var hasPendingQuestions: Bool = false
    public var unread: Int
    public var lastMessage: String?
    public var lastAt: Int64

    public var isGroup: Bool { kind == "group" }
    public var isWorking: Bool { status == "working" || status == "needsInput" }
    /// Working right now in this chat (not in a group it's in, or the reverse).
    public func isWorking(in chat: String) -> Bool {
        isWorking && (workingChat ?? id) == chat
    }
    public var needsInput: Bool { status == "needsInput" }
    public var folderName: String { managedWorkspace ? String(localized: "Espaço de trabalho pessoal", bundle: .module) : (cwd as NSString).lastPathComponent }
}

/// A lista `pendingQuestions` que o Hub manda (`fork-owner-question.ts` do desktop), lida SEM guardar nada: o iPhone só
/// precisa saber SE existe uma pergunta válida. Limita a entrada: a leitura para na primeira pergunta válida e examina
/// no máximo `maxExaminadas` elementos, então uma lista de 10.000 itens ou strings de 1 MB não viram memória retida.
enum PendingQuestion {
    /// Quantos elementos a leitura examina, no máximo (o desktop guarda cinco; sobra margem para inválidos no meio).
    /// Uma pergunta válida depois de mais de 20 inválidas não conta: entrada hostil, nunca vinda de um Hub atual.
    static let maxExaminadas = 20

    private enum HubKeys: String, CodingKey { case pendingQuestions }

    /// Como `readPendingQuestions`: `entryId`, `chatId` e `text` não vazios e `askedAt` finito. Só confere, não retém.
    private struct Contrato: Decodable {
        private enum CodingKeys: String, CodingKey { case entryId, chatId, text, askedAt }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let entryId = try c.decode(String.self, forKey: .entryId)
            let chatId = try c.decode(String.self, forKey: .chatId)
            let text = try c.decode(String.self, forKey: .text)
            let askedAt = try c.decode(Double.self, forKey: .askedAt)
            guard !entryId.isEmpty, !chatId.isEmpty, !text.isEmpty, askedAt.isFinite else {
                throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "pergunta fora do contrato"))
            }
        }
    }

    /// Um elemento que pode vir fora do contrato: nunca lança, então o contêiner sempre avança (e o bot nunca cai).
    private struct Slot: Decodable {
        let valida: Bool
        init(from decoder: Decoder) throws {
            valida = (try? Contrato(from: decoder)) != nil
        }
    }

    /// Há ao menos uma pergunta válida em `pendingQuestions`? Ausente, `null` ou de forma desconhecida: não.
    static func existe(em decoder: Decoder) -> Bool { examinar(em: decoder).existe }

    /// Como `existe(em:)`, e diz também quantos elementos examinou (nunca mais que `maxExaminadas`): prova o limite
    /// sem medir tempo. Interno, para `@testable`.
    static func examinar(em decoder: Decoder) -> (existe: Bool, examinadas: Int) {
        guard let hub = try? decoder.container(keyedBy: HubKeys.self),
              var lista = try? hub.nestedUnkeyedContainer(forKey: .pendingQuestions) else { return (false, 0) }
        var examinadas = 0
        while !lista.isAtEnd, examinadas < maxExaminadas {
            examinadas += 1
            if let slot = try? lista.decode(Slot.self), slot.valida { return (true, examinadas) }
        }
        return (false, examinadas)
    }
}

/// Just enough of a `bot` event to detect deletions.
struct BotStub: Decodable {
    var id: String
    var deleted: Bool?
    var rev: Int64
}

public struct Entry: Codable, Identifiable, Hashable, Sendable {
    public var id: String
    public var seq: Int64
    /// The chat it's in: a bot's, or a group's.
    public var botId: String
    public var rev: Int64
    /// user | agent | thought | tool | plan | permission | notice
    public var kind: String
    public var turn: Int64
    public var data: EntryData
    public var createdAt: Int64
    public var updatedAt: Int64
    public var sessionId: String?

    public init(id: String, seq: Int64, botId: String, rev: Int64, kind: String, turn: Int64, data: EntryData, createdAt: Int64, updatedAt: Int64, sessionId: String? = nil) {
        self.id = id
        self.seq = seq
        self.botId = botId
        self.rev = rev
        self.kind = kind
        self.turn = turn
        self.data = data
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.sessionId = sessionId
    }

    private enum CodingKeys: String, CodingKey { case id, seq, botId, rev, kind, turn, data, createdAt, updatedAt, sessionId }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        seq = try c.decode(Int64.self, forKey: .seq)
        botId = try c.decode(String.self, forKey: .botId)
        rev = try c.decode(Int64.self, forKey: .rev)
        kind = try c.decode(String.self, forKey: .kind)
        turn = try c.decode(Int64.self, forKey: .turn)
        data = try c.decode(EntryData.self, forKey: .data)
        createdAt = try c.decode(Int64.self, forKey: .createdAt)
        updatedAt = try c.decode(Int64.self, forKey: .updatedAt)
        sessionId = try c.decodeIfPresent(String.self, forKey: .sessionId)
        if let sessionId, sessionId.isEmpty || sessionId.utf8.count > 256 {
            throw DecodingError.dataCorruptedError(forKey: .sessionId, in: c, debugDescription: "Invalid session id")
        }
    }

    /// Ordem da fila do Hub; entradas sem posição usam seq e empates seguem seq.
    public static func ordenadaPelaFila(_ entries: [Entry]) -> [Entry] {
        entries.sorted {
            let a = $0.data.queueOrder ?? $0.seq
            let b = $1.data.queueOrder ?? $1.seq
            return a == b ? $0.seq < $1.seq : a < b
        }
    }

    /// Chat-visible (Grok Bot model): user messages, final replies, approval cards, notices. Of memory and context,
    /// only a fresh conversation gets a line: memory lives in the bot's memory settings, a summary keeps the same
    /// conversation going, and a kept handoff changed nothing.
    public var isChat: Bool {
        switch kind {
        case "user", "permission": true
        case "notice": data.memory == nil && data.memoryFile == nil && data.compaction == nil && data.handoff?.status != "kept"
        case "agent": data.final == true
        default: false
        }
    }

    /// A message a reply can quote: the user's or a final bot reply, once the computer has it.
    public var isReplyable: Bool {
        (kind == "user" || (kind == "agent" && data.final == true)) && !id.hasPrefix("local-")
    }

    /// How a reply quotes this message until the computer's own quote arrives: who wrote it and its
    /// text, or its file names.
    public var replyQuote: ReplyQuote {
        let names = (data.attachments?.map(\.name) ?? []) + (data.files?.map(\.name) ?? [])
        let text = data.text.flatMap { $0.isEmpty ? nil : $0 } ?? names.joined(separator: ", ")
        return ReplyQuote(id: id, author: kind == "user" ? "user" : data.author ?? botId, text: String(text.prefix(500)))
    }

    public var date: Date { Date(milliseconds: createdAt) }
}

public struct FileDiff: Codable, Hashable, Sendable {
    public var path: String
    public var added: Int
    public var removed: Int
    public var isNew: Bool
    public var startLine: Int
    public var patch: String
}

public struct Location: Codable, Hashable, Sendable {
    public var path: String
    public var line: Int?
}

public struct PermissionOption: Codable, Hashable, Sendable, Identifiable {
    public var optionId: String
    public var name: String
    /// allow_once | allow_always | reject_once | reject_always
    public var kind: String
    public var id: String { optionId }
}

public struct PlanItem: Codable, Hashable, Sendable {
    public var content: String
    public var status: String
    public var priority: String?
}

public struct Usage: Codable, Hashable, Sendable {
    public var providers: [UsageProvider]
    public init(providers: [UsageProvider] = []) { self.providers = providers }
}

public struct UsageProvider: Codable, Hashable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var windows: [UsageWindow]
    public var source: String
    public var updatedAt: Int64
}

public struct UsageWindow: Codable, Hashable, Sendable, Identifiable {
    public var id: String
    public var label: String
    public var percent: Double
    public var resetsAt: Int64?
    /// Human text when only that is known ("Sep 26 at 12pm (Asia/Taipei)").
    public var resetsText: String?
    public var resetDate: Date? { resetsAt.map(Date.init(milliseconds:)) ?? resetsText.flatMap { Self.parseReset($0) } }

    /// Claude Code's text: "Sep 26 at 12pm (Asia/Taipei)", "Sep 25 at 7:30pm (Asia/Taipei)", or just "7:30pm (…)".
    /// The year isn't given: the next such date from `now`.
    static func parseReset(_ text: String, now: Date = .now) -> Date? {
        guard let match = text.firstMatch(of: /^(?:(\w{3} \d{1,2}) at )?(\d{1,2})(?::(\d{2}))?(am|pm)(?: \(([^)]+)\))?$/) else { return nil }
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = match.5.flatMap { TimeZone(identifier: String($0)) } ?? .current
        guard let hour12 = Int(match.2) else { return nil }
        var parts = cal.dateComponents([.year, .month, .day], from: now)
        if let day = match.1 {
            let f = DateFormatter()
            f.locale = Locale(identifier: "en_US_POSIX")
            f.dateFormat = "MMM d"
            guard let md = f.date(from: String(day)) else { return nil }
            let c = Calendar(identifier: .gregorian).dateComponents([.month, .day], from: md)
            parts.month = c.month
            parts.day = c.day
        }
        parts.hour = hour12 % 12 + (match.4 == "pm" ? 12 : 0)
        parts.minute = match.3.flatMap { Int($0) } ?? 0
        guard var date = cal.date(from: parts) else { return nil }
        // Past by more than a day: it means next year (or tomorrow, for a bare time).
        if date < now - 86_400 || (match.1 == nil && date < now) {
            date = cal.date(byAdding: match.1 == nil ? .day : .year, value: 1, to: date) ?? date
        }
        return date
    }

    /// "resets in 3h 20m" / "resets Sep 26 at 12pm", or nil.
    public var resetDescription: String? {
        if let d = resetDate { return String(localized: "reinicia em \(RelativeTime.until(d))", bundle: .module) }
        return resetsText.map { String(localized: "reinicia \($0.replacingOccurrences(of: #" \(.*\)$"#, with: "", options: .regularExpression))", bundle: .module) }
    }
}

public struct Backend: Codable, Hashable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var available: Bool
    public var command: String
    public var installHint: String
    /// ACP registry id, for the agent's icon (`agent-<id>` in the asset catalog).
    public var registry: String?
    public var description: String?
    public var installed: Bool?
    /// Known to Codync by name: it can install and sign in to it.
    public var curated: Bool?
    /// Missing when the host can't tell.
    public var signedIn: Bool?
    public var canInstall: Bool?
    /// Only for an agent that runs on Node.js, on a computer without its own: Codync's copy
    /// is `missing` or `added`.
    public var node: String?

    /// It can't run until Node.js is added to the computer.
    public var needsNode: Bool { node == "missing" }
}

/// How an agent can be signed in, from asking the agent itself.
public struct AgentAuth: Decodable, Sendable {
    public var signedIn: Bool?
    /// The agent's own words when it wants signing in (can hold a pairing code).
    public var detail: String?
    public var methods: [AuthMethod]
    /// Keys saved on the computer for this agent (names only).
    public var savedEnv: [String]?
    /// Codync's own sign-in command is available.
    public var login: Bool?
}

public struct AuthMethod: Decodable, Sendable, Identifiable, Hashable {
    public enum Kind: String, Decodable, Sendable {
        /// Runs in a setup terminal.
        case terminal
        /// The agent signs itself in (usually a browser on the computer).
        case agent
        /// Keys typed here, kept on the computer.
        case envVar
    }

    public struct Var: Decodable, Sendable, Hashable {
        public var name: String
        public var label: String
        public var secret: Bool
        public var optional: Bool
    }

    public var id: String
    public var name: String
    public var description: String?
    public var kind: Kind?
    public var vars: [Var]?
    public var link: String?
}

/// What a setup terminal on the computer runs.
public enum SetupStep: String, Codable, Sendable {
    case install, node, login
}

public struct Hello: Codable, Sendable {
    public var hostId: String
    public var name: String
    public var version: String
    public var os: String
    /// laptop · macmini · macstudio · imac · macpro · desktop · linux (missing from older hosts).
    public var device: String?
    public var home: String?
    public var backends: [Backend]
    public var rev: Int64
    /// Missing from hosts that predate remote screen.
    public var screen: ScreenState?
    /// The host's current direct addresses.
    public var urls: [String]?
    /// Channel identity (§9.4); missing only on loopback to an older host.
    public var computerId: ComputerID?
    public var signKey: String?
    public var boxKey: String?
    /// The oldest app version this host serves (missing from hosts that predate the check).
    public var minApp: String?
    /// The cloud relay the host uses; nil = cloud off.
    public var cloud: String?
}

public struct DirListing: Codable, Sendable {
    public var path: String
    public var parent: String?
    public var isGit: Bool
    public var dirs: [DirItem]
}

public struct DirItem: Codable, Hashable, Sendable, Identifiable {
    public var name: String
    public var path: String
    public var isGit: Bool
    public var id: String { path }
}

/// The source is carried by a draft, but is not a form value or part of its wire format.
struct DraftSource: Hashable, Sendable {
    var bot: Bot? = nil
    static func == (lhs: Self, rhs: Self) -> Bool { true }
    func hash(into hasher: inout Hasher) {}
}

/// Create / update payload for a group chat.
public struct GroupDraft: Codable, Hashable, Sendable {
    public var id: String?
    public var kind = "group"
    public var name: String
    /// What the group is for; its bots are told.
    public var description = ""
    public var members: [String]
    public var pinned: Bool?
    var source = DraftSource()
    private enum CodingKeys: String, CodingKey { case id, kind, name, description, members, pinned }

    public init(name: String, description: String = "", members: [String]) {
        self.name = name
        self.description = description
        self.members = members
    }

    public init(_ group: Bot) {
        id = group.id
        name = group.name
        description = group.description
        members = group.members
        pinned = group.pinned
        source = DraftSource(bot: group)
    }
}

/// Create / update payload for a bot.
public struct BotDraft: Codable, Hashable, Sendable {
    public var id: String?
    public var name: String
    public var description: String
    public var avatarColor: String
    public var avatarShape: String
    public var backend: String
    public var command: String?
    public var cwd: String
    public var permission: String
    /// Intenção do editor; nunca copiada do bot salvo ao montar ações de perfil.
    public var permissionChosenByUser: Bool?
    public var model: String?
    public var modelOptions: [String: String] = [:]
    public var fastMode: Bool?
    public var pinned: Bool?
    public var hidden: Bool?
    public var notify: Bool?
    public var connectors: [String]?
    public var skills: [String]?
    public var computer: Bool?
    public var manageTeam: Bool?
    /// Only sent when the user changes it: unset, the host picks by the bot's folder.
    public var sharedMemory: Bool?
    /// Only sent when the user changes it.
    public var harnessMemory: Bool?
    var source = DraftSource()
    private enum CodingKeys: String, CodingKey {
        case id, name, description, avatarColor, avatarShape, backend, command, cwd, permission, permissionChosenByUser
        case model, modelOptions, fastMode, pinned, hidden, notify, connectors, skills, computer, manageTeam, sharedMemory, harnessMemory
    }

    public init(name: String = "", description: String = "", avatarColor: String = AvatarPalette.colors.randomElement()!.id,
                avatarShape: String = AvatarPalette.shapes.randomElement()!, backend: String = "claude", cwd: String = "",
                permission: String = "auto") {
        self.name = name
        self.description = description
        self.avatarColor = avatarColor
        self.avatarShape = avatarShape
        self.backend = backend
        self.cwd = cwd
        self.permission = permission
    }

    public init(_ bot: Bot) {
        id = bot.id
        name = bot.name
        description = bot.description
        avatarColor = bot.avatarColor
        avatarShape = bot.avatarShape
        backend = bot.backend
        command = bot.command
        cwd = bot.managedWorkspace ? "" : bot.cwd
        permission = bot.permission
        model = bot.model
        modelOptions = bot.modelOptions
        fastMode = bot.fastMode
        pinned = bot.pinned
        hidden = bot.hidden
        notify = bot.notify
        connectors = bot.connectors
        skills = bot.skills
        computer = bot.computer
        manageTeam = bot.manageTeam
        source = DraftSource(bot: bot)
    }

    // Perfil e criação sem seleção deixam a autonomia a cargo do host.
    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encodeIfPresent(id, forKey: .id)
        try c.encode(name, forKey: .name)
        try c.encode(description, forKey: .description)
        try c.encode(avatarColor, forKey: .avatarColor)
        try c.encode(avatarShape, forKey: .avatarShape)
        try c.encode(backend, forKey: .backend)
        try c.encodeIfPresent(command, forKey: .command)
        try c.encode(cwd, forKey: .cwd)
        try c.encodeIfPresent(model, forKey: .model)
        try c.encode(modelOptions, forKey: .modelOptions)
        try c.encodeIfPresent(fastMode, forKey: .fastMode)
        try c.encodeIfPresent(pinned, forKey: .pinned)
        try c.encodeIfPresent(hidden, forKey: .hidden)
        try c.encodeIfPresent(notify, forKey: .notify)
        try c.encodeIfPresent(connectors, forKey: .connectors)
        try c.encodeIfPresent(skills, forKey: .skills)
        try c.encodeIfPresent(computer, forKey: .computer)
        try c.encodeIfPresent(manageTeam, forKey: .manageTeam)
        try c.encodeIfPresent(sharedMemory, forKey: .sharedMemory)
        try c.encodeIfPresent(harnessMemory, forKey: .harnessMemory)
        if permissionChosenByUser == true {
            try c.encode(permission, forKey: .permission)
            try c.encode(true, forKey: .permissionChosenByUser)
        }
    }

    /// This draft with each field it hasn't changed from `base` (the bot as the form loaded it) taken from `fresh`
    /// (the bot as the host has it now), so saving the form keeps a change made elsewhere: a bot that manages the
    /// team, another device.
    public func rebased(from base: BotDraft, onto fresh: BotDraft) -> BotDraft {
        var draft = self
        func follow<Value: Equatable>(_ field: WritableKeyPath<BotDraft, Value>) {
            if self[keyPath: field] == base[keyPath: field] { draft[keyPath: field] = fresh[keyPath: field] }
        }
        follow(\.name); follow(\.description); follow(\.avatarColor); follow(\.avatarShape); follow(\.backend)
        follow(\.command); follow(\.cwd); follow(\.model); follow(\.modelOptions)
        if permissionChosenByUser != true { follow(\.permission) }
        follow(\.fastMode); follow(\.pinned); follow(\.hidden); follow(\.notify); follow(\.connectors)
        follow(\.skills); follow(\.computer); follow(\.manageTeam); follow(\.sharedMemory); follow(\.harnessMemory)
        draft.source = fresh.source
        return draft
    }
}

public extension Date {
    init(milliseconds: Int64) { self.init(timeIntervalSince1970: Double(milliseconds) / 1000) }
}

extension Bot {
    /// Teto, em BYTES UTF-8, de `avatarColor` e `avatarShape` (ids curtos como "orange" ou "blob").
    static let maxAvatarId = 64

    /// Id curto do Hub (nome da cor ou da forma): aceita só ASCII, não vazio, com no máximo `maxAvatarId` bytes
    /// (`utf8.count`, não caracteres: um caractere pode carregar marcas combinantes sem fim) e só `[A-Za-z0-9#_-]`.
    /// Ausente ou fora disso, devolve `padrao`, o mesmo valor usado quando o campo falta.
    static func idCurto(_ valor: String?, padrao: String) -> String {
        guard let valor, !valor.isEmpty, valor.utf8.count <= maxAvatarId, valor.utf8.allSatisfy(Self.byteDeId) else { return padrao }
        return valor
    }

    private static func byteDeId(_ byte: UInt8) -> Bool {
        switch byte {
        case UInt8(ascii: "A")...UInt8(ascii: "Z"), UInt8(ascii: "a")...UInt8(ascii: "z"), UInt8(ascii: "0")...UInt8(ascii: "9"),
             UInt8(ascii: "#"), UInt8(ascii: "_"), UInt8(ascii: "-"): true
        default: false
        }
    }

    /// Lenient: a host that is a little newer or older than the app still yields a usable bot.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? "agent"
        members = try c.decodeIfPresent([String].self, forKey: .members) ?? []
        name = try c.decodeIfPresent(String.self, forKey: .name) ?? "Bot"
        title = BotSnapshot.limit(try c.decodeIfPresent(String.self, forKey: .title) ?? "", 256)
        description = try c.decodeIfPresent(String.self, forKey: .description) ?? ""
        // Ids curtos (nome da cor e da forma): validação estrita evita guardar (e levar ao snapshot dos widgets) texto enorme do Hub.
        avatarColor = Self.idCurto(try c.decodeIfPresent(String.self, forKey: .avatarColor), padrao: "blue")
        avatarShape = Self.idCurto(try c.decodeIfPresent(String.self, forKey: .avatarShape), padrao: "blob")
        // Lenient on purpose: a malformed avatar never costs the bot (the face falls back to the default mascot).
        // E só a forma já validada fica no Bot (e daí no snapshot que widgets e extensões decodificam inteiro):
        // inválido, grande demais ou de tipo errado vira nil, igual ao que `BotAvatar.resolve` já faria com ele.
        avatarConfig = (try? c.decodeIfPresent(LegacyAvatarConfig.self, forKey: .avatarConfig))?.ifValid
        mexeriquinha = (try? c.decodeIfPresent(MexeriquinhaConfig.self, forKey: .mexeriquinha))?.ifValid
        backend = try c.decodeIfPresent(String.self, forKey: .backend) ?? "custom"
        command = try c.decodeIfPresent(String.self, forKey: .command)
        cwd = try c.decodeIfPresent(String.self, forKey: .cwd) ?? ""
        managedWorkspace = try c.decodeIfPresent(Bool.self, forKey: .managedWorkspace) ?? false
        permission = try c.decodeIfPresent(String.self, forKey: .permission) ?? "auto"
        model = try c.decodeIfPresent(String.self, forKey: .model)
        modelOptions = try c.decodeIfPresent([String: String].self, forKey: .modelOptions) ?? [:]
        fastMode = try c.decodeIfPresent(Bool.self, forKey: .fastMode) ?? false
        fastModeSupport = try c.decodeIfPresent(String.self, forKey: .fastModeSupport) ?? "unknown"
        fastModeIssue = try c.decodeIfPresent(String.self, forKey: .fastModeIssue)
        pinned = try c.decodeIfPresent(Bool.self, forKey: .pinned) ?? false
        position = try c.decodeIfPresent(Int.self, forKey: .position) ?? 0
        hidden = try c.decodeIfPresent(Bool.self, forKey: .hidden) ?? false
        notify = try c.decodeIfPresent(Bool.self, forKey: .notify)
        connectors = try c.decodeIfPresent([String].self, forKey: .connectors) ?? []
        skills = try c.decodeIfPresent([String].self, forKey: .skills) ?? []
        computer = try c.decodeIfPresent(Bool.self, forKey: .computer) ?? false
        manageTeam = try c.decodeIfPresent(Bool.self, forKey: .manageTeam)
        createdBy = try c.decodeIfPresent(String.self, forKey: .createdBy)
        sharedMemory = try c.decodeIfPresent(Bool.self, forKey: .sharedMemory)
        harnessMemory = try c.decodeIfPresent(Bool.self, forKey: .harnessMemory)
        memoryRev = try c.decodeIfPresent(Int64.self, forKey: .memoryRev)
        autoName = try c.decodeIfPresent(Bool.self, forKey: .autoName) ?? false
        createdAt = try c.decodeIfPresent(Int64.self, forKey: .createdAt) ?? 0
        rev = try c.decodeIfPresent(Int64.self, forKey: .rev) ?? 0
        status = try c.decodeIfPresent(String.self, forKey: .status) ?? "idle"
        activity = try c.decodeIfPresent(String.self, forKey: .activity) ?? ""
        startedAt = try c.decodeIfPresent(Int64.self, forKey: .startedAt)
        workingChat = try c.decodeIfPresent(String.self, forKey: .workingChat)
        // `hasPendingQuestions` é a chave do snapshot dos widgets (o app grava o Bot); `pendingQuestions` é a lista do Hub.
        hasPendingQuestions = ((try? c.decodeIfPresent(Bool.self, forKey: .hasPendingQuestions)) ?? false) || PendingQuestion.existe(em: decoder)
        unread = try c.decodeIfPresent(Int.self, forKey: .unread) ?? 0
        lastMessage = try c.decodeIfPresent(String.self, forKey: .lastMessage)
        lastAt = try c.decodeIfPresent(Int64.self, forKey: .lastAt) ?? createdAt
    }
}
