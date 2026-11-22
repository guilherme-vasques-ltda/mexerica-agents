// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation
import Testing
@testable import CodyncUI

@Test func computerFilterSupportsMultipleAndSingleSelections() {
    let all = ["a", "b", "c"]
    let initial = ComputerSelection(all: all, hidden: "")
    #expect(initial.shown == all)
    let withoutB = ComputerSelection(all: all, hidden: initial.toggling("b"))
    #expect(withoutB.shown == ["a", "c"])
    let onlyC = ComputerSelection(all: all, hidden: withoutB.only("c"))
    #expect(onlyC.shown == ["c"])
    #expect(ComputerSelection(all: all, hidden: onlyC.toggling("a")).shown == ["a", "c"])
}

@Test func computerFilterKeepsOneComputerAndRecoversFromRemovedComputers() {
    let single = ComputerSelection(all: ["a", "b"], hidden: "b")
    #expect(ComputerSelection(all: single.all, hidden: single.toggling("a")).shown == ["a"])
    let removed = ComputerSelection(all: ["b", "c"], hidden: "b,c")
    #expect(removed.shown == ["b", "c"])
    #expect(ComputerSelection(all: removed.all, hidden: removed.toggling("b")).shown == ["c"])
    #expect(ComputerSelection(all: ["a", "b", "new"], hidden: "b").shown == ["a", "new"])
    #expect(ComputerSelection(all: [], hidden: "a").shown.isEmpty)
}

@Test func headerSummarizesOnlyTheSelectedConnectionsInBothLanguages() throws {
    let ptPath = try #require(ConnectionSummary.resourceBundle.path(forResource: "pt-BR", ofType: "lproj"))
    let enPath = try #require(ConnectionSummary.resourceBundle.path(forResource: "en", ofType: "lproj"))
    let pt = try #require(Bundle(path: ptPath))
    let en = try #require(Bundle(path: enPath))
    let cases: [([BotStore.Connection], String, String)] = [
        ([], "Computadores", "Computers"),
        ([.online], "1 conectado", "1 connected"),
        ([.online, .online], "2 conectados", "2 connected"),
        ([.online, .computerOffline(lastSeen: nil)], "1/2 conectados", "1/2 connected"),
        ([.offline("timeout")], "1 desconectado", "1 offline"),
        ([.offline("timeout"), .computerOffline(lastSeen: nil)], "2 desconectados", "2 offline"),
        ([.offline("timeout"), .connecting], "Conectando…", "Connecting…"),
        ([.unauthorized("revoked")], "Sem acesso", "No access"),
        ([.unpaired], "Não pareado", "Not paired"),
    ]
    for (connections, portuguese, english) in cases {
        let summary = ConnectionSummary(connections: connections)
        #expect(summary.text(bundle: pt) == portuguese)
        #expect(summary.text(bundle: en) == english)
    }
}

@Test func avatarHintAndBotCountsHaveRealTranslations() throws {
    for (language, hint) in [("pt-BR", "Edite o avatar no app do Mac"), ("en", "Edit the avatar in the Mac app")] {
        let path = try #require(ConnectionSummary.resourceBundle.path(forResource: language, ofType: "lproj"))
        let bundle = try #require(Bundle(path: path))
        #expect(NSLocalizedString("Edite o avatar no app do Mac", bundle: bundle, comment: "") == hint)
        #expect(String(localized: "\(1) bot", bundle: bundle) == "1 bot")
        #expect(String(localized: "\(2) bot", bundle: bundle) == "2 bots")
        let expected = language == "pt-BR" ? "2 tokens, acima do limite de 1 · hoje" : "2 tokens, over the 1 cap · hoje"
        #expect(String(localized: "\(2) token, acima do limite de \(1) · \("hoje")", bundle: bundle) == expected)
    }
}
