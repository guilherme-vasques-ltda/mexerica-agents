#if DEBUG
import CodyncKit
import Foundation
import Observation
import SwiftUI

// Mission Board v1 (PLANO §4.2; fonte confiável em MISSION-BOARD-FONTE.md). Só em Debug: em Release este arquivo e os outros
// desta tela não existem, e os textos ficam aqui, em código (pt-BR e en), não nos catálogos, que entram em todos os builds.
//
// O que a v1 é: um espelho DERIVADO do que o app já recebeu (roster do `BotStore`, `pendingQuestions`/`needsInput` e os
// cartões `permission`/`pending` das entradas carregadas da conversa Principal). Não lê a Torre de Controle (v1.1), não
// responde nada (abrir a conversa é a única ação) e nunca afirma "zero global": a cobertura é PARCIAL e a tela diz isso
// sempre, inclusive com a lista vazia.

// MARK: - Textos

/// Os textos da tela, em pt-BR e en (qualquer outro idioma cai no pt-BR, como o app).
public struct MXTextosDoQuadro: Equatable, Sendable {
    public let ingles: Bool

    public init(ingles: Bool = MXTextosDebug.ingles()) {
        self.ingles = ingles
    }

    private func t(_ pt: String, _ en: String) -> String { ingles ? en : pt }

    public var titulo: String { "Mission Board" }
    public var ajustesEConta: String { t("Ajustes e conta", "Settings and account") }

    // Ligação (subtítulo)
    public func aoVivo(_ computador: String) -> String { t("Ao vivo · \(computador)", "Live · \(computador)") }
    public func reconectando(_ computador: String) -> String { t("Reconectando · \(computador)", "Reconnecting · \(computador)") }
    public func semConexao(_ computador: String) -> String { t("Sem conexão · \(computador)", "No connection · \(computador)") }
    public func atualizando(_ computador: String) -> String { t("Atualizando · \(computador)", "Updating · \(computador)") }
    public var ligacaoAoVivo: String { t("Ao vivo", "Live") }
    public var ligacaoParada: String { t("Sem conexão", "No connection") }
    public var ligacaoReconectando: String { t("Reconectando", "Reconnecting") }

    // Blocos e seções
    public var voce: String { t("Você", "You") }
    public var trabalhando: String { t("Trabalhando", "Working") }
    public var problema: String { t("Problema", "Problem") }
    public var livres: String { t("Livres", "Free") }
    public var secaoVoce: String { t("Precisam de você", "Need you") }
    public var secaoTrabalhando: String { t("Trabalhando agora", "Working now") }
    public var secaoProblema: String { t("Com problema", "With a problem") }
    public func secaoComContagem(_ titulo: String, _ n: Int) -> String { "\(titulo) · \(n)" }
    public func blocoValor(_ n: Int) -> String { "\(n)" }

    // Filtro (texto grande)
    public var filtrarTodos: String { t("Filtrar: todos", "Filter: all") }
    public func filtrar(_ nome: String) -> String { t("Filtrar: \(nome)", "Filter: \(nome)") }
    public var mostrarTodos: String { t("Mostrar todos", "Show all") }
    /// Os blocos: tocar de novo no ativo limpa.
    public var dicaDoFiltro: String { t("Toque de novo no filtro ativo para limpar", "Tap the active filter again to clear it") }
    /// O menu do texto grande: escolher o mesmo valor mantém o filtro; limpar é escolher "Mostrar todos".
    public var dicaDoMenuDeFiltro: String { t("Escolha Mostrar todos para limpar o filtro", "Choose Show all to clear the filter") }

    // Cartões
    public var pedePermissao: String { t("Pede sua permissão para uma ação", "Asks your permission for an action") }
    public var temPergunta: String { t("Tem uma pergunta para você", "Has a question for you") }
    public var esperaVoce: String { t("Espera algo de você", "Is waiting on you") }
    public var abrir: String { t("Abrir", "Open") }
    public func abrirConversa(_ nome: String) -> String { t("Abrir a conversa de \(nome)", "Open \(nome)'s chat") }
    public var parouComErro: String { t("Parou com erro", "Stopped with an error") }
    public var estadoNaoInformado: String { t("Estado não informado", "Status not reported") }
    public var semMensagens: String { t("Sem mensagens ainda", "No messages yet") }
    public var livre: String { t("Livre", "Free") }
    public var trabalhandoRotulo: String { t("Trabalhando", "Working") }
    public var dicaDoAbrir: String {
        t("Abre a conversa principal do bot. Se o pedido estiver em outra sessão ou num canal, ele não aparece lá.",
          "Opens the bot's main chat. If the request is in another session or a channel, it won't show up there.")
    }

    // Livres
    public func livresRecolhido(_ n: Int) -> String { t("Livres, \(n), recolhido", "Free, \(n), collapsed") }
    public func livresAberto(_ n: Int) -> String { t("Livres, \(n), aberto", "Free, \(n), expanded") }
    public var dicaLivresAbrir: String { t("Toque para mostrar quem está livre", "Tap to show who is free") }
    public var dicaLivresFechar: String { t("Toque para recolher", "Tap to collapse") }

    // Cobertura (sempre à vista, mesmo com a lista vazia)
    public var coberturaTitulo: String { t("Cobertura parcial", "Partial coverage") }
    public var cobertura: String {
        t("Só mostra o que este iPhone já recebeu. Pedidos em outras sessões, em canais ou ainda não carregados podem faltar.",
          "Only shows what this iPhone has already received. Requests in other sessions, in channels or not loaded yet may be missing.")
    }
    public var coberturaCarregando: String {
        t("Ainda atualizando: o que aparece pode estar incompleto.", "Still updating: what shows up may be incomplete.")
    }

    // Vazio com ressalva
    public var vazioTitulo: String { t("Nada à vista neste iPhone", "Nothing in sight on this iPhone") }
    public var vazioTexto: String {
        t("Nenhum pedido ou pergunta chegou aqui. Isso não garante que não haja outros: confira no computador.",
          "No request or question has reached here. That doesn't mean there are no others: check on the computer.")
    }
    public var semBotsTitulo: String { t("Nenhum bot à vista", "No bots in sight") }
    public var semBotsTexto: String {
        t("Este computador ainda não mostrou nenhum bot a este iPhone. Crie um no computador.",
          "This computer hasn't shown any bot to this iPhone yet. Create one on the computer.")
    }

    // Estados
    public var carregandoTitulo: String { t("Carregando…", "Loading…") }
    public var carregandoTexto: String { t("Buscando o que o computador já tem.", "Fetching what the computer already has.") }
    public var semComputadorTitulo: String { t("Nenhum computador conectado", "No computer connected") }
    public var semComputadorTexto: String {
        t("Conecte um computador para ver quem espera por você.", "Connect a computer to see who is waiting on you.")
    }
    public var conectarUm: String { t("Conectar um computador", "Connect a computer") }
    public var abreOsAjustes: String { t("Abre os Ajustes: computadores e conta", "Opens Settings: computers and account") }
    public var semAcessoTitulo: String { t("Sem acesso a este computador", "No access to this computer") }
    public var semAcessoTexto: String {
        t("Este iPhone não tem mais permissão para ver este computador. Por segurança, nada é mostrado. Veja os computadores nos Ajustes.",
          "This iPhone no longer has permission to see this computer. For safety, nothing is shown. Check the computers in Settings.")
    }
    public var abrirAjustes: String { t("Abrir Ajustes", "Open Settings") }
    public var erroTitulo: String { t("Não foi possível ler este computador", "Couldn't read this computer") }
    public var erroTexto: String {
        t("O computador manda dados que este app não entende. Atualize o app ou o computador. Nada é mostrado para não induzir a erro.",
          "The computer sends data this app doesn't understand. Update the app or the computer. Nothing is shown so as not to mislead.")
    }

    // Dados parados (queda)
    public var paradosTitulo: String { t("Sem conexão com o computador", "No connection to the computer") }
    public func paradosTexto(visto: String?) -> String {
        let base = t("Estes dados estão parados e podem estar desatualizados.", "This data is frozen and may be out of date.")
        guard let visto else { return base }
        return "\(base) \(visto)"
    }
    public var paradosSemHora: String { t("São de antes da queda, há mais de 5 min.", "It's from before the drop, more than 5 min ago.") }
    public var tentarAgora: String { t("Tentar agora", "Try now") }
    public var tentando: String { t("Tentando…", "Trying…") }

    // Tempo
    public func ha(_ desde: Date, agora: Date) -> String {
        let minutos = max(0, Int(agora.timeIntervalSince(desde) / 60))
        if minutos < 1 { return t("agora", "just now") }
        if minutos < 60 { return t("há \(minutos) min", "\(minutos) min ago") }
        let horas = minutos / 60
        if horas < 24 { return t("há \(horas) h", "\(horas) h ago") }
        let dias = horas / 24
        return t("há \(dias) d", "\(dias) d ago")
    }

    /// "visto há 3 min": quando a ligação caiu (nulo sem data).
    public func visto(desde: Date?, agora: Date) -> String? {
        guard let desde else { return nil }
        return t("Visto \(ha(desde, agora: agora)).", "Last seen \(ha(desde, agora: agora)).")
    }

    /// "8 min" trabalhando.
    public func duracao(_ desde: Date, agora: Date) -> String {
        let minutos = max(0, Int(agora.timeIntervalSince(desde) / 60))
        if minutos < 1 { return t("agora", "now") }
        if minutos < 60 { return "\(minutos) min" }
        let horas = minutos / 60
        let resto = minutos % 60
        return resto == 0 ? "\(horas) h" : "\(horas) h \(resto) min"
    }

    // VoiceOver

    /// "Byte, pede sua permissão, há 3 minutos".
    public func leituraDoCartao(nome: String, motivo: MXQuadroMotivo, desde: Date?, agora: Date) -> String {
        let quando = desde.map { ", " + ha($0, agora: agora) } ?? ""
        switch motivo {
        case .permissao: return t("\(nome), pede sua permissão\(quando)", "\(nome), asks your permission\(quando)")
        case .pergunta: return t("\(nome), tem uma pergunta para você\(quando)", "\(nome), has a question for you\(quando)")
        case .espera: return t("\(nome), espera algo de você\(quando)", "\(nome), is waiting on you\(quando)")
        }
    }

    public func leituraDoTrabalho(nome: String, detalhe: String, desde: Date?, agora: Date) -> String {
        let atividade = detalhe.isEmpty ? "" : ", " + detalhe
        let tempo = desde.map { ", " + duracao($0, agora: agora) } ?? ""
        return t("\(nome), trabalhando\(atividade)\(tempo)", "\(nome), working\(atividade)\(tempo)")
    }

    public func leituraDoProblema(nome: String) -> String { t("\(nome), parou com erro", "\(nome), stopped with an error") }

    public func leituraDoLivre(nome: String, detalhe: String, desconhecido: Bool) -> String {
        let estado = desconhecido ? estadoNaoInformado : livre
        return detalhe.isEmpty ? "\(nome), \(estado)" : "\(nome), \(estado), \(detalhe)"
    }

    public func nomeDoFiltro(_ filtro: MXQuadroFiltro) -> String {
        switch filtro {
        case .todos: t("todos", "all")
        case .voce: voce
        case .trabalhando: trabalhando
        case .problema: problema
        case .livres: livres
        }
    }
}

// MARK: - Dados

/// Por que o bot pede você. O app só sabe o FATO (e o tipo), nunca o texto do pedido: por isso o cartão é genérico.
public enum MXQuadroMotivo: Equatable, Sendable {
    /// Há um cartão `permission`/`pending` nas entradas carregadas da conversa Principal.
    case permissao
    /// `pendingQuestions` do roster.
    case pergunta
    /// `needsInput` sem cartão nem pergunta conhecidos (pode estar em outra sessão, num canal ou fora da janela).
    case espera
}

public struct MXQuadroItem: Identifiable, Equatable, Sendable {
    public let id: String
    public let nome: String
    public let rosto: MXRosto
    public let estado: MexeriquinhaState
    public let motivo: MXQuadroMotivo?
    /// Permissão: quando o cartão chegou. Trabalhando: quando o turno começou. Nulo quando o app não sabe.
    public let desde: Date?
    /// A atividade (trabalhando) ou a última fala (livre); vazio quando não há.
    public let detalhe: String
    /// O Hub mandou um estado que o app não conhece (`unavailable`, `waiting`…): cai em Livres, dito como "não informado".
    public let desconhecido: Bool

    public init(id: String, nome: String, rosto: MXRosto, estado: MexeriquinhaState, motivo: MXQuadroMotivo? = nil,
                desde: Date? = nil, detalhe: String = "", desconhecido: Bool = false) {
        self.id = id
        self.nome = nome
        self.rosto = rosto
        self.estado = estado
        self.motivo = motivo
        self.desde = desde
        self.detalhe = detalhe
        self.desconhecido = desconhecido
    }
}

/// A ligação com o computador como o quadro a conta: dados ao vivo, em reconexão (parados) ou parados por prazo longo.
public enum MXQuadroLigacao: Equatable, Sendable {
    case aoVivo
    case reconectando(desde: Date)
    case parado
}

public enum MXQuadroEstado: Equatable, Sendable {
    /// Nada guardado ainda e a primeira leitura não terminou.
    case carregando
    /// O app soltou o computador (saiu da conta, removeu): nada a mostrar.
    case semComputador
    /// O computador recusou este aparelho (acesso encerrado, 401/403): falha fechada, nada é mostrado.
    case semAcesso
    /// O computador manda dados que este app não lê: falha fechada, nada é mostrado.
    case erro
    /// Há o que mostrar (inclusive lista vazia, sempre com a ressalva de cobertura parcial).
    case pronto
}

public enum MXQuadroFiltro: String, CaseIterable, Sendable {
    case todos, voce, trabalhando, problema, livres
}

public struct MXQuadroDados: Equatable, Sendable {
    public var estado: MXQuadroEstado
    public var computador: String
    public var ligacao: MXQuadroLigacao
    public var voce: [MXQuadroItem]
    public var trabalhando: [MXQuadroItem]
    public var problema: [MXQuadroItem]
    public var livres: [MXQuadroItem]
    /// A primeira leitura ainda não terminou (há dados do cache, mas podem estar incompletos).
    public var atualizando: Bool

    public init(estado: MXQuadroEstado, computador: String = "", ligacao: MXQuadroLigacao = .aoVivo, voce: [MXQuadroItem] = [],
                trabalhando: [MXQuadroItem] = [], problema: [MXQuadroItem] = [], livres: [MXQuadroItem] = [], atualizando: Bool = false) {
        self.estado = estado
        self.computador = computador
        self.ligacao = ligacao
        self.voce = voce
        self.trabalhando = trabalhando
        self.problema = problema
        self.livres = livres
        self.atualizando = atualizando
    }

    public var total: Int { voce.count + trabalhando.count + problema.count + livres.count }

    /// Só há o que mostrar em `.pronto`; em qualquer outro estado a tela não mostra nenhuma linha de bot.
    public var mostraBots: Bool { estado == .pronto }

    /// O contador da aba: quantos esperam você, e zero fora de `.pronto` (nunca um número de dado que não vale).
    public var contador: Int { mostraBots ? voce.count : 0 }

    /// Pronto, mas sem pedido nenhum à vista (a tela diz "nada à vista", nunca "tudo certo").
    public var semPedidos: Bool { mostraBots && voce.isEmpty }

    public func itens(_ filtro: MXQuadroFiltro) -> Int {
        switch filtro {
        case .todos: total
        case .voce: voce.count
        case .trabalhando: trabalhando.count
        case .problema: problema.count
        case .livres: livres.count
        }
    }
}

// MARK: - Regras (puras)

public enum MXQuadroRegras {
    /// Quando chegou o cartão de permissão pendente mais antigo destas entradas (as da conversa Principal já carregadas).
    /// Nulo: não há (ou não está carregado: o app não separa os dois casos, por isso a tela nunca diz "sem pedidos").
    public static func pedidoPendente(_ entradas: [Entry]) -> Int64? {
        entradas.filter { $0.kind == "permission" && $0.data.status == "pending" }.map(\.createdAt).min()
    }

    /// O estado do quadro a partir da ligação do store. Falha fechada: acesso encerrado vence qualquer outro sinal.
    public static func estadoDaLigacao(conexao: MXEstadoConexao, motivo: MotivoDesconexao?) -> (estado: MXQuadroEstado?, ligacao: MXQuadroLigacao) {
        if case .some(.acessoEncerrado) = motivo { return (.semAcesso, .parado) }
        switch conexao {
        case .conectado: return (nil, .aoVivo)
        case let .reconectando(desde): return (nil, .reconectando(desde: desde))
        case .desconectado:
            if case .some(.encerradoNoApp) = motivo { return (.semComputador, .parado) }
            return (nil, .parado)
        }
    }

    /// Os estados que o Hub manda e o app conhece. Outro (`unavailable`, `waiting`…) cai em Livres, "não informado".
    static let estadosConhecidos: Set<String> = ["idle", "working", "needsInput", "error"]

    /// Monta o quadro. `bots` já vem do roster (sem ocultos); canais (grupos) não entram. `pedidos`: por bot, o cartão de
    /// permissão pendente mais antigo da conversa Principal carregada (`pedidoPendente`).
    public static func montar(bots: [Bot], pedidos: [String: Int64], conexao: MXEstadoConexao, motivo: MotivoDesconexao?,
                              computador: String, carregou: Bool, ilegivel: Bool, agora: Date) -> MXQuadroDados {
        let (bloqueio, ligacao) = estadoDaLigacao(conexao: conexao, motivo: motivo)
        // Falha fechada: sem acesso, sem computador ou dado ilegível, nenhuma linha de bot sai daqui.
        if let bloqueio { return MXQuadroDados(estado: bloqueio, computador: computador, ligacao: ligacao) }
        if ilegivel { return MXQuadroDados(estado: .erro, computador: computador, ligacao: ligacao) }
        let agentes = bots.filter { !$0.isGroup }
        if agentes.isEmpty && !carregou { return MXQuadroDados(estado: .carregando, computador: computador, ligacao: ligacao) }

        var dados = MXQuadroDados(estado: .pronto, computador: computador, ligacao: ligacao, atualizando: !carregou)
        for bot in agentes {
            let estado = bot.avatarState(now: agora)
            let rosto = MXRosto(bot.avatar)
            let desde = { (ms: Int64?) in ms.flatMap { $0 > 0 ? Date(milliseconds: $0) : nil } }
            if let pedido = pedidos[bot.id] {
                dados.voce.append(MXQuadroItem(id: bot.id, nome: bot.name, rosto: rosto, estado: .precisaResposta, motivo: .permissao,
                                               desde: desde(pedido)))
            } else if bot.hasPendingQuestions {
                dados.voce.append(MXQuadroItem(id: bot.id, nome: bot.name, rosto: rosto, estado: .precisaResposta, motivo: .pergunta))
            } else if bot.needsInput {
                dados.voce.append(MXQuadroItem(id: bot.id, nome: bot.name, rosto: rosto, estado: .precisaResposta, motivo: .espera))
            } else if bot.status == "error" {
                dados.problema.append(MXQuadroItem(id: bot.id, nome: bot.name, rosto: rosto, estado: .erro))
            } else if bot.status == "working" {
                dados.trabalhando.append(MXQuadroItem(id: bot.id, nome: bot.name, rosto: rosto, estado: .trabalhando,
                                                      desde: desde(bot.startedAt), detalhe: bot.activity))
            } else {
                dados.livres.append(MXQuadroItem(id: bot.id, nome: bot.name, rosto: rosto, estado: estado,
                                                 detalhe: MessagePreview.text(bot.lastMessage ?? ""),
                                                 desconhecido: !estadosConhecidos.contains(bot.status)))
            }
        }
        return dados
    }

    /// A ressalva que a tela SEMPRE mostra quando há dados: a cobertura é parcial (só o que este iPhone já recebeu), com a lista
    /// cheia ou vazia. Nulo só onde a tela não mostra bot nenhum (e portanto não afirma nada sobre eles).
    public enum Ressalva: Equatable, Sendable { case coberturaParcial }

    public static func ressalva(_ dados: MXQuadroDados) -> Ressalva? {
        dados.mostraBots ? .coberturaParcial : nil
    }

    /// O que a tela diz quando não há cartão: nenhum bot à vista ou nenhum pedido à vista. Nunca "tudo certo".
    public enum Vazio: Equatable, Sendable { case semBots, semPedidos }

    public static func vazio(_ dados: MXQuadroDados) -> Vazio? {
        guard dados.mostraBots else { return nil }
        if dados.total == 0 { return .semBots }
        return dados.voce.isEmpty ? .semPedidos : nil
    }

    /// Abrir/recolher Livres: a animação que também move o layout (`MXTroca.animacaoDaPosicao`); nula com Reduzir movimento.
    public static func animacaoDaPosicaoDosLivres(reduzir: Bool) -> Animation? {
        MX.Movimento.troca(reduzir: reduzir).animacaoDaPosicao
    }

    /// A seta de Livres: 0° recolhido, 180° aberto (só gira de verdade se a troca interpola).
    public static func anguloDaSetaDosLivres(aberto: Bool) -> Double { aberto ? 180 : 0 }

    /// "Abrir" leva à conversa PRINCIPAL do bot (nunca a uma sessão ou canal: o app não sabe onde o pedido está).
    public static func destinoDoAbrir(_ botId: String) -> MXDestino { .conversa(botId) }

    /// O filtro aplicado: o toque no ativo limpa.
    public static func alternar(_ atual: MXQuadroFiltro, _ toque: MXQuadroFiltro) -> MXQuadroFiltro {
        atual == toque || toque == .todos ? .todos : toque
    }

    /// Cada seção aparece com o filtro `todos` ou com o filtro dela.
    public static func mostra(_ secao: MXQuadroFiltro, com filtro: MXQuadroFiltro) -> Bool {
        filtro == .todos || filtro == secao
    }
}

// MARK: - Invalidação

/// Os sinais do ciclo que invalidam o quadro (MISSION-BOARD-FONTE.md §1.3/§4: o conjunto que o desktop relê,
/// `REREAD_EVENTS` de `torre-session.ts`). Na v1 não há leitura a refazer (o quadro é derivado do espelho): cada sinal faz o
/// quadro ser recalculado do que o espelho tem agora, e nenhum resultado fica guardado de um sinal para o outro.
public enum MXQuadroSinal: String, CaseIterable, Sendable {
    case entrada, bot, botExcluido, sessao, conversa, space, hello, resync, torre

    /// O `type` do evento no fio. `bot` com `deleted: true` chega como `botExcluido` (já decodificado pelo app).
    public static func doFio(_ tipo: String) -> MXQuadroSinal? {
        switch tipo {
        case "entry": .entrada
        case "bot": .bot
        case "session": .sessao
        case "conversation": .conversa
        case "spaces": .space
        case "hello": .hello
        case "resync": .resync
        case "donnaControlTowerChanged": .torre
        default: nil
        }
    }

    /// O sinal de um evento já decodificado; nulo para o que não invalida (uso e tela do computador, e o que o app não leu).
    public static func de(_ evento: HostEvent) -> MXQuadroSinal? {
        switch evento {
        case .hello: .hello
        case .bot: .bot
        case .botDeleted: .botExcluido
        case .entry: .entrada
        case .resync: .resync
        case let .cycleChanged(tipo): doFio(tipo)
        case .usage, .screen, .undecodable: nil
        }
    }
}

/// O contador de sinais que o `BotStore` mexe a cada evento do ciclo. O quadro lê `geracao` e por isso é recalculado em todo
/// sinal, mesmo quando o espelho não mudou de forma visível (uma sessão que fecha, um Space que muda).
@MainActor
@Observable
public final class MXQuadroSinais {
    public private(set) var geracao = 0
    public private(set) var ultimo: MXQuadroSinal?
    public private(set) var contagem: [MXQuadroSinal: Int] = [:]

    public init() {}

    public func registrar(_ sinal: MXQuadroSinal?) {
        guard let sinal else { return }
        geracao += 1
        ultimo = sinal
        contagem[sinal, default: 0] += 1
    }
}

// MARK: - A fonte

/// O que uma fonte dá ao Mission Board. As duas fontes do redesenho (real e de exemplo) a implementam só em Debug.
@MainActor
public protocol MXQuadroFonte: MXFonte {
    var quadro: MXQuadroDados { get }
}

extension MXFonteReal: MXQuadroFonte {
    /// O quadro derivado do que o `BotStore` tem agora. Lê o contador de sinais (todo evento do ciclo o mexe), o roster, as
    /// entradas Principais carregadas e a ligação; nada é guardado de uma leitura para a outra.
    public var quadro: MXQuadroDados {
        _ = store.quadroSinais.geracao
        let motivo: MotivoDesconexao? = if case let .desconectado(m) = store.estadoConexao { m } else { nil }
        let bots = store.roster
        var pedidos: [String: Int64] = [:]
        for bot in bots where !bot.isGroup {
            if let quando = MXQuadroRegras.pedidoPendente(store.chat(bot.id)) { pedidos[bot.id] = quando }
        }
        return MXQuadroRegras.montar(bots: bots, pedidos: pedidos, conexao: conexao, motivo: motivo, computador: store.hostName,
                                     carregou: store.caughtUp, ilegivel: store.unreadable || store.mismatch != nil, agora: agora())
    }
}

/// A variação do quadro de exemplo (imagens e prévia sem login).
public enum MXCenaDoQuadro: String, CaseIterable, Sendable {
    case padrao, vazio, semBots, carregando, parado, reconectando, semComputador, semAcesso, erro
}

extension MXFonteExemplo: MXQuadroFonte {
    public var quadro: MXQuadroDados { MXExemplo.quadro(cenaDoQuadro, agora: agoraDoQuadro ?? Date()) }
}

public extension MXExemplo {
    /// Os dados do mockup (`mock-ios-mission-board-*`): Byte pede permissão, Prosa tem pergunta, Crivo trabalha, Trato com
    /// erro e três livres. NÃO são do Hub.
    static func quadro(_ cena: MXCenaDoQuadro, agora: Date) -> MXQuadroDados {
        let computador = "MacBook de exemplo"
        let byte = MXQuadroItem(id: "byte", nome: "Byte", rosto: .conjunto("developer"), estado: .precisaResposta, motivo: .permissao,
                                desde: agora.addingTimeInterval(-3 * 60))
        let prosa = MXQuadroItem(id: "prosa", nome: "Prosa", rosto: .conjunto("assistente"), estado: .precisaResposta, motivo: .pergunta)
        let crivo = MXQuadroItem(id: "crivo", nome: "Crivo", rosto: .conjunto("pesquisa"), estado: .trabalhando,
                                 desde: agora.addingTimeInterval(-8 * 60), detalhe: "Lendo as avaliações da loja")
        let trato = MXQuadroItem(id: "trato", nome: "Trato", rosto: .conjunto("vendas"), estado: .erro)
        let livres = [
            MXQuadroItem(id: "saldo", nome: "Saldo", rosto: .conjunto("financeiro"), estado: .livre, detalhe: "Conferi os repasses de hoje."),
            MXQuadroItem(id: "vitrine", nome: "Vitrine", rosto: .conjunto("marketing"), estado: .dormindo, detalhe: "As novidades da semana chegaram."),
            MXQuadroItem(id: "berga", nome: "Berga", rosto: .conjunto("berga"), estado: .livre, detalhe: "Bom dia!"),
        ]
        switch cena {
        case .padrao:
            return MXQuadroDados(estado: .pronto, computador: computador, voce: [byte, prosa], trabalhando: [crivo], problema: [trato], livres: livres)
        case .vazio:
            return MXQuadroDados(estado: .pronto, computador: computador, livres: livres)
        case .semBots:
            return MXQuadroDados(estado: .pronto, computador: computador)
        case .carregando:
            return MXQuadroDados(estado: .carregando, computador: computador)
        case .parado:
            return MXQuadroDados(estado: .pronto, computador: computador, ligacao: .parado, voce: [byte, prosa], trabalhando: [crivo],
                                 problema: [trato], livres: livres)
        case .reconectando:
            return MXQuadroDados(estado: .pronto, computador: computador, ligacao: .reconectando(desde: agora.addingTimeInterval(-90)),
                                 voce: [byte, prosa], trabalhando: [crivo], problema: [trato], livres: livres)
        case .semComputador:
            return MXQuadroDados(estado: .semComputador)
        case .semAcesso:
            return MXQuadroDados(estado: .semAcesso, computador: computador, ligacao: .parado)
        case .erro:
            return MXQuadroDados(estado: .erro, computador: computador)
        }
    }
}
#endif
