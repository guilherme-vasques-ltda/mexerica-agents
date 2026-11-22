import Foundation
import SwiftUI
import Testing
@testable import CodyncKit
@testable import CodyncUI

/// Etapa 2 do redesenho iOS v1: as decisões da Conversa, da fila, do cartão de aprovação, da busca e do
/// mapeamento dos dados reais para as telas.

private let calendario: Calendar = {
    var c = Calendar(identifier: .gregorian)
    c.timeZone = TimeZone(identifier: "America/Sao_Paulo")!
    return c
}()

private func data(_ ano: Int, _ mes: Int, _ dia: Int, _ hora: Int = 12, _ minuto: Int = 0) -> Date {
    calendario.date(from: DateComponents(year: ano, month: mes, day: dia, hour: hora, minute: minuto))!
}

private func ms(_ d: Date) -> Int64 { Int64(d.timeIntervalSince1970 * 1000) }

private func bot(_ campos: [String: Any]) throws -> Bot {
    var json: [String: Any] = ["id": "byte", "name": "Byte", "backend": "claude", "cwd": "/Users/user/pace", "permission": "ask"]
    json.merge(campos) { _, novo in novo }
    return try JSONDecoder().decode(Bot.self, from: JSONSerialization.data(withJSONObject: json))
}

private func entrada(_ id: String, seq: Int64, kind: String, turn: Int64 = 1, quando: Date, rev: Int64 = 1,
                     _ ajuste: (inout EntryData) -> Void = { _ in }) -> Entry {
    var d = EntryData()
    ajuste(&d)
    return Entry(id: id, seq: seq, botId: "byte", rev: rev, kind: kind, turn: turn, data: d,
                 createdAt: ms(quando), updatedAt: ms(quando))
}

private let fila3 = [
    MXItemDaFila(id: "a", texto: "Depois roda o lint também", revisao: 4),
    MXItemDaFila(id: "b", texto: "E atualiza o README", revisao: 7),
    MXItemDaFila(id: "c", texto: "Abre um PR", revisao: 2),
]

struct RedesignCaixaTests {
    @Test func semTextoEhAoVivo() {
        let caixa = MXCaixa.estado(texto: "  ", editando: false, esperandoDecisao: false, trabalhando: false,
                                   conexao: .conectado, nome: "Byte")
        #expect(caixa.botao == .aoVivo)
        #expect(caixa.ligado)
        #expect(caixa.dica == catalogo("Mensagem para \("Byte")"))
        #expect(caixa.rotulo == catalogo("Falar ao vivo com \("Byte")"))
    }

    @Test func comTextoViraEnviarEAvisaQueEntraNaFila() {
        let livre = MXCaixa.estado(texto: "oi", editando: false, esperandoDecisao: false, trabalhando: false,
                                   conexao: .conectado, nome: "Byte")
        #expect(livre.botao == .enviar && livre.ligado && livre.rotulo == catalogo("Enviar"))
        let trabalhando = MXCaixa.estado(texto: "oi", editando: false, esperandoDecisao: false, trabalhando: true,
                                         conexao: .conectado, nome: "Byte")
        #expect(trabalhando.botao == .enviar)
        #expect(trabalhando.rotulo == catalogo("Enviar (\("Byte") está trabalhando: entra na fila)"))
    }

    @Test func editandoViraConfirmarELigaSoComTexto() {
        let comTexto = MXCaixa.estado(texto: "novo", editando: true, esperandoDecisao: false, trabalhando: true,
                                      conexao: .conectado, nome: "Byte")
        #expect(comTexto.botao == .salvarEdicao && comTexto.ligado)
        #expect(comTexto.rotulo == catalogo("Salvar a mensagem da fila"))
        let vazio = MXCaixa.estado(texto: "", editando: true, esperandoDecisao: false, trabalhando: true,
                                   conexao: .conectado, nome: "Byte")
        #expect(vazio.botao == .salvarEdicao && !vazio.ligado)
    }

    @Test func esperandoDecisaoMostraPararEADica() {
        let caixa = MXCaixa.estado(texto: "", editando: false, esperandoDecisao: true, trabalhando: true,
                                   conexao: .conectado, nome: "Byte")
        #expect(caixa.botao == .parar && caixa.ligado)
        #expect(caixa.dica == catalogo("Entra depois da sua decisão"))
        // Escrevendo, vira enviar (a mensagem entra na fila depois da decisão).
        let escrevendo = MXCaixa.estado(texto: "e mais isso", editando: false, esperandoDecisao: true, trabalhando: true,
                                        conexao: .conectado, nome: "Byte")
        #expect(escrevendo.botao == .enviar && escrevendo.ligado)
    }

    @Test(arguments: [MXEstadoConexao.reconectando(desde: Date(timeIntervalSince1970: 0)), .desconectado])
    func semConexaoEnviarEAprovarFicamVisiveisEDesligados(conexao: MXEstadoConexao) {
        #expect(!conexao.podeAgir)
        let enviar = MXCaixa.estado(texto: "oi", editando: false, esperandoDecisao: false, trabalhando: false,
                                    conexao: conexao, nome: "Byte")
        #expect(enviar.botao == .enviar && !enviar.ligado)
        #expect(enviar.dica == MXTextoDeQueda.semEnvio)
        let salvar = MXCaixa.estado(texto: "oi", editando: true, esperandoDecisao: false, trabalhando: true,
                                    conexao: conexao, nome: "Byte")
        #expect(salvar.botao == .salvarEdicao && !salvar.ligado)
        let parar = MXCaixa.estado(texto: "", editando: false, esperandoDecisao: true, trabalhando: true,
                                   conexao: conexao, nome: "Byte")
        #expect(parar.botao == .parar && !parar.ligado)
        #expect(MXEstadoConexao.conectado.podeAgir)
    }

    @Test func textosDaQuedaIguaisAosDoDesktop() {
        #expect(MXTextoDeQueda.titulo == catalogo("Sem conexão — reconectando…"))
        #expect(MXTextoDeQueda.semAprovar == catalogo("Sem conexão: permitir e recusar voltam quando reconectar."))
    }
}

struct RedesignFilaTests {
    @Test func visivelERolaDepoisDeDuas() {
        #expect(!MXFilaRegras.visivel([]))
        #expect(MXFilaRegras.visivel(Array(fila3.prefix(1))))
        #expect(!MXFilaRegras.rola(Array(fila3.prefix(2))))
        #expect(MXFilaRegras.rola(fila3))
        #expect(MXFilaRegras.titulo(fila3) == catalogo("Na fila · \(3)"))
        #expect(MXFilaRegras.resumoRecolhida(fila3) == "· Depois roda o lint também")
        #expect(MXFilaRegras.resumoRecolhida([]) == nil)
    }

    @Test func medidasDoDesenho() {
        #expect(MX.Conversa.filaLinha == 31)
        #expect(MX.Conversa.filaCabecalho == 22)
        #expect(MX.TipoDaConversa.filaLinha.tamanho == 12)
        #expect(MX.Conversa.filaLinhasVisiveis == 2)
        #expect(MX.Conversa.filaRecuo == 22) // um pouco mais estreita que a caixa
        #expect(MX.Conversa.filaEsmaecer == 14)
    }

    @Test func posicaoETextoDeEdicao() {
        #expect(MXFilaRegras.posicao("b", em: fila3) == 2)
        #expect(MXFilaRegras.posicao("z", em: fila3) == nil)
        #expect(MXFilaRegras.textoEditando(posicao: 2) == catalogo("Editando a \(2)ª da fila"))
    }

    @Test func editarLevaOTextoParaACaixaECancelarDevolveORascunho() {
        var edicao = MXEdicaoDaFila()
        #expect(!edicao.ativa)
        let caixa = edicao.comecar(fila3[1], caixa: "meu rascunho")
        #expect(caixa == "E atualiza o README")
        #expect(edicao.ativa && edicao.item?.id == "b")
        // Trocar de mensagem no meio mantém o rascunho original.
        _ = edicao.comecar(fila3[2], caixa: "E atualiza o README editado")
        #expect(edicao.item?.id == "c")
        #expect(edicao.encerrar() == "meu rascunho")
        #expect(!edicao.ativa)
    }

    @Test func mensagemQueSaiuDaFilaEncerraAEdicao() {
        var edicao = MXEdicaoDaFila()
        _ = edicao.comecar(fila3[1], caixa: "rascunho")
        #expect(edicao.filaMudou(fila3) == nil)
        #expect(edicao.ativa)
        #expect(edicao.filaMudou([fila3[0], fila3[2]]) == "rascunho")
        #expect(!edicao.ativa)
    }

    @Test func umSalvarPorVezEORespostaSoEncerraSeACaixaNaoMudou() {
        var edicao = MXEdicaoDaFila()
        _ = edicao.comecar(fila3[1], caixa: "rascunho")
        let a = edicao.iniciarSalvar("A")
        #expect(a != nil)
        #expect(edicao.iniciarSalvar("A") == nil) // segundo ✓: nada sai
        #expect(MXCaixa.estado(texto: "A", editando: true, esperandoDecisao: false, trabalhando: true, conexao: .conectado,
                               nome: "Byte", salvando: true).ligado == false)
        // A pessoa continuou digitando ("AB") enquanto o Hub respondia: a edição continua, com "A" como base.
        #expect(edicao.concluirSalvar(a!, ok: true, caixa: "AB") == nil)
        #expect(edicao.ativa && edicao.item?.texto == "A" && edicao.salvando == nil)
        // Agora salva "AB" e a caixa não mudou: encerra e devolve o rascunho.
        let ab = edicao.iniciarSalvar("AB")!
        #expect(edicao.concluirSalvar(ab, ok: true, caixa: "AB") == "rascunho")
        #expect(!edicao.ativa)
    }

    @Test func reabrirAEdicaoNaoLiberaUmSegundoSalvarDaMesmaMensagem() {
        var edicao = MXEdicaoDaFila()
        // A reserva mora na fonte (aqui, o mesmo valor que ela guarda), por agente e mensagem.
        var salvamentos = MXSalvamentosPendentes()
        _ = edicao.comecar(fila3[1], caixa: "rascunho")
        // 1ª tentativa: salvar A da mensagem "b" do Byte.
        let primeira = salvamentos.iniciar("b", em: "byte")
        #expect(primeira)
        let a = edicao.iniciarSalvar("A")
        #expect(a != nil)
        // A pessoa cancela e reabre a mesma mensagem enquanto A está a caminho: a edição nova zera o seu `salvando`…
        _ = edicao.encerrar()
        _ = edicao.comecar(fila3[1], caixa: "rascunho")
        #expect(edicao.salvando == nil)
        // …mas a reserva segura: não reabre nem sai um 2º salvar.
        #expect(salvamentos.pendente("b", em: "byte"))
        let segunda = salvamentos.iniciar("b", em: "byte")
        #expect(!segunda)
        #expect(MXCaixa.estado(texto: "B", editando: true, esperandoDecisao: false, trabalhando: true, conexao: .conectado,
                               nome: "Byte", salvando: salvamentos.pendente("b", em: "byte")).ligado == false)
        // Outra mensagem, ou a mesma id em outro agente, não fica presa.
        let outra = salvamentos.iniciar("c", em: "byte")
        #expect(outra)
        #expect(!salvamentos.pendente("b", em: "pixel"))
        // A volta: A não encerra a edição nova (geração diferente) e libera a mensagem para o próximo salvar.
        salvamentos.concluir("b", em: "byte")
        #expect(edicao.concluirSalvar(a!, ok: true, caixa: "A") == nil)
        #expect(edicao.ativa)
        let depois = salvamentos.iniciar("b", em: "byte")
        #expect(depois)
    }

    @Test func erroAoSalvarMantemAEdicaoERespostaAntigaNaoMexeEmEdicaoNova() {
        var edicao = MXEdicaoDaFila()
        _ = edicao.comecar(fila3[1], caixa: "rascunho")
        let s = edicao.iniciarSalvar("A")!
        #expect(edicao.concluirSalvar(s, ok: false, caixa: "A") == nil)
        #expect(edicao.ativa && edicao.item == fila3[1]) // intacta
        // Salvar, cancelar e começar outra edição: a resposta do salvar antigo não encerra a nova.
        let velho = edicao.iniciarSalvar("A")!
        _ = edicao.encerrar()
        _ = edicao.comecar(fila3[2], caixa: "outro")
        #expect(edicao.concluirSalvar(velho, ok: true, caixa: "A") == nil)
        #expect(edicao.item?.id == "c")
    }

    @Test func mandarAgoraSoEncerraAEdicaoQuandoAMensagemSaiDaFila() {
        var edicao = MXEdicaoDaFila()
        _ = edicao.comecar(fila3[1], caixa: "rascunho")
        // O Hub recusou: a mensagem continua na fila e a edição também.
        #expect(edicao.filaMudou(fila3) == nil)
        #expect(edicao.ativa)
        // O espelho confirmou a saída: aí a edição acaba e o rascunho volta.
        #expect(edicao.filaMudou([fila3[0], fila3[2]]) == "rascunho")
    }

    @Test func revisaoDoSalvar() {
        let original = fila3[1]
        // Só a ordem mudou (texto igual): vale a revisão atual.
        let reordenada = [MXItemDaFila(id: "b", texto: original.texto, revisao: 9), fila3[0]]
        #expect(MXFilaRegras.revisaoParaSalvar(original, fila: reordenada) == 9)
        // O texto mudou em outro aparelho: vai a revisão antiga (o Hub recusa com conflito).
        let mudada = [MXItemDaFila(id: "b", texto: "outro", revisao: 9)]
        #expect(MXFilaRegras.revisaoParaSalvar(original, fila: mudada) == 7)
    }

    @Test(arguments: [
        ("queued_entry_dispatched: a mensagem já saiu da fila", catalogo("Essa mensagem já saiu da fila.")),
        ("queued_entry_not_found", catalogo("Essa mensagem já saiu da fila.")),
        ("queued_revision_conflict: a mensagem mudou; releia a fila", catalogo("A fila mudou enquanto isso. Confira e tente de novo.")),
        ("not_allowed: este aparelho não tem a autorização do dono", catalogo("Este aparelho não pode mais mexer na fila.")),
        ("queued_control_pending", catalogo("O computador não confirmou a tempo. Confira a fila.")),
        ("Can't reach your computer.", catalogo("Não foi possível mudar a fila.")),
    ])
    func falhaDoHubViraFraseSemOTextoCru(mensagem: String, frase: String) {
        #expect(MXFilaRegras.falha(mensagem) == frase)
    }
}

struct RedesignSegurarTests {
    @Test func segurarDuraOMesmoQueNoMac() {
        #expect(MX.Movimento.segurarParaPermitir == 0.9)
    }

    @Test func progressoCresceDeZeroAUm() {
        let inicio = Date(timeIntervalSince1970: 100)
        #expect(MXSegurarRegras.progresso(desde: nil, agora: inicio) == 0)
        #expect(MXSegurarRegras.progresso(desde: inicio, agora: inicio) == 0)
        #expect(abs(MXSegurarRegras.progresso(desde: inicio, agora: inicio.addingTimeInterval(0.45)) - 0.5) < 1e-6)
        #expect(MXSegurarRegras.progresso(desde: inicio, agora: inicio.addingTimeInterval(5)) == 1)
        #expect(MXSegurarRegras.progresso(desde: inicio, agora: inicio.addingTimeInterval(-1)) == 0)
    }

    @Test func soltarAntesCancela() {
        #expect(!MXSegurarRegras.confirma(segurouPor: 0.5))
        #expect(!MXSegurarRegras.confirma(segurouPor: 0.899))
        #expect(MXSegurarRegras.confirma(segurouPor: 0.9))
    }

    @Test func permitirSempreExigeSegurarMesmoComReduzirMovimento() {
        #expect(MXSegurarRegras.modo(reduzirMovimento: false) == .segurarAnimado)
        #expect(MXSegurarRegras.modo(reduzirMovimento: true) == .segurarParado) // segura igual, só sem animação
        for modo in [MXModoDePermitir.segurarAnimado, .segurarParado] {
            #expect(MXSegurarRegras.dica(modo) == catalogo("Segure para permitir; soltar antes cancela."))
            #expect(MXSegurarRegras.dicaAcessivel(modo) == catalogo("Ative para pedir a confirmação."))
        }
        #expect(MXSegurarRegras.rotuloSegurando == catalogo("Segure…"))
    }

    @Test func tecnologiaAssistivaPedeConfirmacaoEmVezDeAprovar() {
        #expect(!MXSegurarRegras.ativacaoAcessivelAprovaDireto)
        #expect(MXSegurarRegras.pergunta("Permitir uma vez") == "Permitir uma vez?")
        #expect(MXSegurarRegras.pergunta("Sempre permitir") == "Sempre permitir?")
    }
}

struct RedesignHoraEBuscaTests {
    @Test func horaNaLista() {
        let agora = data(2026, 11, 11, 10)
        #expect(MXHora.naLista(data(2026, 11, 11, 9, 12), agora: agora, calendario: calendario) == "09:12")
        #expect(MXHora.naLista(data(2026, 11, 10, 23, 59), agora: agora, calendario: calendario) == catalogo("Ontem"))
        #expect(MXHora.naLista(data(2026, 11, 8), agora: agora, calendario: calendario) == "8 " + catalogo("nov"))
        #expect(MXHora.naLista(data(2026, 10, 28), agora: agora, calendario: calendario) == "28 " + catalogo("out"))
        #expect(MXHora.naLista(data(2025, 3, 2), agora: agora, calendario: calendario) == "2 " + catalogo("mar") + " 2025")
    }

    @Test func chipEntreMensagens() {
        let agora = data(2026, 11, 11, 20)
        #expect(MXHora.separador(data(2026, 11, 11, 19, 22), agora: agora, calendario: calendario) == catalogo("Hoje \("19:22")"))
        #expect(MXHora.separador(data(2026, 11, 10, 8, 5), agora: agora, calendario: calendario) == catalogo("Ontem \("08:05")"))
        #expect(MXHora.separador(data(2026, 11, 8, 14, 0), agora: agora, calendario: calendario) == "8 " + catalogo("nov") + " 14:00")
    }

    @Test func buscaFiltraPorNomeOuFuncaoSemAcento() {
        let todos = MXExemplo.agentes
        #expect(MXConversasRegras.filtrar(todos, busca: "").count == todos.count)
        #expect(MXConversasRegras.filtrar(todos, busca: "byte").map(\.id) == ["byte"])
        #expect(MXConversasRegras.filtrar(todos, busca: "  PROSA ").map(\.id) == ["prosa"])
        #expect(MXConversasRegras.filtrar(todos, busca: "e-commerce").map(\.id) == ["vitrine"])
        #expect(MXConversasRegras.filtrar(todos, busca: "financeiro").map(\.id) == ["saldo"])
        #expect(MXConversasRegras.filtrar(todos, busca: "xyz").isEmpty)
        let canais = MXExemplo.canais
        #expect(MXConversasRegras.canaisVisiveis(canais, abertos: false, busca: "").isEmpty)
        #expect(MXConversasRegras.canaisVisiveis(canais, abertos: true, busca: "").count == 3)
        // A busca acha o canal mesmo com Canais recolhido, e ignora o acento.
        #expect(MXConversasRegras.canaisVisiveis(canais, abertos: false, busca: "lancamento").map(\.nome) == ["lançamento"])
        #expect(MXConversasRegras.semResultado(" xyz ") == catalogo("Nada encontrado para “\("xyz")”."))
    }

    @Test func descricaoSemFuncaoNaoDeixaVirgulaSobrando() {
        let a = MXAgenteResumo(id: "x", nome: "Byte", funcao: "", rosto: .conjunto("developer"), estado: .livre,
                               hora: "09:12", previa: "Oi", tom: .normal, sessoesRodando: 0, sessoesEsperando: 0, naoLidas: 0)
        #expect(MXConversasRegras.descricao(a) == "Byte, 09:12, Oi")
    }
}

struct RedesignMapeamentoTests {
    @Test func bergaEhOBotDoNomeReservado() throws {
        #expect(MXMapeamento.ehBerga(try bot(["id": "b", "name": "Berga"])))
        #expect(!MXMapeamento.ehBerga(try bot(["name": "Byte"])))
        #expect(!MXMapeamento.ehBerga(try bot(["id": "g", "name": "Berga", "kind": "group"])))
    }

    @Test func previaETom() throws {
        let trabalhando = try bot(["status": "working", "activity": "Lendo as avaliações…"])
        #expect(MXMapeamento.previa(trabalhando, aprovacaoPendente: false).texto == "Lendo as avaliações…")
        #expect(MXMapeamento.previa(trabalhando, aprovacaoPendente: false).tom == .ativo)
        let esperando = try bot(["status": "needsInput"])
        #expect(MXMapeamento.previa(esperando, aprovacaoPendente: true).texto == catalogo("Precisa da sua aprovação"))
        #expect(MXMapeamento.previa(esperando, aprovacaoPendente: true).tom == .alerta)
        let livre = try bot(["lastMessage": "## Mandei a **proposta**\nsegunda linha"])
        #expect(MXMapeamento.previa(livre, aprovacaoPendente: false).texto == "Mandei a proposta")
        #expect(MXMapeamento.previa(livre, aprovacaoPendente: false).tom == .normal)
    }

    @Test func linhaDoAgenteLevaAFuncaoDoHubEOcultaOQueAindaNaoVem() throws {
        let agora = data(2026, 11, 11, 10)
        let b = try bot(["status": "working", "unread": 2, "lastAt": ms(data(2026, 11, 11, 9, 5)), "title": " Dev "])
        let linha = MXMapeamento.agente(b, aprovacaoPendente: false, agora: agora, calendario: calendario)
        #expect(linha.hora == "09:05")
        #expect(linha.naoLidas == 2)
        #expect(linha.funcao == "Dev") // `Bot.title` do Hub, sem espaços nas pontas
        #expect(MXMapeamento.funcaoDoHub(b) == linha.funcao)
        // Hub antigo (sem `title`): vazia, a etiqueta fica oculta.
        #expect(MXMapeamento.funcaoDoHub(try bot([:])).isEmpty)
        #expect(linha.sessoesRodando == 0 && linha.sessoesEsperando == 0) // contagem por sessão: o Hub ainda não manda
        #expect(linha.estado == .trabalhando)
    }

    @Test func autonomiaEModelo() throws {
        #expect(MXMapeamento.autonomia(try bot(["permission": "ask"])) == catalogo("Pedir"))
        #expect(MXMapeamento.autonomia(try bot(["permission": "auto"])) == catalogo("Liberar"))
        #expect(MXMapeamento.modelo(motor: "Claude Code", modelo: "opus") == "Claude Code · opus")
        #expect(MXMapeamento.modelo(motor: "Claude Code", modelo: nil) == "Claude Code")
        #expect(MXMapeamento.modelo(motor: "", modelo: "") == nil)
    }

    @Test func filaSaoAsMensagensQueuedNaOrdemDeChegada() {
        let t = data(2026, 11, 11, 19)
        let entries = [
            entrada("u2", seq: 12, kind: "user", quando: t, rev: 30) { $0.text = "segunda"; $0.status = "queued" },
            entrada("u1", seq: 10, kind: "user", quando: t, rev: 20) { $0.text = "primeira"; $0.status = "queued" },
            entrada("u0", seq: 5, kind: "user", quando: t) { $0.text = "já foi"; $0.status = "sent" },
        ]
        let fila = MXMapeamento.fila(entries)
        #expect(fila.map(\.id) == ["u1", "u2"])
        #expect(fila.map(\.revisao) == [20, 30])
        #expect(fila.first?.texto == "primeira")
    }

    @Test func ordemDaFilaDoHubComSeqDeReserva() throws {
        let t = data(2026, 11, 11, 19)
        func fila(_ ordem: [String: Int64]) -> [Entry] {
            ["a", "b", "c"].enumerated().map { i, id in
                entrada(id, seq: 10 + Int64(i), kind: "user", quando: t) {
                    $0.text = id; $0.status = "queued"; $0.queueOrder = ordem[id]
                }
            }
        }
        // Hub antigo (sem `queueOrder`): seq como reserva e a ordem não é confiável (sem posição numerada).
        let reserva = MXMapeamento.ordenarFila(fila([:]))
        #expect(reserva.itens.map(\.id) == ["a", "b", "c"] && !reserva.confiavel)
        #expect(MXMapeamento.ordemDoHub(fila([:])[0]) == nil)
        // Com a ordem do Hub (reordenada no Mac: c foi para a frente): segue o Hub e é confiável.
        let hub = fila(["a": 11, "b": 12, "c": 10])
        #expect(MXMapeamento.ordemDoHub(hub[2]) == 10) // `EntryData.queueOrder`
        let reordenada = MXMapeamento.ordenarFila(hub)
        #expect(reordenada.itens.map(\.id) == ["c", "a", "b"] && reordenada.confiavel)
        // A mesma ordem de `Entry.ordenadaPelaFila(_:)`.
        #expect(reordenada.itens.map(\.id) == Entry.ordenadaPelaFila(hub).map(\.id))
        // Empate na chave: desempata por seq, como o Hub.
        let empate = MXMapeamento.ordenarFila(fila(["a": 1, "b": 1, "c": 1]))
        #expect(empate.itens.map(\.id) == ["a", "b", "c"] && empate.confiavel)
        // Só parte com a ordem do Hub: não confiável.
        #expect(!MXMapeamento.ordenarFila(fila(["a": 1])).confiavel)
        // Uma só na fila: a posição é óbvia.
        #expect(MXMapeamento.ordenarFila([fila([:])[0]]).confiavel)
        // Com ordem confiável, a Conversa volta a prometer a posição ("Editando a 2ª da fila").
        let b = try bot(["status": "working"])
        let conversa = MXMapeamento.conversa(b, entries: hub, computador: "Mac", motor: "", respondendo: [:], agora: t)
        #expect(conversa.ordemDaFilaConfiavel && conversa.fila.map(\.id) == ["c", "a", "b"])
    }

    @Test func semOrdemConfiavelNaoPrometePosicao() {
        #expect(MXFilaRegras.textoEditando(posicao: 2) == catalogo("Editando a \(2)ª da fila"))
        #expect(MXFilaRegras.textoEditando(posicao: nil) == catalogo("Editando uma mensagem da fila"))
    }

    @Test func passosDaResposta() {
        let t = data(2026, 11, 11, 19, 23)
        var leu = entrada("t1", seq: 2, kind: "tool", quando: t) {
            $0.toolKind = "read"; $0.status = "completed"; $0.locations = [Location(path: "/Users/user/pace/src/pace.js")]
        }
        leu.updatedAt = ms(t)
        var rodou = entrada("t2", seq: 3, kind: "tool", quando: t) {
            $0.toolKind = "execute"; $0.status = "completed"; $0.command = "npm test"
        }
        rodou.updatedAt = ms(t.addingTimeInterval(130))
        let passos = MXMapeamento.passos([leu, rodou], pasta: "/Users/user/pace")
        #expect(passos?.resumo == catalogo("\(2) passos") + " · " + catalogo("Leitura") + ", " + catalogo("Comando") + " · 2 min")
        #expect(passos?.itens.map(\.texto) == [catalogo("Leu \("src/pace.js")"), catalogo("Rodou \("npm test")")])
        #expect(passos?.itens.allSatisfy(\.concluido) == true)
        #expect(MXMapeamento.passos([], pasta: "") == nil)
        #expect(MXMapeamento.duracao(400) == nil)
        #expect(MXMapeamento.duracao(40_000) == "40 s")
        #expect(MXConversaRegras.passos(1) == catalogo("1 passo"))
    }

    private func pedido(status: String = "pending", selected: String? = nil) -> Entry {
        entrada("p1", seq: 4, kind: "permission", quando: data(2026, 11, 11, 19, 30), rev: 8) {
            $0.toolKind = "edit"
            $0.title = "Edit src/pace.js"
            $0.status = status
            $0.selected = selected
            $0.cwd = "/Users/user/pace"
            $0.options = [
                PermissionOption(optionId: "r", name: "Reject", kind: "reject_once"),
                PermissionOption(optionId: "s", name: "Always", kind: "allow_always"),
                PermissionOption(optionId: "o", name: "Once", kind: "allow_once"),
            ]
        }
    }

    @Test func cartaoTemAsOpcoesDoHubNaOrdemDoMac() {
        let cartao = MXMapeamento.aprovacao(pedido(), autor: "Byte", computador: "MacBook Pro", respondendo: nil)
        #expect(cartao.titulo == "Byte " + catalogo("quer mudar arquivos"))
        #expect(cartao.detalhe?.previa == "Edit src/pace.js" && cartao.detalhe?.cortadoNaPrevia == false)
        #expect(cartao.acao == catalogo("Edição de arquivos"))
        #expect(cartao.onde == "MacBook Pro · pace")
        #expect(cartao.opcoes.map(\.id) == ["o", "s", "r"])
        #expect(cartao.opcoes.map(\.rotulo) == [catalogo("Permitir uma vez"), catalogo("Sempre permitir"), catalogo("Recusar")])
        #expect(cartao.opcoes.map(\.principal) == [true, false, false])
        #expect(cartao.opcoes.map(\.permite) == [true, true, false])
        #expect(cartao.pendente)
    }

    @Test func respostaSoSaiSeOPedidoAindaEOQueEstavaNaTela() {
        let e = pedido()
        let naTela = MXMapeamento.aprovacao(e, autor: "Byte", computador: "Mac", respondendo: nil)
        #expect(naTela.revisao == 8)
        #expect(MXMapeamento.respostaConfere(e, apresentado: naTela, opcao: "o"))
        // Revisão nova com o mesmo id e opção, ainda pendente: não confere.
        var nova = e
        nova.rev = 9
        #expect(!MXMapeamento.respostaConfere(nova, apresentado: naTela, opcao: "o"))
        // Deixou de estar pendente.
        var respondido = e
        respondido.data.status = "answered"
        #expect(!MXMapeamento.respostaConfere(respondido, apresentado: naTela, opcao: "o"))
        // Opções mudaram na mesma revisão.
        var outras = e
        outras.data.options = [PermissionOption(optionId: "o", name: "Once", kind: "allow_once")]
        #expect(!MXMapeamento.respostaConfere(outras, apresentado: naTela, opcao: "o"))
        // Opção que não existe.
        #expect(!MXMapeamento.respostaConfere(e, apresentado: naTela, opcao: "x"))
        // A versão muda com revisão, pendência ou opções (o diálogo fecha e o segurar cancela).
        let depois = MXMapeamento.aprovacao(nova, autor: "Byte", computador: "Mac", respondendo: nil)
        #expect(naTela.versao != depois.versao)
        #expect(MXSegurarRegras.confirmaAinda(versaoInicial: naTela.versao, versaoAtual: naTela.versao))
        #expect(!MXSegurarRegras.confirmaAinda(versaoInicial: naTela.versao, versaoAtual: depois.versao))
        #expect(!MXSegurarRegras.confirmaAinda(versaoInicial: nil, versaoAtual: naTela.versao))
    }

    @Test func cartaoRespondidoMostraOResultado() {
        let permitido = MXMapeamento.aprovacao(pedido(status: "answered", selected: "o"), autor: "Byte", computador: "Mac", respondendo: nil)
        #expect(permitido.resultado == .permitido(catalogo("Permitido uma vez")))
        let recusado = MXMapeamento.aprovacao(pedido(status: "answered", selected: "r"), autor: "Byte", computador: "Mac", respondendo: nil)
        #expect(recusado.resultado == .recusado(catalogo("Recusado")))
        let expirou = MXMapeamento.aprovacao(pedido(status: "expired"), autor: "Byte", computador: "Mac", respondendo: nil)
        #expect(expirou.resultado == .encerrado(catalogo("Expirou — o bot seguiu sem a resposta")))
        let respondendo = MXMapeamento.aprovacao(pedido(), autor: "Byte", computador: "Mac", respondendo: "o")
        #expect(respondendo.respondendo == "o")
    }

    @Test func cartaoLevaAsAlteracoesDoPedidoParaDetalhes() {
        let diff = FileDiff(path: "src/pace.js", added: 3, removed: 1, isNew: false, startLine: 12, patch: "@@\n+a")
        // Só título e alterações (sem comando nem saída): Detalhes continua lá, com as alterações.
        var soDiffs = pedido()
        soDiffs.data.command = nil
        soDiffs.data.detail = nil
        soDiffs.data.diffs = [diff]
        let cartao = MXMapeamento.aprovacao(soDiffs, autor: "Byte", computador: "Mac", respondendo: nil)
        #expect(cartao.detalhes.isEmpty)
        #expect(cartao.diffs.map(\.caminho.texto) == ["src/pace.js"] && cartao.diffs.map(\.id) == [0])
        #expect(cartao.temDetalhes)
        // Muitos arquivos: todos no modelo; em Detalhes, uma página por vez.
        var muitos = pedido()
        muitos.data.diffs = Array(repeating: diff, count: MX.Conversa.diffsMaximo + 3)
        let grande = MXMapeamento.aprovacao(muitos, autor: "Byte", computador: "Mac", respondendo: nil)
        #expect(grande.diffs.count == MX.Conversa.diffsMaximo + 3) // todas no modelo
        #expect(MXConversaRegras.paginasDeDiffs(grande.diffs.count) == 2)
        #expect(MXConversaRegras.paginaDeDiffs(total: grande.diffs.count, pagina: 0) == 0..<MX.Conversa.diffsMaximo)
        let segunda = MXConversaRegras.paginaDeDiffs(total: grande.diffs.count, pagina: 1)
        #expect(segunda == MX.Conversa.diffsMaximo..<MX.Conversa.diffsMaximo + 3)
        #expect(MXConversaRegras.rotuloDaPagina(segunda, total: grande.diffs.count) == catalogo("Arquivos \(21)–\(23) de \(23)"))
        // Identidade pela posição: arquivos iguais não colidem.
        #expect(Set(grande.diffs.map(\.id)).count == grande.diffs.count)
        // Sem nada: sem Detalhes.
        #expect(!MXMapeamento.aprovacao(pedido(), autor: "Byte", computador: "Mac", respondendo: nil).temDetalhes)
        // O pedido de exemplo (06) traz uma alteração.
        #expect(MXExemplo.exemploDeDiff != nil && MXExemplo.pedido.temDetalhes)
    }

    @Test func paginaDeAlteracoesLimitaOTrabalhoPorMaiorQueSejaOPedido() {
        let total = 10_000
        #expect(MXConversaRegras.paginasDeDiffs(total) == 500)
        // Cada página tem no máximo `diffsMaximo`; páginas fora do intervalo vão para a mais próxima.
        #expect(MXConversaRegras.paginaDeDiffs(total: total, pagina: 499) == 9_980..<10_000)
        #expect(MXConversaRegras.paginaDeDiffs(total: total, pagina: 9_999) == 9_980..<10_000)
        #expect(MXConversaRegras.paginaDeDiffs(total: total, pagina: -1) == 0..<20)
        #expect(MXConversaRegras.paginaDeDiffs(total: 0, pagina: 0).isEmpty)
        #expect(MXConversaRegras.paginasDeDiffs(MX.Conversa.diffsMaximo) == 1) // uma página: sem paginador
    }

    @Test func paginaForaDoIntervaloNavegaAPartirDaExibida() {
        // Estava na página de índice 4; uma revisão encolheu o pedido para 2 páginas (35 arquivos).
        let total = 35
        #expect(MXConversaRegras.paginaNormalizada(total: total, pagina: 4) == 1) // a exibida é a última
        #expect(MXConversaRegras.paginaDeDiffs(total: total, pagina: 4) == 20..<35)
        // "Anteriores" volta UMA página de verdade (de 1 para 0), não de 4 para 3.
        #expect(MXConversaRegras.paginaAnterior(total: total, pagina: 4) == 0)
        // "Próximos" não passa da última.
        #expect(MXConversaRegras.proximaPagina(total: total, pagina: 4) == 1)
        #expect(MXConversaRegras.proximaPagina(total: total, pagina: 0) == 1)
        #expect(MXConversaRegras.paginaAnterior(total: total, pagina: 0) == 0)
        // Sem alterações: tudo fica na página 0.
        #expect(MXConversaRegras.paginaNormalizada(total: 0, pagina: 7) == 0)
    }

    @Test func previaDoDiffLimitaOTrabalhoAntesDeProcessar() {
        // Uma linha minificada enorme: a prévia corta a linha; o patch inteiro fica para "Mostrar tudo".
        let enorme = "+" + String(repeating: "a", count: 1_000_000)
        let d = FileDiff(path: "dist/app.min.js", added: 1, removed: 0, isNew: false, startLine: 1, patch: enorme)
        let alteracao = MXDiffPrevia(d, indice: 3)
        #expect(alteracao.id == 3)
        let previa = alteracao.linhasDaPrevia()
        #expect(previa.linhas.count == 1 && previa.linhas[0].count == MX.Conversa.diffLinhaMaxima + 1)
        #expect(previa.cortado && alteracao.patch.cortadoNaPrevia)
        let previaPatch = alteracao.patch
        #expect(previaPatch.excedeLimite) // 1 milhão de bytes: acima do limite absoluto
        // Muitas linhas curtas: até 80.
        let muitas = (0..<500).map { "+linha \($0)" }.joined(separator: "\n")
        let lista = MXDiffPrevia(FileDiff(path: "a", added: 500, removed: 0, isNew: true, startLine: 1, patch: muitas), indice: 0)
        #expect(lista.linhasDaPrevia().linhas.count == MX.Conversa.diffLinhas && lista.linhasDaPrevia().cortado
                && !lista.patch.excedeLimite)
        // Pequeno: inteiro, sem corte.
        let curto = MXDiffPrevia(FileDiff(path: "b", added: 1, removed: 1, isNew: false, startLine: 1, patch: "-a\n+b"), indice: 1)
        #expect(curto.linhasDaPrevia() == MXLinhasDoDiff(linhas: ["-a", "+b"], cortado: false))
    }

    @Test func caminhoLongoFicaInteiroEAcimaDoLimiteBloqueiaPermitir() {
        // ~600 bytes: o nome final do arquivo fica no fim. Em Detalhes, o caminho aparece INTEIRO, sem depender do corte
        // do patch (patch curto, sem "Mostrar tudo").
        let caminho = "src/" + String(repeating: "pasta/", count: 100) + "segredo.env"
        let previa = MXDiffPrevia(FileDiff(path: caminho, added: 1, removed: 0, isNew: false, startLine: 1, patch: "+a"), indice: 0)
        #expect(previa.caminho.texto == caminho) // inteiro no modelo
        #expect(previa.caminhoNaTela(inteiro: false) == caminho)
        #expect(!previa.caminhoPedeMostrarTudo && !previa.linhasDaPrevia().cortado)
        #expect(!previa.caminho.excedeLimite)
        // Maior que um caminho de verdade (PATH_MAX): o começo, e "Mostrar o caminho inteiro" junto dele.
        let enorme = String(repeating: "d/", count: 3000) + "fim.txt"
        let grande = MXDiffPrevia(FileDiff(path: enorme, added: 1, removed: 0, isNew: false, startLine: 1, patch: "+a"), indice: 1)
        #expect(grande.caminhoPedeMostrarTudo && !grande.caminhoNaTela(inteiro: false).contains("fim.txt"))
        #expect(grande.caminhoNaTela(inteiro: true) == enorme)
        // Um pedido só com essa alteração continua permitível (dá para conferir tudo).
        var e = pedido()
        e.data.diffs = [FileDiff(path: caminho, added: 1, removed: 0, isNew: false, startLine: 1, patch: "+a")]
        #expect(!MXMapeamento.aprovacao(e, autor: "Byte", computador: "Mac", respondendo: nil).bloqueadoPorTamanho)
        // Caminho acima do limite absoluto: permitir bloqueia, recusar continua.
        e.data.diffs = [FileDiff(path: String(repeating: "p", count: MX.Conversa.inspecaoMaxima + 1), added: 1, removed: 0,
                                 isNew: false, startLine: 1, patch: "+a")]
        let cartao = MXMapeamento.aprovacao(e, autor: "Byte", computador: "Mac", respondendo: nil)
        #expect(cartao.bloqueadoPorTamanho)
        #expect(!MXMapeamento.respostaConfere(e, apresentado: cartao, opcao: "o"))
        #expect(MXMapeamento.respostaConfere(e, apresentado: cartao, opcao: "r"))
    }

    @Test func alteracaoAcimaDoLimiteBloqueiaPermitir() {
        var e = pedido()
        e.data.diffs = [FileDiff(path: "a", added: 1, removed: 0, isNew: false, startLine: 1,
                                 patch: String(repeating: "b", count: MX.Conversa.inspecaoMaxima + 1))]
        let cartao = MXMapeamento.aprovacao(e, autor: "Byte", computador: "Mac", respondendo: nil)
        #expect(cartao.bloqueadoPorTamanho)
        #expect(!MXMapeamento.respostaConfere(e, apresentado: cartao, opcao: "o"))
    }

    @Test func limitesSaoPorBytesMesmoComMuitasMarcasCombinantes() {
        // Uma letra com 100 mil marcas combinantes é UM caractere de 200 001 bytes.
        let unico = "e" + String(repeating: "\u{0301}", count: 100_000)
        #expect(unico.count == 1)
        let t = MXTextoLongo(unico, previa: 2000, limite: 150_000)
        // A prévia não atravessa como "um caractere só": fica no limite de bytes, sem escalar partido.
        #expect(t.cortadoNaPrevia && t.previa.utf8.count <= 2000 + "…".utf8.count)
        #expect(!t.previa.unicodeScalars.contains("\u{FFFD}"))
        #expect(t.excedeLimite)
        // Inspeção: começo e fim limitados, nada partido, e o que ficou de fora nunca é negativo.
        let inspecao = t.paraInspecao(limite: 150_000)
        #expect(inspecao.utf8.count <= 150_000 + 100)
        #expect(!inspecao.unicodeScalars.contains("\u{FFFD}"))
        #expect(inspecao.contains(catalogo("… \("51 KB") ficaram de fora …"))) // 200 001 − 74 999 − 75 000
        #expect(!inspecao.contains("-"))
        // Emoji (4 bytes) não é cortado no meio.
        let emojis = MXTextoLongo(String(repeating: "😀", count: 1000), previa: 10)
        #expect(emojis.previa == "😀😀…")
        // O balão também corta por bytes.
        let balao = MXConversaRegras.cortar(unico)
        #expect(balao.cortado && balao.texto.utf8.count <= MX.Conversa.textoLimite + "…".utf8.count)
        // O tamanho do que ficou de fora.
        #expect(MXUTF8.tamanho(1) == "1 byte" && MXUTF8.tamanho(9) == "9 bytes")
        #expect(MXUTF8.tamanho(12_000) == "12 KB" && MXUTF8.tamanho(3_400_000) == catalogo("\(3),\(4) MB"))
        #expect(MXUTF8.tamanho(-5) == "0 bytes")
    }

    @Test func comandoLongoFicaInteiroSobDemanda() throws {
        var e = pedido()
        let comando = String(repeating: "x", count: 50_000) + " && rm -rf /tmp/alvo"
        e.data.command = comando
        e.data.title = String(repeating: "y", count: 5_000)
        let cartao = MXMapeamento.aprovacao(e, autor: "Byte", computador: "Mac", respondendo: nil)
        let texto = try #require(cartao.detalhes.first)
        // A prévia é curta e avisa; o inteiro não perdeu o fim (o que importa pode estar lá).
        #expect(texto.cortadoNaPrevia && texto.previa.count == MX.Conversa.detalhesMaximo + 1)
        #expect(texto.paraInspecao() == comando)
        #expect(texto.notaDoCorte == catalogo("Prévia cortada: só o começo. \"Mostrar tudo\" abre o resto."))
        // Título cortado também vai inteiro para Detalhes.
        #expect(cartao.detalhe?.cortadoNaPrevia == true && cartao.temDetalhes)
        // Abaixo do limite absoluto, permitir continua possível.
        #expect(!cartao.bloqueadoPorTamanho)
        #expect(MXMapeamento.respostaConfere(e, apresentado: cartao, opcao: "o"))
    }

    @Test func acimaDoLimiteMostraComecoEFimEBloqueiaPermitir() {
        var e = pedido()
        let limite = MX.Conversa.inspecaoMaxima
        e.data.command = "INICIO" + String(repeating: "x", count: limite) + "FIM"
        let cartao = MXMapeamento.aprovacao(e, autor: "Byte", computador: "Mac", respondendo: nil)
        let texto = cartao.detalhes[0]
        #expect(texto.excedeLimite && cartao.bloqueadoPorTamanho)
        let inspecao = texto.paraInspecao()
        #expect(inspecao.hasPrefix("INICIO") && inspecao.hasSuffix("FIM"))
        #expect(inspecao.contains(catalogo("… \("9 bytes") ficaram de fora …"))) // 6 + limite + 3 − limite
        // Permitir não confere (nem uma vez nem sempre); recusar continua.
        #expect(!MXMapeamento.respostaConfere(e, apresentado: cartao, opcao: "o"))
        #expect(!MXMapeamento.respostaConfere(e, apresentado: cartao, opcao: "s"))
        #expect(MXMapeamento.respostaConfere(e, apresentado: cartao, opcao: "r"))
        #expect(MXTextoDeQueda.grandeDemais == catalogo("Pedido grande demais para conferir no iPhone. Aprove no computador."))
        // Título acima do limite bloqueia igual.
        var titulo = pedido()
        titulo.data.title = String(repeating: "t", count: limite + 1)
        #expect(MXMapeamento.aprovacao(titulo, autor: "Byte", computador: "Mac", respondendo: nil).bloqueadoPorTamanho)
    }

    @Test func conversaRealViraBaloesPassosCartaoETrabalho() throws {
        let agora = data(2026, 11, 11, 20)
        let b = try bot(["status": "needsInput", "activity": "", "lastAt": ms(agora)])
        let entries = [
            entrada("u1", seq: 1, kind: "user", turn: 1, quando: data(2026, 11, 11, 19, 22)) { $0.text = "Cria um helper"; $0.status = "sent" },
            entrada("t1", seq: 2, kind: "tool", turn: 1, quando: data(2026, 11, 11, 19, 23)) { $0.toolKind = "read"; $0.status = "completed"; $0.title = "Read src/pace.js" },
            entrada("a1", seq: 3, kind: "agent", turn: 1, quando: data(2026, 11, 11, 19, 25)) { $0.text = "Adicionei."; $0.final = true },
            entrada("u2", seq: 4, kind: "user", turn: 2, quando: data(2026, 11, 11, 19, 30)) { $0.text = "Corrige o bug"; $0.status = "sent" },
            entrada("t2", seq: 5, kind: "tool", turn: 2, quando: data(2026, 11, 11, 19, 31)) { $0.toolKind = "read"; $0.status = "completed"; $0.title = "Read src/pace.js" },
            pedido(),
            entrada("q1", seq: 7, kind: "user", turn: 3, quando: data(2026, 11, 11, 19, 32)) { $0.text = "Depois roda o lint"; $0.status = "queued" },
            entrada("f1", seq: Int64.max, kind: "user", turn: 0, quando: data(2026, 11, 11, 19, 33)) { $0.text = "falhou"; $0.status = "failed" },
        ]
        let conversa = MXMapeamento.conversa(b, entries: entries, computador: "MacBook Pro", motor: "Claude Code",
                                             respondendo: [:], agora: agora, calendario: calendario)
        let ids = conversa.itens.map(\.id)
        #expect(ids == ["sep-u1", "u1", "a1", "u2", "p1", "f1", "trabalhando-byte"])
        #expect(conversa.itens.first?.conteudo == .separador(catalogo("Hoje \("19:22")")))
        guard case let .agente(resposta) = conversa.itens[2].conteudo else { Issue.record("sem resposta"); return }
        #expect(resposta.cabecalho && resposta.hora == "19:25")
        #expect(resposta.passos?.itens.map(\.id) == ["t1"])
        guard case let .usuario(falhou) = conversa.itens[5].conteudo else { Issue.record("sem falha"); return }
        #expect(falhou.podeReenviar && falhou.nota == catalogo("Não enviada"))
        guard case let .trabalhando(trabalho) = conversa.itens.last?.conteudo else { Issue.record("sem trabalho"); return }
        #expect(trabalho.esperandoDecisao)
        #expect(trabalho.passos?.resumo == catalogo("Trabalhando · \(catalogo("1 passo"))"))
        #expect(trabalho.passos?.itens.map(\.id) == ["t2"])
        // A mensagem na fila não vira balão: vai para a fila.
        #expect(conversa.fila.map(\.id) == ["q1"])
        #expect(conversa.esperandoDecisao && conversa.trabalhando)
        #expect(conversa.modelo == "Claude Code")
        #expect(conversa.contexto == nil) // o Hub não manda o % de contexto
        #expect(conversa.autonomia == catalogo("Pedir"))
    }

    @Test func conexaoVemDaQuedaCurtaDoStore() {
        let agora = Date(timeIntervalSince1970: 1000)
        let antes = Date(timeIntervalSince1970: 900)
        // Só com autoridade (`temAutoridade`) é conectado: enviar e aprovar valem.
        #expect(MXMapeamento.conexao(.conectado, temAutoridade: true, agora: agora) == .conectado)
        // `.conectado` sem autoridade ainda (cliente nulo até o `hello`): reconectando, nada sai.
        #expect(MXMapeamento.conexao(.conectado, temAutoridade: false, agora: agora) == .reconectando(desde: agora))
        // Queda curta: reconectando, com o começo que o store guardou.
        #expect(MXMapeamento.conexao(.reconectando(desde: antes), temAutoridade: false, agora: agora) == .reconectando(desde: antes))
        // Prazo longo, acesso encerrado, encerrado no app: desconectado.
        for motivo in [MotivoDesconexao.prazoLongo, .acessoEncerrado("revogado"), .encerradoNoApp] {
            #expect(MXMapeamento.conexao(.desconectado(motivo), temAutoridade: false, agora: agora) == .desconectado)
        }
        // A faixa: reconectando só depois da carência do store (`faixaDeQueda`); desconectado, sempre.
        #expect(MXTextoDeQueda.tituloDaFaixa(.conectado, faixa: true) == nil)
        #expect(MXTextoDeQueda.tituloDaFaixa(.reconectando(desde: agora), faixa: false) == nil)
        #expect(MXTextoDeQueda.tituloDaFaixa(.reconectando(desde: agora), faixa: true) == MXTextoDeQueda.titulo)
        #expect(MXTextoDeQueda.tituloDaFaixa(.desconectado, faixa: false) == MXTextoDeQueda.tituloDesconectado)
    }

    @Test func textosDaQuedaIguaisAosDoAppNosDoisIdiomas() throws {
        // Os mesmos textos da `FaixaDeQueda` do app (`QuedaTextos`), agora vindos do catálogo em pt-BR e em en.
        for idioma in ["pt-BR", "en"] {
            let caminho = try #require(ConnectionSummary.resourceBundle.path(forResource: idioma, ofType: "lproj"))
            let bundle = try #require(Bundle(path: caminho))
            let app = try #require(QuedaTextos.porIdioma[idioma])
            func texto(_ chave: String.LocalizationValue) -> String { String(localized: chave, bundle: bundle) }
            #expect(texto("Sem conexão — reconectando…") == app.titulo)
            #expect(texto("Suas conversas continuam aqui. Enviar e aprovar voltam quando reconectar; o que você digitar fica guardado.") == app.detalhe)
            #expect(texto("Sem conexão: o texto fica guardado e você envia quando reconectar.") == app.semEnvio)
            #expect(texto("Sem conexão: permitir e recusar voltam quando reconectar.") == app.semAprovar)
            #expect(texto("Tentar agora") == app.tentar && texto("Tentando…") == app.tentando)
        }
    }

    @Test func caixaNoTextoGrandeParaEmMetadeDaTela() {
        // Padrão: nada muda (5 linhas no campo, sem limite de altura).
        let normal = MXArranjoDaCaixa(acessibilidade: false)
        #expect(normal.linhasDoCampo == 5 && normal.alturaMaxima(tela: 852) == nil)
        // Acessibilidade: o campo rola depois de 3 linhas e a caixa para em metade da tela.
        let grande = MXArranjoDaCaixa(acessibilidade: true)
        #expect(grande.linhasDoCampo == 3 && grande.alturaMaxima(tela: 852) == 426)
        // Antes de medir a tela, sem limite (não colapsa a caixa).
        #expect(grande.alturaMaxima(tela: 0) == nil)
        // A dica do campo vazio: no padrão, o prompt de sempre; no texto grande, até 2 linhas, encolhendo até 85%.
        #expect(!normal.dicaPropria && normal.linhasDaDica == 1 && normal.escalaMinimaDaDica == 1)
        #expect(grande.dicaPropria && grande.linhasDaDica == 2 && grande.escalaMinimaDaDica == 0.85)
    }

    @Test func filaNoTextoGrandePoeAsAcoesEmbaixo() {
        // Padrão: nada muda (uma linha de texto, ações ao lado, o passo da linha).
        let normal = MXArranjoDaFila(acessibilidade: false)
        #expect(normal.linhasDoTexto == 1 && !normal.acoesEmbaixo)
        #expect(normal.alturaDaLinha(linha: 31, passo: 44, acao: 44) == 44)
        // Acessibilidade: 2 linhas de texto e as ações numa linha embaixo.
        let grande = MXArranjoDaFila(acessibilidade: true)
        #expect(grande.linhasDoTexto == 2 && grande.acoesEmbaixo)
        #expect(grande.alturaDaLinha(linha: 60, passo: 60, acao: 44) == 164)
        // As ações continuam com 44 cada e o mesmo afastamento.
        let areas = MXAlvoDeToque.areasDasAcoesDaFila()
        #expect(areas.allSatisfy { $0.upperBound - $0.lowerBound >= 44 })
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaAcao) - MX.Conversa.filaAcao == 16)
    }

    @Test func capsulaDoTopoComoABarraNativa() {
        // O VoiceOver lê tudo: nome, função (se houver) e estado.
        #expect(MXConversaRegras.rotuloDaCapsula(nome: "Byte", funcao: "Dev", estado: "Trabalhando") == "Byte, Dev, Trabalhando")
        #expect(MXConversaRegras.rotuloDaCapsula(nome: "Byte", funcao: "", estado: "Precisa de você") == "Byte, Precisa de você")
        // O texto da cápsula para de crescer em "extra extra extra grande" (nunca nos tamanhos de acessibilidade).
        #expect(MX.Conversa.capsulaTextoAte == .xxxLarge && !MX.Conversa.capsulaTextoAte.isAccessibilitySize)
        // Voltar e WhatsApp com alvo de 44.
        #expect(MX.Medida.botaoRedondo >= MX.Espaco.alvoMinimo)
        #expect(MX.Conversa.whatsappLargura + 2 * MX.Conversa.whatsappRespiro >= MX.Espaco.alvoMinimo)
    }

    @Test func fixarNoMenuSoComAutoridade() {
        let ligado = MXItemDeFixar(fixado: false, conexao: .conectado)
        #expect(ligado == MXItemDeFixar(fixado: false, conexao: .conectado))
        #expect(ligado.titulo == catalogo("Fixar no topo") && ligado.ligado && ligado.valorAcessivel == nil)
        #expect(MXItemDeFixar(fixado: true, conexao: .conectado).titulo == catalogo("Desafixar"))
        // Sem autoridade (reconectando ou desconectado): à vista, desligado, e o VoiceOver diz por quê.
        for conexao in [MXEstadoConexao.reconectando(desde: Date()), .desconectado] {
            let item = MXItemDeFixar(fixado: true, conexao: conexao)
            #expect(item.titulo == catalogo("Desafixar") && !item.ligado)
            #expect(item.valorAcessivel == catalogo("Indisponível sem conexão"))
        }
    }

    @Test func falhaComOLinkTrocadoAvisaDaLigacaoENaoDoHub() {
        let generico = catalogo("Não foi possível mudar a fila.")
        // Mesmo link: o aviso é o da ação (o Hub recusou).
        #expect(MXMapeamento.avisoDaFalha(tocada: AnyHashable(3), agora: AnyHashable(3), daAcao: generico) == generico)
        // Caiu e voltou (outro link) ou caiu e não voltou: a causa é a ligação.
        #expect(MXMapeamento.avisoDaFalha(tocada: AnyHashable(3), agora: AnyHashable(4), daAcao: generico) == MXTextoDeQueda.linkMudou)
        #expect(MXMapeamento.avisoDaFalha(tocada: AnyHashable(3), agora: nil, daAcao: generico) == MXTextoDeQueda.linkMudou)
        #expect(MXTextoDeQueda.linkMudou == catalogo("Sem conexão — tente de novo."))
    }

    @Test func chamadaSoValeSeVoltouPeloMesmoLink() {
        #expect(MXMapeamento.mesmoLink(AnyHashable(1), AnyHashable(1)))
        #expect(!MXMapeamento.mesmoLink(AnyHashable(1), AnyHashable(2))) // caiu e voltou no meio
        #expect(!MXMapeamento.mesmoLink(AnyHashable(1), nil)) // caiu e não voltou
        #expect(!MXMapeamento.mesmoLink(nil, nil)) // nunca houve link
    }

    @Test func fixarConfereRevisaoAntesDeAplicarOuDesfazer() {
        // Resposta mais velha que o bot no espelho (um evento mais novo chegou): não entra.
        #expect(!MXMapeamento.aplicaRespostaDeFixar(atualRev: 9, respostaRev: 8))
        #expect(MXMapeamento.aplicaRespostaDeFixar(atualRev: 8, respostaRev: 9))
        #expect(!MXMapeamento.aplicaRespostaDeFixar(atualRev: nil, respostaRev: 9)) // o bot sumiu
        // Falhou sem nada novo no meio: volta ao de antes.
        #expect(MXMapeamento.desfazFixar(atual: true, atualRev: 5, pedido: true, anterior: false, revInicial: 5) == false)
        // Chegou evento mais novo: ele vale, nada é desfeito.
        #expect(MXMapeamento.desfazFixar(atual: true, atualRev: 6, pedido: true, anterior: false, revInicial: 5) == nil)
        // O valor já não é o que pedimos: deixa.
        #expect(MXMapeamento.desfazFixar(atual: false, atualRev: 5, pedido: true, anterior: false, revInicial: 5) == nil)
    }

    @Test func pendenciasDaBarra() throws {
        let bots = [
            try bot(["id": "a", "unread": 2, "status": "needsInput"]),
            try bot(["id": "b", "unread": 0]),
            try bot(["id": "g", "kind": "group", "unread": 1]),
        ]
        let p = MXMapeamento.pendencias(bots)
        #expect(p[.conversas] == 2)
        #expect(p[.missionBoard] == 1)
    }

    @MainActor @Test func textoInalteradoNaoEReinterpretado() {
        MXCacheDeTexto.limpar()
        let antes = MXCacheDeTexto.interpretacoes
        _ = MXCacheDeTexto.rico("**a**", chave: "e1#1")
        _ = MXCacheDeTexto.rico("**a**", chave: "e1#1")
        #expect(MXCacheDeTexto.interpretacoes == antes + 1)
        _ = MXCacheDeTexto.rico("**b**", chave: "e1#2") // revisão nova: interpreta de novo
        #expect(MXCacheDeTexto.interpretacoes == antes + 2)
    }

    @Test func textoRicoMantemOTextoEMarcaOCodigo() throws {
        let rico = MXConversaRegras.textoRico("Adicionei `weeklyTotals(runs)` em **src**.")
        #expect(String(rico.characters) == "Adicionei weeklyTotals(runs) em src.")
        let codigo = rico.runs.filter { $0.inlinePresentationIntent?.contains(.code) == true }
        #expect(codigo.count == 1)
        // O código usa a monoespaçada relativa ao estilo (escala com o Dynamic Type), não um tamanho fixo.
        let faixa = try #require(codigo.first?.range)
        #expect(rico[faixa][AttributeScopes.SwiftUIAttributes.FontAttribute.self] == Font.system(.footnote, design: .monospaced))
        #expect(MXConversaRegras.fonteDoCodigo == Font.system(.footnote, design: .monospaced))
        // Markdown quebrado não some: vira o texto como veio.
        #expect(!String(MXConversaRegras.textoRico("[link sem fim").characters).isEmpty)
    }
}

struct RedesignTextoGrandeTests {
    @Test func textoGrandeEntraCortado() {
        #expect(MXConversaRegras.cortar("curto").cortado == false)
        #expect(MXConversaRegras.cortar("curto").texto == "curto")
        let grande = String(repeating: "a", count: MX.Conversa.textoLimite + 10)
        let corte = MXConversaRegras.cortar(grande)
        #expect(corte.cortado)
        #expect(corte.texto.count == MX.Conversa.textoLimite + 1) // + "…"
    }

    @Test func respostaRealLevaIdERevisaoComoChaveDoCache() throws {
        let b = try bot(["status": "idle"])
        let e = entrada("a9", seq: 3, kind: "agent", quando: data(2026, 11, 11, 19), rev: 42) { $0.text = "oi"; $0.final = true }
        let itens = MXMapeamento.itens([e], bot: b, computador: "Mac", respondendo: [:], agora: data(2026, 11, 11, 20), calendario: calendario)
        guard case let .agente(m) = itens.last?.conteudo else { Issue.record("sem resposta"); return }
        #expect(m.chave == "a9#42")
    }
}

struct RedesignLinksTests {
    @Test(arguments: ["https://app.example.invalid", "http://exemplo.com/a?b=c", "HTTPS://EXEMPLO.COM", "mailto:oi@exemplo.com"])
    func webEEmailAbrem(_ texto: String) throws {
        #expect(MXLinks.permitido(try #require(URL(string: texto))))
    }

    @Test(arguments: ["tel:5511999999999", "file:///etc/hosts", "javascript:alert(1)", "donnabot://pair?v=3",
                      "shortcuts://run-shortcut?name=x", "sms:123", "data:text/html,oi", "pasta/relativa"])
    func outrosEsquemasNaoAbrem(_ texto: String) throws {
        #expect(!MXLinks.permitido(try #require(URL(string: texto))))
    }
}

struct RedesignCrossFadeTests {
    @Test func reduzirMovimentoFazOConteudoEsmaecerComOTokenCurtoSemAnimarPosicao() {
        let reduzido = MX.Movimento.troca(reduzir: true)
        #expect(reduzido.conteudoAnimaSozinho)
        #expect(reduzido.animacaoDaPosicao == nil) // o layout não desliza
        #expect(reduzido.animacao == MX.Movimento.crossFade) // a opacidade usa o cross-fade curto
        #expect(!MX.Movimento.troca(reduzir: false).conteudoAnimaSozinho)
    }
}

struct RedesignLeituraTests {
    @Test func lidaSoVisivelNumaCenaAtiva() {
        #expect(MXConversaRegras.lendo(visivel: true, cenaAtiva: true))
        #expect(!MXConversaRegras.lendo(visivel: true, cenaAtiva: false))
        #expect(!MXConversaRegras.lendo(visivel: false, cenaAtiva: true))
    }
}

struct RedesignHistoricoTests {
    @Test func caixaQueCresceComAPessoaNoFimSegueOFim() {
        let r = MXFimDaRolagem.self
        // No fim: o fim do conteúdo encosta na caixa (janela 800, caixa 120, conteúdo 2000 rolado até 1320).
        #expect(r.falta(conteudo: 2000, deslocamento: 1320, janela: 800, margemDeBaixo: 120) == 0)
        #expect(r.noFim(falta: 0) && r.noFim(falta: 24))
        // Quem parou lendo mais acima não é puxado para o fim quando a caixa cresce.
        #expect(!r.noFim(falta: r.falta(conteudo: 2000, deslocamento: 900, janela: 800, margemDeBaixo: 120)))
        #expect(!r.noFim(falta: 25))
    }

    @Test func chegarAoTopoCarregaAPaginaAnteriorUmaVezEComLigacao() {
        #expect(MXConversaRegras.carregaAnteriores(pode: true, carregando: false, conexao: .conectado))
        #expect(!MXConversaRegras.carregaAnteriores(pode: true, carregando: true, conexao: .conectado)) // uma por vez
        #expect(!MXConversaRegras.carregaAnteriores(pode: false, carregando: false, conexao: .conectado)) // não há mais
        #expect(!MXConversaRegras.carregaAnteriores(pode: true, carregando: false, conexao: .desconectado))
    }

    @Test func reconectarComOTopoAVistaTentaDeNovoEFalhaOferecePraTentar() {
        // Caiu com o topo à vista: ao voltar, tenta sozinha (o indicador não ganha outro onAppear).
        #expect(MXConversaRegras.recarregaAoReconectar(topoVisivel: true, conexao: .conectado, pode: true, carregando: false))
        #expect(!MXConversaRegras.recarregaAoReconectar(topoVisivel: false, conexao: .conectado, pode: true, carregando: false))
        #expect(!MXConversaRegras.recarregaAoReconectar(topoVisivel: true, conexao: .desconectado, pode: true, carregando: false))
        #expect(!MXConversaRegras.recarregaAoReconectar(topoVisivel: true, conexao: .conectado, pode: true, carregando: true))
        // Deu certo se a fronteira (menor seq) recuou ou se acabou o histórico; senão, "Tentar de novo".
        #expect(MXConversaRegras.carregou(fronteiraAntes: 400, fronteiraDepois: 300, aindaHaMais: true))
        #expect(MXConversaRegras.carregou(fronteiraAntes: 400, fronteiraDepois: 400, aindaHaMais: false))
        #expect(!MXConversaRegras.carregou(fronteiraAntes: 400, fronteiraDepois: 400, aindaHaMais: true))
        #expect(MXConversaRegras.carregou(fronteiraAntes: nil, fronteiraDepois: 300, aindaHaMais: true))
        #expect(!MXConversaRegras.carregou(fronteiraAntes: nil, fronteiraDepois: nil, aindaHaMais: true))
    }

    @Test func mensagemNovaDoStreamNaoMascaraFalhaDoHistorico() {
        func m(_ id: String, _ seq: Int64) -> Entry {
            Entry(id: id, seq: seq, botId: "byte", rev: 1, kind: "user", turn: 1, data: EntryData(text: "oi", status: "sent"),
                  createdAt: 1, updatedAt: 1)
        }
        let antes = [m("a", 400), m("b", 401), m("local-x", Int64.max)]
        // O histórico falhou, mas chegou uma mensagem nova pelo stream: a contagem subiu, a fronteira não.
        let depois = antes + [m("c", 402)]
        #expect(MXMapeamento.fronteiraDoHistorico(antes) == 400)
        #expect(!MXConversaRegras.carregou(fronteiraAntes: MXMapeamento.fronteiraDoHistorico(antes),
                                           fronteiraDepois: MXMapeamento.fronteiraDoHistorico(depois), aindaHaMais: true))
        // A página anterior veio: a fronteira recuou.
        let comPagina = [m("z", 300)] + depois
        #expect(MXConversaRegras.carregou(fronteiraAntes: 400, fronteiraDepois: MXMapeamento.fronteiraDoHistorico(comPagina),
                                          aindaHaMais: true))
        // Só entradas locais (seq 0 ou máximo): sem fronteira.
        #expect(MXMapeamento.fronteiraDoHistorico([m("local-y", Int64.max), m("local-z", 0)]) == nil)
    }
}

struct RedesignNavegacaoTests {
    @Test func voltarArrastandoPassaDe90() {
        #expect(!MXNavegacaoRegras.voltaArrastando(90))
        #expect(MXNavegacaoRegras.voltaArrastando(91))
    }

    @Test func barraSomeDentroDaConversa() {
        #expect(MXNavegacaoRegras.mostraBarra(aba: .conversas, caminho: []))
        #expect(!MXNavegacaoRegras.mostraBarra(aba: .conversas, caminho: [.conversa("byte")]))
        #expect(MXNavegacaoRegras.mostraBarra(aba: .estado, caminho: [.conversa("byte")]))
    }

    @Test func cenaLidaDaLinhaDeComando() {
        #expect(MXCena.daLinhaDeComando(["app", "-MexericaCena", "menu"], ambiente: [:]) == .menu)
        #expect(MXCena.daLinhaDeComando(["app"], ambiente: ["MEXERICA_CENA": "aprovar"]) == .aprovar)
        #expect(MXCena.daLinhaDeComando(["app", "-MexericaCena"], ambiente: [:]) == nil)
        #expect(MXCena.daLinhaDeComando(["app", "-MexericaCena", "outra"], ambiente: [:]) == nil)
    }

    @Test func cenaDeCapturaComecaNaConversaDoByte() {
        #expect(MXNavegacaoRegras.caminhoInicial(nil).isEmpty)
        // As cenas da Conversa (05–06) abrem nela; as da etapa 3 estão em `RedesignSessoesTests`.
        for cena in [MXCena.conversa, .filaEditando, .menu, .aprovar] {
            #expect(MXNavegacaoRegras.caminhoInicial(cena) == [.conversa("byte")])
        }
        #expect(MXCena(rawValue: "fila-editando") == .filaEditando)
    }

    @Test func largurasDoHtmlSaoDoConteudoEOSwiftUILimitaPorFora() {
        #expect(MX.Conversa.balaoDoUsuarioLarguraExterna == 328) // 300 + 14 + 14
        #expect(MX.Conversa.balaoDoAgenteLarguraExterna == 344) // 316 + 14 + 14
        #expect(MX.Conversa.menuLarguraExterna() == 256) // 244 + 6 + 6
        #expect(MX.Conversa.anguloDaSetaAberta == 90 && MX.Conversa.anguloDoInicioDoAnel == -90)
    }

    @Test func larguraDoBalaoSoltaNosTamanhosDeAcessibilidade() {
        #expect(MXConversaRegras.larguraMaxima(300, .large) == 300)
        #expect(MXConversaRegras.larguraMaxima(300, .accessibility1) == .infinity)
        #expect(!MXConversaRegras.umaColuna(.xxxLarge))
        #expect(MXConversaRegras.umaColuna(.accessibility3))
    }

    @Test func estadoNaCapsula() {
        #expect(MXEstadoRegras.texto(.trabalhando) == catalogo("Trabalhando"))
        #expect(MXEstadoRegras.cor(.trabalhando) == MX.Claro.sucessoTexto)
        #expect(MXEstadoRegras.texto(.precisaResposta) == catalogo("Precisa de você"))
        #expect(MXEstadoRegras.cor(.precisaResposta) == MX.Claro.mexericaTexto)
    }

    @Test func textoSobreOLaranjaDaConversaEhMarrom() {
        #expect(MX.Claro.balaoDoUsuario == MX.Claro.mexerica)
        #expect(MX.Claro.sobreMexerica.hex == "#2A1C14")
        #expect(MX.Vidro.opacoCaixa.a == 1 && MX.Vidro.opacoMenu.a == 1 && MX.Vidro.opacoFila.a == 1)
    }
}

@MainActor
struct RedesignFonteExemploTests {
    @Test func semCenaByteEsperaADecisao() throws {
        let fonte = MXFonteExemplo()
        let byte = try #require(fonte.conversa("byte"))
        #expect(byte.esperandoDecisao && byte.estado == .precisaResposta)
        #expect(byte.itens.contains { if case .aprovacao = $0.conteudo { true } else { false } })
    }

    @Test func permitirLiberaOTrabalho() throws {
        let fonte = MXFonteExemplo(cena: .aprovar)
        fonte.responder(MXExemplo.pedido, opcao: "allow_once", em: "byte")
        let byte = try #require(fonte.conversa("byte"))
        #expect(!byte.esperandoDecisao && byte.estado == .trabalhando)
        let pedido = byte.itens.compactMap { item -> MXPedidoDeAprovacao? in
            if case let .aprovacao(p) = item.conteudo { return p }
            return nil
        }.first
        #expect(pedido?.resultado == .permitido("Permitido uma vez"))
    }

    @Test func enviarTrabalhandoEntraNaFilaEMandarAgoraSai() async throws {
        let fonte = MXFonteExemplo(cena: .conversa)
        #expect(fonte.conversa("byte")?.fila.count == 2)
        fonte.enviar("mais uma", para: "byte")
        let fila = try #require(fonte.conversa("byte")?.fila)
        #expect(fila.count == 3 && fila.last?.texto == "mais uma")
        #expect(Set(fila.map(\.id)).count == 3) // ids novos não colidem com os do desenho
        fonte.mandarAgora(fila[0], em: "byte")
        #expect(fonte.conversa("byte")?.fila.count == 2)
        #expect(await fonte.editar(fila[1], texto: "editado", revisao: 1, em: "byte")?.value == true)
        #expect(fonte.conversa("byte")?.fila.first?.texto == "editado")
        fonte.remover(fila[1], em: "byte")
        #expect(fonte.conversa("byte")?.fila.count == 1)
    }

    @Test func cena06NaoTemWhatsAppNemRodape() throws {
        let aprovar = try #require(MXFonteExemplo(cena: .aprovar).conversa("byte"))
        #expect(!aprovar.mostraWhatsApp && aprovar.modelo == nil && aprovar.contexto == nil)
        let conversa = try #require(MXFonteExemplo(cena: .conversa).conversa("byte"))
        #expect(conversa.mostraWhatsApp && conversa.modelo == "Claude · Opus" && conversa.contexto == 42)
    }

    @Test func cenasDeCaptura() {
        #expect(MXFonteExemplo(cena: .filaEditando).conversa("byte")?.fila.count == 3)
        #expect(MXFonteExemplo(cena: .menu).conversa("byte")?.funcao == "Desenvolvedor")
        #expect(MXFonteExemplo(cena: .conversa).conversa("byte")?.contexto == 42)
    }
}

/// Nenhum alvo de toque abaixo de 44 pt; o desenho fica do tamanho desenhado.
struct RedesignAlvoDeToqueTests {
    private func semSobreposicao(_ areas: [ClosedRange<CGFloat>]) -> Bool {
        zip(areas, areas.dropFirst()).allSatisfy { $0.upperBound <= $1.lowerBound }
    }

    private func largura(_ a: ClosedRange<CGFloat>) -> CGFloat { a.upperBound - a.lowerBound }

    @Test func desenhoPequenoGanhaAreaDe44() {
        #expect(MXAlvoDeToque.minimo == 44)
        for desenhado: CGFloat in [22, 26, 28, 30, 31, 36, 40] { #expect(MXAlvoDeToque.lado(desenhado) == 44, "\(desenhado)") }
        #expect(MXAlvoDeToque.lado(52) == 52) // maior que 44 fica como está
    }

    @Test func filaNoDesenhoDivideAsAreasDasAcoesSemSobrepor() {
        // O compacto do desenho continua no código (o padrão segue a Apple).
        // As três ações como desenhadas: 28 de largura, encostadas (centros 14, 42, 70).
        #expect(MXAlvoDeToque.centros(3, passo: MX.Conversa.filaAcao) == [14, 42, 70])
        let areas = MXAlvoDeToque.areasDasAcoesDaFila(.desenho)
        #expect(semSobreposicao(areas))
        #expect(areas.map(largura) == [36, 28, 36])
        #expect(areas.first?.lowerBound == -8 && areas.last?.upperBound == 92) // as pontas crescem para fora
        // Linhas, cabeçalho e ações no tamanho desenhado: a linha inteira (31) é o alvo vertical.
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaLinha, .desenho) == 31)
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaCabecalho, .desenho) == 22)
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaAcao, .desenho) == 28)
        let linhas = MXAlvoDeToque.areas(centros: MXAlvoDeToque.centros(3, passo: 31), desenhado: MX.Conversa.filaLinha)
        #expect(semSobreposicao(linhas) && largura(linhas[1]) == 31)
        // Altura da fila como no desenho: cabeçalho + 2 linhas visíveis.
        let altura = MXAlvoDeToque.passoNaFila(MX.Conversa.filaCabecalho, .desenho)
            + CGFloat(MX.Conversa.filaLinhasVisiveis) * MXAlvoDeToque.passoNaFila(MX.Conversa.filaLinha, .desenho)
        #expect(altura == 84)
    }

    @Test func filaPadraoEAEspacadaDaAppleComAlvosDe44EAfastados() {
        // Fila espaçada conforme o padrão da Apple.
        #expect(MX.Conversa.filaEspacamento == .apple)
        // O que a tela usa (sem argumento): cada ação (Mandar agora, Editar, Remover) com 44 × 44, sem sobrepor.
        let areas = MXAlvoDeToque.areasDasAcoesDaFila()
        #expect(areas.count == 3 && areas.allSatisfy { largura($0) >= 44 } && semSobreposicao(areas))
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaLinha) >= 44) // altura do alvo: a linha inteira
        // Os desenhos ficam afastados: 44 entre centros, 16 pt entre ícones de 28.
        let passo = MXAlvoDeToque.passoNaFila(MX.Conversa.filaAcao)
        #expect(passo == 44 && passo - MX.Conversa.filaAcao == 16)
        // Cabeçalho e seta também com 44.
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaCabecalho) == 44)
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaSetaLargura) == 44)
    }

    @Test func filaNoPadraoDaAppleTemAlvosDe44SemSobrepor() {
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaAcao, .apple) == 44)
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaLinha, .apple) == 44)
        #expect(MXAlvoDeToque.passoNaFila(MX.Conversa.filaCabecalho, .apple) == 44)
        let areas = MXAlvoDeToque.areasDasAcoesDaFila(.apple)
        #expect(areas.map(largura) == [44, 44, 44])
        #expect(semSobreposicao(areas))
        let linhas = MXAlvoDeToque.areas(centros: MXAlvoDeToque.centros(3, passo: 44), desenhado: MX.Conversa.filaLinha)
        #expect(linhas.allSatisfy { largura($0) >= 44 } && semSobreposicao(linhas))
        let altura = 44 + CGFloat(MX.Conversa.filaLinhasVisiveis) * 44
        #expect(altura == 132)
    }

    @Test func todosOsAlvosPequenosDoDesenhoChegamA44SemMudarODesenho() {
        // Fora da fila (que segue `filaEspacamento`): a área cresce para 44 sem mudar o desenho.
        let desenhados: [CGFloat] = [
            MX.Conversa.filaSetaLargura, MX.Conversa.cancelarAltura, MX.Conversa.passosAltura, MX.Conversa.ditadoLargura,
            MX.Medida.canalAltura, MX.Medida.capsulaAltura, MX.Medida.buscaAltura,
        ]
        for d in desenhados { #expect(MXAlvoDeToque.lado(d) >= MX.Espaco.alvoMinimo, "\(d)") }
        // O desenho continua o do protótipo.
        #expect(MX.Conversa.filaAcao == 28 && MX.Conversa.filaLinha == 31 && MX.Conversa.cancelarAltura == 28)
        #expect(MX.Conversa.passosAltura == 30 && MX.Conversa.filaCabecalho == 22)
    }
}
