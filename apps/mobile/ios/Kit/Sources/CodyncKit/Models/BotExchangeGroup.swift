// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

/// A run of consecutive bot exchanges with the same peer in a chat's main transcript. Shown as
/// one row: "Messaged Dex" for one exchange, "3 messages with Dex" for more.
public struct BotExchangeGroup: Identifiable, Equatable, Sendable {
    /// The exchanges, oldest first; never empty.
    public let exchanges: [BotExchange]

    /// The first exchange's entry id.
    public var id: String { exchanges[0].id }
    public var first: BotExchange { exchanges[0] }
    public var peerId: String { first.peerId }
    public var count: Int { exchanges.count }
    public var date: Date { first.date }
    /// Any exchange failed.
    public var failed: Bool { exchanges.contains { $0.outcome.isFailed } }

    /// What the row says before the peer's name.
    public var title: String { count == 1 ? first.verb : String(localized: "\(count) mensagens com", bundle: .module) }

    public func accessibilityLabel(peerName: String) -> String {
        if count == 1 {
            if first.isOutgoing {
                return failed ? String(localized: "Enviou mensagem para \(peerName), falhou", bundle: .module) : String(localized: "Enviou mensagem para \(peerName)", bundle: .module)
            }
            return failed ? String(localized: "Mensagem de \(peerName), falhou", bundle: .module) : String(localized: "Mensagem de \(peerName)", bundle: .module)
        }
        return failed ? String(localized: "\(count) mensagens com \(peerName), falhou", bundle: .module) : String(localized: "\(count) mensagens com \(peerName)", bundle: .module)
    }

    /// A chat-visible entry of the main transcript, or a group of exchanges.
    public enum Item: Identifiable, Equatable, Sendable {
        case entry(Entry)
        case exchanges(BotExchangeGroup)

        public var id: String {
            switch self {
            case .entry(let e): e.id
            case .exchanges(let g): g.id
            }
        }

        public var date: Date {
            switch self {
            case .entry(let e): e.date
            case .exchanges(let g): g.date
            }
        }
    }

    /// The chat-visible entries with consecutive same-peer exchanges merged. Entries that aren't
    /// shown in the chat (trace) don't end a run; anything shown does.
    public static func collapse(_ entries: [Entry]) -> [Item] {
        var out: [Item] = []
        var run: [BotExchange] = []
        func flush() {
            if !run.isEmpty { out.append(.exchanges(BotExchangeGroup(exchanges: run))) }
            run = []
        }
        for entry in entries where entry.isChat {
            guard let x = BotExchange(entry) else {
                flush()
                out.append(.entry(entry))
                continue
            }
            if let last = run.last, last.peerId != x.peerId { flush() }
            run.append(x)
        }
        flush()
        return out
    }
}
