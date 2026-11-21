import Foundation
import SwiftUI
import Testing
import UIKit
@testable import CodyncKit

// O avatar do bot no iPhone tem de ser o do desktop. Os casos seguem `fork-mexeriquinha-config.ts`
// (avatarDoBot / normalizarAvatarDoBot) do desktop. O arquivo Fixtures/avatar-wire-fixture.json
// é cópia de `apps/desktop/src/shared/fork-mexeriquinha-wire-fixture.json` desse repositório: os bots como o Hub os manda.

private let azul = "#3B82F6"
private let roxoHex = "#8d56d8"
private let corCurta = "#12345"
private let berga = MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "blazer", acessorio: "cartola", objeto: "chave")

private func legacy(shape: String = "oval", color: String = "#f28a1e", eye: String = "auto", head: String = "none",
                    face: String = "none", neck: String = "none", badge: String = "none") -> String {
    #"{"shape":"\#(shape)","color":"\#(color)","eye":"\#(eye)","head":"\#(head)","face":"\#(face)","ears":"none","chin":"none","neck":"\#(neck)","cheeks":"none","badge":"\#(badge)","badgeStyle":"light"}"#
}

private func bot(_ fields: String = "", status: String = "idle") throws -> Bot {
    let json = #"{"id":"b1","name":"Berga","avatarColor":"orange","avatarShape":"blob","status":"\#(status)"\#(fields.isEmpty ? "" : "," + fields)}"#
    return try JSONDecoder().decode(Bot.self, from: Data(json.utf8))
}

@Test func wireFixtureResolvesLikeTheDesktop() throws {
    let url = try #require(Bundle.module.url(forResource: "avatar-wire-fixture", withExtension: "json", subdirectory: "Fixtures"))
    let bots = try JSONDecoder().decode([Bot].self, from: Data(contentsOf: url))
    #expect(bots.count == 2)
    // Simples: a `mexeriquinha` v2 cuja projeção é o `avatarConfig` recebido.
    #expect(bots[0].mexeriquinha?.estilo == "simples" && bots[0].avatarConfig?.face == "glasses")
    #expect(bots[0].avatar == .simples(MexeriquinhaAparencia(cor: "laranja", olhos: "oculos-redondos", roupa: "blazer", acessorio: "coroa", objeto: "gravata")))
    // Feltro: o conjunto do catálogo, não a aparência aproximada.
    #expect(bots[1].avatar == .pelucia(conjunto: "developer", assetId: nil))
}

@Test func principalBotIsBergaNotDots() throws {
    let b = try bot(#""avatarConfig":\#(legacy()),"mexeriquinha":{"v":2,"estilo":"simples","modeloId":"berga","aparencia":{"cor":"laranja","olhos":"ponto","roupa":"blazer","acessorio":"cartola","objeto":"chave"}}"#)
    #expect(b.avatar == .simples(berga))
}

@Test func legacyOnlyBotIsApproximatedToTheMexeriquinha() throws {
    // Sem `mexeriquinha`: o desktop aproxima o avatarConfig (cor mais próxima, óculos, boina, gravata e o selo do código).
    let b = try bot(#""avatarConfig":\#(legacy(color: azul, head: "beret", face: "glasses", neck: "tie", badge: "code"))"#)
    #expect(b.avatar == .simples(MexeriquinhaAparencia(cor: "celeste", olhos: "oculos-redondos", roupa: "moletom", acessorio: "boina", objeto: "notebook")))
    #expect(try bot(#""avatarConfig":\#(legacy(head: "cap", neck: "bowtie"))"#).avatar
            == .simples(MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "blazer", acessorio: "bone", objeto: "gravata")))
    #expect(try bot(#""avatarConfig":\#(legacy(eye: "cat", head: "beanie"))"#).avatar
            == .simples(MexeriquinhaAparencia(cor: "laranja", olhos: "piscando", roupa: "nenhuma", acessorio: "capuz", objeto: "nenhum")))
}

@Test func missingOrInvalidAvatarFallsBackToTheMexeriquinhaMascot() throws {
    #expect(try bot().avatar == .padrao)
    #expect(BotAvatar.padrao == .simples(MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "nenhuma", acessorio: "nenhum", objeto: "nenhum")))
    // Um avatar quebrado nunca derruba o bot.
    let broken = try bot(#""avatarConfig":{"shape":"blob"},"mexeriquinha":{"v":9,"estilo":"holograma"}"#)
    #expect(broken.name == "Berga" && broken.avatarConfig == nil && broken.avatar == .padrao)
    let wrongType = try bot(#""avatarConfig":"azul","mexeriquinha":42"#)
    #expect(wrongType.avatar == .padrao)
    // Um valor que não está no schema invalida só aquele campo.
    #expect(try bot(#""avatarConfig":\#(legacy(color: corCurta))"#).avatar == .padrao)
    #expect(try bot(#""avatarConfig":\#(legacy(head: "crown"))"#).avatar == .padrao)
}

@Test func legacyEditWinsOverAStaleMexeriquinha() throws {
    // `avatarConfig` diferente da projeção: foi editado pelo caminho legado, e vale (como em avatarDoBot).
    let b = try bot(#""avatarConfig":\#(legacy(color: roxoHex)),"mexeriquinha":{"v":2,"estilo":"simples","aparencia":{"cor":"laranja","olhos":"ponto","roupa":"nenhuma","acessorio":"nenhum","objeto":"nenhum"}}"#)
    #expect(b.avatar == .simples(MexeriquinhaAparencia(cor: "roxo", olhos: "ponto", roupa: "nenhuma", acessorio: "nenhum", objeto: "nenhum")))
    // Sem `avatarConfig` válido, a `mexeriquinha` vale sozinha.
    let only = try bot(#""mexeriquinha":{"v":2,"estilo":"simples","aparencia":{"cor":"lima","olhos":"sonolentos","roupa":"polo","acessorio":"flor","objeto":"caixa"}}"#)
    #expect(only.avatar == .simples(MexeriquinhaAparencia(cor: "lima", olhos: "sonolentos", roupa: "polo", acessorio: "flor", objeto: "caixa")))
}

@Test func generatedFeltSetShowsTheCatalogFallback() throws {
    let id = "3f2b8c1e-5d4a-4b7e-9c21-7a6d0e8f1b34"
    // berga: laranja, olhos ponto => projeção igual ao avatarConfig padrão.
    let b = try bot(#""avatarConfig":\#(legacy()),"mexeriquinha":{"v":3,"estilo":"3d","assetId":"\#(id)","fallbackConjuntoId":"berga"}"#)
    #expect(b.avatar == .pelucia(conjunto: "berga", assetId: id))
    // assetId fora do formato (maiúsculas, versão errada): o campo é inválido e vale o legado.
    let upper = try bot(#""avatarConfig":\#(legacy()),"mexeriquinha":{"v":3,"estilo":"3d","assetId":"\#(id.uppercased())","fallbackConjuntoId":"berga"}"#)
    #expect(upper.avatar == .simples(MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "nenhuma", acessorio: "nenhum", objeto: "nenhum")))
    let v1 = try bot(#""mexeriquinha":{"v":3,"estilo":"3d","assetId":"3f2b8c1e-5d4a-1b7e-9c21-7a6d0e8f1b34","fallbackConjuntoId":"berga"}"#)
    #expect(v1.avatar == .padrao)
    // Conjunto que não está no catálogo.
    #expect(try bot(#""mexeriquinha":{"v":2,"estilo":"3d","conjuntoId":"unicornio"}"#).avatar == .padrao)
}

@Test func mexeriquinhaSurvivesTheWidgetCache() throws {
    // O Bot preserva o avatar ao atravessar a codificação.
    let b = try bot(#""avatarConfig":\#(legacy()),"mexeriquinha":{"v":2,"estilo":"simples","modeloId":"berga","aparencia":{"cor":"laranja","olhos":"ponto","roupa":"blazer","acessorio":"cartola","objeto":"chave"}}"#)
    let copy = try JSONDecoder().decode(Bot.self, from: JSONEncoder().encode(b))
    #expect(copy.avatar == .simples(berga) && copy.mexeriquinha == b.mexeriquinha && copy.avatarConfig == b.avatarConfig)
}

@Test func nearestPaletteColor() {
    #expect(BotAvatar.corProxima("#f28a1e") == "laranja")
    #expect(BotAvatar.corProxima(azul) == "celeste")
    #expect(BotAvatar.corProxima("#000000") == "lima")
    #expect(BotAvatar.corProxima("xyz") == "laranja")  // sem cor legível, a padrão
}

@Test func avatarStateFollowsTheDesktopPrecedence() {
    let now = Date(timeIntervalSince1970: 1_000_000)
    let ms = Int64(now.timeIntervalSince1970 * 1000)
    let hour: Int64 = 3_600_000
    func state(_ status: String, lastAt: Int64, startedAt: Int64? = nil) -> MexeriquinhaState {
        MexeriquinhaState.of(status: status, lastAt: lastAt, createdAt: 0, startedAt: startedAt, now: now)
    }
    #expect(state("error", lastAt: 0) == .erro)
    #expect(state("needsInput", lastAt: ms) == .precisaResposta)
    #expect(state("working", lastAt: ms - 20 * hour) == .trabalhando)
    #expect(state("idle", lastAt: ms - 8 * hour) == .livre)  // exatamente 8 h ainda é livre
    #expect(state("idle", lastAt: ms - 8 * hour - 1) == .dormindo)
    #expect(state("idle", lastAt: ms - 20 * hour, startedAt: ms - hour) == .livre)  // a atividade mais recente conta
    #expect(state("idle", lastAt: 0) == .livre)  // sem registro
    #expect(state("idle", lastAt: ms + hour) == .livre)  // horário no futuro
    #expect(state("unavailable", lastAt: ms - 20 * hour) == .livre)
    // Pergunta pendente pesa como `needsInput` (acima de `working`), mas nunca acima de `error` nem de `unavailable`.
    func comPergunta(_ status: String) -> MexeriquinhaState {
        MexeriquinhaState.of(status: status, lastAt: ms, createdAt: 0, startedAt: nil, hasPendingQuestions: true, now: now)
    }
    #expect(comPergunta("working") == .precisaResposta && comPergunta("idle") == .precisaResposta)
    #expect(comPergunta("error") == .erro && comPergunta("unavailable") == .livre)
    #expect(MexeriquinhaState(phase: .needsInput) == .precisaResposta && MexeriquinhaState(phase: .failed) == .erro)
}

@MainActor
@Test func everyFeltSetHasItsFiveFrames() {
    #expect(MexeriquinhaData.conjuntos.count == 22)
    for conjunto in MexeriquinhaData.conjuntos.keys {
        for state in [MexeriquinhaState.livre, .trabalhando, .dormindo, .precisaResposta, .erro] {
            #expect(MexeriquinhaPeluciaView.quadro(conjunto: conjunto, state: state) != nil, "\(conjunto) \(state.quadro)")
        }
    }
}

@Test func svgPathReaderMatchesTheDesktopShapes() {
    let square = SVGPath.path("M10 10h20v20h-20z").boundingRect
    #expect(square == CGRect(x: 10, y: 10, width: 20, height: 20))
    // O círculo de duas meias-voltas (`a`) das lentes: caixa de 38 a 58 por 60 a 80.
    let circle = SVGPath.path("M38 70a10 10 0 1 0 20 0a10 10 0 1 0-20 0Z").boundingRect
    #expect(abs(circle.minX - 38) < 0.01 && abs(circle.maxX - 58) < 0.01 && abs(circle.minY - 60) < 0.01 && abs(circle.maxY - 80) < 0.01)
    // Números colados por sinal e ponto: "M-4-3.5C-4-8.5 4-8.5 4-3.5".
    let hook = SVGPath.path("M-4-3.5C-4-8.5 4-8.5 4-3.5C4-.5 0-.5 0 2.5").boundingRect
    #expect(abs(hook.minX + 4) < 0.01 && abs(hook.maxX - 4) < 0.01 && abs(hook.maxY - 2.5) < 0.01)
    // Relativos com `t` (a boca de erro): termina 16 à direita do começo.
    let wave = SVGPath.path("M56 86q2 -3 4 0t4 0t4 0t4 0").boundingRect
    #expect(abs(wave.maxX - 72) < 0.01 && abs(wave.minX - 56) < 0.01)
}

@MainActor
@Test func everyAppearanceDrawsInEveryState() throws {
    var apareceu = 0
    for cor in ["laranja", "roxo"] {
        for state in [MexeriquinhaState.livre, .trabalhando, .dormindo, .precisaResposta, .erro] {
            for size in [24.0, 96.0] {
                let view = MexeriquinhaView(avatar: .simples(MexeriquinhaAparencia(cor: cor, olhos: "oculos-escuros", roupa: "blazer", acessorio: "coroa", objeto: "gravata")),
                                            state: state, size: size)
                let renderer = ImageRenderer(content: view)
                renderer.scale = 2
                let image = try #require(renderer.uiImage)
                #expect(image.size == CGSize(width: size, height: size))
                apareceu += 1
            }
        }
    }
    #expect(apareceu == 20)
}

@Test func strictValidationMatchesTheDesktopSchema() throws {
    let lima = #"{"cor":"lima","olhos":"ponto","roupa":"blazer","acessorio":"coroa","objeto":"gravata"}"#
    let projecao = legacy(color: "#94c630")
    // modeloId fora do catálogo: a `mexeriquinha` é rejeitada e o legado (aproximado) prevalece.
    let semModelo = try bot(#""avatarConfig":\#(projecao),"mexeriquinha":{"v":2,"estilo":"simples","modeloId":"inexistente","aparencia":\#(lima)}"#)
    #expect(semModelo.avatar == .simples(MexeriquinhaAparencia(cor: "lima", olhos: "ponto", roupa: "nenhuma", acessorio: "nenhum", objeto: "nenhum")))
    let comModelo = try bot(#""avatarConfig":\#(projecao),"mexeriquinha":{"v":2,"estilo":"simples","modeloId":"berga","aparencia":\#(lima)}"#)
    #expect(comModelo.avatar == .simples(MexeriquinhaAparencia(cor: "lima", olhos: "ponto", roupa: "blazer", acessorio: "coroa", objeto: "gravata")))
    // Chave a mais (na raiz ou na aparência): inválida; `null` também. Nunca derruba o bot.
    let extra = try bot(#""mexeriquinha":{"v":2,"estilo":"simples","extra":1,"aparencia":\#(lima)}"#)
    #expect(extra.mexeriquinha == nil && extra.avatar == .padrao)
    let extraAparencia = try bot(#""mexeriquinha":{"v":2,"estilo":"simples","aparencia":{"cor":"lima","olhos":"ponto","roupa":"blazer","acessorio":"coroa","objeto":"gravata","x":1}}"#)
    #expect(extraAparencia.mexeriquinha == nil)
    #expect(try bot(#""mexeriquinha":{"v":2,"estilo":"simples","modeloId":null,"aparencia":\#(lima)}"#).mexeriquinha == nil)
    // Legado com chave a mais é inválido: não prevalece sobre uma Mexeriquinha válida.
    let legadoExtra = String(legacy().dropLast()) + #","extra":"x"}"#
    let b = try bot(#""avatarConfig":\#(legadoExtra),"mexeriquinha":{"v":2,"estilo":"simples","aparencia":\#(lima)}"#)
    let aparenciaLima = try #require(b.mexeriquinha?.aparencia)
    #expect(b.avatarConfig == nil && b.avatar == .simples(aparenciaLima))
    // Cor com sinais dentro não é hexadecimal.
    #expect(LegacyAvatarConfig.hexa("#-10000") == nil && LegacyAvatarConfig.hexa("#+f0000") == nil && LegacyAvatarConfig.hexa("#１２３４５６") == nil)
    #expect(LegacyAvatarConfig.hexa("#0aFf10") == [10, 255, 16])
    // Inválido não fica no Bot: vira nil (e o avatar cai no mascote padrão).
    #expect(try bot(#""avatarConfig":\#(legacy(color: "#-10000"))"#).avatarConfig == nil)
}

@Test func pendingQuestionsDecodeLeniently() throws {
    let q = #"{"entryId":"e1","chatId":"b1","text":"Posso?","askedAt":1700000000000}"#
    #expect(try bot(#""pendingQuestions":[\#(q)]"#).hasPendingQuestions)
    // Ausente, tipo errado ou elementos fora do contrato: falso (ou só os válidos contam), e o bot continua.
    #expect(try bot().hasPendingQuestions == false)
    #expect(try bot(#""pendingQuestions":"muitas""#).hasPendingQuestions == false)
    #expect(try bot(#""pendingQuestions":null"#).hasPendingQuestions == false)
    #expect(try bot(#""pendingQuestions":[]"#).hasPendingQuestions == false)
    let mixed = try bot(#""pendingQuestions":[42,{"entryId":"","chatId":"b1","text":"x","askedAt":1},\#(q)]"#)
    #expect(mixed.name == "Berga" && mixed.hasPendingQuestions)
    #expect(try bot(#""pendingQuestions":[42,{"entryId":"e","chatId":"b1","text":"","askedAt":1},{"x":1}]"#).hasPendingQuestions == false)
    // Trabalhando com pergunta pendente: "precisa de você", como no desktop.
    #expect(try bot(#""pendingQuestions":[\#(q)]"#, status: "working").avatarState() == .precisaResposta)
    #expect(try bot(#""pendingQuestions":[\#(q)]"#, status: "error").avatarState() == .erro)
}

@Test func pendingQuestionsAreBoundedAtTheInput() throws {
    let q = #"{"entryId":"e1","chatId":"b1","text":"Posso?","askedAt":1700000000000}"#
    let enorme = #"{"entryId":"e1","chatId":"b1","text":"\#(String(repeating: "a", count: 1 << 20))","askedAt":1}"#
    // 10.000 perguntas, as três primeiras com texto de 1 MB: decodifica rápido e não guarda texto nenhum.
    let itens = ([enorme, enorme, enorme] + Array(repeating: q, count: 10_000)).joined(separator: ",")
    let b = try bot(#""pendingQuestions":[\#(itens)]"#)
    #expect(b.hasPendingQuestions && b.name == "Berga")
    // O que vai ao snapshot dos widgets é pequeno e não carrega nenhum texto da pergunta.
    let json = try JSONEncoder().encode(b)
    #expect(json.count < 2048)
    #expect(!String(decoding: json, as: UTF8.self).contains("aaaa"))
    #expect(try JSONDecoder().decode(Bot.self, from: json).hasPendingQuestions)
    // Só inválidos, mesmo 10.000: falso, sem varrer a lista toda (`PendingQuestion.maxExaminadas`).
    #expect(try bot(#""pendingQuestions":[\#(Array(repeating: "42", count: 10_000).joined(separator: ","))]"#).hasPendingQuestions == false)
    let muitosInvalidos = Array(repeating: "42", count: PendingQuestion.maxExaminadas) + [q]
    #expect(try bot(#""pendingQuestions":[\#(muitosInvalidos.joined(separator: ","))]"#).hasPendingQuestions == false)
    let poucosInvalidos = Array(repeating: "42", count: PendingQuestion.maxExaminadas - 1) + [q]
    #expect(try bot(#""pendingQuestions":[\#(poucosInvalidos.joined(separator: ","))]"#).hasPendingQuestions)
}

/// Lê só `pendingQuestions` e guarda o que `PendingQuestion.examinar` conta, para provar o limite sem medir tempo.
private struct SondaPerguntas: Decodable {
    let existe: Bool
    let examinadas: Int
    init(from decoder: Decoder) throws {
        let r = PendingQuestion.examinar(em: decoder)
        existe = r.existe
        examinadas = r.examinadas
    }
}

private func sonda(_ itens: [String]) throws -> SondaPerguntas {
    try JSONDecoder().decode(SondaPerguntas.self, from: Data(#"{"pendingQuestions":[\#(itens.joined(separator: ","))]}"#.utf8))
}

@Test func pendingQuestionScanExaminesAtMostTheLimit() throws {
    let q = #"{"entryId":"e1","chatId":"b1","text":"Posso?","askedAt":1}"#
    let limite = PendingQuestion.maxExaminadas
    #expect(limite == 20)
    // 10.000 inválidos: examina exatamente `limite` elementos e para.
    let todos = try sonda(Array(repeating: "42", count: 10_000))
    #expect(!todos.existe && todos.examinadas == limite)
    // A única pergunta válida na posição 21 (índice 20): fora do limite, então falso, e ainda só `limite` examinados.
    let fora = try sonda(Array(repeating: "42", count: limite) + [q] + Array(repeating: "42", count: 100))
    #expect(!fora.existe && fora.examinadas == limite)
    // Na posição 20 (a última examinada): verdadeiro.
    let ultima = try sonda(Array(repeating: "42", count: limite - 1) + [q])
    #expect(ultima.existe && ultima.examinadas == limite)
    // Na posição 1: verdadeiro e para na primeira válida, sem examinar o resto.
    let primeira = try sonda([q] + Array(repeating: "42", count: 10_000))
    #expect(primeira.existe && primeira.examinadas == 1)
    // Ausente: nada examinado.
    let ausente = try JSONDecoder().decode(SondaPerguntas.self, from: Data("{}".utf8))
    #expect(!ausente.existe && ausente.examinadas == 0)
}

@Test func invalidOrOversizedAvatarIsDroppedAtDecodeAndAbsentFromTheSnapshot() throws {
    let grande = String(repeating: "x", count: 1 << 20)
    let lima = #"{"cor":"lima","olhos":"ponto","roupa":"blazer","acessorio":"coroa","objeto":"gravata"}"#
    let invalidos: [String] = [
        // Campo de 1 MB em cada lugar onde o Hub manda texto livre, e uma chave a mais com 1 MB.
        #""avatarConfig":\#(legacy(color: grande)),"mexeriquinha":{"v":2,"estilo":"simples","modeloId":"\#(grande)","aparencia":\#(lima)}"#,
        #""avatarConfig":\#(legacy(head: grande)),"mexeriquinha":{"v":2,"estilo":"simples","aparencia":\#(lima),"extra":"\#(grande)"}"#,
        #""avatarConfig":\#(legacy(color: "#12345")),"mexeriquinha":{"v":3,"estilo":"3d","assetId":"\#(grande)","fallbackConjuntoId":"berga"}"#,
        #""avatarConfig":{"shape":"\#(grande)"},"mexeriquinha":{"v":2,"estilo":"simples","aparencia":{"cor":"\#(grande)","olhos":"ponto","roupa":"blazer","acessorio":"coroa","objeto":"gravata"}}"#,
        #""avatarConfig":"\#(grande)","mexeriquinha":["\#(grande)","\#(grande)"]"#,
        // Bem formado, mas fora do catálogo ou com versão desconhecida.
        #""mexeriquinha":{"v":9,"estilo":"holograma"}"#,
    ]
    for campos in invalidos {
        let b = try bot(campos)
        #expect(b.name == "Berga" && b.avatarConfig == nil && b.mexeriquinha == nil && b.avatar == .padrao)
        // Snapshot que widgets e NotificationService decodificam inteiro: sem as chaves e sem o texto.
        let json = try JSONEncoder().encode(BotSnapshot(computerId: "c1", bot: b))
        #expect(json.count < 2048)
        #expect(!String(decoding: json, as: UTF8.self).contains("xxxx"))
        let objeto = try #require(JSONSerialization.jsonObject(with: json) as? [String: Any])
        let dentro = try #require(objeto["bot"] as? [String: Any])
        #expect(dentro["avatarConfig"] == nil && dentro["mexeriquinha"] == nil)
        #expect(try JSONDecoder().decode(BotSnapshot.self, from: json).bot.avatar == .padrao)
    }
}

@Test func validAvatarSurvivesTheSnapshotSmall() throws {
    let b = try bot(#""avatarConfig":\#(legacy()),"mexeriquinha":{"v":2,"estilo":"simples","modeloId":"berga","aparencia":{"cor":"laranja","olhos":"ponto","roupa":"blazer","acessorio":"cartola","objeto":"chave"}}"#)
    #expect(b.avatarConfig != nil && b.mexeriquinha != nil && b.avatar == .simples(berga))
    // O teto teórico das duas configurações válidas (ids mais longos do catálogo) cabe folgado na parte do avatar.
    let json = try JSONEncoder().encode(b.avatarConfig)
    let mx = try JSONEncoder().encode(b.mexeriquinha)
    #expect(json.count <= 260 && mx.count <= MexeriquinhaConfig.maxBytes)
}

@Test func avatarColorAndShapeAreStrictShortIds() throws {
    func decodificar(cor: String? = nil, forma: String? = nil) throws -> Bot {
        func campo(_ nome: String, _ valor: String?) -> String { valor.map { #","\#(nome)":"\#($0)""# } ?? "" }
        return try JSONDecoder().decode(Bot.self, from: Data(#"{"id":"b1"\#(campo("avatarColor", cor))\#(campo("avatarShape", forma))}"#.utf8))
    }
    let padraoCor = "blue", padraoForma = "blob"
    // Hostis: 1 caractere com 100.000 marcas combinantes (tamanho em Characters = 1, em bytes = 200.001), não-ASCII,
    // 65 bytes, vazio, espaço e caracteres fora de [A-Za-z0-9#_-].
    let combinantes = "e" + String(repeating: "\u{301}", count: 100_000)
    #expect(combinantes.count == 1)
    let hostis = [combinantes, "é", "laranja🍊", String(repeating: "a", count: 65), "", "or ange", "a/b", "a.b", "a\\n", "ａ"]
    for valor in hostis {
        let b = try decodificar(cor: valor, forma: valor)
        #expect(b.avatarColor == padraoCor && b.avatarShape == padraoForma)
        // O snapshot dos widgets guarda só o padrão: pequeno e sem o texto hostil.
        let json = try JSONEncoder().encode(BotSnapshot(computerId: "c1", bot: b))
        #expect(json.count < 2048)
        #expect(!String(decoding: json, as: UTF8.self).contains("\u{301}"))
    }
    // No limite: 64 bytes ASCII valem; ausente usa o padrão.
    let sessentaEQuatro = String(repeating: "a", count: Bot.maxAvatarId)
    let noLimite = try decodificar(cor: sessentaEQuatro, forma: sessentaEQuatro)
    #expect(noLimite.avatarColor == sessentaEQuatro && noLimite.avatarShape == sessentaEQuatro)
    let ausente = try decodificar()
    #expect(ausente.avatarColor == padraoCor && ausente.avatarShape == padraoForma)
    // Os ids legítimos (catálogos `AvatarPalette`, fixtures de prévia dos widgets, ids com hífen, sublinhado e `#`) continuam aceitos.
    for cor in AvatarPalette.colors.map(\.id) + ["#3B82F6", "dark-blue", "sea_green", "Orange1"] {
        #expect(try decodificar(cor: cor).avatarColor == cor)
    }
    for forma in AvatarPalette.shapes + ["squircle", "hex", "gray"] {
        #expect(try decodificar(forma: forma).avatarShape == forma)
    }
    // O bot do helper `bot()` dos outros testes também fica intacto.
    let b = try bot()
    #expect(b.avatarColor == "orange" && b.avatarShape == "blob")
}

@Test func rosterKeepsTheOtherBotsWhenOneHasHostileAvatarAndQuestions() throws {
    let grande = String(repeating: "z", count: 1 << 20)
    let lima = #"{"cor":"lima","olhos":"sonolentos","roupa":"polo","acessorio":"flor","objeto":"caixa"}"#
    let q = #"{"entryId":"e1","chatId":"b2","text":"Posso?","askedAt":1}"#
    let roster = """
    [{"id":"b1","name":"Um","status":"idle","avatarConfig":\(legacy()),"mexeriquinha":{"v":2,"estilo":"simples","aparencia":\(lima)}},
     {"id":"b2","name":"Hostil","status":"idle","avatarConfig":{"shape":"\(grande)"},"mexeriquinha":{"v":2,"estilo":"simples","modeloId":"\(grande)","aparencia":\(lima)},"pendingQuestions":[42,\(q),"\(grande)"]},
     {"id":"b3","name":"Tres","status":"working","pendingQuestions":"\(grande)"}]
    """
    let bots = try JSONDecoder().decode([Bot].self, from: Data(roster.utf8))
    #expect(bots.map(\.name) == ["Um", "Hostil", "Tres"])
    #expect(bots[0].avatarConfig != nil && bots[0].mexeriquinha != nil)
    #expect(bots[1].avatarConfig == nil && bots[1].mexeriquinha == nil && bots[1].hasPendingQuestions && bots[1].avatar == .padrao)
    #expect(bots[2].hasPendingQuestions == false && bots[2].avatarState() == .trabalhando)
    // O snapshot do roster inteiro continua pequeno e o flag de pergunta atravessa o cache dos widgets.
    let json = try JSONEncoder().encode(bots.map { BotSnapshot(computerId: "c1", bot: $0) })
    #expect(json.count < 4096)
    let volta = try JSONDecoder().decode([BotSnapshot].self, from: json).map(\.bot)
    #expect(volta[1].hasPendingQuestions && volta[1].avatarState() == .precisaResposta && volta[0].avatar == bots[0].avatar)
}
