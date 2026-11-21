// FONTE DE EXEMPLO — NÃO FALA COM O HUB.
//
// Os dados do desenho (`MXExemplos.swift`) com as ações imitadas na memória, para a casca de Debug abrir sem login
// (`-MexericaRedesign`) e para as capturas (`-MexericaCena <nome>`). Nada aqui chama o Hub.
import CodyncKit
import Foundation
import Observation

@MainActor
@Observable
public final class MXFonteExemplo: MXFonte {
    /// Uma conversa de exemplo, mutável.
    struct Conversa {
        var estado: MexeriquinhaState
        var itens: [MXItemDaConversa]
        var fila: [MXItemDaFila]
        var trabalhando: Bool
        var esperandoDecisao: Bool
        /// Composição da captura 06: sem WhatsApp no topo e sem o rodapé de modelo e contexto.
        var comoNa06 = false
    }

    private var conversas: [String: Conversa] = [:]
    private var rascunhos: [String: String] = [:]
    private var fixados: Set<String> = []
    private var contador = 0
    /// Sessões de exemplo mexidas (arquivadas, criadas), por agente.
    private var arvores: [String: [MXSessao]] = [:]
    private var arquivadasNovas: [String: [MXSessao]] = [:]
    private var autonomias: [String: MXAutonomia] = [:]
    private var avisosDesligados: Set<String> = []
    private var excluidos: Set<String> = []
    #if DEBUG
    /// A variação do Mission Board de exemplo (imagens e prévia sem login).
    @ObservationIgnored var cenaDoQuadro: MXCenaDoQuadro = .padrao
    /// O relógio do quadro de exemplo (as imagens fixam; nulo, a hora de agora).
    @ObservationIgnored var agoraDoQuadro: Date?
    #endif

    public init(cena: MXCena? = nil) {
        let byte = "Byte"
        switch cena {
        case nil, .aprovar:
            conversas["byte"] = Conversa(estado: .precisaResposta, itens: MXExemplo.aprovar(autor: byte), fila: [],
                                         trabalhando: true, esperandoDecisao: true, comoNa06: true)
        case .conversa, .sessoes, .ajustesDoAgente:
            conversas["byte"] = Self.trabalhando(byte, fila: MXExemplo.fila)
        case .sessaoAberta:
            // 07b (ChatSessao): a sessão aberta não tem fila no desenho.
            conversas["byte"] = Self.trabalhando(byte, fila: [])
        case .filaEditando, .menu:
            conversas["byte"] = Self.trabalhando(byte, fila: MXExemplo.fila + [MXExemplo.terceiraDaFila])
        }
    }

    static func trabalhando(_ autor: String, fila: [MXItemDaFila], atividade: String = "Adicionando a guarda de data inválida") -> Conversa {
        Conversa(estado: .trabalhando, itens: MXExemplo.historico(autor: autor) + [MXExemplo.trabalho(autor: autor, atividade: atividade)],
                 fila: fila, trabalhando: true, esperandoDecisao: false)
    }

    /// A conversa de exemplo de um agente, criada na primeira vez que abre.
    private func dados(_ id: String) -> Conversa? {
        if let pronta = conversas[id] { return pronta }
        if id == MXExemplo.berga.id {
            return Conversa(estado: MXExemplo.berga.estado, itens: [
                MXItemDaConversa(id: "berga-sep", .separador("Hoje 08:30")),
                MXItemDaConversa(id: "berga-1", .agente(MXMensagemDoAgente(autor: MXExemplo.berga.nome, hora: "08:30", cabecalho: true,
                                                                              texto: MXExemplo.berga.mensagem))),
            ], fila: [], trabalhando: false, esperandoDecisao: false)
        }
        guard let a = MXExemplo.agentes.first(where: { $0.id == id }) else { return nil }
        if a.estado == .trabalhando { return Self.trabalhando(a.nome, fila: [], atividade: a.previa) }
        return Conversa(estado: a.estado, itens: MXExemplo.historico(autor: a.nome), fila: [], trabalhando: false, esperandoDecisao: false)
    }

    private func mudar(_ id: String, _ mudanca: (inout Conversa) -> Void) {
        guard var c = dados(id) else { return }
        mudanca(&c)
        conversas[id] = c
    }

    /// Id novo, que nunca colide com os do desenho (`fila-1`…): prefixo próprio e contador.
    private func novoId(_ prefixo: String) -> String {
        contador += 1
        return "nova-\(prefixo)-\(contador)"
    }

    /// Como o desenho: tudo ligado; o que ainda não existe leva a "Em breve".
    public let capacidades = MXCapacidades.exemplo

    // MARK: MXFonte

    public var conexao: MXEstadoConexao { .conectado }
    public var faixaDeQueda: Bool { false }
    public var podeTentarAgora: Bool { false }
    public func tentarAgora() {}

    public var lista: MXConversasDados {
        MXConversasDados(berga: MXExemplo.berga, space: MXExemplo.space, canais: MXExemplo.canais,
                         agentes: MXExemplo.agentes.filter { !excluidos.contains($0.id) })
    }

    public var pendencias: [MXAba: Int] { MXExemplo.pendencias }

    public var aviso: String? { nil }

    public func conversa(_ id: String) -> MXConversaDados? {
        guard let c = dados(id) else { return nil }
        let berga = id == MXExemplo.berga.id
        let agente = MXExemplo.agentes.first { $0.id == id }
        return MXConversaDados(
            id: id, nome: berga ? MXExemplo.berga.nome : agente?.nome ?? id,
            funcao: berga ? MXExemplo.berga.space : MXExemplo.funcaoLonga(id),
            rosto: berga ? MXExemplo.berga.rosto : agente?.rosto ?? .conjunto("base"),
            estado: c.estado, autonomia: (autonomias[id] ?? .pedir).nome, fixado: fixados.contains(id), itens: c.itens, fila: c.fila,
            trabalhando: c.trabalhando, esperandoDecisao: c.esperandoDecisao,
            modelo: c.comoNa06 ? nil : MXExemplo.modelo, contexto: c.comoNa06 ? nil : MXExemplo.contexto,
            mostraWhatsApp: !c.comoNa06
        )
    }

    public func rascunho(_ id: String) -> String { rascunhos[id] ?? "" }

    public func guardarRascunho(_ texto: String, em id: String) {
        if rascunhos[id] != texto { rascunhos[id] = texto }
    }

    public func abrindo(_ id: String) async {}

    public func lendo(_ id: String, token: UUID, ativo: Bool) {}

    /// O exemplo já traz a conversa inteira.
    public func podeCarregarAnteriores(_ id: String) -> Bool { false }
    public func carregarAnteriores(_ id: String) async -> Bool { true }

    public func enviar(_ texto: String, para id: String) {
        let t = texto.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { return }
        let fila = novoId("fila")
        let balao = novoId("enviada")
        mudar(id) { c in
            if c.trabalhando || c.esperandoDecisao {
                c.fila.append(MXItemDaFila(id: fila, texto: t, revisao: 1))
            } else {
                c.itens.append(MXItemDaConversa(id: balao, .usuario(MXMensagemDoUsuario(texto: t))))
            }
        }
    }

    public func reenviar(_ mensagem: String, em id: String) {}

    public func parar(_ id: String) {
        mudar(id) { c in
            c.itens = c.itens.compactMap { item in
                switch item.conteudo {
                case .trabalhando: return nil
                case let .aprovacao(p) where p.pendente: return MXItemDaConversa(id: item.id, .aprovacao(Self.com(p, .encerrado("Cancelado"))))
                default: return item
                }
            }
            c.trabalhando = false
            c.esperandoDecisao = false
            c.estado = .livre
        }
    }

    public func mandarAgora(_ item: MXItemDaFila, em id: String) {
        let balao = novoId("agora")
        let nome = conversa(id)?.nome ?? ""
        mudar(id) { c in
            c.fila.removeAll { $0.id == item.id }
            c.itens.append(MXItemDaConversa(id: balao, .usuario(MXMensagemDoUsuario(texto: item.texto, nota: "Mandada agora · \(nome) lê em seguida"))))
        }
    }

    /// No exemplo o salvar vale na hora: nunca fica a caminho.
    public func salvando(_ item: String, em id: String) -> Bool { false }

    public func editar(_ item: MXItemDaFila, texto: String, revisao: Int64, em id: String) -> Task<Bool, Never>? {
        var achou = false
        mudar(id) { c in
            guard let i = c.fila.firstIndex(where: { $0.id == item.id }) else { return }
            c.fila[i] = MXItemDaFila(id: item.id, texto: texto, revisao: c.fila[i].revisao + 1)
            achou = true
        }
        let ok = achou
        return Task { ok }
    }

    public func remover(_ item: MXItemDaFila, em id: String) {
        mudar(id) { c in c.fila.removeAll { $0.id == item.id } }
    }

    public func responder(_ pedido: MXPedidoDeAprovacao, opcao: String, em id: String) {
        mudar(id) { c in
            var permitiu = false
            c.itens = c.itens.map { item in
                guard case let .aprovacao(p) = item.conteudo, p.versao == pedido.versao, p.pendente else { return item }
                let escolhida = p.opcoes.first { $0.id == opcao }
                permitiu = escolhida?.permite ?? false
                let texto = escolhida.map { $0.permite ? ($0.principal ? "Permitido uma vez" : "Sempre permitido") : "Recusado" } ?? "Respondido"
                return MXItemDaConversa(id: item.id, .aprovacao(Self.com(p, permitiu ? .permitido(texto) : .recusado(texto))))
            }
            c.itens = c.itens.map { item in
                guard case let .trabalhando(t) = item.conteudo, t.esperandoDecisao else { return item }
                return MXItemDaConversa(id: item.id, .trabalhando(MXTrabalhoEmCurso(
                    autor: t.autor, atividade: t.atividade, passos: t.passos, esperandoDecisao: false,
                    decisao: permitiu ? "✓ Você permitiu" : "✕ Você recusou")))
            }
            c.esperandoDecisao = false
            c.estado = permitiu ? .trabalhando : .livre
            c.trabalhando = permitiu
        }
    }

    public func alternarFixar(_ id: String) {
        if fixados.contains(id) { fixados.remove(id) } else { fixados.insert(id) }
    }

    public func limparAviso() {}

    // MARK: Sessões e ajustes (exemplo)

    /// Nome, função, rosto e estado de um agente (ou do Berga) do exemplo.
    private func cabecalho(_ id: String) -> (nome: String, funcao: String, rosto: MXRosto, estado: MexeriquinhaState, texto: String)? {
        if id == MXExemplo.berga.id {
            let b = MXExemplo.berga
            return (b.nome, b.space, b.rosto, b.estado, b.estadoTexto)
        }
        guard !excluidos.contains(id), let a = MXExemplo.agentes.first(where: { $0.id == id }) else { return nil }
        return (a.nome, a.funcao, a.rosto, a.estado, MXEstadoRegras.texto(a.estado))
    }

    private func arvore(_ id: String) -> [MXSessao] { arvores[id] ?? MXExemplo.sessoes(id).sessoes }

    public func sessoes(_ id: String) -> MXSessoesDados? {
        guard let c = cabecalho(id) else { return nil }
        return MXSessoesDados(agenteId: id, nome: c.nome, funcao: c.funcao, rosto: c.rosto, estado: c.estado, estadoTexto: c.texto,
                              sessoes: arvore(id), arquivadas: MXExemplo.sessoes(id).arquivadas + (arquivadasNovas[id]?.count ?? 0),
                              podeArquivar: true, podeCriar: true)
    }

    public func carregarSessoes(_ id: String) async {}

    /// No exemplo, toda sessão abre a conversa de exemplo do agente (como `telas/ChatSessao.dc.html`).
    public func conversaDaSessao(_ sessao: String, em id: String) -> String? { cabecalho(id) == nil ? nil : id }

    public func novaSessao(em id: String) -> String? {
        guard cabecalho(id) != nil else { return nil }
        let nova = MXSessao(id: novoId("sessao"), nome: "Nova sessão", estado: .livre, hora: "agora")
        var lista = arvore(id)
        let depoisDaPrincipal = lista.firstIndex { !$0.principal } ?? lista.count
        lista.insert(nova, at: depoisDaPrincipal)
        arvores[id] = lista
        return nova.id
    }

    public func arquivarSessao(_ sessao: String, em id: String) {
        var lista = arvore(id)
        guard let tirada = Self.tirar(sessao, de: &lista), !tirada.principal else { return }
        arvores[id] = lista
        arquivadasNovas[id, default: []].append(tirada)
    }

    public func desfazerArquivar(_ sessao: String, em id: String) {
        guard let i = arquivadasNovas[id]?.firstIndex(where: { $0.id == sessao }), let volta = arquivadasNovas[id]?.remove(at: i) else { return }
        // Volta no lugar de origem do desenho quando há; senão, no fim.
        let original = MXExemplo.sessoes(id).sessoes.map(\.id)
        var lista = arvore(id)
        if let pai = Self.pai(de: sessao, em: MXExemplo.sessoes(id).sessoes), Self.inserir(volta, filhaDe: pai, em: &lista) {
            arvores[id] = lista
            return
        }
        let posicao = original.firstIndex(of: sessao).map { alvo in lista.firstIndex { (original.firstIndex(of: $0.id) ?? .max) > alvo } ?? lista.count }
        lista.insert(volta, at: posicao ?? lista.count)
        arvores[id] = lista
    }

    private static func tirar(_ id: String, de lista: inout [MXSessao]) -> MXSessao? {
        if let i = lista.firstIndex(where: { $0.id == id }) { return lista.remove(at: i) }
        for i in lista.indices { if let achou = tirar(id, de: &lista[i].filhas) { return achou } }
        return nil
    }

    private static func pai(de id: String, em lista: [MXSessao]) -> String? {
        for s in lista {
            if s.filhas.contains(where: { $0.id == id }) { return s.id }
            if let p = pai(de: id, em: s.filhas) { return p }
        }
        return nil
    }

    private static func inserir(_ s: MXSessao, filhaDe pai: String, em lista: inout [MXSessao]) -> Bool {
        for i in lista.indices {
            if lista[i].id == pai { lista[i].filhas.append(s); return true }
            if inserir(s, filhaDe: pai, em: &lista[i].filhas) { return true }
        }
        return false
    }

    public func ajustes(_ id: String) -> MXAjustesDoAgente? {
        guard let c = cabecalho(id) else { return nil }
        return MXAjustesDoAgente(id: id, nome: c.nome, funcao: c.funcao, rosto: c.rosto, estado: c.estado,
                                 sincronia: MXAjustesRegras.sincronia(computador: "MacBook Pro", conectado: true), sincronizado: true,
                                 autonomia: autonomias[id] ?? .pedir, itens: MXExemplo.itensDeAjuste,
                                 avisos: !avisosDesligados.contains(id), podeExportar: true, podeExcluir: true)
    }

    public func mudarAutonomia(_ autonomia: MXAutonomia, em id: String) {
        guard autonomia.disponivel else { return }
        autonomias[id] = autonomia
    }

    public func alternarAvisos(em id: String) {
        if avisosDesligados.contains(id) { avisosDesligados.remove(id) } else { avisosDesligados.insert(id) }
    }

    public func excluirAgente(_ id: String) -> Bool {
        guard cabecalho(id) != nil, id != MXExemplo.berga.id else { return false }
        excluidos.insert(id)
        return true
    }

    /// O mesmo pedido com outro resultado.
    static func com(_ p: MXPedidoDeAprovacao, _ resultado: MXResultadoDaAprovacao) -> MXPedidoDeAprovacao {
        MXPedidoDeAprovacao(id: p.id, revisao: p.revisao + 1, titulo: p.titulo, detalhe: p.detalhe, acao: p.acao, onde: p.onde, opcoes: p.opcoes,
                            resultado: resultado, respondendo: nil, detalhes: p.detalhes, diffs: p.diffs)
    }
}
