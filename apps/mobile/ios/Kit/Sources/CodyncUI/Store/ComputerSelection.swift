// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import Foundation

/// An empty exclusion list means all computers, including newly paired computers.
public struct ComputerSelection: Equatable, Sendable {
    public let all: [ComputerID]
    private let hidden: Set<ComputerID>

    public init(all: [ComputerID], hidden: String) {
        self.all = all
        self.hidden = Set(hidden.split(separator: ",").map(String.init))
    }

    public var shown: [ComputerID] {
        let visible = all.filter { !hidden.contains($0) }
        return visible.isEmpty ? all : visible
    }

    public func toggling(_ id: ComputerID) -> String {
        guard all.contains(id) else { return encoded(Set(all).subtracting(shown)) }
        var visible = Set(shown)
        if visible.contains(id) {
            if visible.count > 1 { visible.remove(id) }
        } else {
            visible.insert(id)
        }
        return encoded(Set(all).subtracting(visible))
    }

    public func only(_ id: ComputerID) -> String {
        guard all.contains(id) else { return "" }
        return encoded(Set(all).subtracting([id]))
    }

    private func encoded(_ ids: Set<ComputerID>) -> String { ids.sorted().joined(separator: ",") }
}

struct ConnectionSummary {
    let connections: [BotStore.Connection]

    var text: String { text(bundle: .module) }

    func text(bundle: Bundle) -> String {
        func localized(_ key: String) -> String {
            NSLocalizedString(key, bundle: bundle, comment: "Resumo das conexões")
        }
        guard !connections.isEmpty else { return localized("Computadores") }
        let online = connections.filter { $0 == .online }.count
        if online == connections.count {
            let key = "%lld conectado"
            return String.localizedStringWithFormat(localized(key), online)
        }
        if online > 0 {
            return String.localizedStringWithFormat(localized("%lld/%lld conectados"), online, connections.count)
        }
        if connections.contains(.connecting) { return localized("Conectando…") }
        if connections.allSatisfy({ if case .unauthorized = $0 { true } else { false } }) { return localized("Sem acesso") }
        if connections.allSatisfy({ $0 == .unpaired }) { return localized("Não pareado") }
        let key = "%lld desconectado"
        return String.localizedStringWithFormat(localized(key), connections.count)
    }

    static var resourceBundle: Bundle { .module }
}
