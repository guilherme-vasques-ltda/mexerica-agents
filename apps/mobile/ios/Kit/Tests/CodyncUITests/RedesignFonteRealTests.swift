import Foundation
import Testing
@testable import CodyncKit
@testable import CodyncUI

/// Etapa 2: a fonte real do redesenho sobre um `BotStore` de verdade, com o transporte falso dos testes do store
/// (`FakeRemote`) e a chamada da fila ao Hub trocada por uma falsa (payloads codificados, conflito, erro).

@MainActor
private func fonteReal(_ estado: MXEstadoConexao? = .conectado) throws -> (MXFonteReal, BotStore) {
    let suite = "RedesignFonteReal.\(UUID().uuidString)"
    // Conta única por chamada, como nas suítes irmãs: o arquivo de espelho em disco leva conta e computador no nome
    // (a `suite` não entra), e um store que nasce com cache alheio já vem "com histórico".
    let storage = SharedStore.Context(accountID: UUID().uuidString, suite: suite)
    let chave = Data(repeating: 7, count: 32)
    let codificada = chave.base64EncodedString().replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    let computador = Computer(id: RelayCrypto.computerId(signKey: chave), name: "Mac", signKey: codificada)
    let store = BotStore(computer: computador, storage: storage) { _ in FakeRemote(.connecting) }
    store.bots["byte"] = try JSONDecoder().decode(Bot.self, from: Data(#"{"id":"byte","name":"Byte","status":"working","rev":1}"#.utf8))
    let fonte = MXFonteReal(store: store)
    fonte.conexaoForcada = estado
    return (fonte, store)
}

private func entradaNaFila(_ id: String, seq: Int64, rev: Int64, texto: String, status: String = "queued") -> Entry {
    Entry(id: id, seq: seq, botId: "byte", rev: rev, kind: "user", turn: 2, data: EntryData(text: texto, status: status),
          createdAt: 1, updatedAt: 1)
}

@MainActor
struct RedesignFonteRealTests {
    @Test func storeSemLinkNoArNaoEstaConectado() throws {
        let (fonte, store) = try fonteReal(nil)
        // O transporte falso nunca sobe: o Hub não se identificou, o store não tem autoridade e a fonte não deixa agir.
        #expect(!store.temAutoridade && store.client == nil)
        #expect(!fonte.conexao.podeAgir)
        #expect(fonte.clienteComAutoridade() == nil)
        // Antes do primeiro `hello` o store está reconectando, e a faixa só depois da carência.
        let reconectando = if case .reconectando = fonte.conexao { true } else { false }
        #expect(reconectando)
        #expect(fonte.faixaDeQueda == store.faixaDeQueda)
    }

    @Test func pedidosDaFilaVaoCodificadosComoOHubEspera() async throws {
        let (fonte, store) = try fonteReal()
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "antes"))
        var recebidos: [(String, [String: Any])] = []
        fonte.chamarFila = { metodo, pedido in
            let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(pedido)) as? [String: Any] ?? [:]
            recebidos.append((metodo, json))
            return MXRespostaDaFila(entries: [entradaNaFila("q1", seq: 10, rev: 6, texto: "depois")])
        }
        let item = MXItemDaFila(id: "q1", texto: "antes", revisao: 5)
        #expect(await fonte.editar(item, texto: "depois", revisao: 5, em: "byte")?.value == true)
        #expect(recebidos.first?.0 == "queuedEdit")
        #expect(recebidos.first?.1["botId"] as? String == "byte")
        #expect(recebidos.first?.1["entryId"] as? String == "q1")
        #expect(recebidos.first?.1["text"] as? String == "depois")
        #expect(recebidos.first?.1["expectedRevision"] as? Int == 5)
        // A resposta do Hub entra no espelho na hora.
        #expect(fonte.conversa("byte")?.fila.first?.texto == "depois")
        // Mandar agora e Remover não levam texto nem revisão.
        let geracao = try #require(fonte.geracaoDoLink())
        #expect(await fonte.acaoNaFila("queuedSendNow", MXPedidoDaFila(botId: "byte", entryId: "q1"), geracao: geracao))
        #expect(recebidos.last?.1["text"] == nil && recebidos.last?.1["expectedRevision"] == nil)
    }

    @Test func conflitoDeRevisaoViraAvisoENaoMudaAFila() async throws {
        let (fonte, store) = try fonteReal()
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "antes"))
        fonte.chamarFila = { _, _ in throw HostError.http(409, "queued_revision_conflict: a mensagem mudou; releia a fila") }
        let ok = await fonte.editar(MXItemDaFila(id: "q1", texto: "antes", revisao: 5), texto: "depois", revisao: 5, em: "byte")?.value
        #expect(ok == false)
        #expect(fonte.aviso == catalogo("A fila mudou enquanto isso. Confira e tente de novo."))
        #expect(fonte.conversa("byte")?.fila.first?.texto == "antes")
    }

    @Test func semConexaoNadaSaiParaOHub() async throws {
        let (fonte, store) = try fonteReal(.reconectando(desde: Date()))
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "antes"))
        var chamadas = 0
        fonte.chamarFila = { _, _ in chamadas += 1; return MXRespostaDaFila(entries: nil) }
        // Sem conexão no toque: o salvar nem sai (nada fica reservado).
        #expect(fonte.editar(MXItemDaFila(id: "q1", texto: "antes", revisao: 5), texto: "x", revisao: 5, em: "byte") == nil)
        #expect(fonte.aviso == MXTextoDeQueda.semAcao)
        #expect(!fonte.salvando("q1", em: "byte"))
        fonte.mandarAgora(MXItemDaFila(id: "q1", texto: "antes", revisao: 5), em: "byte")
        try await Task.sleep(for: .milliseconds(50))
        fonte.enviar("oi", para: "byte")
        #expect(fonte.aviso == MXTextoDeQueda.semEnvio)
        fonte.responder(MXPedidoDeAprovacao(id: "p1", titulo: "", detalhe: nil, acao: "", onde: "", opcoes: [],
                                            resultado: .pendente), opcao: "o", em: "byte")
        #expect(fonte.aviso == MXTextoDeQueda.semAprovar)
        #expect(chamadas == 0)
        // O envio não entrou como balão local: a pessoa pede de novo depois de reconectar.
        #expect(!store.chat("byte").contains { $0.id.hasPrefix("local-") })
    }

    @Test func duasAcoesAoMesmoTempoChegamUmaVezCadaAoHub() async throws {
        let (fonte, store) = try fonteReal()
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "a"))
        store.upsert(entradaNaFila("q2", seq: 11, rev: 6, texto: "b"))
        var metodos: [String] = []
        fonte.chamarFila = { metodo, pedido in
            metodos.append("\(metodo):\(pedido.entryId)")
            try await Task.sleep(for: .milliseconds(10))
            return MXRespostaDaFila(entries: [entradaNaFila(pedido.entryId, seq: 10, rev: 9, texto: "x", status: "cancelled")])
        }
        let geracao = try #require(fonte.geracaoDoLink())
        // Duas tarefas no ator principal (a geração não é Sendable), intercaladas no `sleep` da chamada falsa.
        let um = Task { await fonte.acaoNaFila("queuedRemove", MXPedidoDaFila(botId: "byte", entryId: "q1"), geracao: geracao) }
        let dois = Task { await fonte.acaoNaFila("queuedSendNow", MXPedidoDaFila(botId: "byte", entryId: "q2"), geracao: geracao) }
        let (a, b) = await (um.value, dois.value)
        #expect(a && b)
        #expect(Set(metodos) == ["queuedRemove:q1", "queuedSendNow:q2"])
        #expect(fonte.conversa("byte")?.fila.isEmpty == true)
    }

    @Test func historicoAnteriorVemDoStore() throws {
        let (fonte, store) = try fonteReal()
        #expect(!fonte.podeCarregarAnteriores("byte")) // nada carregado ainda
        store.upsert(entradaNaFila("m1", seq: 40, rev: 1, texto: "oi", status: "sent"))
        #expect(fonte.podeCarregarAnteriores("byte") == store.canLoadOlder("byte"))
        #expect(fonte.podeCarregarAnteriores("byte"))
        store.historyComplete.insert("byte")
        #expect(!fonte.podeCarregarAnteriores("byte"))
    }

    @Test func leituraUsaOMesmoRegistroDaConversaAtual() throws {
        let (fonte, store) = try fonteReal()
        let token = UUID()
        fonte.lendo("byte", token: token, ativo: true)
        #expect(store.readingViews[token] == "byte")
        fonte.lendo("byte", token: token, ativo: false)
        #expect(store.readingViews[token] == nil)
    }

    @Test func fixarQueOHubRecusaVoltaAtras() async throws {
        let (fonte, store) = try fonteReal()
        fonte.atualizarBot = { _ in throw HostError.http(500, "falhou") }
        fonte.alternarFixar("byte")
        #expect(store.bots["byte"]?.pinned == true) // aparece na hora
        for _ in 0..<50 where store.bots["byte"]?.pinned == true { try await Task.sleep(for: .milliseconds(10)) }
        #expect(store.bots["byte"]?.pinned == false)
        #expect(fonte.aviso == catalogo("Não foi possível fixar \("Byte"). Tente de novo."))
    }

    @Test func fixarQueValeFicaComOQueOHubDevolveu() async throws {
        let (fonte, store) = try fonteReal()
        var enviado: MXPatchDoBot?
        fonte.atualizarBot = { patch in
            enviado = patch
            var bot = try #require(store.bots["byte"])
            bot.pinned = true
            return bot
        }
        fonte.alternarFixar("byte")
        for _ in 0..<50 where enviado == nil { try await Task.sleep(for: .milliseconds(10)) }
        // Só o id e o campo (o Hub aplica como patch).
        #expect(enviado == MXPatchDoBot(id: "byte", pinned: true))
        #expect(store.bots["byte"]?.pinned == true)
    }

    @Test func linkQueCaiEVoltaDuranteAChamadaViraFalhaSemAplicarNada() async throws {
        let (fonte, store) = try fonteReal()
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "antes"))
        fonte.chamarFila = { _, _ in
            fonte.geracaoForcada += 1 // o link caiu e voltou enquanto esperava a resposta
            return MXRespostaDaFila(entries: [entradaNaFila("q1", seq: 10, rev: 6, texto: "depois")])
        }
        let ok = await fonte.editar(MXItemDaFila(id: "q1", texto: "antes", revisao: 5), texto: "depois", revisao: 5, em: "byte")?.value
        #expect(ok == false)
        #expect(fonte.aviso == MXTextoDeQueda.linkMudou)
        #expect(fonte.conversa("byte")?.fila.first?.texto == "antes") // a resposta não entrou no espelho
    }

    @Test func toqueNumLinkQueJaFoiTrocadoNaoSai() async throws {
        let (fonte, store) = try fonteReal()
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "antes"))
        var chamadas = 0
        fonte.chamarFila = { _, _ in chamadas += 1; return MXRespostaDaFila(entries: nil) }
        let doToque = try #require(fonte.geracaoDoLink())
        fonte.geracaoForcada += 1 // trocou entre o toque e o despacho
        #expect(!(await fonte.acaoNaFila("queuedRemove", MXPedidoDaFila(botId: "byte", entryId: "q1"), geracao: doToque)))
        #expect(chamadas == 0)
        #expect(fonte.aviso == MXTextoDeQueda.linkMudou)
    }

    @Test func salvarLevaAGeracaoDoToqueAteODespacho() async throws {
        let (fonte, store) = try fonteReal()
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "antes"))
        var chamadas = 0
        fonte.chamarFila = { _, _ in chamadas += 1; return MXRespostaDaFila(entries: nil) }
        // O toque guarda a geração; o link cai e volta antes de a tarefa começar a rodar.
        let tarefa = try #require(fonte.editar(MXItemDaFila(id: "q1", texto: "antes", revisao: 5), texto: "x", revisao: 5, em: "byte"))
        fonte.geracaoForcada += 1
        #expect(await tarefa.value == false)
        #expect(chamadas == 0) // não adotou o link novo
        #expect(fonte.aviso == MXTextoDeQueda.linkMudou)
        #expect(!fonte.salvando("q1", em: "byte"))
    }

    @Test func linkQueCaiAntesDoDespachoESemConexaoNaoLinkMudou() async throws {
        let (fonte, store) = try fonteReal()
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "antes"))
        var chamadas = 0
        fonte.chamarFila = { _, _ in chamadas += 1; return MXRespostaDaFila(entries: nil) }
        let tarefa = try #require(fonte.editar(MXItemDaFila(id: "q1", texto: "antes", revisao: 5), texto: "x", revisao: 5, em: "byte"))
        fonte.conexaoForcada = .reconectando(desde: Date()) // caiu e ainda não voltou
        #expect(await tarefa.value == false)
        #expect(chamadas == 0)
        #expect(fonte.aviso == MXTextoDeQueda.semAcao)
    }

    @Test func reservaDoSalvarEDaFonteESobreviveAReabrirAConversa() async throws {
        let (fonte, store) = try fonteReal()
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "antes"))
        store.upsert(entradaNaFila("q2", seq: 11, rev: 6, texto: "outra"))
        var chamadas = 0
        var liberar: CheckedContinuation<Void, Never>?
        fonte.chamarFila = { _, _ in
            chamadas += 1
            await withCheckedContinuation { liberar = $0 }
            return MXRespostaDaFila(entries: nil)
        }
        let q1 = MXItemDaFila(id: "q1", texto: "antes", revisao: 5)
        let a = try #require(fonte.editar(q1, texto: "A", revisao: 5, em: "byte"))
        for _ in 0..<50 where liberar == nil { try await Task.sleep(for: .milliseconds(10)) }
        // A tela foi embora e outra abriu a mesma conversa: ela pergunta à mesma fonte, e B não sai.
        #expect(fonte.salvando("q1", em: "byte"))
        #expect(fonte.editar(q1, texto: "B", revisao: 5, em: "byte") == nil)
        // Outra mensagem da fila não fica presa.
        #expect(!fonte.salvando("q2", em: "byte"))
        #expect(chamadas == 1)
        liberar?.resume()
        #expect(await a.value)
        #expect(!fonte.salvando("q1", em: "byte"))
    }

    @Test func erroDaFilaComOLinkTrocadoAvisaDaLigacao() async throws {
        let (fonte, store) = try fonteReal()
        store.upsert(entradaNaFila("q1", seq: 10, rev: 5, texto: "antes"))
        fonte.chamarFila = { _, _ in
            fonte.geracaoForcada += 1 // o link caiu e voltou, e a chamada falhou
            throw HostError.unreachable
        }
        let geracao = try #require(fonte.geracaoDoLink())
        #expect(!(await fonte.acaoNaFila("queuedRemove", MXPedidoDaFila(botId: "byte", entryId: "q1"), geracao: geracao)))
        #expect(fonte.aviso == MXTextoDeQueda.linkMudou) // e não "Não foi possível mudar a fila."
    }

    @Test func fixarQueFalhaComOLinkTrocadoAvisaDaLigacao() async throws {
        let (fonte, store) = try fonteReal()
        var chamou = false
        fonte.atualizarBot = { _ in
            chamou = true
            fonte.geracaoForcada += 1
            throw HostError.unreachable
        }
        fonte.alternarFixar("byte")
        for _ in 0..<50 where !chamou || store.bots["byte"]?.pinned == true { try await Task.sleep(for: .milliseconds(10)) }
        #expect(store.bots["byte"]?.pinned == false) // voltou atrás
        #expect(fonte.aviso == MXTextoDeQueda.linkMudou)
    }

    @Test func sessoesVemDoHubEAsExtrasAbremAConversaDelas() async throws {
        let (fonte, _) = try fonteReal()
        // Antes de ler: só a Principal, com a nota.
        let antes = try #require(fonte.sessoes("byte"))
        #expect(antes.sessoes.map(\.id) == ["byte"] && antes.nota == MXTextoDasSessoes.esperandoOComputador)
        #expect(antes.arquivadas == nil && !antes.podeArquivar && !antes.podeCriar)
        var pedidos: [String] = []
        fonte.lerSessoes = { id in
            pedidos.append(id)
            return MXRespostaDeSessoes(sessions: [
                MXSessaoDoHub(id: "byte", principal: true, parentSession: nil, title: "", state: "open", lastUsedAt: 1),
                MXSessaoDoHub(id: "s1", principal: false, parentSession: nil, title: "Revisão", state: "open", lastUsedAt: 2),
                MXSessaoDoHub(id: "s2", principal: false, parentSession: nil, title: "Velha", state: "closed", lastUsedAt: 3),
            ])
        }
        await fonte.carregarSessoes("byte")
        #expect(pedidos == ["byte"])
        let depois = try #require(fonte.sessoes("byte"))
        #expect(depois.sessoes.map(\.nome) == [catalogo("Principal"), "Revisão"] && depois.nota == nil)
        // A Principal abre a Conversa de sempre; uma extra abre a conversa dela.
        #expect(fonte.conversaDaSessao("byte", em: "byte") == "byte")
        let s1 = try #require(fonte.conversaDaSessao("s1", em: "byte"))
        #expect(MXIdDaConversa(s1) == MXIdDaConversa(agente: "byte", sessao: "s1"))
        #expect(fonte.conversa(s1)?.agenteId == "byte")
        // Criar sessão: a base não tem caminho público ("Em breve").
        #expect(fonte.novaSessao(em: "byte") == nil)
    }

    /// 07b numa sessão extra: o histórico é o DELA, o envio vai para ELA e só com o envio por sessão confirmado pelo
    /// computador nesta ligação (nunca para a Principal achando que foi para a extra).
    @Test func sessaoExtraMostraOHistoricoDelaEMandaParaEla() async throws {
        let (fonte, store) = try fonteReal()
        let s1 = MXIdDaConversa(agente: "byte", sessao: "s1").texto
        store.entries["byte"] = [
            Entry(id: "p", seq: 1, botId: "byte", rev: 1, kind: "user", turn: 1, data: EntryData(text: "da principal"),
                  createdAt: 1, updatedAt: 1),
            Entry(id: "x", seq: 2, botId: "byte", rev: 2, kind: "user", turn: 1, data: EntryData(text: "da extra"),
                  createdAt: 2, updatedAt: 2, sessionId: "s1"),
        ]
        func textos(_ id: String) -> [String] {
            (fonte.conversa(id)?.itens ?? []).compactMap { item -> String? in
                if case let .usuario(m) = item.conteudo { return m.texto }
                return nil
            }
        }
        #expect(textos(s1) == ["da extra"])
        #expect(textos("byte") == ["da principal"])
        // Antes de listar as sessões nesta ligação: a caixa fica desligada, conferindo; nada sai.
        #expect(fonte.conversa(s1)?.semEnvio == MXTextoDasSessoes.conferindoEnvio)
        fonte.enviar("oi", para: s1)
        #expect(store.chat("byte", sessionId: "s1").count == 1 && fonte.aviso == MXTextoDasSessoes.conferindoEnvio)
        // O Hub listou sem anunciar "send" (Hub antigo): continua desligada, com o motivo.
        fonte.lerSessoes = { _ in MXRespostaDeSessoes(sessions: [], sessionActions: ["list"]) }
        var historico: [(String, String, Int64)] = []
        fonte.lerHistoricoDaSessao = { agente, sessao, antes in historico.append((agente, sessao, antes)); return [] }
        await fonte.abrindo(s1)
        #expect(historico.count == 1 && historico.first?.1 == "s1" && historico.first?.2 == Int64.max)
        #expect(fonte.conversa(s1)?.semEnvio == MXTextoDasSessoes.envioNaoConfirmado)
        #expect(!fonte.podeCarregarAnteriores(s1)) // a página veio curta: acabou
        // Com o envio por sessão anunciado nesta ligação (e guardado pelo store): libera.
        fonte.lerSessoes = { _ in MXRespostaDeSessoes(sessions: [], sessionActions: ["list", "send"]) }
        store.sessionSendSupported.insert("byte")
        await fonte.abrindo(s1)
        #expect(fonte.conversa(s1)?.semEnvio == nil)
        // A ligação caiu e voltou noutra (outra geração): espera nova leitura, mesmo com o store ainda dizendo que sim.
        fonte.geracaoForcada += 1
        #expect(fonte.conversa(s1)?.semEnvio == MXTextoDasSessoes.conferindoEnvio)
        await fonte.abrindo(s1)
        #expect(fonte.conversa(s1)?.semEnvio == nil)
        // A mensagem vai para a extra, não para a Principal.
        fonte.limparAviso()
        fonte.enviar("para a extra", para: s1)
        #expect(store.chat("byte", sessionId: "s1").contains { $0.data.text == "para a extra" })
        #expect(!store.chat("byte").contains { $0.data.text == "para a extra" })
        // O rascunho da extra é dela.
        fonte.guardarRascunho("meio", em: s1)
        #expect(fonte.rascunho(s1) == "meio" && fonte.rascunho("byte") != "meio")
    }

    /// A leitura de sessões é uma operação com identidade. A tela Sessões (A) e a abertura de uma extra (B)
    /// leem o mesmo agente; B confirma o envio e A termina DEPOIS com erro: a resposta velha não derruba a vigente.
    @Test func leituraAntigaComErroNaoDerrubaALeituraMaisNovaQueConfirmouOEnvio() async throws {
        let (fonte, store) = try fonteReal()
        let s1 = MXIdDaConversa(agente: "byte", sessao: "s1").texto
        var pendentes: [CheckedContinuation<MXRespostaDeSessoes, Error>] = []
        fonte.lerSessoes = { _ in try await withCheckedThrowingContinuation { pendentes.append($0) } }
        fonte.lerHistoricoDaSessao = { _, _, _ in [] }
        let tarefaA = Task { await fonte.carregarSessoes("byte") }
        for _ in 0..<50 where pendentes.count < 1 { try await Task.sleep(for: .milliseconds(10)) }
        let tarefaB = Task { await fonte.abrindo(s1) }
        for _ in 0..<50 where pendentes.count < 2 { try await Task.sleep(for: .milliseconds(10)) }
        #expect(pendentes.count == 2)
        // B (a mais nova) confirma o envio; o store guarda (o gancho não passa por ele).
        store.sessionSendSupported.insert("byte")
        pendentes[1].resume(returning: MXRespostaDeSessoes(sessions: [], sessionActions: ["list", "send"]))
        await tarefaB.value
        #expect(fonte.conversa(s1)?.semEnvio == nil)
        // A (a mais velha) termina depois com erro: não substitui a vigente.
        pendentes[0].resume(throwing: HostError.unreachable)
        await tarefaA.value
        #expect(fonte.conversa(s1)?.semEnvio == nil)
        #expect(fonte.leiturasDeSessoes["byte"]?.resultado == .anunciaEnvio && !fonte.sessoesFalharam.contains("byte"))
    }

    /// A mesma ordem com sucesso velho: a resposta antiga, mesmo boa, não troca a mais nova; e o store só guarda o envio
    /// por sessão da leitura vigente.
    @Test func leituraAntigaComSucessoNaoTrocaALeituraMaisNova() async throws {
        let (fonte, store) = try fonteReal()
        var pendentes: [CheckedContinuation<MXRespostaDeSessoes, Error>] = []
        fonte.lerSessoes = { _ in try await withCheckedThrowingContinuation { pendentes.append($0) } }
        let tarefaA = Task { await fonte.carregarSessoes("byte") }
        for _ in 0..<50 where pendentes.count < 1 { try await Task.sleep(for: .milliseconds(10)) }
        let tarefaB = Task { await fonte.carregarSessoes("byte") }
        for _ in 0..<50 where pendentes.count < 2 { try await Task.sleep(for: .milliseconds(10)) }
        pendentes[1].resume(throwing: HostError.http(400, "unknown method"))
        await tarefaB.value
        #expect(fonte.leiturasDeSessoes["byte"]?.resultado == .falhou)
        // A velha chega boa depois da falha da nova: a vigente (a falha) fica.
        pendentes[0].resume(returning: MXRespostaDeSessoes(sessions: [], sessionActions: ["list", "send"]))
        await tarefaA.value
        #expect(fonte.leiturasDeSessoes["byte"]?.resultado == .falhou && fonte.sessoesFalharam.contains("byte"))
        // No store: a ordem aplicada é a da mais nova; a velha não mexe no envio por sessão.
        let velha = store.novaLeituraDeSessoes("byte")
        let nova = store.novaLeituraDeSessoes("byte")
        #expect(store.aplicarLeituraDeSessoes("byte", ordem: nova))
        #expect(!store.aplicarLeituraDeSessoes("byte", ordem: velha))
        #expect(store.aplicarLeituraDeSessoes("byte", ordem: nova))
    }

    /// Cancelar a leitura não é falha do computador: nada muda no que já se sabia.
    @Test func leituraCanceladaNaoViraFalha() async throws {
        let (fonte, _) = try fonteReal()
        var pendentes: [CheckedContinuation<MXRespostaDeSessoes, Error>] = []
        fonte.lerSessoes = { _ in try await withCheckedThrowingContinuation { pendentes.append($0) } }
        let tarefa = Task { await fonte.carregarSessoes("byte") }
        for _ in 0..<50 where pendentes.isEmpty { try await Task.sleep(for: .milliseconds(10)) }
        pendentes[0].resume(throwing: CancellationError())
        await tarefa.value
        #expect(fonte.leiturasDeSessoes["byte"] == nil && !fonte.sessoesFalharam.contains("byte"))
    }

    /// Entradas da sessão `s1` do Byte com `seq` no intervalo (para as páginas de histórico dos testes de fim).
    private func entradasDaSessao(_ seqs: ClosedRange<Int64>) -> [Entry] {
        seqs.map { seq in
            Entry(id: "h\(seq)", seq: seq, botId: "byte", rev: 1, kind: "user", turn: 1, data: EntryData(text: "m\(seq)"),
                  createdAt: seq, updatedAt: seq, sessionId: "s1")
        }
    }

    /// P2: o fim do histórico da sessão se decide pelo número BRUTO de linhas da página, não pelas entradas que sobraram.
    /// Uma página bruta cheia (100) que perdeu uma entrada (99 devolvidas) NÃO marca o fim: a página anterior é buscada e
    /// aceita; só uma página bruta curta (< 100) marca o fim.
    @Test func paginaBrutaCheiaQuePerdeuUmaEntradaNaoMarcaOFimDaSessao() async throws {
        let (fonte, store) = try fonteReal()
        let s1 = MXIdDaConversa(agente: "byte", sessao: "s1").texto
        fonte.lerSessoes = { _ in MXRespostaDeSessoes(sessions: [], sessionActions: ["list", "send"]) }
        var antesPedidos: [Int64] = []
        fonte.lerPaginaDaSessao = { _, _, antes in
            antesPedidos.append(antes)
            // 1ª página: 100 linhas brutas, 99 entradas (1 descartada). 2ª: 40 linhas, as mais antigas.
            let pagina = antes == .max ? self.entradasDaSessao(102...200) : self.entradasDaSessao(62...101)
            store.entries["byte", default: []] += pagina
            return BotStore.PaginaDeHistorico(entries: pagina, rawCount: antes == .max ? 100 : 40)
        }
        await fonte.abrindo(s1)
        #expect(antesPedidos == [Int64.max])
        #expect(fonte.historicoAcabou(s1) == false && fonte.podeCarregarAnteriores(s1))
        // A página anterior é buscada a partir da entrada mais antiga que já temos e aceita.
        #expect(await fonte.carregarAnteriores(s1))
        #expect(antesPedidos == [Int64.max, 102])
        #expect(store.chat("byte", sessionId: "s1").contains { $0.seq == 62 } && store.chat("byte", sessionId: "s1").count == 139)
        // Só agora (linhas brutas < 100) o fim fica marcado.
        #expect(fonte.historicoAcabou(s1) && !fonte.podeCarregarAnteriores(s1))
    }

    /// O adaptador antigo (`lerHistoricoDaSessao`, só com as entradas) segue valendo: sem número bruto, conta as entradas.
    @Test func adaptadorAntigoDoHistoricoContaAsEntradasDevolvidas() async throws {
        let (fonte, store) = try fonteReal()
        let s1 = MXIdDaConversa(agente: "byte", sessao: "s1").texto
        fonte.lerSessoes = { _ in MXRespostaDeSessoes(sessions: [], sessionActions: ["list", "send"]) }
        fonte.lerHistoricoDaSessao = { _, _, _ in
            let pagina = self.entradasDaSessao(101...200)
            store.entries["byte", default: []] += pagina
            return pagina
        }
        await fonte.abrindo(s1)
        #expect(!fonte.historicoAcabou(s1) && fonte.podeCarregarAnteriores(s1)) // 100 entradas: cheia
        fonte.lerHistoricoDaSessao = { _, _, _ in [] }
        #expect(await fonte.carregarAnteriores(s1))
        #expect(fonte.historicoAcabou(s1)) // 0 entradas: curta
    }

    /// P2: o fim visto só vale no link em que foi visto e com evidência. Reconectar não herda o fim antigo; página bruta
    /// cheia remove o fim; uma leitura que termina depois de o link mudar não registra nada.
    @Test func fimDoHistoricoDaSessaoSoValeNoLinkEmQueFoiVisto() async throws {
        let (fonte, store) = try fonteReal()
        let s1 = MXIdDaConversa(agente: "byte", sessao: "s1").texto
        fonte.lerSessoes = { _ in MXRespostaDeSessoes(sessions: [], sessionActions: ["list", "send"]) }
        var bruto = 3
        var trocarLinkNoMeio = false
        fonte.lerPaginaDaSessao = { _, _, _ in
            let pagina = self.entradasDaSessao(1...3)
            store.entries["byte"] = pagina
            if trocarLinkNoMeio { store.linkId += 1 }
            return BotStore.PaginaDeHistorico(entries: pagina, rawCount: bruto)
        }
        // Página curta: fim marcado neste link.
        await fonte.abrindo(s1)
        #expect(fonte.historicoAcabou(s1) && !fonte.podeCarregarAnteriores(s1))
        // Reconectou (outra marca do link): o fim antigo não vale; é preciso reler a página mais nova.
        store.linkId += 1
        #expect(!fonte.historicoAcabou(s1) && fonte.podeCarregarAnteriores(s1))
        // Releitura com página bruta cheia: o fim some de vez (não volta sozinho se a marca voltasse igual).
        bruto = 100
        await fonte.abrindo(s1)
        #expect(fonte.historicoCompletoDaSessao[s1] == nil && fonte.podeCarregarAnteriores(s1))
        // Releitura curta no link de agora reafirma o fim.
        bruto = 3
        await fonte.abrindo(s1)
        #expect(fonte.historicoAcabou(s1))
        // Página lida num link que mudou durante a chamada não registra fim (nem remove nada pela leitura velha).
        store.linkId += 1
        trocarLinkNoMeio = true
        await fonte.abrindo(s1)
        #expect(!fonte.historicoAcabou(s1) && fonte.podeCarregarAnteriores(s1))
    }

    /// Uma tarefa por agente espera a resposta do próprio `deleteBot`: erro alheio não solta a trava; a falha do pedido
    /// solta e avisa; o ok segue travado até o evento do Hub tirar o agente (o store limpa).
    @Test func excluirTemTravaPorAgentePresaAoProprioPedido() async throws {
        let (fonte, store) = try fonteReal()
        var pedidos: [String] = []
        var responder: CheckedContinuation<Void, Error>?
        fonte.excluirNoHub = { id in
            pedidos.append(id)
            try await withCheckedThrowingContinuation { responder = $0 }
        }
        #expect(fonte.ajustes("byte")?.motivoSemExcluir == nil)
        #expect(fonte.excluirAgente("byte"))
        for _ in 0..<50 where responder == nil { try await Task.sleep(for: .milliseconds(10)) }
        #expect(pedidos == ["byte"] && fonte.ajustes("byte")?.excluindo == true)
        // Um erro de outra ação chega ao store: a trava de Byte não solta e o segundo toque não sai.
        store.lastError = "Outra janela"
        #expect(fonte.ajustes("byte")?.excluindo == true)
        #expect(!fonte.excluirAgente("byte") && pedidos.count == 1)
        fonte.limparAviso()
        #expect(fonte.ajustes("byte")?.excluindo == true && !fonte.excluirAgente("byte"))
        // O computador recusou ESTE pedido: a trava solta, o agente fica e o aviso diz por quê.
        responder?.resume(throwing: HostError.http(400, "unknown bot"))
        for _ in 0..<50 where fonte.ajustes("byte")?.excluindo == true { try await Task.sleep(for: .milliseconds(10)) }
        #expect(fonte.ajustes("byte")?.excluindo == false && store.bots["byte"] != nil)
        #expect(fonte.aviso == catalogo("Não foi possível excluir \("Byte").") + " " + catalogo("O computador não encontrou esse bot.")
            && !(fonte.aviso ?? "").contains("unknown"))
        // De novo, agora com ok: segue "Excluindo…" até o evento do Hub; o store tira o agente.
        responder = nil
        fonte.limparAviso()
        #expect(fonte.excluirAgente("byte"))
        for _ in 0..<50 where responder == nil { try await Task.sleep(for: .milliseconds(10)) }
        responder?.resume()
        for _ in 0..<50 where fonte.exclusoes.estado("byte") != .confirmada { try await Task.sleep(for: .milliseconds(10)) }
        #expect(fonte.ajustes("byte")?.excluindo == true && !fonte.excluirAgente("byte") && pedidos.count == 2)
        store.bots["byte"] = nil
        #expect(fonte.ajustes("byte") == nil)
        // A ligação muda durante a chamada: o sucesso antigo não vira confirmada; a trava solta e o aviso é honesto.
        store.bots["trato"] = try JSONDecoder().decode(Bot.self, from: Data(#"{"id":"trato","name":"Trato","rev":1}"#.utf8))
        responder = nil
        #expect(fonte.excluirAgente("trato"))
        for _ in 0..<50 where responder == nil { try await Task.sleep(for: .milliseconds(10)) }
        fonte.geracaoForcada += 1
        responder?.resume()
        for _ in 0..<50 where fonte.ajustes("trato")?.excluindo == true { try await Task.sleep(for: .milliseconds(10)) }
        #expect(fonte.exclusoes.estado("trato") == nil && fonte.ajustes("trato")?.excluindo == false)
        #expect(fonte.aviso == MXAjustesRegras.exclusaoSemLigacao && store.bots["trato"] != nil)
        // Um novo toque pode sair (se o computador já excluiu, ele só responde "unknown bot").
        responder = nil
        #expect(fonte.excluirAgente("trato"))
        // O gancho só roda quando o teste cede o ator: a contagem é conferida depois de o pedido chegar a ele.
        for _ in 0..<50 where responder == nil { try await Task.sleep(for: .milliseconds(10)) }
        #expect(pedidos.count == 4)
        responder?.resume(throwing: HostError.http(400, "unknown bot"))
        // A tarefa termina antes de seguir (nada dela corre por baixo das conferências seguintes).
        for _ in 0..<50 where fonte.ajustes("trato")?.excluindo == true { try await Task.sleep(for: .milliseconds(10)) }
        #expect(fonte.ajustes("trato")?.excluindo == false && fonte.aviso == catalogo("Não foi possível excluir \("Trato").") + " " + catalogo("O computador não encontrou esse bot.")
            && !(fonte.aviso ?? "").contains("unknown"))
        // Sem ligação, nada sai; o Berga não se exclui.
        store.bots["prosa"] = try JSONDecoder().decode(Bot.self, from: Data(#"{"id":"prosa","name":"Prosa","rev":1}"#.utf8))
        fonte.conexaoForcada = .desconectado
        #expect(!fonte.excluirAgente("prosa") && pedidos.count == 4)
        fonte.conexaoForcada = .conectado
        store.bots["berga"] = try JSONDecoder().decode(Bot.self, from: Data(#"{"id":"berga","name":"Berga","rev":1}"#.utf8))
        #expect(fonte.ajustes("berga")?.podeExcluir == false && !fonte.excluirAgente("berga"))
    }

    @Test func sessoesSemLigacaoNaoPedemEFalhaDeixaANota() async throws {
        let (fonte, _) = try fonteReal(.reconectando(desde: Date()))
        var pedidos = 0
        fonte.lerSessoes = { _ in pedidos += 1; return MXRespostaDeSessoes(sessions: []) }
        await fonte.carregarSessoes("byte")
        #expect(pedidos == 0)
        fonte.conexaoForcada = .conectado
        fonte.lerSessoes = { _ in throw HostError.http(400, "unknown method") }
        await fonte.carregarSessoes("byte")
        #expect(fonte.sessoes("byte")?.nota == MXTextoDasSessoes.semSessoesNoHub)
    }

    @Test func autonomiaVaiComoEscolhaDaPessoa() async throws {
        let (fonte, store) = try fonteReal()
        store.bots["byte"]?.permission = "ask" // o Bot lê a ausência de permission como "auto"; aqui o agente começa em Pedir
        var enviado: MXPatchDoBot?
        fonte.atualizarBot = { patch in
            enviado = patch
            var bot = try #require(store.bots["byte"])
            bot.permission = patch.permission ?? bot.permission
            return bot
        }
        #expect(fonte.ajustes("byte")?.autonomia == .pedir)
        fonte.mudarAutonomia(.liberar, em: "byte")
        #expect(store.bots["byte"]?.permission == "auto") // aparece na hora
        for _ in 0..<50 where enviado == nil { try await Task.sleep(for: .milliseconds(10)) }
        #expect(enviado == MXPatchDoBot(id: "byte", permission: "auto"))
        #expect(enviado?.permissionChosenByUser == true)
        // Total não existe no Hub: nada sai.
        enviado = nil
        fonte.mudarAutonomia(.total, em: "byte")
        try await Task.sleep(for: .milliseconds(30))
        #expect(enviado == nil)
    }

    @Test func avisosQueOHubRecusaVoltamAtras() async throws {
        let (fonte, store) = try fonteReal()
        fonte.atualizarBot = { _ in throw HostError.http(500, "falhou") }
        #expect(fonte.ajustes("byte")?.avisos == true)
        fonte.alternarAvisos(em: "byte")
        #expect(store.bots["byte"]?.notify == false)
        for _ in 0..<50 where store.bots["byte"]?.notify == false { try await Task.sleep(for: .milliseconds(10)) }
        #expect(store.bots["byte"]?.notify != false)
        #expect(fonte.aviso == catalogo("Não foi possível mudar os avisos de \("Byte"). Tente de novo."))
    }

    @Test func avisosVaoSoComOCampoMesmoSeOEspelhoMudarNoMeio() async throws {
        let (fonte, store) = try fonteReal()
        var enviado: MXPatchDoBot?
        fonte.atualizarBot = { patch in
            // No meio da chamada chega um evento mais novo do bot (o Mac mudou os conectores).
            store.bots["byte"]?.connectors = ["novo"]
            store.bots["byte"]?.rev = 9
            enviado = patch
            var bot = try #require(store.bots["byte"])
            bot.notify = patch.notify
            return bot
        }
        fonte.alternarAvisos(em: "byte")
        for _ in 0..<50 where enviado == nil { try await Task.sleep(for: .milliseconds(10)) }
        try await Task.sleep(for: .milliseconds(20))
        // O pedido não leva nada do espelho daqui (nem os conectores velhos, nem o resto do bot).
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(try #require(enviado))) as? [String: Any]
        #expect(json?.keys.sorted() == ["id", "notify"])
        #expect(json?["notify"] as? Bool == false)
        // O que o Mac mudou fica.
        #expect(store.bots["byte"]?.connectors == ["novo"] && store.bots["byte"]?.notify == false)
    }

    @Test func oQueAFonteRealNaoTemFicaDesligado() throws {
        let (fonte, _) = try fonteReal()
        let ajustes = try #require(fonte.ajustes("byte"))
        #expect(!ajustes.podeLigar && !ajustes.podeExportar)
        #expect(ajustes.itens.allSatisfy { !$0.disponivel })
        let sessoes = try #require(fonte.sessoes("byte"))
        #expect(!sessoes.podeLigar && !sessoes.podeCriar)
        // Tudo vem das capacidades da fonte (nenhum controle ligado só para abrir "Em breve").
        #expect(fonte.capacidades == .real)
        #expect(ajustes.itens.allSatisfy { $0.disponivel == fonte.capacidades.ligado(.ajuste($0.id)) })
        #expect(fonte.conversaDaSessao("byte", em: "byte") == "byte")
        #expect(fonte.conversaDaSessao("outra", em: "byte") != nil && fonte.capacidades.ligado(.sessaoExtra))
        let menu = MXMenuRegras.itens(try #require(fonte.conversa("byte")), conexao: .conectado, capacidades: fonte.capacidades)
        #expect(menu.filter(\.ligado).map(\.controle) == [.ajustesDoAgente, .autonomia, .fixar])
    }

    @Test func storeSemLinkNoArNaoTemGeracao() throws {
        let (fonte, _) = try fonteReal(nil)
        #expect(fonte.geracaoDoLink() == nil) // `FakeRemote(.connecting)`: sem autoridade, sem geração
    }

    @Test func fixarEUmPorVezPorBotERespostaVelhaNaoEntra() async throws {
        let (fonte, store) = try fonteReal()
        var chamadas = 0
        var liberar: CheckedContinuation<Void, Never>?
        fonte.atualizarBot = { rascunho in
            chamadas += 1
            await withCheckedContinuation { liberar = $0 }
            var velho = try #require(store.bots["byte"])
            velho.pinned = rascunho.pinned ?? false
            velho.rev = 1 // mais velho que o evento que chega no meio
            return velho
        }
        fonte.alternarFixar("byte")
        for _ in 0..<50 where liberar == nil { try await Task.sleep(for: .milliseconds(10)) }
        // Segundo toque enquanto o primeiro está a caminho: não sai.
        fonte.alternarFixar("byte")
        #expect(chamadas == 1)
        #expect(fonte.aviso == MXTextoDeQueda.fixarACaminho)
        // Chega um evento mais novo do bot (desafixado em outro aparelho).
        store.bots["byte"]?.rev = 7
        store.bots["byte"]?.pinned = false
        liberar?.resume()
        try await Task.sleep(for: .milliseconds(50))
        #expect(store.bots["byte"]?.pinned == false && store.bots["byte"]?.rev == 7) // a resposta velha não sobrescreveu
    }

    @Test func historicoSemLigacaoFalhaNaHora() async throws {
        let (fonte, store) = try fonteReal(.reconectando(desde: Date()))
        store.upsert(entradaNaFila("m1", seq: 40, rev: 1, texto: "oi", status: "sent"))
        #expect(!(await fonte.carregarAnteriores("byte")))
    }
}
