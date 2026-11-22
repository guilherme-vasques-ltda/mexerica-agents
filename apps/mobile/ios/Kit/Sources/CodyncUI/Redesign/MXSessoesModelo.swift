import CodyncKit
import Foundation

// MARK: - Sessões (07, 07b)

/// O estado de uma sessão na árvore (`telas/Sessoes.dc.html`): pede você (laranja), trabalhando (verde, com gomo)
/// ou livre (anel, mostra a hora).
public enum MXEstadoDaSessao: Equatable, Sendable {
    case precisaDeVoce, trabalhando, livre
}

/// Uma sessão do agente (ou do canal), com as filhas.
public struct MXSessao: Identifiable, Equatable, Sendable {
    public let id: String
    public let nome: String
    public let estado: MXEstadoDaSessao
    /// "09:12", "Ontem", "Seg", "2 nov".
    public let hora: String
    /// A Sessão Principal (a conversa de sempre do agente): não arquiva.
    public let principal: Bool
    public var filhas: [MXSessao]

    public init(id: String, nome: String, estado: MXEstadoDaSessao, hora: String, principal: Bool = false,
                filhas: [MXSessao] = []) {
        self.id = id
        self.nome = nome
        self.estado = estado
        self.hora = hora
        self.principal = principal
        self.filhas = filhas
    }
}

/// A tela Sessões de um agente.
public struct MXSessoesDados: Equatable, Sendable {
    public let agenteId: String
    public let nome: String
    public let funcao: String
    public let rosto: MXRosto
    public let estado: MexeriquinhaState
    public let estadoTexto: String
    /// As raízes da árvore (a Principal primeiro).
    public let sessoes: [MXSessao]
    /// Quantas estão arquivadas; nulo = a fonte não sabe (a linha "Arquivadas" fica oculta).
    public let arquivadas: Int?
    /// A fonte sabe arquivar (o botão de arquivar aparece nas paradas).
    public let podeArquivar: Bool
    /// A fonte sabe criar uma sessão e abrir nela; senão, "Nova sessão" fica à vista e desligada ("Em breve").
    public let podeCriar: Bool
    /// A voz do topo leva a algum lugar; senão, fica desligada ("Em breve").
    public let podeLigar: Bool
    /// Uma frase sobre o que ainda não veio (ex.: as outras sessões só com o computador conectado).
    public let nota: String?

    public init(agenteId: String, nome: String, funcao: String, rosto: MXRosto, estado: MexeriquinhaState,
                estadoTexto: String, sessoes: [MXSessao], arquivadas: Int?, podeArquivar: Bool, podeCriar: Bool,
                podeLigar: Bool = true, nota: String? = nil) {
        self.agenteId = agenteId
        self.nome = nome
        self.funcao = funcao
        self.rosto = rosto
        self.estado = estado
        self.estadoTexto = estadoTexto
        self.sessoes = sessoes
        self.arquivadas = arquivadas
        self.podeArquivar = podeArquivar
        self.podeCriar = podeCriar
        self.podeLigar = podeLigar
        self.nota = nota
    }
}

/// Uma linha da árvore como a tela mostra.
public struct MXLinhaDeSessao: Identifiable, Equatable, Sendable {
    public let id: String
    public let nome: String
    public let estado: MXEstadoDaSessao
    /// "Precisa de você", "Trabalhando", a hora ou, na busca, "em <pai>".
    public let info: String
    /// Profundidade na árvore (0 na raiz e na busca), já limitada a `MX.Sessoes.niveisMaximos`.
    public let nivel: Int
    public let principal: Bool
    /// Tem filhas (e não está na busca): mostra a seta que abre e fecha.
    public let temFilhas: Bool
    public let aberta: Bool
    /// Quantas sessões há dentro, quando recolhida.
    public let quantidade: Int?
    public let arquivavel: Bool
    /// A informação é "em <pai>" (busca): o nome do resultado tem a prioridade e o pai é que corta.
    public let infoEhDoPai: Bool

    /// O que o VoiceOver lê.
    public var rotuloAcessivel: String {
        var partes = [nome, info]
        if let quantidade { partes.append(MXSessoesRegras.dentro(quantidade)) }
        if temFilhas { partes.append(aberta ? String(localized: "aberta", bundle: .module) : String(localized: "recolhida", bundle: .module)) }
        return partes.joined(separator: ", ")
    }
}

/// "Agora · 2" e "Paradas · 6".
public struct MXGrupoDeSessoes: Identifiable, Equatable, Sendable {
    public let titulo: String
    /// Quantas raízes há no grupo.
    public let quantidade: Int
    public let linhas: [MXLinhaDeSessao]
    public var id: String { titulo }
}

/// Regras puras da tela Sessões (testadas).
public enum MXSessoesRegras {
    /// A sessão (ou alguma filha) está trabalhando ou pede você.
    public static func ativa(_ s: MXSessao) -> Bool { s.estado != .livre || s.filhas.contains(where: ativa) }

    /// Quantas sessões há na lista, contando as filhas.
    public static func conta(_ lista: [MXSessao]) -> Int { lista.reduce(0) { $0 + 1 + conta($1.filhas) } }

    /// "17 sessões", "1 sessão".
    public static func contagem(_ n: Int) -> String {
        n == 1 ? String(localized: "\(n) sessão", bundle: .module) : String(localized: "\(n) sessões", bundle: .module)
    }

    /// "3 sessões dentro".
    static func dentro(_ n: Int) -> String {
        n == 1 ? String(localized: "\(n) sessão dentro", bundle: .module) : String(localized: "\(n) sessões dentro", bundle: .module)
    }

    /// O texto da direita da linha.
    public static func info(_ s: MXSessao) -> String {
        switch s.estado {
        case .precisaDeVoce: String(localized: "Precisa de você", bundle: .module)
        case .trabalhando: String(localized: "Trabalhando", bundle: .module)
        case .livre: s.hora
        }
    }

    /// Arquivar só a parada que não é a Principal (e só se a fonte souber arquivar).
    public static func arquivavel(_ s: MXSessao, fonteArquiva: Bool) -> Bool {
        fonteArquiva && !s.principal && !ativa(s)
    }

    /// A profundidade desenhada: a árvore vai até 4 níveis (README); mais fundo que isso fica no 4º.
    public static func nivelVisivel(_ nivel: Int) -> Int { min(max(0, nivel), MX.Sessoes.niveisMaximos - 1) }

    /// As linhas em "Agora" (a Principal e as que trabalham ou pedem você, com as filhas) e "Paradas". Na busca, as
    /// sessões cujo nome tem o texto, de qualquer nível, sem árvore, com "em <pai>" quando têm pai.
    public static func grupos(_ sessoes: [MXSessao], busca: String, abertas: Set<String>,
                              fonteArquiva: Bool) -> [MXGrupoDeSessoes] {
        let q = MXConversasRegras.normalizar(busca)
        let agora = sessoes.filter { $0.principal || ativa($0) }
        let paradas = sessoes.filter { !($0.principal || ativa($0)) }
        var grupos: [MXGrupoDeSessoes] = []
        for (titulo, raizes) in [(String(localized: "Agora", bundle: .module), agora), (String(localized: "Paradas", bundle: .module), paradas)] {
            var linhas: [MXLinhaDeSessao] = []
            plano(raizes, nivel: 0, pai: nil, q: q, abertas: abertas, fonteArquiva: fonteArquiva, em: &linhas)
            if !linhas.isEmpty { grupos.append(MXGrupoDeSessoes(titulo: titulo, quantidade: raizes.count, linhas: linhas)) }
        }
        return grupos
    }

    private static func plano(_ lista: [MXSessao], nivel: Int, pai: String?, q: String, abertas: Set<String>,
                              fonteArquiva: Bool, em linhas: inout [MXLinhaDeSessao]) {
        for s in lista {
            let buscando = !q.isEmpty
            let aberta = !buscando && abertas.contains(s.id)
            if !buscando || MXConversasRegras.normalizar(s.nome).contains(q) {
                let temFilhas = !s.filhas.isEmpty && !buscando
                linhas.append(MXLinhaDeSessao(
                    id: s.id, nome: s.nome, estado: s.estado,
                    info: buscando && pai != nil ? String(localized: "em \(pai!)", bundle: .module) : info(s),
                    nivel: buscando ? 0 : nivelVisivel(nivel), principal: s.principal,
                    temFilhas: temFilhas, aberta: aberta,
                    quantidade: temFilhas && !aberta ? conta(s.filhas) : nil,
                    arquivavel: arquivavel(s, fonteArquiva: fonteArquiva),
                    infoEhDoPai: buscando && pai != nil))
            }
            if buscando || aberta {
                plano(s.filhas, nivel: nivel + 1, pai: s.nome, q: q, abertas: abertas, fonteArquiva: fonteArquiva, em: &linhas)
            }
        }
    }

    /// Onde a árvore começa: tudo recolhido (README, "Gestos": "Tudo começa recolhido"), salvo
    /// `MX.Sessoes.comecaRecolhida` falso, que abre as que estão ativas (como a captura 07).
    public static func abertasIniciais(_ sessoes: [MXSessao], recolhida: Bool = MX.Sessoes.comecaRecolhida) -> Set<String> {
        guard !recolhida else { return [] }
        var abertas: Set<String> = []
        func visitar(_ lista: [MXSessao]) {
            for s in lista where !s.filhas.isEmpty && (s.principal || ativa(s)) {
                abertas.insert(s.id)
                visitar(s.filhas)
            }
        }
        visitar(sessoes)
        return abertas
    }

    /// "Nenhuma sessão com “x”."
    public static func semResultado(_ busca: String) -> String { String(localized: "Nenhuma sessão com “\(busca)”.", bundle: .module) }

    /// Puxar a linha da sessão para a esquerda: o deslocamento (até 140) e se abre ao soltar (passou de 70).
    public static func arrastoDaSessao(_ dx: CGFloat) -> CGFloat {
        min(max(0, dx), MX.Movimento.puxarSessaoMaximo)
    }

    public static func abreAoSoltar(_ arrasto: CGFloat) -> Bool { arrasto > MX.Movimento.puxarSessaoLimiar }

    /// Puxar o agente na lista de Conversas para a esquerda: o deslocamento (até 150) e se abre as Sessões (passou de 80).
    public static func arrastoParaSessoes(_ dx: CGFloat) -> CGFloat {
        min(max(0, dx), MX.Movimento.puxarParaSessoesMaximo)
    }

    public static func abreSessoesAoSoltar(_ arrasto: CGFloat) -> Bool { arrasto > MX.Movimento.puxarParaSessoesLimiar }

    /// O arrasto conta como horizontal (e não rolagem) quando anda mais para o lado que para cima ou para baixo.
    public static func ehHorizontal(dx: CGFloat, dy: CGFloat) -> Bool { abs(dx) > abs(dy) }

    /// O aviso de arquivada ("Arquivada: Notas da versão").
    public static func avisoDeArquivada(_ nome: String) -> String { String(localized: "Arquivada: \(nome)", bundle: .module) }
}

/// Textos da tela Sessões.
public enum MXTextoDasSessoes {
    /// Ainda sem ligação: só a Principal, até o computador responder.
    public static var esperandoOComputador: String { String(localized: "As outras sessões aparecem quando o computador estiver conectado.", bundle: .module) }
    /// O computador não respondeu `sessions` (versão antiga ou falha).
    public static var semSessoesNoHub: String { String(localized: "Este computador ainda não mostra as outras sessões no iPhone.", bundle: .module) }
    /// Sessão extra antes de as sessões do agente serem lidas NESTA ligação (o anúncio de envio por sessão vale só
    /// para a ligação em que foi lido): a caixa espera.
    public static var conferindoEnvio: String { String(localized: "Carregando sessões…", bundle: .module) }
    /// A leitura das sessões falhou nesta ligação: a caixa da extra continua desligada.
    public static var sessoesNaoVieram: String { String(localized: "Não foi possível carregar as sessões. Volte e abra de novo.", bundle: .module) }
    /// O computador listou as sessões sem confirmar o envio por sessão (Hub antigo pode mandar para a Principal):
    /// nada sai daqui para não cair na conversa errada.
    public static var envioNaoConfirmado: String { String(localized: "Este computador não confirmou mensagens nesta sessão. Mande pelo computador.", bundle: .module) }
    /// O histórico da sessão não veio.
    public static var historicoNaoVeio: String { String(localized: "Não foi possível carregar esta sessão. Tente de novo.", bundle: .module) }
}

/// Um toque que pode virar gesto (puxar, segurar): se virou, o toque que o termina não abre. Zera no começo de cada
/// toque e no fim ou cancelamento de um gesto que não chegou a ser usado, então um puxão curto nunca engole o toque
/// seguinte. Regra pura (testada), usada pelas linhas da árvore e da lista.
public struct MXGestoDoToque: Equatable, Sendable {
    public private(set) var usado = false
    public private(set) var tocando = false

    public init() {}

    /// O dedo desceu (ou se mexeu pela primeira vez neste toque).
    public mutating func movimento() {
        guard !tocando else { return }
        tocando = true
        usado = false
    }

    /// O gesto valeu (abriu as Sessões, os ajustes, a sessão): o toque que termina não abre de novo.
    public mutating func usar() { usado = true }

    /// O gesto acabou (ou foi cancelado). `valeu`: o gesto fez algo; senão, nada fica marcado.
    public mutating func fim(valeu: Bool) {
        tocando = false
        usado = valeu
    }

    /// O toque do botão: abre se nenhum gesto valeu neste toque; consome a marca.
    public mutating func toqueAbre() -> Bool {
        defer { usado = false }
        return !usado
    }
}

/// Texto e valor acessível do que a fonte não oferece: à vista, desligado.
public enum MXIndisponivel {
    public static var emBreve: String { String(localized: "Em breve", bundle: .module) }
    public static var valorAcessivel: String { String(localized: "Indisponível", bundle: .module) }
}

// MARK: - Ajustes do agente (08)

/// Autonomia do agente (`telas/ConfigBot.dc.html`): Pedir (`ask`), Liberar (`auto`) e Total (em breve).
public enum MXAutonomia: String, CaseIterable, Identifiable, Sendable {
    case pedir, liberar, total

    public var id: String { rawValue }

    public var nome: String {
        switch self {
        case .pedir: String(localized: "Pedir", bundle: .module)
        case .liberar: String(localized: "Liberar", bundle: .module)
        case .total: String(localized: "Total", bundle: .module)
        }
    }

    /// A frase à direita de "Autonomia".
    public var dica: String {
        switch self {
        case .pedir: String(localized: "Espera sua aprovação", bundle: .module)
        // O desenho diz "Age e avisa, com Desfazer"; não há Desfazer: o texto diz o que acontece.
        case .liberar: String(localized: "Age sem pedir e avisa.", bundle: .module)
        case .total: String(localized: "Em breve", bundle: .module)
        }
    }

    /// Da permissão do bot no Hub (`ask`/`auto`); outra coisa, desconhecida.
    public static func doHub(_ permissao: String) -> MXAutonomia? {
        switch permissao {
        case "ask": .pedir
        case "auto": .liberar
        default: nil
        }
    }

    /// Para a permissão do Hub; Total ainda não existe lá.
    public var paraOHub: String? {
        switch self {
        case .pedir: "ask"
        case .liberar: "auto"
        case .total: nil
        }
    }

    /// Dá para escolher no iPhone (Total fica à vista, desligado, "Em breve").
    public var disponivel: Bool { self != .total || MX.Ajustes.autonomiaTotalDisponivel }
}

/// Uma linha da lista de ajustes ("I Identidade · Nome, função e aparência ›").
public struct MXItemDeAjuste: Identifiable, Equatable, Sendable {
    public let id: MXEmBreve
    public let letra: String
    public let nome: String
    /// Nulo: a fonte não sabe (só o nome aparece).
    public let valor: String?
    /// Leva à tela de dentro; senão, fica desligado com "Em breve".
    public let disponivel: Bool

    public init(_ id: MXEmBreve, letra: String, nome: String, valor: String?, disponivel: Bool = true) {
        self.id = id
        self.letra = letra
        self.nome = nome
        self.valor = valor
        self.disponivel = disponivel
    }
}

/// A folha de ajustes de um agente.
public struct MXAjustesDoAgente: Equatable, Sendable {
    public let id: String
    public let nome: String
    public let funcao: String
    public let rosto: MXRosto
    public let estado: MexeriquinhaState
    /// "Sincronizado com o MacBook Pro · agora" ou "Sem conexão com o MacBook Pro".
    public let sincronia: String
    public let sincronizado: Bool
    /// Nula: a permissão do bot não é uma das conhecidas (nenhum segmento marcado).
    public let autonomia: MXAutonomia?
    public let itens: [MXItemDeAjuste]
    /// "Avisos deste bot"; nulo = a fonte não sabe (a linha some).
    public let avisos: Bool?
    public let podeExportar: Bool
    /// A linha Excluir aparece (o Berga não se exclui).
    public let podeExcluir: Bool
    /// Por que Excluir está desligado aqui ("Exclua no computador"); nulo = liga.
    public let motivoSemExcluir: String?
    /// Ligar leva a algum lugar; senão, fica desligado ("Em breve").
    public let podeLigar: Bool
    /// O pedido de excluir saiu e o computador ainda não respondeu: a linha diz "Excluindo…" e fica desligada; o agente
    /// só some daqui com o ok do computador (se ele recusar, o aviso aparece e a linha volta).
    public let excluindo: Bool

    public init(id: String, nome: String, funcao: String, rosto: MXRosto, estado: MexeriquinhaState, sincronia: String,
                sincronizado: Bool, autonomia: MXAutonomia?, itens: [MXItemDeAjuste], avisos: Bool?, podeExportar: Bool,
                podeExcluir: Bool, motivoSemExcluir: String? = nil, podeLigar: Bool = true, excluindo: Bool = false) {
        self.excluindo = excluindo
        self.id = id
        self.nome = nome
        self.funcao = funcao
        self.rosto = rosto
        self.estado = estado
        self.sincronia = sincronia
        self.sincronizado = sincronizado
        self.autonomia = autonomia
        self.itens = itens
        self.avisos = avisos
        self.podeExportar = podeExportar
        self.podeExcluir = podeExcluir
        self.motivoSemExcluir = motivoSemExcluir
        self.podeLigar = podeLigar
    }
}

/// Regras puras da folha de ajustes (testadas).
public enum MXAjustesRegras {
    /// "Sincronizado com o MacBook Pro · agora"; sem ligação, "Sem conexão com o MacBook Pro".
    public static func sincronia(computador: String, conectado: Bool) -> String {
        let nome = computador.isEmpty ? String(localized: "computador", bundle: .module) : computador
        return conectado ? String(localized: "Sincronizado com o \(nome) · agora", bundle: .module)
            : String(localized: "Sem conexão com o \(nome)", bundle: .module)
    }

    /// "4 conectores · 3 skills" (sem os zerados; nada, "Nenhuma").
    public static func ferramentas(conectores: Int, skills: Int) -> String {
        var partes: [String] = []
        if conectores > 0 {
            partes.append(conectores == 1 ? String(localized: "\(conectores) conector", bundle: .module) : String(localized: "\(conectores) conectores", bundle: .module))
        }
        if skills > 0 {
            partes.append(skills == 1 ? String(localized: "\(skills) skill", bundle: .module) : String(localized: "\(skills) skills", bundle: .module))
        }
        return partes.isEmpty ? String(localized: "Nenhuma", bundle: .module) : partes.joined(separator: " · ")
    }

    /// A primeira linha da descrição do agente (o "Comportamento"); vazia, nula.
    public static func comportamento(_ descricao: String) -> String? {
        let primeira = descricao.split(whereSeparator: \.isNewline).first.map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
        return primeira.isEmpty ? nil : primeira
    }

    /// Excluir desligado com motivo, para uma fonte sem excluir seguro.
    public static var excluirNoComputador: String { String(localized: "Exclua no computador", bundle: .module) }
    public static var excluirIndisponivelAcessivel: String { String(localized: "Indisponível no iPhone", bundle: .module) }
    /// O pedido de excluir saiu e o computador ainda não respondeu.
    public static var excluindo: String { String(localized: "Excluindo…", bundle: .module) }
    /// A ligação mudou durante a exclusão: não se sabe se o computador excluiu.
    public static var exclusaoSemLigacao: String {
        String(localized: "A ligação caiu durante a exclusão. Se o computador excluiu, o bot some da lista ao reconectar.", bundle: .module)
    }

    /// O aviso de um excluir que o computador recusou. O texto do servidor (`motivo`) só serve para reconhecer a recusa:
    /// nunca vai para a tela nem para o VoiceOver. Recusa conhecida tem mensagem do catálogo; as demais, o fixo.
    public static func falhaAoExcluir(_ nome: String, motivo: String?) -> String {
        let base = String(localized: "Não foi possível excluir \(nome).", bundle: .module)
        let recusa = motivo?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() ?? ""
        if recusa.hasPrefix("unknown bot") {
            return base + " " + String(localized: "O computador não encontrou esse bot.", bundle: .module)
        }
        return base + " " + String(localized: "Tente de novo.", bundle: .module)
    }

    /// A altura do segmento de autonomia que se toca: 34 no desenho, 44 de alvo (o trilho cresce junto).
    public static var alturaDoSegmento: CGFloat { max(MX.Ajustes.segmentoAltura, MX.Espaco.alvoMinimo) }

    /// A pergunta de Excluir.
    public static func perguntaDeExcluir(_ nome: String) -> String { String(localized: "Excluir \(nome)?", bundle: .module) }
    public static var avisoDeExcluir: String { String(localized: "O bot e as conversas dele saem deste computador. Não dá para desfazer.", bundle: .module) }
}

/// Onde está o pedido de excluir de um agente.
public enum MXEstadoDaExclusao: Equatable, Sendable {
    /// Saiu e o computador ainda não respondeu.
    case aCaminho
    /// O computador confirmou; o agente sai daqui quando chegar o evento do Hub (o store o tira).
    case confirmada
}

/// A trava do Excluir, por agente (regra pura, testada). Só o resultado do PRÓPRIO pedido mexe na trava de um agente:
/// um erro de outra ação, ou o de outro agente, não solta. Um pedido por agente; o Berga nunca; só com ligação.
public struct MXExclusoes: Equatable, Sendable {
    public private(set) var estados: [String: MXEstadoDaExclusao] = [:]

    public init() {}

    public func estado(_ id: String) -> MXEstadoDaExclusao? { estados[id] }

    /// A linha mostra "Excluindo…" e fica desligada.
    public func excluindo(_ id: String) -> Bool { estados[id] != nil }

    public func podePedir(_ id: String, berga: Bool, conectado: Bool) -> Bool {
        !berga && conectado && estados[id] == nil
    }

    /// O toque: trava o agente. Falso se não pode (nada sai).
    public mutating func pedir(_ id: String, berga: Bool, conectado: Bool) -> Bool {
        guard podePedir(id, berga: berga, conectado: conectado) else { return false }
        estados[id] = .aCaminho
        return true
    }

    /// O resultado do pedido deste agente: ok fica travado até o agente sair; falha solta (pode pedir de novo).
    public mutating func concluir(_ id: String, ok: Bool) {
        guard estados[id] == .aCaminho else { return }
        estados[id] = ok ? .confirmada : nil
    }

    /// O resultado conferido com a ligação: se ela mudou durante a chamada (`mesmoLink` falso), o resultado não vale
    /// nesta ligação. Não marca confirmada nem falha: solta a trava e devolve o aviso de ligação caída (um novo toque
    /// pode sair; se o computador já excluiu, ele só responde "unknown bot"). Na mesma ligação, como `concluir(_:ok:)`
    /// e devolve nulo.
    public mutating func concluir(_ id: String, ok: Bool, mesmoLink: Bool) -> String? {
        guard mesmoLink else {
            guard estados[id] == .aCaminho else { return nil }
            estados[id] = nil
            return MXAjustesRegras.exclusaoSemLigacao
        }
        concluir(id, ok: ok)
        return nil
    }

    /// O agente saiu do espelho (o evento do Hub chegou): não há mais o que travar.
    public mutating func saiu(_ id: String) { estados[id] = nil }
}
