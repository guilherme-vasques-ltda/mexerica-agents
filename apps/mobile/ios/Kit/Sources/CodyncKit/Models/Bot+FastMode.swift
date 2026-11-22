// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

/// Fast mode in the chat (the composer's `/fast`) and bot settings; the host reports what the agent offers
/// (`fastModeSupport`, `fastModeIssue`): docs/features/marketplace.md, "Fast mode".
public extension Bot {
    enum FastModeState: Sendable { case on, off, refused }

    /// An agent bot unless its agent said it has no Fast mode (an unknown one may: its first session
    /// settles it), or one that has it on (so it can be turned off).
    var fastModeShown: Bool {
        !isGroup && (fastMode || fastModeSupport != "unsupported")
    }

    var fastModeState: FastModeState {
        guard fastMode else { return .off }
        return fastModeSupport == "refused" ? .refused : .on
    }

    /// Why it isn't in effect, when the agent turned it off itself.
    var fastModeRefusal: String? {
        guard fastModeState == .refused else { return nil }
        return String(localized: "O motor desativou: \(fastModeIssue ?? String(localized: "indisponível", bundle: .module)). Desative e ative novamente para tentar outra vez.", bundle: .module)
    }

    /// What picking `/fast` in the composer's command list does, or why it isn't in effect.
    var fastModeDetail: String {
        switch fastModeState {
        case .off: String(localized: "Ative para respostas mais rápidas e maior consumo", bundle: .module)
        case .on: String(localized: "Desative para voltar à velocidade padrão", bundle: .module)
        case .refused: fastModeRefusal ?? ""
        }
    }
}

/// A group's `/fast`: the members that show Fast mode, switched together. Each member's own
/// setting, so it holds in their own chats too.
public struct GroupFastMode: Sendable {
    public let members: [Bot]

    public init(_ members: [Bot]) { self.members = members.filter(\.fastModeShown) }

    /// On once every member that shows it has it on; refused when one of their agents turned it off.
    public var state: Bot.FastModeState {
        guard !members.isEmpty, members.allSatisfy(\.fastMode) else { return .off }
        return members.contains { $0.fastModeState == .refused } ? .refused : .on
    }

    public var detail: String {
        switch state {
        case .off: String(localized: "Ative para todos os bots aqui: respostas mais rápidas e maior consumo", bundle: .module)
        case .on: String(localized: "Desative para todos os bots aqui e volte à velocidade padrão", bundle: .module)
        case .refused: members.first { $0.fastModeState == .refused }.map { "\($0.name): \($0.fastModeRefusal ?? "")" } ?? ""
        }
    }
}
