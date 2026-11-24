// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

/// How the encrypted channel reaches a host: direct on the LAN or through the cloud relay.
public enum HostRoute: Sendable, Equatable {
    case direct, relay
}

public enum HostStreamRequest: Sendable {
    case events(since: Int64, client: String)
    case term(String)
}

public enum LinkState: Sendable, Equatable {
    case connecting
    case ready(HostRoute)
    /// The relay says the computer isn't connected (asleep, off): the mailbox still works.
    case hostOffline(lastSeen: Date?)
    /// Revoked, or the computer's identity changed: retrying won't help.
    case unauthorized(String)
    /// Can't reach the computer at all (neither directly nor through the relay), or it can't confirm
    /// this device's access for now.
    case failed(String)
}

public protocol HostTransport: Sendable {
    func call(_ method: String, body: Data, timeout: TimeInterval) async throws -> Data
    /// Each element is one event's JSON (what SSE puts after `data:`).
    func stream(_ request: HostStreamRequest) -> AsyncThrowingStream<Data, Error>
    /// A new stream per call: the current state first, then every change.
    func states() -> AsyncStream<LinkState>
    /// The channel a call would go out on right now (`nil`: none is ready). Each reconnect is a new one.
    func currentLink() async -> Int?
    /// A call bound to channel `link`: if that channel is gone (dropped, or replaced by a reconnect),
    /// it fails at once instead of waiting for, or going out over, a newer one.
    func call(_ method: String, body: Data, timeout: TimeInterval, onLink link: Int) async throws -> Data
    /// The screen needs TURN: the channel goes through the relay (wrappers ask the transport they wrap).
    func screenRelayRequired() async throws -> Bool
}

/// Transports without channels of their own (HTTP, test fakes): one link that never changes.
public extension HostTransport {
    func currentLink() async -> Int? { 0 }
    func call(_ method: String, body: Data, timeout: TimeInterval, onLink link: Int) async throws -> Data {
        try await call(method, body: body, timeout: timeout)
    }
    func screenRelayRequired() async throws -> Bool { false }
}

/// A transport to a remote computer: learns its addresses and keys, and holds messages
/// in the relay mailbox while it is offline. `ChannelTransport` is the real one.
public protocol RemoteTransport: HostTransport {
    func computerUpdates() -> AsyncStream<Computer>
    func enqueue(botId: String, text: String, clientNonce: String, replyTo: String?, sessionId: String?) async throws
    func cancelQueued(clientNonce: String) async -> MailboxCancel
    func listQueued() async throws -> [QueuedItem]
    func mailboxEvents() -> AsyncStream<MailboxEvent>
    /// Stops for good: closes sockets and ends every stream.
    func shutdown() async
}

public enum HostError: LocalizedError, Sendable, Equatable {
    case http(Int, String)
    /// A 400 whose Hub text is held for recognition only (`RoutineRefusal`); it shows the fixed generic phrase.
    case refused(HubRefusal)
    case unreachable
    case computerOffline(lastSeen: Date?)
    case unauthorized(String)
    case upgradeRequired

    public var errorDescription: String? {
        switch self {
        case .http(401, _): "Este iPhone não tem mais acesso a este computador."
        case let .http(_, message): message
        case .refused: "Não foi possível concluir. Tente de novo."
        case .unreachable: String(localized: "Não foi possível alcançar seu computador. Ele está ligado e conectado à internet?", bundle: .module)
        case .computerOffline: String(localized: "Seu computador está sem conexão.", bundle: .module)
        case let .unauthorized(message): message
        case .upgradeRequired: String(localized: "Atualize a Mexerica para conectar a este computador.", bundle: .module)
        }
    }

    /// The link failed, not the request: the same call can work once the computer is reachable again.
    public var isTransient: Bool {
        switch self {
        case .unreachable, .computerOffline: true
        default: false
        }
    }
}
