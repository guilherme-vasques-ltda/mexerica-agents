import Foundation
import Testing
@testable import CodyncKit
@testable import CodyncUI

/// Etapa 3 do redesenho iOS v1: Sessões (07, 07b) e Ajustes do agente (08). Regras puras, o mapeamento das sessões
/// do Hub e a fonte de exemplo.

private let calendario: Calendar = {
    var c = Calendar(identifier: .gregorian)
    c.timeZone = TimeZone(identifier: "America/Sao_Paulo")!
    return c
}()

private func bot(_ campos: [String: Any]) throws -> Bot {
    var json: [String: Any] = ["id": "byte", "name": "Byte", "backend": "claude", "cwd": "/Users/user/pace", "permission": "ask"]
    json.merge(campos) { _, novo in novo }
    return try JSONDecoder().decode(Bot.self, from: JSONSerialization.data(withJSONObject: json))
}

private let byte = MXExemplo.sessoes("byte").sessoes

struct RedesignSessoesTests {
    @Test func arvoreDoByteComoNoDesenho() {
        // "Byte · Precisa de você · 17 sessões" (captura 07).
        #expect(MXSessoesRegras.conta(byte) == 17)
        #expect(MXSessoesRegras.contagem(17) == catalogo("\(17) sessões") && MXSessoesRegras.contagem(1) == catalogo("\(1) sessão"))
        // Tudo recolhido: Agora · 2 (Principal e Guarda de data inválida) e Paradas · 6.
        let grupos = MXSessoesRegras.grupos(byte, busca: "", abertas: [], fonteArquiva: true)
        #expect(grupos.map(\.titulo) == [catalogo("Agora"), catalogo("Paradas")])
        #expect(grupos.map(\.quantidade) == [2, 6])
        #expect(grupos[0].linhas.map(\.nome) == ["Principal", "Guarda de data inválida"])
        // Recolhida, a linha diz quantas há dentro (Principal: Revisão, Suíte, Teste instável, Notas).
        #expect(grupos[0].linhas[0].quantidade == 4 && !grupos[0].linhas[0].aberta)
        #expect(grupos[0].linhas[0].info == catalogo("Precisa de você"))
        #expect(grupos[1].linhas.first { $0.nome == "Migração do banco" }?.quantidade == 3)
        #expect(grupos[1].linhas.first { $0.nome == "Rascunho do changelog" }?.info == "Ontem")
    }

    @Test func comecaRecolhidaEACapturaAbreAsAtivas() {
        // README, "Gestos": tudo começa recolhido.
        #expect(MX.Sessoes.comecaRecolhida)
        #expect(MXSessoesRegras.abertasIniciais(byte).isEmpty)
        // A captura 07: as ativas com filhas abertas (Principal, Revisão, Suíte, Guarda); Benchmark fica recolhida.
        let abertas = MXSessoesRegras.abertasIniciais(byte, recolhida: false)
        #expect(abertas == ["byte-principal", "byte-revisao", "byte-suite", "byte-guarda"])
        let agora = MXSessoesRegras.grupos(byte, busca: "", abertas: abertas, fonteArquiva: true)[0].linhas
        #expect(agora.map(\.nome) == ["Principal", "Revisão dos testes", "Suíte do checkout", "Teste instável do cupom",
                                      "Notas da versão", "Guarda de data inválida", "Pesquisa de fusos horários"])
        #expect(agora.map(\.nivel) == [0, 1, 2, 3, 1, 0, 1])
        // Aberta, a linha não mostra a quantidade.
        #expect(agora[0].aberta && agora[0].quantidade == nil)
    }

    @Test func soArquivaParadaQueNaoEPrincipalESoSeAFonteSouber() {
        let abertas = MXSessoesRegras.abertasIniciais(byte, recolhida: false)
        let linhas = MXSessoesRegras.grupos(byte, busca: "", abertas: abertas, fonteArquiva: true).flatMap(\.linhas)
        func linha(_ nome: String) -> MXLinhaDeSessao? { linhas.first { $0.nome == nome } }
        #expect(linha("Principal")?.arquivavel == false)
        #expect(linha("Guarda de data inválida")?.arquivavel == false) // trabalhando
        #expect(linha("Notas da versão")?.arquivavel == true)
        #expect(linha("Pesquisa de fusos horários")?.arquivavel == true)
        // Fonte que não arquiva (a real): nenhuma.
        let semArquivar = MXSessoesRegras.grupos(byte, busca: "", abertas: abertas, fonteArquiva: false).flatMap(\.linhas)
        #expect(semArquivar.allSatisfy { !$0.arquivavel })
    }

    @Test func buscaAchaEmQualquerNivelSemArvoreEComOPai() {
        let cupom = MXSessoesRegras.grupos(byte, busca: "cupom", abertas: [], fonteArquiva: true).flatMap(\.linhas)
        #expect(cupom.map(\.nome) == ["Teste instável do cupom"])
        #expect(cupom[0].info == catalogo("em \("Suíte do checkout")") && cupom[0].nivel == 0 && !cupom[0].temFilhas)
        // Sem acento e sem maiúscula.
        let revisao = MXSessoesRegras.grupos(byte, busca: "REVISAO", abertas: [], fonteArquiva: true).flatMap(\.linhas)
        #expect(revisao.map(\.nome) == ["Revisão dos testes", "Revisão do PR de pagamentos"])
        // Nada: sem grupos e a frase.
        #expect(MXSessoesRegras.grupos(byte, busca: "zzz", abertas: [], fonteArquiva: true).isEmpty)
        #expect(MXSessoesRegras.semResultado("zzz") == catalogo("Nenhuma sessão com “\("zzz")”."))
    }

    @Test func arvoreVaiAteQuatroNiveis() {
        #expect(MX.Sessoes.niveisMaximos == 4)
        #expect((0...6).map(MXSessoesRegras.nivelVisivel) == [0, 1, 2, 3, 3, 3, 3])
        // Migração › Backup › Conferir › Relatório: 4 níveis.
        let abertas: Set<String> = ["byte-migracao", "byte-backup", "byte-indices"]
        let linhas = MXSessoesRegras.grupos(byte, busca: "", abertas: abertas, fonteArquiva: true)[1].linhas
        let migracao = linhas.drop { $0.nome != "Migração do banco" }.prefix(4)
        #expect(migracao.map(\.nivel) == [0, 1, 2, 3])
    }

    @Test func puxaoCurtoNaoEngoleOProximoToque() {
        var g = MXGestoDoToque()
        // Toque simples: abre.
        g.movimento()
        g.fim(valeu: false)
        let simples = g.toqueAbre()
        #expect(simples)
        // Puxão curto (não abriu): o fim zera, e o próximo toque abre.
        g.movimento()
        g.fim(valeu: false)
        #expect(!g.usado)
        g.movimento()
        let depoisDoCurto = g.toqueAbre()
        #expect(depoisDoCurto)
        // Puxão que abriu: o toque que o termina não abre de novo; o seguinte abre.
        g.movimento()
        g.fim(valeu: true)
        let doPuxao = g.toqueAbre()
        let seguinte = g.toqueAbre()
        #expect(!doPuxao && seguinte)
        // Uma marca esquecida some no começo do toque seguinte.
        g.usar()
        g.movimento()
        let esquecida = g.toqueAbre()
        #expect(esquecida)
    }

    @Test func naBuscaONomeDoResultadoTemPrioridade() {
        let busca = MXSessoesRegras.grupos(byte, busca: "cupom", abertas: [], fonteArquiva: true).flatMap(\.linhas)
        #expect(busca.first?.infoEhDoPai == true)
        let arvore = MXSessoesRegras.grupos(byte, busca: "", abertas: [], fonteArquiva: true).flatMap(\.linhas)
        #expect(arvore.allSatisfy { !$0.infoEhDoPai })
    }

    @Test func linhaDeSessaoParaOVoiceOver() {
        let linhas = MXSessoesRegras.grupos(byte, busca: "", abertas: [], fonteArquiva: true)
        #expect(linhas[0].linhas[0].rotuloAcessivel
                == "Principal, " + [catalogo("Precisa de você"), catalogo("\(4) sessões dentro"), catalogo("recolhida")].joined(separator: ", "))
        #expect(linhas[1].linhas[0].rotuloAcessivel == "Rascunho do changelog, Ontem")
    }

    @Test func puxarAbreComOsLimiaresDoDesenho() {
        // Sessão: abre passando de 70 pt, até 140.
        #expect(!MXSessoesRegras.abreAoSoltar(70) && MXSessoesRegras.abreAoSoltar(71))
        #expect(MXSessoesRegras.arrastoDaSessao(500) == 140 && MXSessoesRegras.arrastoDaSessao(-20) == 0)
        // Agente na lista: Sessões passando de 80 pt, até 150.
        #expect(!MXSessoesRegras.abreSessoesAoSoltar(80) && MXSessoesRegras.abreSessoesAoSoltar(81))
        #expect(MXSessoesRegras.arrastoParaSessoes(500) == 150)
        // Só conta o puxão mais horizontal que vertical (o vertical é a rolagem).
        #expect(MXSessoesRegras.ehHorizontal(dx: -30, dy: 10) && !MXSessoesRegras.ehHorizontal(dx: -10, dy: 30))
        #expect(MX.Movimento.segurarParaAjustes == 0.55)
        #expect(MX.Sessoes.linhaAltura == 44) // o desenho mostra 36 pt
    }
}

struct RedesignSessoesDoHubTests {
    private let agora = Date(timeIntervalSince1970: 1_762_900_000)

    private func linha(_ id: String, pai: String? = nil, titulo: String = "x", estado: String = "open",
                       principal: Bool = false) -> MXSessaoDoHub {
        MXSessaoDoHub(id: id, principal: principal, parentSession: pai, title: titulo, state: estado,
                      lastUsedAt: Int64(agora.timeIntervalSince1970 * 1000))
    }

    @Test func arvoreDoHubPeloPai() {
        let linhas = [
            linha("byte", titulo: "", principal: true),
            linha("a", titulo: "Revisão"),
            linha("b", pai: "a", titulo: "Suíte"),
            linha("c", pai: "byte", titulo: "Notas"),
            linha("d", pai: "sumiu", titulo: "Órfã"),
            linha("e", titulo: "Encerrada", estado: "closed"),
            linha("f", titulo: "   "),
        ]
        let arvore = MXMapeamento.arvore(linhas, botId: "byte", principal: .trabalhando, horaDaPrincipal: "09:12",
                                         agora: agora, calendario: calendario)
        // A Principal primeiro (id do bot), com as filhas dela; encerradas de fora; pai que sumiu vira raiz.
        #expect(arvore.map(\.id) == ["byte", "a", "d", "f"])
        #expect(arvore[0].principal && arvore[0].nome == catalogo("Principal") && arvore[0].estado == .trabalhando)
        #expect(arvore[0].hora == "09:12" && arvore[0].filhas.map(\.id) == ["c"])
        #expect(arvore[1].filhas.map(\.nome) == ["Suíte"])
        // O Hub não manda o estado por sessão: as outras ficam livres, com a hora.
        #expect(arvore[1].estado == .livre && !arvore[1].hora.isEmpty)
        #expect(arvore[3].nome == catalogo("Sessão sem nome"))
    }

    @Test func cicloNaoTravaENadaSome() {
        let linhas = [linha("a", pai: "b"), linha("b", pai: "a")]
        let arvore = MXMapeamento.arvore(linhas, botId: "byte", principal: .livre, horaDaPrincipal: "", agora: agora)
        #expect(MXSessoesRegras.conta(arvore) == 3) // Principal + a + b, cada uma uma vez
    }

    @Test func semRespostaDoHubSoAPrincipal() {
        let arvore = MXMapeamento.arvore([], botId: "byte", principal: .precisaDeVoce, horaDaPrincipal: "Ontem", agora: agora)
        #expect(arvore.map(\.id) == ["byte"] && arvore[0].estado == .precisaDeVoce)
    }

    @Test func estadoDaPrincipalPeloBot() throws {
        #expect(MXMapeamento.estadoDaPrincipal(try bot(["status": "idle"]), aprovacaoPendente: false) == .livre)
        #expect(MXMapeamento.estadoDaPrincipal(try bot(["status": "working"]), aprovacaoPendente: false) == .trabalhando)
        #expect(MXMapeamento.estadoDaPrincipal(try bot(["status": "needsInput"]), aprovacaoPendente: false) == .precisaDeVoce)
        #expect(MXMapeamento.estadoDaPrincipal(try bot(["status": "working"]), aprovacaoPendente: true) == .precisaDeVoce)
        // Trabalhando num canal (não na própria conversa): a Principal fica livre.
        #expect(MXMapeamento.estadoDaPrincipal(try bot(["status": "working", "workingChat": "canal"]), aprovacaoPendente: false) == .livre)
    }

    @Test func respostaDoHubDecodificaSoOQueATelaUsa() throws {
        let json = #"{"sessions":[{"id":"byte","chatId":"byte","principal":true,"origin":"owner","sourceSession":null,"parentSession":null,"depth":0,"title":"","titleSource":"principal","state":"open","closedReason":null,"closedAt":null,"createdAt":1,"lastUsedAt":2,"revision":3,"engine":null}],"sessionActions":["create","send"]}"#
        let r = try JSONDecoder().decode(MXRespostaDeSessoes.self, from: Data(json.utf8))
        #expect(r.sessions == [MXSessaoDoHub(id: "byte", principal: true, parentSession: nil, title: "", state: "open", lastUsedAt: 2)])
    }

    @Test func desfazMudancaSoSeNadaMaisNovoChegou() throws {
        var pedido = try bot(["rev": 5, "permission": "ask"])
        pedido.permission = "auto"
        #expect(MXMapeamento.desfazMudanca(atual: pedido, pedido: pedido, revInicial: 5))
        var maisNovo = pedido
        maisNovo.rev = 6
        #expect(!MXMapeamento.desfazMudanca(atual: maisNovo, pedido: pedido, revInicial: 5))
    }
}

struct RedesignAjustesTests {
    @Test func pedidoDoUpdateBotLevaSoOCampo() throws {
        func chaves(_ p: MXPatchDoBot) throws -> [String: Any] {
            try JSONSerialization.jsonObject(with: JSONEncoder().encode(p)) as? [String: Any] ?? [:]
        }
        let avisos = try chaves(MXPatchDoBot(id: "byte", notify: false))
        #expect(avisos.keys.sorted() == ["id", "notify"] && avisos["notify"] as? Bool == false)
        // A autonomia vai como escolha da pessoa (o Hub recusa `permission` sem ela).
        let autonomia = try chaves(MXPatchDoBot(id: "byte", permission: "auto"))
        #expect(autonomia.keys.sorted() == ["id", "permission", "permissionChosenByUser"])
        #expect(autonomia["permission"] as? String == "auto" && autonomia["permissionChosenByUser"] as? Bool == true)
        #expect(try chaves(MXPatchDoBot(id: "byte", pinned: true)).keys.sorted() == ["id", "pinned"])
    }

    @Test func segmentoDeAutonomiaTem44() {
        #expect(MXAjustesRegras.alturaDoSegmento == 44 && MX.Ajustes.segmentoAltura == 34)
    }

    @Test func excluirEIndisponiveis() {
        #expect(MXAjustesRegras.excluirNoComputador == catalogo("Exclua no computador"))
        #expect(MXAjustesRegras.excluirIndisponivelAcessivel == catalogo("Indisponível no iPhone"))
        #expect(MXIndisponivel.emBreve == catalogo("Em breve") && MXIndisponivel.valorAcessivel == catalogo("Indisponível"))
        #expect(MXItemDeAjuste(.motor, letra: "M", nome: "Motor", valor: nil).disponivel)
        #expect(!MXItemDeAjuste(.motor, letra: "M", nome: "Motor", valor: nil, disponivel: false).disponivel)
    }

    @Test func autonomiaDoDesenhoEDoHub() {
        #expect(MXAutonomia.allCases.map(\.nome) == [catalogo("Pedir"), catalogo("Liberar"), catalogo("Total")])
        // "Liberar": o desenho diz "Age e avisa, com Desfazer"; não há Desfazer, o texto diz o que acontece.
        #expect(MXAutonomia.allCases.map(\.dica) == [catalogo("Espera sua aprovação"), catalogo("Age sem pedir e avisa."), catalogo("Em breve")])
        #expect(MXAutonomia.doHub("ask") == .pedir && MXAutonomia.doHub("auto") == .liberar && MXAutonomia.doHub("x") == nil)
        #expect(MXAutonomia.pedir.paraOHub == "ask" && MXAutonomia.liberar.paraOHub == "auto" && MXAutonomia.total.paraOHub == nil)
        // Total fica à vista e desligado ("Em breve").
        #expect(!MXAutonomia.total.disponivel && MXAutonomia.pedir.disponivel)
    }

    @Test func valoresDaLista() {
        #expect(MXAjustesRegras.ferramentas(conectores: 4, skills: 3) == catalogo("\(4) conectores") + " · " + catalogo("\(3) skills"))
        #expect(MXAjustesRegras.ferramentas(conectores: 1, skills: 0) == catalogo("\(1) conector"))
        #expect(MXAjustesRegras.ferramentas(conectores: 0, skills: 1) == catalogo("\(1) skill"))
        #expect(MXAjustesRegras.ferramentas(conectores: 0, skills: 0) == catalogo("Nenhuma"))
        #expect(MXAjustesRegras.comportamento("Direto, sem enrolar\nSegunda linha") == "Direto, sem enrolar")
        #expect(MXAjustesRegras.comportamento("  \n") == nil)
        #expect(MXAjustesRegras.sincronia(computador: "MacBook Pro", conectado: true) == catalogo("Sincronizado com o \("MacBook Pro") · agora"))
        #expect(MXAjustesRegras.sincronia(computador: "MacBook Pro", conectado: false) == catalogo("Sem conexão com o \("MacBook Pro")"))
        #expect(MXAjustesRegras.perguntaDeExcluir("Byte") == catalogo("Excluir \("Byte")?"))
        // A lista do desenho, na ordem (08).
        #expect(MXExemplo.itensDeAjuste.map(\.nome) == ["Identidade", "Motor", "Comportamento", "Ferramentas", "WhatsApp",
                                                        "Memória", "Voz", "Rotinas"])
        #expect(MXExemplo.itensDeAjuste.map(\.letra) == ["I", "M", "C", "F", "W", "R", "V", "T"])
    }
}

@MainActor
struct RedesignFonteExemploSessoesTests {
    @Test func arquivarEDesfazerVoltaAoLugar() throws {
        let fonte = MXFonteExemplo(cena: .sessoes)
        let antes = try #require(fonte.sessoes("byte"))
        #expect(antes.arquivadas == 3 && antes.podeArquivar && antes.podeCriar)
        fonte.arquivarSessao("byte-notas", em: "byte")
        let depois = try #require(fonte.sessoes("byte"))
        #expect(MXSessoesRegras.conta(depois.sessoes) == 16 && depois.arquivadas == 4)
        fonte.desfazerArquivar("byte-notas", em: "byte")
        let volta = try #require(fonte.sessoes("byte"))
        #expect(volta.sessoes == antes.sessoes && volta.arquivadas == 3)
        // A Principal não arquiva.
        fonte.arquivarSessao("byte-principal", em: "byte")
        #expect(MXSessoesRegras.conta(try #require(fonte.sessoes("byte")).sessoes) == 17)
    }

    @Test func novaSessaoEntraDepoisDaPrincipalEAbre() throws {
        let fonte = MXFonteExemplo(cena: .sessoes)
        let nova = try #require(fonte.novaSessao(em: "byte"))
        let sessoes = try #require(fonte.sessoes("byte")).sessoes
        #expect(sessoes[1].id == nova && sessoes[1].nome == "Nova sessão")
        // No exemplo, toda sessão abre a conversa de exemplo do agente (como o desenho).
        #expect(fonte.conversaDaSessao(nova, em: "byte") == "byte")
        #expect(fonte.conversaDaSessao("x", em: "ninguem") == nil)
    }

    @Test func ajustesDoExemploMudamNaMemoria() throws {
        let fonte = MXFonteExemplo()
        let ajustes = try #require(fonte.ajustes("byte"))
        #expect(ajustes.autonomia == .pedir && ajustes.avisos == true && ajustes.sincronizado)
        fonte.mudarAutonomia(.liberar, em: "byte")
        fonte.mudarAutonomia(.total, em: "byte") // em breve: não muda
        #expect(fonte.ajustes("byte")?.autonomia == .liberar)
        #expect(fonte.conversa("byte")?.autonomia == catalogo("Liberar")) // o menu do agente acompanha
        fonte.alternarAvisos(em: "byte")
        #expect(fonte.ajustes("byte")?.avisos == false)
        // Excluir tira da lista; o Berga não se exclui.
        #expect(!fonte.excluirAgente("berga"))
        #expect(fonte.excluirAgente("trato"))
        #expect(!fonte.lista.agentes.contains { $0.id == "trato" } && fonte.ajustes("trato") == nil)
    }
}

struct RedesignNavegacaoDaEtapa3Tests {
    @Test func cenasDaEtapa3() {
        #expect(MXCena.daLinhaDeComando(["app", "-MexericaCena", "sessoes"], ambiente: [:]) == .sessoes)
        #expect(MXCena.daLinhaDeComando(["app", "-MexericaCena", "sessao-aberta"], ambiente: [:]) == .sessaoAberta)
        #expect(MXCena.daLinhaDeComando(["app"], ambiente: ["MEXERICA_CENA": "ajustes-do-agente"]) == .ajustesDoAgente)
        #expect(MXNavegacaoRegras.caminhoInicial(.sessoes) == [.sessoes("byte")])
        #expect(MXNavegacaoRegras.caminhoInicial(.sessaoAberta) == [.sessoes("byte"), .sessao(agente: "byte", sessao: "byte-suite")])
        #expect(MXNavegacaoRegras.caminhoInicial(.ajustesDoAgente).isEmpty)
        #expect(MXNavegacaoRegras.ajustesIniciais(.ajustesDoAgente) == "byte" && MXNavegacaoRegras.ajustesIniciais(.sessoes) == nil)
    }

    @Test func ajustesNaoEmpilhamOQueJaEstaAberto() {
        // Conversar dentro da conversa do Byte (ou da Principal aberta pelas Sessões) só fecha a folha.
        #expect(MXNavegacaoRegras.destinoDepoisDosAjustes(.conversa("byte"), topo: .conversa("byte")) == nil)
        #expect(MXNavegacaoRegras.destinoDepoisDosAjustes(.conversa("byte"), topo: .sessao(agente: "byte", sessao: "byte")) == nil)
        // Numa sessão extra, Conversar abre a Principal (outra conversa).
        #expect(MXNavegacaoRegras.destinoDepoisDosAjustes(.conversa("byte"), topo: .sessao(agente: "byte", sessao: "s"))
            == .conversa("byte"))
        #expect(MXNavegacaoRegras.destinoDepoisDosAjustes(.sessoes("byte"), topo: .sessoes("byte")) == nil)
        // Da lista: abre.
        #expect(MXNavegacaoRegras.destinoDepoisDosAjustes(.sessoes("byte"), topo: nil) == .sessoes("byte"))
        #expect(MXNavegacaoRegras.destinoDepoisDosAjustes(.conversa("byte"), topo: .conversa("prosa")) == .conversa("byte"))
        #expect(MXNavegacaoRegras.destinoDepoisDosAjustes(.emBreve(.aoVivo), topo: .conversa("byte")) == .emBreve(.aoVivo))
    }

    @Test func emBreveDaEtapa3TemTextoProprio() {
        #expect(MXEmBreve.sessaoExtra.texto == catalogo("Abrir esta sessão no iPhone chega numa próxima versão. Ela continua no computador."))
        #expect(MXEmBreve.novaSessao.texto != nil && MXEmBreve.motor.texto == nil)
        #expect(MXEmBreve.arquivadas.titulo == catalogo("Arquivadas"))
    }
}

/// Nota (a) da revisão da etapa 3: na fonte real nenhum controle fica ligado se a única ação dele é abrir "Em breve";
/// fica à vista e desligado ("Em breve", valor acessível "Indisponível"). Na de exemplo, tudo como o desenho.
@MainActor
struct RedesignCapacidadesTests {
    @Test func naFonteRealNenhumControleLigadoSoLevaAEmBreve() {
        let real = MXCapacidades.real
        for controle in MXControle.todos where real.ligado(controle) {
            #expect(real.acao(controle) == .real, "ligado sem ação real: \(controle)")
        }
        // O que a fonte real tem de verdade hoje.
        let ligados = MXControle.todos.filter(real.ligado)
        // Mission Board v1 (só Debug): a aba é montada com o quadro derivado (RedesignMissionBoardTests).
        #expect(Set(ligados) == [.aba(.missionBoard), .aba(.conversas), .ajustesDoAgente, .autonomia, .fixar, .sessaoExtra])
        // Criar sessão segue "Em breve": a base não tem caminho público para `sessionCreate`.
        #expect(!real.ligado(.novaSessao))
    }

    @Test func aListaCobreTodasAsAbasEOsItensDosAjustes() {
        let todos = MXControle.todos
        #expect(Set(todos).count == todos.count)
        for aba in MXAba.allCases { #expect(todos.contains(.aba(aba))) }
        for item in MXExemplo.itensDeAjuste { #expect(todos.contains(.ajuste(item.id)), "\(item.id)") }
        #expect(todos.contains(.ajuste(.exportar)))
    }

    @Test func naFonteDeExemploTudoSegueODesenho() {
        let exemplo = MXFonteExemplo().capacidades
        #expect(MXControle.todos.allSatisfy(exemplo.ligado))
        #expect(exemplo.acao(.novaSessao) == .real && exemplo.acao(.sessaoExtra) == .real)
        #expect(exemplo.acao(.rotinas) == .emBreve) // ligado no exemplo, mas só leva a "Em breve"
    }

    @Test func menuDoAgenteNaFonteRealDesligaRotinasENovaSessao() throws {
        let dados = try #require(MXFonteExemplo().conversa("byte"))
        let itens = MXMenuRegras.itens(dados, conexao: .conectado, capacidades: .real)
        #expect(itens.map(\.controle) == [.rotinas, .ajustesDoAgente, .autonomia, .novaSessao, .fixar])
        for item in itens where item.ligado {
            #expect(MXCapacidades.real.acao(item.controle) == .real, "ligado sem ação real: \(item.titulo)")
        }
        for controle in [MXControle.rotinas, .novaSessao] {
            let item = try #require(itens.first { $0.controle == controle })
            #expect(!item.ligado && item.emBreve)
            #expect(item.valor == catalogo("Em breve") && item.valorAcessivel == catalogo("Indisponível") && item.dicaAcessivel == catalogo("Em breve"))
            #expect(item.rotuloAcessivel == item.titulo)
        }
        #expect(itens.filter(\.ligado).map(\.controle) == [.ajustesDoAgente, .autonomia, .fixar])
        #expect(itens.first { $0.controle == .novaSessao }?.divisoriaAntes == true)
    }

    @Test func menuDoAgenteNoExemploTemTudoLigado() throws {
        let dados = try #require(MXFonteExemplo().conversa("byte"))
        let itens = MXMenuRegras.itens(dados, conexao: .conectado, capacidades: .exemplo)
        #expect(itens.allSatisfy { $0.ligado && !$0.emBreve && $0.valorAcessivel == nil })
        let autonomia = try #require(itens.first { $0.controle == .autonomia })
        #expect(autonomia.valor == dados.autonomia)
        #expect(autonomia.rotuloAcessivel == catalogo("Autonomia") + ": " + (dados.autonomia ?? ""))
        // Sem conexão, só Fixar desliga (pela autoridade, não por "Em breve").
        let sem = MXMenuRegras.itens(dados, conexao: .desconectado, capacidades: .exemplo)
        let fixar = try #require(sem.first { $0.controle == .fixar })
        #expect(!fixar.ligado && !fixar.emBreve && fixar.valorAcessivel == catalogo("Indisponível sem conexão"))
    }

    @Test func aoVivoDaCaixaDesligaQuandoAFonteNaoTem() {
        let sem = MXCaixa.estado(texto: "", editando: false, esperandoDecisao: false, trabalhando: false,
                                 conexao: .conectado, nome: "Byte", aoVivo: MXCapacidades.real.ligado(.aoVivo))
        #expect(sem.botao == .aoVivo && !sem.ligado && sem.emBreve)
        let com = MXCaixa.estado(texto: "", editando: false, esperandoDecisao: false, trabalhando: false,
                                 conexao: .conectado, nome: "Byte", aoVivo: MXCapacidades.exemplo.ligado(.aoVivo))
        #expect(com.ligado && !com.emBreve)
        // Com texto, enviar não depende do ao vivo.
        let enviar = MXCaixa.estado(texto: "oi", editando: false, esperandoDecisao: false, trabalhando: false,
                                    conexao: .conectado, nome: "Byte", aoVivo: false)
        #expect(enviar.botao == .enviar && enviar.ligado && !enviar.emBreve)
    }
}

/// Regras puras de sessões no `BotStore` e exclusão só com a confirmação do computador.
struct RedesignSessoesExtrasTests {
    @Test func idDaConversaSeparaAgenteESessao() {
        let extra = MXIdDaConversa(agente: "byte", sessao: "s1")
        #expect(MXIdDaConversa(extra.texto) == extra && extra.texto != "byte")
        // A Principal (o Hub dá a ela o id do agente) é o próprio agente.
        #expect(MXIdDaConversa(agente: "byte", sessao: "byte").texto == "byte")
        #expect(MXIdDaConversa("byte") == MXIdDaConversa(agente: "byte", sessao: nil))
        #expect(MXIdDaConversa(agente: "byte", sessao: "").sessao == nil)
    }

    @Test func conversaDeUmaSessaoExtraTemOsDadosDela() throws {
        let b = try bot(["status": "working", "workingChat": "byte"])
        let entradas = [Entry(id: "x", seq: 2, botId: "byte", rev: 2, kind: "user", turn: 1, data: EntryData(text: "oi"),
                              createdAt: 2, updatedAt: 2, sessionId: "s1")]
        let dados = MXMapeamento.conversa(b, entries: entradas, computador: "Mac", motor: "Claude", respondendo: [:],
                                          agora: Date(), sessao: "s1", semEnvio: MXTextoDasSessoes.envioNaoConfirmado)
        #expect(MXIdDaConversa(dados.id) == MXIdDaConversa(agente: "byte", sessao: "s1"))
        #expect(dados.agenteId == "byte")
        #expect(dados.itens.contains { if case let .usuario(m) = $0.conteudo { m.texto == "oi" } else { false } })
        // O Hub não diz em que sessão o agente trabalha: numa extra, não promete "trabalhando" nem mostra o
        // balão "Trabalhando…" da Principal.
        #expect(!dados.trabalhando)
        #expect(!dados.itens.contains { if case .trabalhando = $0.conteudo { true } else { false } })
        let naPrincipal = MXMapeamento.conversa(b, entries: entradas, computador: "Mac", motor: "Claude", respondendo: [:],
                                                agora: Date())
        #expect(naPrincipal.itens.contains { if case .trabalhando = $0.conteudo { true } else { false } })
        #expect(dados.semEnvio == MXTextoDasSessoes.envioNaoConfirmado)
        let principal = MXMapeamento.conversa(b, entries: [], computador: "Mac", motor: "Claude", respondendo: [:], agora: Date())
        #expect(principal.id == "byte" && principal.agenteId == "byte" && principal.semEnvio == nil)
    }

    @Test func caixaSemEnvioDesligaEnviarEDizPorque() {
        let motivo = MXTextoDasSessoes.conferindoEnvio
        let comTexto = MXCaixa.estado(texto: "oi", editando: false, esperandoDecisao: false, trabalhando: false,
                                      conexao: .conectado, nome: "Byte", semEnvio: motivo)
        #expect(comTexto.botao == .enviar && !comTexto.ligado && comTexto.dica == motivo)
        let vazia = MXCaixa.estado(texto: "", editando: false, esperandoDecisao: false, trabalhando: false,
                                   conexao: .conectado, nome: "Byte", semEnvio: motivo)
        #expect(!vazia.ligado && vazia.dica == motivo)
        // Editar uma da fila não manda mensagem nova: segue pela ligação.
        let editando = MXCaixa.estado(texto: "novo", editando: true, esperandoDecisao: false, trabalhando: false,
                                      conexao: .conectado, nome: "Byte", semEnvio: motivo)
        #expect(editando.botao == .salvarEdicao && editando.ligado)
        let normal = MXCaixa.estado(texto: "oi", editando: false, esperandoDecisao: false, trabalhando: false,
                                    conexao: .conectado, nome: "Byte")
        #expect(normal.ligado && normal.dica == catalogo("Mensagem para \("Byte")"))
    }

    @Test func sessoesDaBaseViramArvoreComAHoraDaUltimaMensagem() throws {
        let json = #"""
        {"sessions":[
          {"id":"byte","chatId":"byte","principal":true,"depth":0,"title":"","state":"open","revision":1},
          {"id":"s1","chatId":"byte","principal":false,"parentSession":"byte","depth":1,"title":"Revisão","state":"open","revision":2},
          {"id":"s2","chatId":"byte","principal":false,"depth":0,"title":"Velha","state":"closed","revision":3}
        ],"sessionActions":["list","send"]}
        """#
        let base = try JSONDecoder().decode(BotSessions.self, from: Data(json.utf8))
        let resposta = MXRespostaDeSessoes(base)
        #expect(resposta.sessionActions == ["list", "send"])
        #expect(resposta.sessions.map(\.id) == ["byte", "s1", "s2"] && resposta.sessions.allSatisfy { $0.lastUsedAt == nil })
        let arvore = MXMapeamento.arvore(resposta.sessions, botId: "byte", principal: .livre, horaDaPrincipal: "09:00",
                                         agora: Date(), horas: ["s1": "08:15"])
        #expect(arvore.map(\.id) == ["byte"])
        #expect(arvore.first?.filhas.map(\.id) == ["s1"] && arvore.first?.filhas.first?.hora == "08:15")
        // Hub antigo: sem `sessionActions`.
        let antigo = try JSONDecoder().decode(BotSessions.self, from: Data(#"{"sessions":[]}"#.utf8))
        #expect(MXRespostaDeSessoes(antigo).sessionActions == nil)
    }

    /// A trava do Excluir, por agente: só o resultado do próprio pedido mexe nela (P2-1 da 3ª passada).
    @Test func exclusaoPendenteNaoSoltaComErroAlheioNemComOutroAgente() {
        var e = MXExclusoes()
        // Primeiro toque em A: sai.
        let p1 = e.pedir("a", berga: false, conectado: true)
        #expect(p1)
        #expect(e.excluindo("a") && e.estado("a") == .aCaminho)
        // Chega um erro de outra ação (uma aprovação em outra janela): não há como ele mexer na trava de A.
        // Segundo toque em A: não sai; A continua "Excluindo…".
        let p2 = e.pedir("a", berga: false, conectado: true)
        #expect(!p2)
        #expect(e.excluindo("a"))
        // O pedido de B e a falha dele não soltam A.
        let p3 = e.pedir("b", berga: false, conectado: true)
        #expect(p3)
        e.concluir("b", ok: false)
        #expect(e.excluindo("a") && !e.excluindo("b"))
        let p4 = e.pedir("a", berga: false, conectado: true)
        #expect(!p4)
        // A falha do PRÓPRIO pedido de A solta; pode pedir de novo.
        e.concluir("a", ok: false)
        #expect(!e.excluindo("a"))
        let p5 = e.pedir("a", berga: false, conectado: true)
        #expect(p5)
        // O ok fica travado até o agente sair do espelho (o evento do Hub).
        e.concluir("a", ok: true)
        let p6 = e.pedir("a", berga: false, conectado: true)
        #expect(e.estado("a") == .confirmada && !p6)
        // Um resultado atrasado não destrava o que já foi confirmado.
        e.concluir("a", ok: false)
        #expect(e.estado("a") == .confirmada)
        e.saiu("a")
        #expect(!e.excluindo("a"))
        // O Berga nunca; sem ligação, nunca.
        let p7 = e.pedir("berga", berga: true, conectado: true)
        #expect(!p7)
        let p8 = e.pedir("c", berga: false, conectado: false)
        #expect(!p8)
        #expect(MXAjustesRegras.falhaAoExcluir("Byte", motivo: nil) == catalogo("Não foi possível excluir \("Byte").") + " " + catalogo("Tente de novo."))
        // Recusa conhecida: mensagem do catálogo, nunca o texto do servidor; as demais, o fixo.
        #expect(MXAjustesRegras.falhaAoExcluir("Byte", motivo: "unknown bot")
            == catalogo("Não foi possível excluir \("Byte").") + " " + catalogo("O computador não encontrou esse bot."))
        #expect(MXAjustesRegras.falhaAoExcluir("Byte", motivo: "  Unknown bot: byte \n")
            == MXAjustesRegras.falhaAoExcluir("Byte", motivo: "unknown bot"))
        let desconhecida = MXAjustesRegras.falhaAoExcluir("Byte", motivo: "panic em /srv/hub/bots.rs (192.0.2.1)")
        #expect(desconhecida == MXAjustesRegras.falhaAoExcluir("Byte", motivo: nil) && !desconhecida.contains("panic"))
    }

    /// A resposta da exclusão só vale na ligação do toque (P2 da 4ª passada).
    @Test func exclusaoQueVoltaNoutraLigacaoSoltaETemAvisoHonesto() {
        var e = MXExclusoes()
        let p1 = e.pedir("a", berga: false, conectado: true)
        #expect(p1)
        // Um sucesso antigo, chegado depois de trocar a ligação: não vira confirmada; solta e avisa.
        let aviso = e.concluir("a", ok: true, mesmoLink: false)
        #expect(e.estado("a") == nil && !e.excluindo("a"))
        #expect(aviso == catalogo("A ligação caiu durante a exclusão. Se o computador excluiu, o bot some da lista ao reconectar."))
        // Um novo toque pode sair.
        let p2 = e.pedir("a", berga: false, conectado: true)
        #expect(p2)
        // Uma falha noutra ligação também não vira "falhou": solta com o mesmo aviso.
        #expect(e.concluir("a", ok: false, mesmoLink: false) == MXAjustesRegras.exclusaoSemLigacao && !e.excluindo("a"))
        // Na mesma ligação, como antes: ok confirma (sem aviso); falha solta (o aviso é o do computador).
        let p3 = e.pedir("a", berga: false, conectado: true)
        #expect(p3)
        #expect(e.concluir("a", ok: true, mesmoLink: true) == nil && e.estado("a") == .confirmada)
        // Já confirmada: uma resposta atrasada de outra ligação não mexe.
        #expect(e.concluir("a", ok: false, mesmoLink: false) == nil && e.estado("a") == .confirmada)
    }

    /// O anúncio de envio por sessão vale só na ligação em que foi lido (P2-2 da 3ª passada).
    @Test func envioPorSessaoSoNaLigacaoEmQueFoiLido() {
        let a = AnyHashable(1), b = AnyHashable(2)
        let leituraEmA = MXLeituraDeSessoes(geracao: a, resultado: .anunciaEnvio)
        // Em A, anunciou e o store confirma: manda.
        #expect(MXMapeamento.semEnvioNaSessao(leituraEmA, geracaoAtual: a, storeConfirma: true) == nil)
        // Caiu e voltou em B: espera nova leitura, mesmo com o store ainda dizendo que sim.
        #expect(MXMapeamento.semEnvioNaSessao(leituraEmA, geracaoAtual: b, storeConfirma: true) == catalogo("Carregando sessões…"))
        // Sem link: espera.
        #expect(MXMapeamento.semEnvioNaSessao(leituraEmA, geracaoAtual: nil, storeConfirma: true) == catalogo("Carregando sessões…"))
        // Leu em B: libera.
        let leituraEmB = MXLeituraDeSessoes(geracao: b, resultado: .anunciaEnvio)
        #expect(MXMapeamento.semEnvioNaSessao(leituraEmB, geracaoAtual: b, storeConfirma: true) == nil)
        // Leu em B sem "send", ou a leitura falhou: desligada com o motivo.
        #expect(MXMapeamento.semEnvioNaSessao(MXLeituraDeSessoes(geracao: b, resultado: .semEnvio), geracaoAtual: b, storeConfirma: false)
            == MXTextoDasSessoes.envioNaoConfirmado)
        #expect(MXMapeamento.semEnvioNaSessao(MXLeituraDeSessoes(geracao: b, resultado: .falhou), geracaoAtual: b, storeConfirma: false)
            == MXTextoDasSessoes.sessoesNaoVieram)
        // Nunca leu: espera.
        #expect(MXMapeamento.semEnvioNaSessao(nil, geracaoAtual: b, storeConfirma: true) == catalogo("Carregando sessões…"))
    }

    /// O voltar diz o destino real: a tela de baixo na pilha (P3-2 da 3ª passada).
    @Test func voltarDizParaOndeVolta() {
        let nome: (String) -> String? = { $0 == "byte" ? "Byte" : nil }
        #expect(MXNavegacaoRegras.rotuloVoltar(abaixo: nil, nomeDoAgente: nome) == catalogo("Conversas"))
        #expect(MXNavegacaoRegras.rotuloVoltar(abaixo: .sessoes("byte"), nomeDoAgente: nome) == catalogo("Sessões"))
        // A Principal aberta (pelos ajustes) de dentro de uma extra volta para a sessão, não para Conversas.
        #expect(MXNavegacaoRegras.rotuloVoltar(abaixo: .sessao(agente: "byte", sessao: "s1"), nomeDoAgente: nome) == catalogo("Sessão"))
        #expect(MXNavegacaoRegras.rotuloVoltar(abaixo: .conversa("byte"), nomeDoAgente: nome) == "Byte")
        #expect(MXNavegacaoRegras.rotuloVoltar(abaixo: .conversa("sumiu"), nomeDoAgente: nome) == catalogo("Conversa"))
    }

    @Test func excluindoMostraOEstadoNaLinha() {
        #expect(MXAjustesRegras.excluindo == catalogo("Excluindo…"))
        let d = MXAjustesDoAgente(id: "byte", nome: "Byte", funcao: "", rosto: .conjunto("developer"), estado: .livre,
                                  sincronia: "", sincronizado: true, autonomia: .pedir, itens: [], avisos: true,
                                  podeExportar: false, podeExcluir: true, excluindo: true)
        #expect(d.excluindo && d.motivoSemExcluir == nil)
    }
}

/// Etapa 3 (Sessões, sessão aberta e Ajustes do agente): os textos novos vêm do catálogo do CodyncUI, com tradução de
/// verdade em pt-BR e em inglês (mesmo padrão de `RedesignTextosEmDoisIdiomasTests`).
struct RedesignSessoesTextosEmDoisIdiomasTests {
    private func bundle(_ idioma: String) throws -> Bundle {
        let caminho = try #require(ConnectionSummary.resourceBundle.path(forResource: idioma, ofType: "lproj"))
        return try #require(Bundle(path: caminho))
    }

    private static let ausente = "\u{1}ausente\u{1}"

    /// Chaves que as telas da etapa 3 usam (as novas e as que o catálogo já tinha). Sem chaves só de formato: o Xcode não
    /// gera símbolo para elas.
    private static let chaves: [String] = [
        "%lld conector",
        "%lld conectores",
        "%lld skill",
        "%lld skills",
        "%lld sessão",
        "%lld sessões",
        "%lld sessão dentro",
        "%lld sessões dentro",
        "A ligação caiu durante a exclusão. Se o computador excluiu, o bot some da lista ao reconectar.",
        "Abre a sessão",
        "Abrir",
        "Abrir esta sessão no iPhone chega numa próxima versão. Ela continua no computador.",
        "Age sem pedir e avisa.",
        "Agora",
        "Arquivada: %@",
        "Arquivadas",
        "Arquivadas, %lld",
        "As mesmas configurações do app do computador. O que você muda aqui vale lá na hora.",
        "As outras sessões aparecem quando o computador estiver conectado.",
        "Avisos deste bot",
        "Buscar nas sessões",
        "Carregando sessões…",
        "Comportamento",
        "Configurações",
        "Conversar",
        "Criar sessões pelo iPhone chega numa próxima versão. Por enquanto, crie no computador.",
        "Desfazer",
        "Desfazer: %@ volta para as sessões",
        "Espera sua aprovação",
        "Este computador ainda não mostra as outras sessões no iPhone.",
        "Este computador não confirmou mensagens nesta sessão. Mande pelo computador.",
        "Exclua no computador",
        "Excluindo…",
        "Excluir bot",
        "Exportar bot",
        "Falar por voz",
        "Fechar configurações",
        "Ferramentas",
        "Identidade",
        "Indisponível",
        "Indisponível no iPhone",
        "Mostrar as sessões de dentro",
        "Motor",
        "Nenhuma",
        "Nenhuma sessão com “%@”.",
        "Nome, função e aparência",
        "Não foi possível carregar as sessões. Volte e abra de novo.",
        "Não foi possível carregar esta sessão. Tente de novo.",
        "Não foi possível excluir %@.",
        "Não foi possível mudar a autonomia de %@. Tente de novo.",
        "Não foi possível mudar os avisos de %@. Tente de novo.",
        "O bot e as conversas dele saem deste computador. Não dá para desfazer.",
        "Paradas",
        "Pede confirmação",
        "Principal",
        "Pronto",
        "Recolher",
        "Sem conexão com o %@",
        "Sessão",
        "Sessão sem nome",
        "Sessões",
        "Sincronizado com o %@ · agora",
        "Só quando precisar de você",
        "Tente de novo.",
        "Total",
        "Voltar para %@",
        "aberta",
        "recolhida",
        "computador",
        "em %@",
        "Pedir",
        "Liberar",
        "Em breve",
        "Nova sessão",
        "Autonomia",
        "Excluir",
        "Cancelar",
        "Dispensar aviso",
        "Ajustes do bot",
        "Excluir %@?",
        "Indisponível sem conexão",
        "Trabalhando",
        "Precisa de você",
    ]

    /// Iguais nos dois idiomas por serem a mesma palavra.
    private static let iguaisNosDoisIdiomas: Set<String> = ["%lld skill", "%lld skills"]

    @Test func chavesDaEtapa3TemPortuguesEIngles() throws {
        for idioma in ["pt-BR", "en"] {
            let b = try bundle(idioma)
            for chave in Self.chaves {
                let texto = NSLocalizedString(chave, bundle: b, value: Self.ausente, comment: "")
                #expect(texto != Self.ausente, "\(idioma): falta a chave \(chave)")
                if idioma == "pt-BR" {
                    #expect(texto == chave, "pt-BR: \(chave)")
                } else if !Self.iguaisNosDoisIdiomas.contains(chave) {
                    #expect(texto != chave, "en igual ao pt-BR (sem tradução): \(chave)")
                }
            }
        }
    }

    @Test func pluraisEInterpolacoesDaEtapa3SaemCertosNosDoisIdiomas() throws {
        let pt = try bundle("pt-BR")
        let en = try bundle("en")
        func formatar(_ chave: String, _ b: Bundle, _ args: CVarArg...) -> String {
            String(format: NSLocalizedString(chave, bundle: b, comment: ""), arguments: args)
        }
        // Pares singular/plural explícitos, escolhidos pelo número no código.
        #expect(formatar("%lld sessão", pt, 1) == "1 sessão" && formatar("%lld sessão", en, 1) == "1 session")
        #expect(formatar("%lld sessões", pt, 17) == "17 sessões" && formatar("%lld sessões", en, 17) == "17 sessions")
        #expect(formatar("%lld sessão dentro", pt, 1) == "1 sessão dentro" && formatar("%lld sessão dentro", en, 1) == "1 session inside")
        #expect(formatar("%lld sessões dentro", pt, 4) == "4 sessões dentro" && formatar("%lld sessões dentro", en, 4) == "4 sessions inside")
        #expect(formatar("%lld conector", en, 1) == "1 connector" && formatar("%lld conectores", en, 4) == "4 connectors")
        #expect(formatar("%lld skill", en, 1) == "1 skill" && formatar("%lld skills", en, 3) == "3 skills")
        // Mesma ordem dos argumentos nas duas línguas.
        #expect(formatar("Sincronizado com o %@ · agora", pt, "MacBook Pro") == "Sincronizado com o MacBook Pro · agora")
        #expect(formatar("Sincronizado com o %@ · agora", en, "MacBook Pro") == "Synced with MacBook Pro · now")
        #expect(formatar("Sem conexão com o %@", en, "MacBook Pro") == "No connection to MacBook Pro")
        #expect(formatar("Não foi possível excluir %@.", en, "Byte") == "Couldn't delete Byte.")
        #expect(formatar("Não foi possível mudar os avisos de %@. Tente de novo.", en, "Byte") == "Couldn't change the notifications of Byte. Try again.")
        #expect(formatar("Desfazer: %@ volta para as sessões", en, "Notas") == "Undo: Notas goes back to the sessions")
        #expect(formatar("Nenhuma sessão com “%@”.", pt, "x") == "Nenhuma sessão com “x”." && formatar("Nenhuma sessão com “%@”.", en, "x") == "No session matching “x”.")
        #expect(formatar("Arquivadas, %lld", en, 3) == "Archived, 3" && formatar("Voltar para %@", en, "Sessions") == "Back to Sessions")
        #expect(formatar("Excluir %@?", en, "Byte") == "Delete Byte?")
        #expect(formatar("em %@", pt, "Revisão") == "em Revisão" && formatar("em %@", en, "Revisão") == "in Revisão")
    }
}
