// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import Foundation
import Observation

/// Opens the screen viewer, optionally watching a bot that's using the computer.
public struct ScreenRequest: Identifiable, Hashable, Sendable {
    public var watching: String?
    public var id: String { watching ?? "computer" }
    public init(watching: String? = nil) { self.watching = watching }
}

/// Where a message from this phone is: on its way, waiting in the relay mailbox for an offline
/// computer, accepted by the computer, or not sent.
public enum SendProgress: Sendable {
    case sending, queued, delivered, failed, cancelled
}

/// One computer's mirror (bots, transcripts) for a client (iPhone app, Mac window), kept by one
/// events stream (catch-up since `rev`, then live) over the encrypted channel.
/// `AccountStore` holds one per computer.
@MainActor
@Observable
public final class BotStore {
    public enum Connection: Equatable {
        case unpaired
        case connecting
        case online
        /// The relay says the computer isn't connected; messages can wait in its mailbox.
        case computerOffline(lastSeen: Date?)
        /// Can't reach the computer at all (no direct route, no relay), or it can't confirm this
        /// device's access for now.
        case offline(String)
        /// Revoked, or the computer's identity changed.
        case unauthorized(String)
    }

    public private(set) var computer: Computer
    public internal(set) var connection: Connection = .connecting
    /// The first catch-up has arrived: everything up to the host's rev when this app connected.
    public internal(set) var caughtUp = false
    #if DEBUG
    /// Mission Board v1 (só Debug): conta os eventos do ciclo que invalidam o quadro. Objeto próprio e observável.
    let quadroSinais = MXQuadroSinais()
    #endif
    var catchUpRev = Int64.max
    /// The `since` this connection's events stream was opened with; the `hello` that opens its
    /// catch-up reads it (after a mirror reset `rev` no longer says what was requested).
    var requestedSince: Int64 = 0
    /// Per bot, the lowest synced main-chat `seq` held when this connection's catch-up began
    /// (empty when it began from rev 0). See `isOutsideLoadedWindow`.
    var windowFloors: [String: Int64] = [:]
    static let dropGrace: Duration = .seconds(5)
    static let initialConnectionGrace: Duration = .seconds(1)
    var heldDrop: Connection?
    var dropTimer: Task<Void, Never>?
    /// How long a call that the link dropped under may be repeated on that same link.
    static let actionPatience: Duration = .seconds(20)
    /// The link as screens see it. A short drop is `.reconectando` and changes no screen;
    /// only five minutes without a link or a definitive failure make it `.desconectado`.
    public internal(set) var estadoConexao: EstadoConexao
    /// The "no connection" strip shows: reconnecting for longer than `dropGrace`.
    public internal(set) var faixaDeQueda = false
    /// Bumped each time a link ends or the Hub identifies itself on a new one: an action never moves to a newer one.
    @ObservationIgnored var linkId = 0 {
        didSet { linkAtual.definir(linkId) }
    }
    /// `linkId` for threads off the main actor (the microphone's audio thread).
    nonisolated let linkAtual = LinkAtual()
    /// Reads the link right now from any thread: voice engines stamp captured audio with it.
    nonisolated var marcador: @Sendable () -> MarcaDoLink {
        let atual = linkAtual
        return { atual.marca() }
    }
    /// The link right now, to read when the person acts (a tap, the start of an utterance) and hand to
    /// the action, so it never goes out on a link that came after.
    public var marcaDoLink: MarcaDoLink { MarcaDoLink(valor: linkId) }
    /// The client bound to the channel the Hub identified itself on (`hello` checked); nil from the moment
    /// any link ends until the next one is identified. Every action goes out through it.
    var clienteAutorizado: HostClient?
    /// When the current drop began, on a monotonic clock: background time and clock changes don't reset it.
    @ObservationIgnored var inicioDaQueda: ContinuousClock.Instant?
    /// The monotonic clock the deadline reads (tests set it).
    @ObservationIgnored var monotonico: @MainActor () -> ContinuousClock.Instant
    /// The last "Try now" that went through; more within `intervaloTentarAgora` are ignored.
    @ObservationIgnored var ultimaTentativa: ContinuousClock.Instant?
    static let intervaloTentarAgora: Duration = .seconds(3)
    /// The Hub asked to identify itself again after a 403 (one at a time).
    @ObservationIgnored var reverificacao: Task<Void, Never>?
    /// The last such check; no other one starts within `intervaloReverificacao` (no `hello` loop).
    @ObservationIgnored var ultimaReverificacao: ContinuousClock.Instant?
    static let intervaloReverificacao: Duration = .seconds(10)
    /// When `AccountStore` saved this computer just now (paired or approved). Only the first transport
    /// after that, and only within `prazoVinculoRecente`, tolerates the relay's early 403s; reading it
    /// consumes it, so no later transport (foreground, "Try now", a refusal) gets that tolerance again.
    @ObservationIgnored var vinculoGravadoEm: ContinuousClock.Instant?
    /// How long after pairing or approval the relay may still not know this device.
    static let prazoVinculoRecente: Duration = .seconds(120)
    /// Opens a transport with that tolerance; nil uses `makeTransport`.
    @ObservationIgnored var conectarRecemVinculado: (@MainActor (Computer) async throws -> any HostTransport)?
    /// Whose mirror this is; a different one after a drop reloads from scratch.
    @ObservationIgnored var donoDoEspelho: DonoDoEspelho
    /// The clock the drop rules read (tests set it).
    @ObservationIgnored var agora: @MainActor () -> Date
    /// How the drop timer waits (tests make it wait forever and move `agora` instead).
    @ObservationIgnored var esperar: @Sendable (Duration) async throws -> Void
    @ObservationIgnored var prazoTask: Task<Void, Never>?
    public internal(set) var hostRoute: HostRoute?
    /// The current transport's own client: only for identifying the Hub (`runEvents`), never for actions.
    var clienteBase: HostClient?
    /// The client for calls to the Hub: the one bound to the channel it identified itself on, nil
    /// from the moment a link ends until the next one is identified.
    public var client: HostClient? { clienteAutorizado }
    public internal(set) var hello: Hello?
    /// The host's version and `minApp`, kept even when the rest of its `hello` can't be read.
    public internal(set) var hostVersion: HostVersion?
    public internal(set) var bots: [String: Bot] = [:]
    public internal(set) var entries: [String: [Entry]] = [:]
    /// Send support confirmed by this Hub for each chat.
    var sessionSendSupported: Set<String> = []
    /// A ordem das leituras de sessões por agente: a última iniciada e a última aplicada (a resposta mais velha que a
    /// aplicada não vale, nem para o estado daqui nem para o de quem lê).
    @ObservationIgnored var leiturasDeSessoesIniciadas: [String: Int] = [:]
    @ObservationIgnored var leiturasDeSessoesAplicadas: [String: Int] = [:]
    public internal(set) var usage: Usage
    /// The computer's remote screen (`nil`: the host predates it).
    public internal(set) var screen: ScreenState?
    /// Installed on the computer, for the Plugins screen and bot settings.
    public internal(set) var installedConnectors: [InstalledConnector] = []
    public internal(set) var installedSkills: [InstalledSkill] = []
    /// Bots whose older history has been fully paged in.
    public internal(set) var historyComplete: Set<String> = []
    var composerDrafts: [String: String] = [:]
    /// What was pasted into each draft, sent along so memory never takes it as the user's own words.
    @ObservationIgnored var composerPasted: [String: [String]] = [:]
    public var routineDrafts: [String: String] = [:]
    public let fileDownloads: FileDownloads
    public var lastError: String?
    /// The open conversation (navigation path).
    public var selection: String?
    /// The remote screen viewer (iPhone); `donnabot://screen` opens it.
    public var screenRequest: ScreenRequest?

    // Platform hooks (push registration, Live Activities, widgets).
    /// Each time the Hub identifies itself on a new channel (not before).
    public var onConnected: (@MainActor (BotStore) -> Void)?
    public var onBotUpdated: (@MainActor (Bot) -> Void)?
    public var onUsageChanged: (@MainActor (ComputerID, Usage) -> Void)?
    /// A message to the bot from this phone moved along (the Live Activity follows it).
    public var onSent: (@MainActor (Bot, SendProgress) -> Void)?
    /// A bot appeared, changed or went away.
    var onRosterChanged: (@MainActor () -> Void)?
    /// The computer's addresses or keys changed (merged from its `hello`); persist it.
    var onComputerChanged: (@MainActor (Computer) -> Void)?

    public let storage: SharedStore.Context
    /// Opens a channel to the computer as it is now: its route, and the addresses and keys merged since.
    let makeTransport: @MainActor (Computer) async throws -> any HostTransport
    var transport: (any HostTransport)?
    var retired = false
    /// Files of messages not delivered yet (by client nonce), kept for a retry.
    @ObservationIgnored var outgoingFiles: [String: [OutgoingFile]] = [:]

    var rev: Int64 = 0
    var hostId: String?
    var streamTask: Task<Void, Never>?
    var eventsTask: Task<Void, Never>?
    var isActive = true
    struct VoiceCall {
        let botId: String
        let startRev: Int64
        let speak: @MainActor (String) -> Void
        /// A notice from the computer (approval needed), as opposed to the bot's reply.
        let announce: @MainActor (String) -> Void
        let end: @MainActor () -> Void
    }
    @ObservationIgnored var voiceCalls: [UUID: VoiceCall] = [:]
    var saveTask: Task<Void, Never>?
    var rewound = false
    /// Events stayed undecodable after the rewind.
    var unreadable = false
    /// This app's marketing version, compared with the host's `minApp`.
    private let appVersion: String
    /// While one side needs an update there's nothing to sync; `hello` is asked again this often.
    static let mismatchRecheck: Duration = .seconds(30)

    /// Permission cards whose answer is on its way, with the chosen option: the card shows
    /// a spinner on it and takes no second answer.
    public internal(set) var answering: [String: String] = [:]

    /// The chats on screen, by the view showing each.
    @ObservationIgnored var readingViews: [UUID: String] = [:]

    var cacheStamp: String?

    /// Loads this context's `DeviceIdentity` itself.
    public convenience init(computer: Computer, storage: SharedStore.Context) {
        self.init(computer: computer, storage: storage) { current in
            try await HostConnector.connect(current, identity: try DeviceIdentity.load(context: storage))
        }
        conectarRecemVinculado = { current in
            try await HostConnector.connect(current, identity: try DeviceIdentity.load(context: storage), recemVinculado: true)
        }
    }

    init(computer: Computer, storage: SharedStore.Context,
         appVersion: String = AppVersion.current,
         agora: @escaping @MainActor () -> Date = { Date() },
         monotonico: @escaping @MainActor () -> ContinuousClock.Instant = { ContinuousClock.now },
         esperar: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) },
         transport: @escaping @MainActor (Computer) async throws -> any HostTransport) {
        self.appVersion = appVersion
        self.computer = computer
        self.storage = storage
        fileDownloads = FileDownloads(scopeID: storage.id)
        makeTransport = transport
        self.agora = agora
        self.monotonico = monotonico
        self.esperar = esperar
        inicioDaQueda = monotonico()
        // Nothing is identified yet: no action goes out until the first `hello` (the strip waits its grace).
        estadoConexao = .reconectando(desde: agora())
        donoDoEspelho = DonoDoEspelho(conta: storage.mirrorOwnerID, computador: computer.id, chave: computer.signKey)
        usage = storage.usage[computer.id] ?? Usage()
        composerDrafts = Self.chatDrafts(storage.composerDrafts[computer.id] ?? [:])
        loadCache()
    }

    // MARK: derived

    /// Offline in any way: the computer is off, unreachable, or refuses this device.
    public var isOffline: Bool {
        switch connection {
        case .computerOffline, .offline, .unauthorized: true
        default: false
        }
    }

    /// Offline, but sends can wait in the relay mailbox until the computer is back.
    public var canQueue: Bool {
        // Only the warning is delayed; relay delivery can queue during that grace.
        if case .computerOffline = heldDrop ?? connection { computer.boxKey != nil && transport is any RemoteTransport } else { false }
    }

    /// What headers show: a short drop past its grace reads "Connecting…", not offline.
    public var shownConnection: Connection {
        estadoConexao.estaReconectando && connection != .online && connection != .unpaired ? .connecting : connection
    }

    public var hubUnavailable: Bool {
        if estadoConexao.estaReconectando && !faixaDeQueda { return false }
        switch heldDrop ?? connection {
        case .computerOffline, .offline: return true
        default: return false
        }
    }

    /// Sending, approving and every other call to the Hub are allowed: the link is up and the Hub
    /// identified itself on it. False all through a drop, its grace included.
    public var temAutoridade: Bool { estadoConexao == .conectado && clienteAutorizado != nil && live != nil }

    /// Lists and menus keep this computer as if connected: online, or in a short drop.
    public var mantemNaTela: Bool { connection == .online || estadoConexao.estaReconectando }

    /// "Try now" can bring the reconnect forward (the app is in the foreground).
    public var podeTentarAgora: Bool { isActive && !retired }

    public var hostName: String { hello?.name ?? computer.name }

    /// Which side must update before this app and the host can work together
    /// (docs/reference/compatibility.md). `nil` until the host has answered, and whenever
    /// a version can't be read: an unknown is never a reason to lock someone out.
    public var mismatch: VersionMismatch? {
        guard let hostVersion else { return nil }
        if let found = VersionMismatch.check(app: appVersion, hostVersion: hostVersion.version, minApp: hostVersion.minApp) {
            return found
        }
        // The host sends data this app can't read and is newer: this app is behind,
        // whatever the host's `minApp` says.
        if unreadable, AppVersion.isBelow(appVersion, hostVersion.version) {
            return .updateApp(minimum: hostVersion.version)
        }
        return nil
    }

    /// Roster order: pinned first, then where the user dragged them, then most recent activity.
    public var roster: [Bot] {
        bots.values.filter { !$0.hidden }.sorted {
            if $0.pinned != $1.pinned { return $0.pinned }
            if $0.position != $1.position { return $0.position < $1.position }
            return $0.lastAt > $1.lastAt
        }
    }

    /// Display name for a harness id, as the host reports it (falls back to the built-in list).
    public func backendName(_ id: String) -> String {
        hello?.backends.first { $0.id == id }?.name ?? BackendInfo.name(id)
    }

    public var hiddenBots: [Bot] { bots.values.filter(\.hidden).sorted { $0.name < $1.name } }

    /// Everything in a bot's or group's chat this device has, oldest first (trace included).
    public func chat(_ botId: String, sessionId: String? = nil) -> [Entry] {
        let selected = sessionId ?? botId
        return (entries[botId] ?? []).filter { ($0.sessionId ?? botId) == selected }
    }

    public func canLoadOlder(_ botId: String) -> Bool {
        !historyComplete.contains(botId) && chat(botId).contains { $0.seq > 0 && $0.seq != Int64.max }
    }

    /// A group's bots that still exist.
    public func members(of group: Bot) -> [Bot] { group.members.compactMap { bots[$0] } }

    /// Display name of an entry's author in a group chat.
    public func authorName(_ id: String?) -> String { id.flatMap { bots[$0]?.name } ?? String(localized: "Um bot excluído", bundle: .module) }

    func updateComputer(_ change: (inout Computer) -> Void) {
        var c = computer
        change(&c)
        guard c != computer else { return }
        computer = c
        onComputerChanged?(c)
    }
}

/// A file picked in the composer, sent with the next message.
public struct OutgoingFile: Identifiable, Sendable {
    public let id = UUID()
    public let name: String
    public let data: Data

    public init(name: String, data: Data) {
        self.name = name
        self.data = data
    }

    /// Anything larger is refused by the computer.
    public static let maxSize = 100 * 1024 * 1024
}
