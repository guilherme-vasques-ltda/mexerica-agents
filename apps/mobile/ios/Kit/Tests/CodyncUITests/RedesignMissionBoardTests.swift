#if DEBUG
import Foundation
import Observation
import SwiftUI
import Testing
import UIKit
@testable import CodyncKit
@testable import CodyncUI

/// Mission Board v1 (PLANO §4.2, só Debug): regras puras, o quadro derivado do `BotStore` de verdade (cobertura parcial,
/// vazio com ressalva, desconectado, autoridade negada), a invalidação por TODOS os eventos do ciclo, o "Abrir", a barra
/// e a tela em vários temas e tamanhos de texto.

// MARK: - Apoio

private func bot(_ id: String, _ nome: String, status: String = "idle", extra: String = "") throws -> Bot {
    let json = #"{"id":"\#(id)","name":"\#(nome)","status":"\#(status)","rev":1\#(extra)}"#
    return try JSONDecoder().decode(Bot.self, from: Data(json.utf8))
}

private func pedido(_ id: String, em chat: String, rev: Int64 = 2, status: String = "pending", sessao: String? = nil,
                    criado: Int64 = 1_000) -> Entry {
    Entry(id: id, seq: rev, botId: chat, rev: rev, kind: "permission", turn: 1, data: EntryData(status: status),
          createdAt: criado, updatedAt: criado, sessionId: sessao)
}

private let comPergunta = #","pendingQuestions":[{"entryId":"q1","chatId":"x","text":"Qual?","askedAt":1}]"#

@MainActor
private func lojaDeTeste(bots: [Bot] = [], transporte: FakeRemote = FakeRemote(.connecting)) -> BotStore {
    let suite = "RedesignMissionBoard.\(UUID().uuidString)"
    let storage = SharedStore.Context(accountID: UUID().uuidString, suite: suite)
    let chave = Data(repeating: 9, count: 32)
    let codificada = chave.base64EncodedString().replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    let computador = Computer(id: RelayCrypto.computerId(signKey: chave), name: "MacBook de exemplo", signKey: codificada)
    let store = BotStore(computer: computador, storage: storage) { _ in transporte }
    for b in bots { store.bots[b.id] = b }
    return store
}

@MainActor
private func fonteDeTeste(bots: [Bot], estado: MXEstadoConexao = .conectado) -> (MXFonteReal, BotStore) {
    let store = lojaDeTeste(bots: bots)
    let fonte = MXFonteReal(store: store)
    fonte.conexaoForcada = estado
    return (fonte, store)
}

@MainActor
private func ate(_ condicao: @MainActor () async -> Bool) async -> Bool {
    for _ in 0..<200 {
        if await condicao() { return true }
        try? await Task.sleep(for: .milliseconds(10))
    }
    return false
}

private final class Caixa: @unchecked Sendable { var valor = false }

private let agoraFixo = Date(timeIntervalSince1970: 1_800_000_000)

// MARK: - Regras puras

struct RedesignMissionBoardRegrasTests {
    private func montar(_ bots: [Bot], pedidos: [String: Int64] = [:], conexao: MXEstadoConexao = .conectado,
                        motivo: MotivoDesconexao? = nil, carregou: Bool = true, ilegivel: Bool = false) -> MXQuadroDados {
        MXQuadroRegras.montar(bots: bots, pedidos: pedidos, conexao: conexao, motivo: motivo, computador: "Mac", carregou: carregou,
                              ilegivel: ilegivel, agora: agoraFixo)
    }

    @Test func cadaBotCaiNumaClasseSoComAPrecedenciaDoCartao() throws {
        let bots = [
            try bot("a", "Ana", status: "working"),                  // permissão pendente na Principal, bot "working" (screen_access)
            try bot("b", "Beto", status: "idle", extra: comPergunta), // pergunta do roster
            try bot("c", "Caio", status: "needsInput"),              // precisa de você, sem cartão nem pergunta à vista
            try bot("d", "Duda", status: "error"),
            try bot("e", "Edu", status: "working"),
            try bot("f", "Fabi", status: "idle"),
        ]
        let q = montar(bots, pedidos: ["a": 5_000])
        #expect(q.voce.map(\.id) == ["a", "b", "c"])
        #expect(q.voce.map(\.motivo) == [.permissao, .pergunta, .espera])
        #expect(q.trabalhando.map(\.id) == ["e"] && q.problema.map(\.id) == ["d"] && q.livres.map(\.id) == ["f"])
        #expect(q.voce[0].desde == Date(milliseconds: 5_000))
        #expect(q.contador == 3 && q.total == 6)
    }

    @Test func canaisNaoEntramEEstadoDesconhecidoCaiEmLivresComoNaoInformado() throws {
        let q = montar([try bot("g", "Grupo", extra: #","kind":"group""#), try bot("u", "Uma", status: "unavailable"),
                        try bot("w", "Wait", status: "waiting"), try bot("i", "Idle", status: "idle")])
        #expect(!q.livres.contains { $0.id == "g" })
        #expect(q.livres.first { $0.id == "u" }?.desconhecido == true)
        #expect(q.livres.first { $0.id == "w" }?.desconhecido == true)
        #expect(q.livres.first { $0.id == "i" }?.desconhecido == false)
        #expect(q.voce.isEmpty && q.trabalhando.isEmpty && q.problema.isEmpty)
    }

    @Test func oPedidoMaisAntigoDaPrincipalGuiaOCartao() {
        let entradas = [pedido("p2", em: "a", criado: 9_000), pedido("p1", em: "a", criado: 4_000),
                        pedido("p3", em: "a", status: "responded", criado: 1_000)]
        #expect(MXQuadroRegras.pedidoPendente(entradas) == 4_000)
        #expect(MXQuadroRegras.pedidoPendente([pedido("p", em: "a", status: "expired")]) == nil)
        #expect(MXQuadroRegras.pedidoPendente([]) == nil)
    }

    // MARK: Cobertura parcial, vazio com ressalva (nunca "zero global", nunca "tudo certo")

    @Test func aRessalvaDeCoberturaParcialVaiComALista() throws {
        let cheia = montar([try bot("a", "Ana", status: "needsInput")])
        let vazia = montar([try bot("f", "Fabi")])
        let semBots = montar([])
        for q in [cheia, vazia, semBots] {
            #expect(q.estado == .pronto && MXQuadroRegras.ressalva(q) == .coberturaParcial)
        }
        #expect(MXQuadroRegras.vazio(cheia) == nil)
        #expect(MXQuadroRegras.vazio(vazia) == .semPedidos)
        #expect(MXQuadroRegras.vazio(semBots) == .semBots)
        // Onde a tela não mostra bot nenhum, ela também não afirma nada sobre eles.
        for estado in [MXQuadroEstado.carregando, .semComputador, .semAcesso, .erro] {
            let q = MXQuadroDados(estado: estado)
            #expect(MXQuadroRegras.ressalva(q) == nil && MXQuadroRegras.vazio(q) == nil && q.contador == 0)
        }
    }

    @Test func osTextosDeVazioDizemNesteIphoneENuncaTudoCerto() {
        for ingles in [false, true] {
            let t = MXTextosDoQuadro(ingles: ingles)
            let todos = [t.vazioTitulo, t.vazioTexto, t.semBotsTitulo, t.semBotsTexto, t.cobertura, t.coberturaTitulo, t.coberturaCarregando]
                .map { $0.lowercased() }
            for texto in todos {
                for proibido in ["tudo certo", "tudo tranquilo", "nada espera você", "all clear", "all good", "nothing is waiting", "zero"] {
                    #expect(!texto.contains(proibido), "\(ingles): \(texto)")
                }
            }
            // A ressalva fala de "este iPhone", de outras sessões e de canais, e o vazio manda conferir no computador.
            #expect(t.cobertura.contains(ingles ? "this iPhone" : "este iPhone"))
            #expect(t.cobertura.contains(ingles ? "channels" : "canais") && t.cobertura.contains(ingles ? "sessions" : "sessões"))
            #expect(t.vazioTexto.contains(ingles ? "check on the computer" : "confira no computador"))
            #expect(t.vazioTitulo.contains(ingles ? "this iPhone" : "neste iPhone"))
        }
    }

    // MARK: Estados e autoridade (falha fechada)

    @Test func acessoEncerradoFalhaFechadoSemNenhumaLinha() throws {
        let bots = [try bot("a", "Ana", status: "needsInput"), try bot("b", "Beto", status: "working")]
        // O store diz `desconectado(acessoEncerrado)` (401/403, revogação): nada de bot sai, mesmo com o espelho cheio.
        let q = montar(bots, pedidos: ["a": 1], conexao: .desconectado, motivo: .acessoEncerrado("403"))
        #expect(q.estado == .semAcesso && q.total == 0 && q.contador == 0 && !q.mostraBots)
        // A autoridade negada vence qualquer outro sinal, até um `conectado` inconsistente: não há fallback.
        let incoerente = montar(bots, pedidos: ["a": 1], conexao: .conectado, motivo: .acessoEncerrado("revogado"))
        #expect(incoerente.estado == .semAcesso && incoerente.total == 0)
        let reconectando = montar(bots, conexao: .reconectando(desde: agoraFixo), motivo: .acessoEncerrado("403"))
        #expect(reconectando.estado == .semAcesso && reconectando.total == 0)
        #expect(MXQuadroRegras.ressalva(q) == nil)
    }

    @Test func dadoIlegivelFalhaFechadoEPeloMenosNaoViraListaVazia() throws {
        let q = montar([try bot("a", "Ana", status: "needsInput")], ilegivel: true)
        #expect(q.estado == .erro && q.total == 0 && q.contador == 0)
        #expect(MXQuadroRegras.vazio(q) == nil)
    }

    @Test func carregandoSoSemNadaGuardadoECacheApareceAtualizando() throws {
        #expect(montar([], carregou: false).estado == .carregando)
        let comCache = montar([try bot("f", "Fabi")], carregou: false)
        #expect(comCache.estado == .pronto && comCache.atualizando && comCache.livres.count == 1)
        #expect(montar([], carregou: true).estado == .pronto)
    }

    @Test func semComputadorQuandoOAppSoltouOComputador() throws {
        let q = montar([try bot("a", "Ana")], conexao: .desconectado, motivo: .encerradoNoApp)
        #expect(q.estado == .semComputador && q.total == 0)
    }

    @Test func desconectadoCongelaOsDadosComAvisoEReconectandoTambem() throws {
        let bots = [try bot("a", "Ana", status: "needsInput")]
        let parado = montar(bots, conexao: .desconectado, motivo: .prazoLongo)
        #expect(parado.estado == .pronto && parado.ligacao == .parado && parado.voce.count == 1)
        let queda = montar(bots, conexao: .reconectando(desde: agoraFixo))
        #expect(queda.estado == .pronto && queda.ligacao == .reconectando(desde: agoraFixo) && queda.voce.count == 1)
        #expect(montar(bots).ligacao == .aoVivo)
        // Os textos do aviso dizem que os dados estão parados, em pt e en, e "visto há N min" vem da queda.
        let t = MXTextosDoQuadro(ingles: false)
        #expect(t.paradosTexto(visto: nil).contains("parados"))
        #expect(MXTextosDoQuadro(ingles: true).paradosTexto(visto: nil).contains("frozen"))
        #expect(t.visto(desde: agoraFixo.addingTimeInterval(-180), agora: agoraFixo) == "Visto há 3 min.")
        #expect(MXTextosDoQuadro(ingles: true).visto(desde: agoraFixo.addingTimeInterval(-180), agora: agoraFixo) == "Last seen 3 min ago.")
    }

    // MARK: Filtro e Abrir

    @Test func oFiltroAlternaELimpaNoSegundoToque() {
        #expect(MXQuadroRegras.alternar(.todos, .voce) == .voce)
        #expect(MXQuadroRegras.alternar(.voce, .voce) == .todos)
        #expect(MXQuadroRegras.alternar(.voce, .problema) == .problema)
        #expect(MXQuadroRegras.alternar(.problema, .todos) == .todos)
        for secao in MXQuadroFiltro.allCases where secao != .todos {
            #expect(MXQuadroRegras.mostra(secao, com: .todos) && MXQuadroRegras.mostra(secao, com: secao))
            #expect(!MXQuadroRegras.mostra(secao, com: secao == .voce ? .livres : .voce))
        }
    }

    @Test func abrirLevaSempreAConversaPrincipalDoBot() {
        #expect(MXQuadroRegras.destinoDoAbrir("byte") == .conversa("byte"))
        // O id da conversa de um bot sem sessão é a Principal (nunca uma sessão extra nem um canal).
        #expect(MXIdDaConversa("byte").sessao == nil && MXIdDaConversa("byte").agente == "byte")
    }

    @Test func livresSemGiroNemDeslocamentoComReduzirMovimento() {
        // Reduzir movimento: o layout pula (sem animação de posição) e a troca é cross-fade; a seta só muda de lado.
        #expect(MXQuadroRegras.animacaoDaPosicaoDosLivres(reduzir: true) == nil)
        #expect(!MX.Movimento.troca(reduzir: true).interpola && MX.Movimento.troca(reduzir: true).saidaEmDuasFases)
        // Normal: a mola anima posição e giro.
        #expect(MXQuadroRegras.animacaoDaPosicaoDosLivres(reduzir: false) != nil && MX.Movimento.troca(reduzir: false).interpola)
        #expect(MXQuadroRegras.anguloDaSetaDosLivres(aberto: false) == 0 && MXQuadroRegras.anguloDaSetaDosLivres(aberto: true) == 180)
    }

    @Test func menuDoFiltroTemDicaPropriaQueDizComoLimpar() {
        for ingles in [false, true] {
            let t = MXTextosDoQuadro(ingles: ingles)
            #expect(t.dicaDoMenuDeFiltro != t.dicaDoFiltro)
            // O menu não "toca de novo para limpar": manda escolher a opção de mostrar todos.
            #expect(t.dicaDoMenuDeFiltro.contains(ingles ? "Show all" : "Mostrar todos"))
            #expect(!t.dicaDoMenuDeFiltro.lowercased().contains(ingles ? "again" : "de novo"))
            #expect(t.dicaDoFiltro.contains(ingles ? "again" : "de novo"))
            // A opção que o menu oferece tem o mesmo nome da dica.
            #expect(t.mostrarTodos == (ingles ? "Show all" : "Mostrar todos"))
        }
    }

    @Test func osBlocosViramFiltroEmTextoGrande() {
        #expect(!MXQuadroRegras.filtroCompacto(.large) && !MXQuadroRegras.filtroCompacto(.xLarge))
        #expect(MXQuadroRegras.filtroCompacto(.xxLarge) && MXQuadroRegras.filtroCompacto(.accessibility1)
                && MXQuadroRegras.filtroCompacto(.accessibility5))
    }

    // MARK: Textos e medidas

    @Test func textosSaoPtBrEEnEFicamForaDosCatalogos() throws {
        let pt = MXTextosDoQuadro(ingles: false)
        let en = MXTextosDoQuadro(ingles: true)
        #expect(pt.pedePermissao == "Pede sua permissão para uma ação" && en.pedePermissao == "Asks your permission for an action")
        #expect(pt.abrir == "Abrir" && en.abrir == "Open")
        #expect(pt.aoVivo("Mac") == "Ao vivo · Mac" && en.aoVivo("Mac") == "Live · Mac")
        #expect(pt.leituraDoCartao(nome: "Byte", motivo: .permissao, desde: agoraFixo.addingTimeInterval(-180), agora: agoraFixo)
                == "Byte, pede sua permissão, há 3 min")
        #expect(en.leituraDoCartao(nome: "Byte", motivo: .pergunta, desde: nil, agora: agoraFixo) == "Byte, has a question for you")
        #expect(pt.abrirConversa("Byte") == "Abrir a conversa de Byte")
        // "Não prometa resposta direta": nenhum texto do cartão ou do Abrir diz Responder/Aprovar/Permitir.
        for t in [pt, en] {
            for texto in [t.abrir, t.pedePermissao, t.temPergunta, t.esperaVoce, t.dicaDoAbrir].map({ $0.lowercased() }) {
                for proibido in ["responder", "aprovar", "permitir", "reply", "approve", "allow"] {
                    #expect(!texto.contains(proibido), "\(texto)")
                }
            }
        }
        // Os textos ficam em código: nenhuma chave do Mission Board entra nos catálogos compartilhados.
        let raiz = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Sources")
        for catalogo in ["CodyncUI/Resources/Localizable.xcstrings", "CodyncKit/Resources/Localizable.xcstrings"] {
            let dados = try Data(contentsOf: raiz.appendingPathComponent(catalogo))
            let texto = String(decoding: dados, as: UTF8.self)
            for frase in [pt.pedePermissao, pt.cobertura, pt.vazioTitulo, pt.semAcessoTitulo, en.cobertura, en.vazioTitulo] {
                #expect(!texto.contains(frase), "\(frase) em \(catalogo)")
            }
        }
    }

    @Test func alvosDeToqueNaoFicamAbaixoDe44() {
        #expect(MX.Quadro.blocoAltura >= MX.Espaco.alvoMinimo && MX.Quadro.botaoAltura >= MX.Espaco.alvoMinimo)
        #expect(MX.Quadro.linhaAlturaMinima >= MX.Espaco.alvoMinimo && MX.Medida.botaoRedondo >= MX.Espaco.alvoMinimo)
    }

    @Test func contrasteAANosParesDaTela() {
        func luz(_ c: MXCor) -> Double {
            func canal(_ v: Double) -> Double { v <= 0.03928 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
            return 0.2126 * canal(c.r) + 0.7152 * canal(c.g) + 0.0722 * canal(c.b)
        }
        func razao(_ a: MXCor, _ b: MXCor) -> Double {
            let (x, y) = (luz(a), luz(b))
            return (max(x, y) + 0.05) / (min(x, y) + 0.05)
        }
        let cartaoClaro = MX.Claro.balaoDoAgente.sobre(MX.Claro.fundo)
        let painelClaro = MX.Vidro.opacoPainel
        // Claro: texto, secundário, laranja pequeno, verde e vermelho sobre o cartão e sobre o painel de vidro.
        for fundo in [cartaoClaro, painelClaro] {
            for cor in [MX.Claro.texto, MX.Claro.textoSecundario, MX.Claro.mexericaTexto, MX.Claro.sucessoTexto, MX.Claro.erroTexto] {
                #expect(razao(cor, fundo) >= 4.5, "claro \(cor.hex) sobre \(fundo.hex)")
            }
        }
        #expect(razao(MX.Claro.sobreMexerica, MX.Claro.mexerica) >= 4.5)
        // Escuro: sobre o cartão e sobre o painel grafite (o "erro" é o texto principal, ver `MXPaletaDaConta.erro`).
        for fundo in [MX.Escuro.campo, MX.Escuro.superficie] {
            for cor in [MX.Escuro.texto, MX.Escuro.textoSecundario, MX.Escuro.mexericaTexto, MX.Escuro.sucessoTexto] {
                #expect(razao(cor, fundo) >= 4.5, "escuro \(cor.hex) sobre \(fundo.hex)")
            }
        }
        // A barra escura: aba ativa e inativa sobre o realce e sobre a cápsula.
        #expect(razao(MX.Escuro.mexericaTexto, MX.Escuro.etiqueta) >= 4.5)
        #expect(razao(MX.Escuro.textoSecundario, MX.Escuro.superficie) >= 4.5)
    }
}

// MARK: - O quadro sobre o BotStore de verdade

@MainActor
struct RedesignMissionBoardFonteRealTests {
    @Test func aPermissaoPendenteDaPrincipalApareceMesmoComOBotWorking() throws {
        // O caminho `screen_access`: cartão `pending` e o bot segue `working` (o contador antigo omitia).
        let (fonte, store) = fonteDeTeste(bots: [try bot("byte", "Byte", status: "working"), try bot("crivo", "Crivo", status: "working")])
        #expect(fonte.quadro.voce.isEmpty)
        #expect(fonte.pendencias[.missionBoard] == 0)
        store.upsert(pedido("p1", em: "byte", criado: 1_700_000_000_000))
        let q = fonte.quadro
        #expect(q.voce.map(\.id) == ["byte"] && q.voce.first?.motivo == .permissao)
        #expect(q.trabalhando.map(\.id) == ["crivo"])
        #expect(fonte.pendencias[.missionBoard] == 1)
        // Resolvido (o Hub reenvia a entrada): sai do quadro.
        store.upsert(pedido("p1", em: "byte", rev: 3, status: "responded"))
        #expect(fonte.quadro.voce.isEmpty && fonte.pendencias[.missionBoard] == 0)
    }

    @Test func coberturaParcialSessaoExtraECanalNaoViramPedidoESoOEspelhoCarregadoConta() throws {
        let (fonte, store) = fonteDeTeste(bots: [try bot("byte", "Byte"), try bot("grupo", "Time", extra: #","kind":"group""#)])
        // Cartão numa sessão extra e no canal: o app não o enxerga como pedido da Principal (limite declarado).
        store.upsert(pedido("p-sessao", em: "byte", sessao: "byte-suite"))
        store.upsert(pedido("p-canal", em: "grupo"))
        let q = fonte.quadro
        #expect(q.voce.isEmpty && q.livres.map(\.id) == ["byte"])
        // E o quadro diz, na mesma hora, que a cobertura é parcial: com lista vazia, "nada à vista", nunca "zero global".
        #expect(MXQuadroRegras.ressalva(q) == .coberturaParcial && MXQuadroRegras.vazio(q) == .semPedidos)
        // `needsInput` sem cartão carregado ainda pede você (genérico), em vez de sumir.
        store.bots["byte"] = try bot("byte", "Byte", status: "needsInput")
        #expect(fonte.quadro.voce.first?.motivo == .espera)
    }

    @Test func listaVaziaDoStoreAindaNaoCarregadoNaoViraZeroGlobal() {
        let (fonte, _) = fonteDeTeste(bots: [])
        // Nada guardado e o primeiro `catch-up` não chegou: carregando (não "nenhum bot", não "tudo certo").
        #expect(fonte.quadro.estado == .carregando && fonte.quadro.contador == 0)
        #expect(MXQuadroRegras.vazio(fonte.quadro) == nil)
    }

    @Test func acessoEncerradoNoStoreFalhaFechadoEOContadorZera() throws {
        let (fonte, store) = fonteDeTeste(bots: [try bot("byte", "Byte", status: "needsInput")], estado: .desconectado)
        store.estadoConexao = .desconectado(.acessoEncerrado("403"))
        let q = fonte.quadro
        #expect(q.estado == .semAcesso && q.total == 0)
        #expect(fonte.pendencias[.missionBoard] == 0)
        // Mesmo o espelho tendo o bot: nada dele sai, e não há como "abrir" o que não é mostrado.
        #expect(store.bots["byte"]?.needsInput == true)
    }

    @Test func prazoLongoCongelaComAvisoEOStoreSoltoViraSemComputador() throws {
        let (fonte, store) = fonteDeTeste(bots: [try bot("byte", "Byte", status: "needsInput")], estado: .desconectado)
        store.estadoConexao = .desconectado(.prazoLongo)
        #expect(fonte.quadro.estado == .pronto && fonte.quadro.ligacao == .parado && fonte.quadro.voce.count == 1)
        store.estadoConexao = .desconectado(.encerradoNoApp)
        #expect(fonte.quadro.estado == .semComputador && fonte.quadro.total == 0)
    }

    @Test func dadoIlegivelDoStoreViraErroENuncaListaVazia() throws {
        let (fonte, store) = fonteDeTeste(bots: [try bot("byte", "Byte")])
        store.unreadable = true
        let q = fonte.quadro
        #expect(q.estado == .erro && q.total == 0)
    }

    @Test func quedaCurtaMantemOsDadosParadosComAvisoEAbrirContinuaAbrindoOCache() throws {
        let (fonte, store) = fonteDeTeste(bots: [try bot("byte", "Byte", status: "needsInput")], estado: .reconectando(desde: agoraFixo))
        store.upsert(pedido("p1", em: "byte"))
        let q = fonte.quadro
        #expect(q.estado == .pronto && q.ligacao == .reconectando(desde: agoraFixo) && q.voce.first?.motivo == .permissao)
        // O Abrir abre a conversa guardada (cache); aprovar segue desligado na Conversa, como hoje.
        #expect(fonte.conversa(MXQuadroRegras.destinoDoAbrir("byte").idDaConversa ?? "") != nil)
    }

    @Test func aFonteDeExemploTemOsDadosDoMockup() {
        let exemplo = MXFonteExemplo()
        let q = exemplo.quadro
        #expect(q.voce.map(\.nome) == ["Byte", "Prosa"] && q.trabalhando.map(\.nome) == ["Crivo"] && q.problema.map(\.nome) == ["Trato"])
        #expect(q.livres.count == 3 && exemplo.pendencias[.missionBoard] == q.contador)
    }

    @Test func naFonteRealOMissionBoardELigadoESoNoDebug() {
        #expect(MXCapacidades.real.ligado(.aba(.missionBoard)) && MXCapacidades.real.acao(.aba(.missionBoard)) == .real)
        #expect(!MXCapacidades.real.ligado(.aba(.equipes)) && !MXCapacidades.real.ligado(.aba(.rotinas)) && !MXCapacidades.real.ligado(.aba(.estado)))
        // A barra só desenha as abas que existem, na ordem do desenho (Mission Board, Conversas).
        let barra = MXBarraDeAbas(selecionada: .constant(.missionBoard), pendencias: [:], ligadas: [.missionBoard, .conversas], somenteLigadas: true)
        #expect(barra.abas == [.missionBoard, .conversas])
        #expect(MXBarraDeAbas(selecionada: .constant(.missionBoard), pendencias: [:]).abas == MXAba.allCases)
        #expect(!MXNavegacaoRegras.mostraBarra(aba: .missionBoard, caminho: [.conversa("byte")]))
        #expect(MXNavegacaoRegras.mostraBarra(aba: .missionBoard, caminho: []))
    }
}

private extension MXDestino {
    var idDaConversa: String? { if case let .conversa(id) = self { id } else { nil } }
}

// MARK: - Invalidação por todos os eventos do ciclo

@MainActor
struct RedesignMissionBoardInvalidacaoTests {
    @Test func cadaTipoDoFioTemOSeuSinal() {
        let esperados: [String: MXQuadroSinal] = ["entry": .entrada, "bot": .bot, "session": .sessao, "conversation": .conversa,
                                                  "spaces": .space, "hello": .hello, "resync": .resync,
                                                  "donnaControlTowerChanged": .torre]
        for (tipo, sinal) in esperados { #expect(MXQuadroSinal.doFio(tipo) == sinal, "\(tipo)") }
        #expect(MXQuadroSinal.doFio("usage") == nil && MXQuadroSinal.doFio("screen") == nil && MXQuadroSinal.doFio("outro") == nil)
        // O conjunto cobre o do desktop (`REREAD_EVENTS`) e a exclusão, que chega como `bot` com `deleted`.
        #expect(Set(esperados.values).union([.botExcluido]) == Set(MXQuadroSinal.allCases))
    }

    @Test func osEventosNovosDoCicloSaoDecodificadosSoPeloTipo() throws {
        for tipo in ["session", "conversation", "spaces", "donnaControlTowerChanged"] {
            // O corpo é lixo de propósito: nada dele é lido, e um corpo ilegível não derruba nem vira `undecodable`.
            let evento = HostClient.parseEvent(Data(#"{"type":"\#(tipo)","spaceId":"x","payload":[1,{"a":null}]}"#.utf8))
            guard case let .cycleChanged(lido)? = evento else { Issue.record("\(tipo): \(String(describing: evento))"); continue }
            #expect(lido == tipo)
            #expect(MXQuadroSinal.de(try #require(evento)) == MXQuadroSinal.doFio(tipo))
        }
        // O que o app já tratava segue igual.
        if case .resync? = HostClient.parseEvent(Data(#"{"type":"resync"}"#.utf8)) {} else { Issue.record("resync mudou") }
        #expect(HostClient.parseEvent(Data(#"{"type":"tipo-que-nao-existe"}"#.utf8)) == nil)
    }

    @Test func todoEventoDoCicloMexeNoQuadroEOQuadroERecalculado() async throws {
        let fake = FakeRemote(.ready(.direct))
        let store = lojaDeTeste(transporte: fake)
        defer { store.retire() }
        let fonte = MXFonteReal(store: store)
        fonte.conexaoForcada = .conectado
        store.setActive(true)
        #expect(await ate { await fake.subscribed })

        let eventos: [(String, MXQuadroSinal)] = [
            (#"{"type":"hello","hostId":"h1","rev":1}"#, .hello),
            (#"{"type":"bot","bot":{"id":"byte","name":"Byte","status":"needsInput","rev":2,"lastAt":2}}"#, .bot),
            (#"{"type":"entry","entry":{"id":"p1","seq":3,"botId":"byte","rev":3,"kind":"permission","turn":1,"data":{"status":"pending"},"createdAt":1,"updatedAt":1}}"#, .entrada),
            (#"{"type":"session","sessionId":"s1"}"#, .sessao),
            (#"{"type":"conversation","id":"c1"}"#, .conversa),
            (#"{"type":"spaces"}"#, .space),
            (#"{"type":"donnaControlTowerChanged","hubId":"h","spaceId":"s"}"#, .torre),
        ]
        for (json, sinal) in eventos {
            let antes = store.quadroSinais.contagem[sinal] ?? 0
            let geracao = store.quadroSinais.geracao
            let recalculou = Caixa()
            withObservationTracking { _ = fonte.quadro } onChange: { recalculou.valor = true }
            await fake.emit(json)
            #expect(await ate { (store.quadroSinais.contagem[sinal] ?? 0) == antes + 1 }, "\(sinal)")
            #expect(store.quadroSinais.geracao == geracao + 1 && store.quadroSinais.ultimo == sinal)
            #expect(await ate { recalculou.valor }, "o quadro não foi invalidado por \(sinal)")
        }
        // O cartão e o bot chegaram pelos eventos reais.
        #expect(fonte.quadro.voce.map(\.id) == ["byte"] && fonte.quadro.voce.first?.motivo == .permissao)

        // Exclusão: o bot sai do quadro na hora (o evento `bot` com `deleted`, decodificado como `botDeleted`).
        let excluidos = store.quadroSinais.contagem[.botExcluido] ?? 0
        await fake.emit(#"{"type":"bot","bot":{"id":"byte","deleted":true,"rev":9}}"#)
        #expect(await ate { fonte.quadro.total == 0 })
        #expect((store.quadroSinais.contagem[.botExcluido] ?? 0) == excluidos + 1)

        // `resync` recomeça a assinatura e também invalida.
        let resync = store.quadroSinais.contagem[.resync] ?? 0
        await fake.emit(#"{"type":"resync"}"#)
        #expect(await ate { (store.quadroSinais.contagem[.resync] ?? 0) == resync + 1 })
    }

    @Test func eventosQueNaoSaoDoCicloNaoInvalidam() async throws {
        let fake = FakeRemote(.ready(.direct))
        let store = lojaDeTeste(transporte: fake)
        defer { store.retire() }
        store.setActive(true)
        #expect(await ate { await fake.subscribed })
        await fake.emit(#"{"type":"hello","hostId":"h1","rev":1}"#)
        #expect(await ate { store.quadroSinais.contagem[.hello] == 1 })
        let antes = store.quadroSinais.geracao
        await fake.emit(#"{"type":"usage","usage":{"providers":[]}}"#)
        await fake.emit(#"{"type":"tipo-que-nao-existe"}"#)
        try await Task.sleep(for: .milliseconds(80))
        #expect(store.quadroSinais.geracao == antes)
    }

    @Test func sinalDoCicloNaoAdiaOSalvamentoNemMexeNaConexaoMasMudancaRealContinuaSalvando() async throws {
        let fake = FakeRemote(.ready(.direct))
        let store = lojaDeTeste(transporte: fake)
        defer { store.retire() }
        store.setActive(true)
        #expect(await ate { await fake.subscribed })
        // Uma mudança real do espelho agenda o salvamento.
        await fake.emit(#"{"type":"bot","bot":{"id":"byte","name":"Byte","rev":2,"lastAt":2}}"#)
        #expect(await ate { store.saveTask != nil && store.bots["byte"] != nil })
        let pendente = store.saveTask
        let conexao = store.connection
        let rev = store.rev
        // Sinais sem mudança do espelho: contam, mas a tarefa de salvar é a MESMA (não foi cancelada nem reagendada).
        for tipo in ["session", "conversation", "spaces", "donnaControlTowerChanged"] {
            let antes = store.quadroSinais.geracao
            await fake.emit(#"{"type":"\#(tipo)"}"#)
            #expect(await ate { store.quadroSinais.geracao == antes + 1 }, "\(tipo)")
        }
        #expect(store.saveTask == pendente && pendente?.isCancelled == false)
        #expect(store.connection == conexao && store.rev == rev)
        // Mudança real depois dos sinais: volta a agendar (outra tarefa).
        await fake.emit(#"{"type":"bot","bot":{"id":"byte","name":"Byte 2","rev":3,"lastAt":3}}"#)
        #expect(await ate { store.bots["byte"]?.name == "Byte 2" })
        #expect(store.saveTask != pendente)
    }

    @Test func oContadorDeSinaisIgnoraONulo() {
        let sinais = MXQuadroSinais()
        sinais.registrar(nil)
        #expect(sinais.geracao == 0 && sinais.ultimo == nil)
        sinais.registrar(.sessao)
        sinais.registrar(.sessao)
        #expect(sinais.geracao == 2 && sinais.contagem[.sessao] == 2)
    }
}

// MARK: - Imagens e montagem da tela (sem simulador)

/// Renderiza a tela em Mac Catalyst (`ImageRenderer`) e grava PNGs quando `TEST_RUNNER_MX_CAPTURAS_DIR` aponta uma pasta (sem
/// ela, só confere que monta). NÃO são capturas do simulador: o Liquid Glass e o blur podem divergir do aparelho, o Catalyst
/// não escala a fonte (no "AX2" só o arranjo empilhado muda, não o tamanho do texto) e o menu do filtro sai só como botão.
@MainActor
struct RedesignMissionBoardImagensTests {
    private func tela(_ cena: MXCenaDoQuadro, esquema: ColorScheme, tamanho: DynamicTypeSize, semRolagem: Bool = true,
                      filtro: MXQuadroFiltro = .todos, livresAbertos: Bool = false) -> some View {
        let fonte = MXFonteExemplo()
        fonte.cenaDoQuadro = cena
        fonte.agoraDoQuadro = agoraFixo
        return MXQuadroTela(fonte: fonte, esquema: esquema, acoes: MXQuadroAcoes(), textos: MXTextosDoQuadro(ingles: false),
                            semRolagem: semRolagem, relogio: agoraFixo, filtro: filtro, livresAbertos: livresAbertos)
            .environment(\.dynamicTypeSize, tamanho)
            .safeAreaPadding(.top, 54)
    }

    private func gerar<V: View>(_ vista: V, altura: CGFloat?) throws -> Data {
        MXFontes.registrar()
        // Altura dada: a tela na altura natural, cortada embaixo (a primeira tela); sem altura, a tela inteira.
        let natural = vista.fixedSize(horizontal: false, vertical: true)
        let conteudo = altura == nil ? AnyView(natural.frame(width: 390)) : AnyView(natural.frame(width: 390, height: altura, alignment: .top).clipped())
        let renderizador = ImageRenderer(content: conteudo.background(Color.black))
        renderizador.scale = 2
        renderizador.proposedSize = ProposedViewSize(width: 390, height: altura)
        let imagem = try #require(renderizador.uiImage)
        return try #require(imagem.pngData())
    }

    @Test func aTelaMontaEmTodosOsEstadosTemasETamanhos() throws {
        for cena in MXCenaDoQuadro.allCases {
            for esquema in [ColorScheme.light, .dark] {
                for tamanho in [DynamicTypeSize.large, .accessibility2, .accessibility5] {
                    let dados = try gerar(tela(cena, esquema: esquema, tamanho: tamanho), altura: nil)
                    #expect(dados.count > 2_000, "\(cena) \(esquema) \(tamanho)")
                }
            }
        }
    }

    @Test func gravaAsImagensDosEstados() throws {
        guard let caminho = ProcessInfo.processInfo.environment["MX_CAPTURAS_DIR"] else { return }
        let pasta = URL(fileURLWithPath: caminho)
        try FileManager.default.createDirectory(at: pasta, withIntermediateDirectories: true)
        for (esquema, tema) in [(ColorScheme.light, "claro"), (.dark, "escuro")] {
            let barra = { (n: Int, escuro: Bool) in
                MXBarraDeAbas(selecionada: .constant(.missionBoard), pendencias: [.missionBoard: n], ligadas: [.missionBoard, .conversas],
                              somenteLigadas: true, escuro: escuro).padding(.bottom, MX.Medida.barraAbasDaBorda)
            }
            // A primeira tela (390 × 844), como o mockup, com a barra de abas por cima.
            for (cena, nome, tamanho) in [(MXCenaDoQuadro.padrao, "padrao", DynamicTypeSize.large),
                                          (.padrao, "grande", .accessibility2)] {
                let vista = tela(cena, esquema: esquema, tamanho: tamanho)
                    .overlay(alignment: .bottom) { barra(2, esquema == .dark).environment(\.dynamicTypeSize, tamanho) }
                try gerar(vista, altura: 844).write(to: pasta.appendingPathComponent("tela-\(nome)-\(tema).png"))
            }
            // Os estados, na altura natural.
            for cena in MXCenaDoQuadro.allCases {
                try gerar(tela(cena, esquema: esquema, tamanho: .large), altura: nil)
                    .write(to: pasta.appendingPathComponent("estado-\(cena.rawValue)-\(tema).png"))
            }
            try gerar(tela(.padrao, esquema: esquema, tamanho: .large, livresAbertos: true), altura: nil)
                .write(to: pasta.appendingPathComponent("estado-livres-abertos-\(tema).png"))
            try gerar(tela(.padrao, esquema: esquema, tamanho: .large, filtro: .voce), altura: nil)
                .write(to: pasta.appendingPathComponent("estado-filtro-voce-\(tema).png"))
        }
    }
}
#endif
