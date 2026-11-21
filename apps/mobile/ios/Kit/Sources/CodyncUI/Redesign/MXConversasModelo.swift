import CodyncKit
import Foundation

/// As cinco abas do redesenho, na ordem do desenho.
public enum MXAba: String, CaseIterable, Identifiable, Sendable {
    case missionBoard, conversas, equipes, rotinas, estado

    public var id: String { rawValue }

    /// Rótulo da aba. "Estado" vs. "State" é decisão em aberto do dono; seguimos o desenho.
    public var titulo: String {
        switch self {
        case .missionBoard: String(localized: "Mission Board", bundle: .module)
        case .conversas: String(localized: "Conversas", bundle: .module)
        case .equipes: String(localized: "Organograma", bundle: .module)
        case .rotinas: String(localized: "Rotinas", bundle: .module)
        case .estado: String(localized: "Estado", bundle: .module)
        }
    }

    public var icone: MXIcone {
        switch self {
        case .missionBoard: .missionBoard
        case .conversas: .conversas
        case .equipes: .equipes
        case .rotinas: .rotinas
        case .estado: .estado
        }
    }

    /// "Mission Board" ocupa 84; as outras, 64.
    public var largura: CGFloat { self == .missionBoard ? MX.Medida.abaLarguraLonga : MX.Medida.abaLargura }

    /// O nome que a aba mostra na barra: igual ao `titulo`, salvo onde o título não cabe na aba de 64 pt sem
    /// encolher abaixo do piso comum (80%, medido no teste, nos dois idiomas):
    /// - Organograma: "Organograma" (~96 pt no teto) vira "Bots" (pt e en). O título da tela segue "Organograma"
    ///   (en "Org chart"); Setor é um grupo dentro dele, não o nome do conjunto.
    /// - Conversas: só em inglês, "Conversations" (~102 pt no teto) vira "Chats". Em português, "Conversas".
    /// O nome completo continua no VoiceOver, no Controle por Voz e no visualizador de conteúdo grande (`titulo`).
    public var rotulo: String {
        switch self {
        case .equipes: String(localized: "Organograma na barra", bundle: .module)
        case .conversas: String(localized: "Conversas na barra", bundle: .module)
        default: titulo
        }
    }

    /// O que o VoiceOver lê na aba: o nome e, se houver, o contador inteiro ("Conversas, 12 novas").
    func rotuloAcessivel(_ n: Int) -> String {
        n > 0 ? "\(titulo), \(descricaoDoContador(n))" : titulo
    }

    /// Como o contador é lido no VoiceOver (texto do protótipo).
    func descricaoDoContador(_ n: Int) -> String {
        switch self {
        case .missionBoard: n == 1 ? String(localized: "\(n) espera você", bundle: .module) : String(localized: "\(n) esperam você", bundle: .module)
        default: n == 1 ? String(localized: "\(n) nova", bundle: .module) : String(localized: "\(n) novas", bundle: .module)
        }
    }
}

/// Tom da prévia de um agente.
public enum MXTomDaPrevia: Sendable {
    /// Precisa de você: laranja, seminegrito.
    case alerta
    /// Trabalhando: verde.
    case ativo
    /// O resto: marrom secundário.
    case normal
}

/// Uma linha de agente em Conversas.
public struct MXAgenteResumo: Identifiable, Equatable, Sendable {
    public let id: String
    public let nome: String
    /// A função ("Dev"). Vazia quando não se sabe (o app ainda não lê a função do Hub): a etiqueta some.
    public let funcao: String
    /// Pelúcia de feltro (`Mexeriquinhas.xcassets`) ou o desenho do bot.
    public let rosto: MXRosto
    public let estado: MexeriquinhaState
    public let hora: String
    public let previa: String
    public let tom: MXTomDaPrevia
    public let sessoesRodando: Int
    public let sessoesEsperando: Int
    public let naoLidas: Int
}

/// Uma linha de canal em Conversas.
public struct MXCanalResumo: Identifiable, Equatable, Sendable {
    public let id: String
    public let nome: String
    public let naoLidas: Int
    public let sessoesRodando: Int
    public let sessoesEsperando: Int

    /// `id` é o do Hub; sem ele (exemplo), o nome.
    public init(id: String? = nil, nome: String, naoLidas: Int, sessoesRodando: Int, sessoesEsperando: Int) {
        self.id = id ?? nome
        self.nome = nome
        self.naoLidas = naoLidas
        self.sessoesRodando = sessoesRodando
        self.sessoesEsperando = sessoesEsperando
    }
}

/// O cartão do Berga.
public struct MXBergaResumo: Equatable, Sendable {
    /// O bot que a conversa abre.
    public let id: String
    public let nome: String
    public let space: String
    public let rosto: MXRosto
    public let estado: MexeriquinhaState
    public let estadoTexto: String
    public let mensagem: String
}

/// Como a lista de Conversas se arruma no tamanho de texto (regra pura, testada). Fora dos tamanhos de
/// acessibilidade, exatamente o desenho; neles, o que disputava a mesma linha empilha e os textos ganham linhas.
public struct MXArranjoDaLista: Equatable, Sendable {
    /// Cartão do Berga: nome em cima, Space embaixo (não disputam a linha; o nome nunca quebra no meio da palavra).
    public let nomeESpaceEmpilhados: Bool
    /// Cabeçalho de Canais: a contagem ("3 · 3 novas") vai para a linha de baixo.
    public let contagemDosCanaisEmbaixo: Bool
    /// Linhas da mensagem do cartão do Berga (nulo: sem limite).
    public let linhasDaMensagemDoBerga: Int?
    /// Linhas da prévia de cada agente.
    public let linhasDaPrevia: Int
    /// A etiqueta da função do agente vai para uma linha própria, embaixo do nome.
    public let funcaoEmLinhaPropria: Bool

    public init(acessibilidade: Bool) {
        nomeESpaceEmpilhados = acessibilidade
        contagemDosCanaisEmbaixo = acessibilidade
        linhasDaMensagemDoBerga = acessibilidade ? nil : 1
        linhasDaPrevia = acessibilidade ? 3 : 1
        funcaoEmLinhaPropria = acessibilidade
    }
}

/// Regras de Conversas sem desenho (testadas).
public enum MXConversasRegras {
    /// O nome no cartão do Berga, se não couber numa linha, encolhe até este fator; depois corta no fim (nunca
    /// quebra no meio da palavra).
    public static let escalaMinimaDoNome: CGFloat = 0.75

    /// Ícone dos botões redondos do topo (conta e +): cresce com o texto até um teto; o círculo cresce junto, para
    /// o desenho manter o respiro, e nunca fica abaixo de 44 pt.
    public static func iconeDoBotaoRedondo(escalado: CGFloat) -> CGFloat {
        min(max(escalado, MX.Medida.botaoRedondoIcone), MX.Medida.botaoRedondoIconeTeto)
    }

    public static func ladoDoBotaoRedondo(icone: CGFloat) -> CGFloat {
        max(MX.Espaco.alvoMinimo, MX.Medida.botaoRedondo + (icone - MX.Medida.botaoRedondoIcone))
    }

    /// Texto de um contador; nada quando zero; "99+" acima de 99.
    public static func contador(_ n: Int) -> String? {
        n <= 0 ? nil : n > 99 ? "99+" : String(n)
    }

    /// "3 · 3 novas", "3 · 1 nova" ou só "3".
    public static func resumoDosCanais(_ canais: [MXCanalResumo]) -> String {
        let novas = canais.reduce(0) { $0 + max(0, $1.naoLidas) }
        guard novas > 0 else { return "\(canais.count)" }
        return novas == 1 ? String(localized: "\(canais.count) · \(novas) nova", bundle: .module) : String(localized: "\(canais.count) · \(novas) novas", bundle: .module)
    }

    /// A hora fica em laranja seminegrito quando há não lidas.
    public static func horaDestacada(_ agente: MXAgenteResumo) -> Bool { agente.naoLidas > 0 }

    /// O que o VoiceOver lê na linha do agente.
    public static func descricao(_ a: MXAgenteResumo) -> String {
        var partes = [a.nome, a.funcao, a.hora, a.previa].filter { !$0.isEmpty }
        if a.sessoesRodando > 0 { partes.append(a.sessoesRodando == 1 ? String(localized: "\(a.sessoesRodando) sessão rodando", bundle: .module) : String(localized: "\(a.sessoesRodando) sessões rodando", bundle: .module)) }
        if a.sessoesEsperando > 0 { partes.append(String(localized: "\(a.sessoesEsperando) esperando você", bundle: .module)) }
        if a.naoLidas > 0 { partes.append(a.naoLidas == 1 ? String(localized: "\(a.naoLidas) não lida", bundle: .module) : String(localized: "\(a.naoLidas) não lidas", bundle: .module)) }
        return partes.joined(separator: ", ")
    }
}
