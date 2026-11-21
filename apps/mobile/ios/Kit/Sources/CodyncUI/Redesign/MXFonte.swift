import Foundation

/// A fonte de dados fina das telas da etapa 2 (Conversas, Conversa, fila, menu do agente, aprovação).
///
/// Duas implementações:
/// - `MXFonteExemplo`: os dados do desenho (`MXExemplos.swift`, marcados como exemplo), para a casca de Debug
///   abrir sem login e para as capturas.
/// - `MXFonteReal`: adapta o `BotStore` existente (roster, conversa, envio, fila, permissão) sem mudar ele.
///
/// As telas só leem estes valores e chamam estas intenções; nenhuma regra de negócio fica na view.
/// Implementações devem ser `@Observable`: as views leem pela existencial e a observação segue a leitura.
@MainActor
public protocol MXFonte: AnyObject {
    /// Ligação com o computador (a queda curta do store).
    var conexao: MXEstadoConexao { get }
    /// A faixa de queda aparece (reconectando depois da carência; o store decide quando).
    var faixaDeQueda: Bool { get }
    /// "Tentar agora" pode adiantar a reconexão.
    var podeTentarAgora: Bool { get }
    func tentarAgora()
    /// A aba Conversas.
    var lista: MXConversasDados { get }
    /// Contadores laranja da barra de abas.
    var pendencias: [MXAba: Int] { get }
    /// Uma frase sobre a última ação que não valeu (some com `limparAviso`).
    var aviso: String? { get }

    /// A Conversa (sessão principal) de um agente ou do Berga; nulo se ele não existe mais.
    func conversa(_ id: String) -> MXConversaDados?

    /// O rascunho guardado da caixa (sobrevive à queda e a sair da tela).
    func rascunho(_ id: String) -> String
    func guardarRascunho(_ texto: String, em id: String)

    /// A conversa abriu (ou a ligação voltou com ela aberta): a fonte busca o que falta para mostrá-la e mandar nela
    /// (numa sessão extra, a lista das sessões, que diz se o computador aceita mensagem em sessão, e o histórico dela).
    /// Só lê; nada é mandado.
    func abrindo(_ id: String) async

    /// A conversa está (ou deixou de estar) visível numa cena ativa: enquanto está, o que chega já conta como lido.
    /// `token` identifica a tela (várias janelas podem ler a mesma conversa).
    func lendo(_ id: String, token: UUID, ativo: Bool)
    /// Há mensagens mais antigas que este aparelho ainda não carregou.
    func podeCarregarAnteriores(_ id: String) -> Bool
    /// Carrega a página anterior de mensagens (o mesmo `loadOlder` da conversa atual). Falso quando não deu (sem
    /// ligação, falha): a tela oferece "Tentar de novo" e tenta sozinha ao reconectar com o topo à vista.
    func carregarAnteriores(_ id: String) async -> Bool
    /// Manda o texto (entra na fila se o agente está trabalhando).
    func enviar(_ texto: String, para id: String)
    /// Manda de novo uma mensagem que não foi.
    func reenviar(_ mensagem: String, em id: String)
    /// Para o agente.
    func parar(_ id: String)
    func mandarAgora(_ item: MXItemDaFila, em id: String)
    /// Há um salvar desta mensagem da fila a caminho do Hub. A reserva é da fonte (por agente e mensagem), não da
    /// tela: sair e reabrir a conversa não libera um segundo salvar enquanto o primeiro não voltar.
    func salvando(_ item: String, em id: String) -> Bool
    /// Salva o texto novo de uma mensagem da fila. Tudo o que decide se sai acontece no toque, sem esperar: a ligação,
    /// a geração do link (levada até o despacho) e a reserva da mensagem. Nulo quando não saiu (`aviso` diz por quê,
    /// ou já há um salvar desta mensagem); a tarefa devolve se o Hub aceitou.
    func editar(_ item: MXItemDaFila, texto: String, revisao: Int64, em id: String) -> Task<Bool, Never>?
    func remover(_ item: MXItemDaFila, em id: String)
    /// Responde o cartão de aprovação com a opção do Hub. `pedido` é o que estava na tela: se a entrada mudou desde
    /// então (revisão, pendência, opções), não responde e avisa.
    func responder(_ pedido: MXPedidoDeAprovacao, opcao: String, em id: String)
    func alternarFixar(_ id: String)
    func limparAviso()

    // MARK: Sessões (07, 07b) e ajustes do agente (08)

    /// A tela Sessões de um agente; nula se ele não existe mais.
    func sessoes(_ id: String) -> MXSessoesDados?
    /// Pede as sessões ao computador (a tela chama ao abrir e ao reconectar). Só lê; nada executa.
    func carregarSessoes(_ id: String) async
    /// A conversa que a 07b mostra para a sessão (o id que `conversa(_:)` entende); nula se a sessão não abre no iPhone.
    func conversaDaSessao(_ sessao: String, em id: String) -> String?
    /// Cria uma sessão e devolve o id dela (para abrir); nula quando não deu ou a fonte não cria (`podeCriar`).
    func novaSessao(em id: String) -> String?
    /// Arquiva uma sessão parada (só quando `podeArquivar`); `desfazerArquivar` a traz de volta.
    func arquivarSessao(_ sessao: String, em id: String)
    func desfazerArquivar(_ sessao: String, em id: String)

    /// A folha de ajustes de um agente; nula se ele não existe mais.
    func ajustes(_ id: String) -> MXAjustesDoAgente?
    /// Muda a autonomia (Pedir ou Liberar). Só com autoridade, presa à geração do toque; o aviso diz se não valeu.
    func mudarAutonomia(_ autonomia: MXAutonomia, em id: String)
    /// Liga ou desliga os avisos deste agente.
    func alternarAvisos(em id: String)
    /// Exclui o agente (a tela já confirmou). Devolve se o pedido saiu; o agente some quando o computador confirmar
    /// (`ajustes(id)` fica nulo), e enquanto isso `MXAjustesDoAgente.excluindo`.
    func excluirAgente(_ id: String) -> Bool

    /// O que esta fonte tem de verdade: cada controle das telas pergunta aqui se fica ligado (`MXCapacidades`).
    var capacidades: MXCapacidades { get }
}

/// Cada controle das telas do redesenho que pode não ter ação real nesta fonte (etapas 1 a 3).
public enum MXControle: Hashable, Sendable {
    // Conversas (topo, canais, barra de abas)
    case conta, space, novo, canal
    /// Uma aba da barra.
    case aba(MXAba)
    // Conversa (topo e caixa)
    case whatsapp, aoVivo, anexar
    // Menu do agente
    case rotinas, ajustesDoAgente, autonomia, novaSessao, fixar
    // Sessões
    case vozDasSessoes, arquivadas, sessaoExtra
    // Ajustes do agente
    case ligar
    /// Uma linha da lista dos ajustes (Identidade, Motor…, Exportar).
    case ajuste(MXEmBreve)

    /// Todos os controles, para conferir uma fonte inteira (o teste percorre esta lista).
    public static let todos: [MXControle] = [.conta, .space, .novo, .canal]
        + MXAba.allCases.map(MXControle.aba)
        + [.whatsapp, .aoVivo, .anexar, .rotinas, .ajustesDoAgente, .autonomia, .novaSessao, .fixar,
           .vozDasSessoes, .arquivadas, .sessaoExtra, .ligar]
        + MXControle.telasDeAjuste.map(MXControle.ajuste)

    /// As linhas da lista dos ajustes que levam a uma tela de dentro (ainda de outras etapas).
    public static let telasDeAjuste: [MXEmBreve] = [.identidade, .motor, .comportamento, .ferramentas, .memoria, .rotinas,
                                                     .whatsapp, .voz, .exportar]
}

/// O que um controle faz numa fonte.
public enum MXAcaoDoControle: Equatable, Sendable {
    /// Faz o que diz (abre a tela de verdade, manda ao Hub…).
    case real
    /// Só levaria à tela "Em breve" (ou a uma aba ainda sem tela).
    case emBreve
}

/// O que a fonte tem de verdade (nota (a) da revisão da etapa 3). Regra: um controle fica ligado só se faz o que diz
/// ou se a fonte mostra as telas "Em breve" (a de exemplo, que segue o desenho). Na fonte real, o que só levaria a
/// "Em breve" fica à vista e desligado, com "Em breve" (valor acessível "Indisponível").
public struct MXCapacidades: Equatable, Sendable {
    /// Os destinos que ainda não existem abrem a tela "Em breve" (e as abas sem tela abrem vazias).
    public var mostraEmBreve: Bool
    /// Cria uma sessão e abre (Nova sessão nas Sessões e no menu do agente).
    public var criaSessao: Bool
    /// Abre no iPhone as sessões que não são a Principal.
    public var abreSessaoExtra: Bool
    /// As abas com tela montada.
    public var abasMontadas: Set<MXAba>
    #if DEBUG
    /// A casca tem os Ajustes e a conta (`MXContaFonte`): o botão da conta abre a tela de verdade.
    public var abreConta = false
    #endif

    public init(mostraEmBreve: Bool, criaSessao: Bool, abreSessaoExtra: Bool, abasMontadas: Set<MXAba>) {
        self.mostraEmBreve = mostraEmBreve
        self.criaSessao = criaSessao
        self.abreSessaoExtra = abreSessaoExtra
        self.abasMontadas = abasMontadas
    }

    /// A de exemplo: como o desenho (tudo ligado; o que não existe leva a "Em breve").
    public static let exemplo = MXCapacidades(mostraEmBreve: true, criaSessao: true, abreSessaoExtra: true, abasMontadas: [.conversas])
    /// A real hoje: abre as sessões extras (histórico e envio por sessão no `BotStore`); criar sessão não tem
    /// caminho público na base; o resto ainda não existe.
    public static let real = MXCapacidades(mostraEmBreve: false, criaSessao: false, abreSessaoExtra: true, abasMontadas: abasReais)
    #if DEBUG
    /// Em Debug o Mission Board v1 (derivado, só Debug) é uma aba montada; em Release só Conversas.
    private static let abasReais: Set<MXAba> = [.conversas, .missionBoard]
    #else
    private static let abasReais: Set<MXAba> = [.conversas]
    #endif

    /// O que o controle faz nesta fonte.
    public func acao(_ controle: MXControle) -> MXAcaoDoControle {
        switch controle {
        case .ajustesDoAgente, .autonomia, .fixar: .real
        case .novaSessao: criaSessao ? .real : .emBreve
        case .sessaoExtra: abreSessaoExtra ? .real : .emBreve
        case let .aba(aba): abasMontadas.contains(aba) ? .real : .emBreve
        case .conta:
            #if DEBUG
            abreConta ? .real : .emBreve
            #else
            .emBreve
            #endif
        case .space, .novo, .canal, .whatsapp, .aoVivo, .anexar, .rotinas, .vozDasSessoes, .arquivadas, .ligar,
             .ajuste: .emBreve
        }
    }

    /// O controle fica ligado (Fixar ainda depende da autoridade, à parte).
    public func ligado(_ controle: MXControle) -> Bool {
        acao(controle) == .real || mostraEmBreve
    }
}

/// Destinos que ainda não existem nesta etapa: aparecem, levam a uma tela simples "Em breve" e nada mais.
public enum MXEmBreve: String, Hashable, Sendable, CaseIterable {
    case conta, space, novo, canal, whatsapp, aoVivo, anexar, rotinas, ajustesDoAgente, autonomia, novaSessao
    // Etapa 3: o que as Sessões e os ajustes do agente levam e ainda não existe.
    case identidade, motor, comportamento, ferramentas, memoria, voz, exportar, sessaoExtra, arquivadas

    public var titulo: String {
        switch self {
        case .conta: String(localized: "Ajustes e conta", bundle: .module)
        case .space: String(localized: "Spaces", bundle: .module)
        case .novo: String(localized: "Novo bot ou canal", bundle: .module)
        case .canal: String(localized: "Canal", bundle: .module)
        case .whatsapp: String(localized: "WhatsApp do bot", bundle: .module)
        case .aoVivo: String(localized: "Falar ao vivo", bundle: .module)
        case .anexar: String(localized: "Anexar foto, arquivo ou colar", bundle: .module)
        case .rotinas: String(localized: "Rotinas", bundle: .module)
        case .ajustesDoAgente: String(localized: "Ajustes do bot", bundle: .module)
        case .autonomia: String(localized: "Autonomia", bundle: .module)
        case .novaSessao: String(localized: "Nova sessão", bundle: .module)
        case .identidade: String(localized: "Identidade", bundle: .module)
        case .motor: String(localized: "Motor", bundle: .module)
        case .comportamento: String(localized: "Comportamento", bundle: .module)
        case .ferramentas: String(localized: "Ferramentas", bundle: .module)
        case .memoria: String(localized: "Memória", bundle: .module)
        case .voz: String(localized: "Voz", bundle: .module)
        case .exportar: String(localized: "Exportar bot", bundle: .module)
        case .sessaoExtra: String(localized: "Sessão", bundle: .module)
        case .arquivadas: String(localized: "Arquivadas", bundle: .module)
        }
    }

    /// O texto da tela "Em breve" quando há um específico.
    public var texto: String? {
        switch self {
        case .sessaoExtra: String(localized: "Abrir esta sessão no iPhone chega numa próxima versão. Ela continua no computador.", bundle: .module)
        case .novaSessao: String(localized: "Criar sessões pelo iPhone chega numa próxima versão. Por enquanto, crie no computador.", bundle: .module)
        default: nil
        }
    }
}

/// Para onde a navegação de Conversas leva.
public enum MXDestino: Hashable, Sendable {
    /// A Conversa (sessão principal) de um agente ou do Berga.
    case conversa(String)
    case emBreve(MXEmBreve)
    /// As sessões de um agente (07).
    case sessoes(String)
    /// Uma sessão aberta (07b): a conversa dela, com voltar para Sessões.
    case sessao(agente: String, sessao: String)
    /// Os ajustes do agente (08): abre como folha, não empurra.
    case ajustes(String)
}

/// O que a lista de Conversas faz quando tocam nela.
public struct MXAcoesDaLista {
    /// Tocar no agente ou no Berga: abre a sessão principal.
    public var abrir: (String) -> Void
    /// Tocar num canal.
    public var abrirCanal: (String) -> Void
    /// Botões do topo e destinos que ainda não existem.
    public var emBreve: (MXEmBreve) -> Void
    /// Gestos da tabela "Gestos" do README do desenho (etapa 3): puxar da direita para a esquerda abre as Sessões;
    /// segurar 0,55 s abre os ajustes do agente (folha). Nulos: o gesto não existe.
    public var sessoes: ((String) -> Void)?
    public var ajustes: ((String) -> Void)?

    public init(abrir: @escaping (String) -> Void = { _ in }, abrirCanal: @escaping (String) -> Void = { _ in },
                emBreve: @escaping (MXEmBreve) -> Void = { _ in }, sessoes: ((String) -> Void)? = nil,
                ajustes: ((String) -> Void)? = nil) {
        self.abrir = abrir
        self.abrirCanal = abrirCanal
        self.emBreve = emBreve
        self.sessoes = sessoes
        self.ajustes = ajustes
    }
}

/// Cenas de captura (só a fonte de exemplo): o app abre direto nelas com `-MexericaCena <nome>`.
public enum MXCena: String, Sendable, CaseIterable {
    /// 05: Byte trabalhando, 2 na fila.
    case conversa
    /// 05a: 3 na fila, editando a 2ª.
    case filaEditando = "fila-editando"
    /// 05b: 3 na fila, menu do agente aberto.
    case menu
    /// 06: cartão de aprovação esperando.
    case aprovar
    /// 07: as sessões do Byte, com as ativas abertas (como a captura).
    case sessoes
    /// 07b: uma sessão aberta, com voltar para Sessões.
    case sessaoAberta = "sessao-aberta"
    /// 08: a folha de ajustes do Byte.
    case ajustesDoAgente = "ajustes-do-agente"

    /// A cena pedida ao processo (`-MexericaCena <nome>` ou `MEXERICA_CENA=<nome>`), se houver. Só em Debug; fora
    /// dele é sempre nula.
    public static var doProcesso: MXCena? {
        #if DEBUG
        let processo = ProcessInfo.processInfo
        return daLinhaDeComando(processo.arguments, ambiente: processo.environment)
        #else
        return nil
        #endif
    }

    /// Lê a cena dos argumentos (`-MexericaCena <nome>`) ou do ambiente (`MEXERICA_CENA`).
    static func daLinhaDeComando(_ argumentos: [String], ambiente: [String: String]) -> MXCena? {
        if let i = argumentos.firstIndex(of: "-MexericaCena"), argumentos.indices.contains(i + 1) {
            return MXCena(rawValue: argumentos[i + 1])
        }
        return ambiente["MEXERICA_CENA"].flatMap(MXCena.init(rawValue:))
    }
}
