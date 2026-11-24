import CodyncKit
import Foundation
import os

// Uma queda curta de conexão não fecha a tela. Este arquivo tem só as regras puras; `BotStore` as aplica.

/// Por que um computador está desconectado. Só aqui a tela de desconectado de hoje aparece.
public enum MotivoDesconexao: Equatable, Sendable {
    /// Cinco minutos seguidos sem link.
    case prazoLongo
    /// O computador recusou este aparelho, na hora: acesso revogado, 401/403, chave do servidor mudou.
    case acessoEncerrado(String)
    /// O app soltou o computador, na hora: saída ou troca de conta, computador removido.
    case encerradoNoApp
}

/// O link de um computador como as telas o veem (`BotStore.estadoConexao`).
public enum EstadoConexao: Equatable, Sendable {
    /// Link de pé e o Hub se identificou nesta conexão: enviar e aprovar valem.
    case conectado
    /// Queda curta (ou a primeira conexão ainda em andamento): a tela não muda, nada fica sem dono,
    /// mas nenhuma ação sai até voltar a `conectado`.
    case reconectando(desde: Date)
    /// Prazo longo ou falha definitiva: a tela de desconectado de hoje.
    case desconectado(MotivoDesconexao)

    /// Quanto tempo sem link até soltar o computador.
    public static let prazo: TimeInterval = 5 * 60

    public var estaReconectando: Bool {
        if case .reconectando = self { true } else { false }
    }

    public var estaDesconectado: Bool {
        if case .desconectado = self { true } else { false }
    }

    /// O próximo estado quando a conexão passa a `nova` em `agora`. Uma queda só começa a contar
    /// a partir de `conectado`; desconectado só sai voltando a `conectado`.
    static func proximo(_ atual: EstadoConexao, nova: BotStore.Connection, agora: Date) -> EstadoConexao {
        switch nova {
        case .online:
            return .conectado
        case let .unauthorized(message):
            return .desconectado(.acessoEncerrado(message))
        case .unpaired:
            return .desconectado(.encerradoNoApp)
        case .connecting, .offline, .computerOffline:
            if case .conectado = atual { return .reconectando(desde: agora) }
            return atual
        }
    }

    /// O mesmo estado depois de `passou` sem link, medido num relógio monotônico (mudar a hora do
    /// aparelho não mexe nele): no prazo, a queda vira `desconectado(.prazoLongo)`.
    func depoisDe(_ passou: Duration) -> EstadoConexao {
        guard estaReconectando, passou >= .seconds(Self.prazo) else { return self }
        return .desconectado(.prazoLongo)
    }
}

extension HostError {
    static var semAcesso: String { String(localized: "Este aparelho não tem mais acesso a este computador.", bundle: .module) }
    static var identidadeMudou: String { String(localized: "A identidade deste computador mudou. Pareie novamente para continuar usando-o.", bundle: .module) }

    /// Uma recusa que tentar de novo não resolve, com o motivo: `.unauthorized` e 401 em qualquer chamada,
    /// e 403 enquanto o link se identifica (conexão, `hello`, eventos). Um 403 em resposta a uma ação é
    /// só aquela ação recusada pelo Hub ("Only on the computer."), não o fim do acesso.
    func recusaDefinitiva(identificando: Bool) -> String? {
        switch self {
        case let .unauthorized(message): return message
        case .http(401, _): return errorDescription ?? Self.semAcesso
        case let .http(403, message) where identificando: return message.isEmpty ? Self.semAcesso : message
        default: return nil
        }
    }
}

/// O transporte com autoridade: cada chamada sai só no canal em que o Hub se identificou (`link`), sem
/// esperar nem seguir para um canal mais novo; e uma recusa definitiva solta o computador na hora, venha
/// da chamada que vier.
struct TransporteDoLink: HostTransport {
    let base: any HostTransport
    let link: Int
    let aoRecusar: @Sendable (String) -> Void
    /// A 403 answering one action: maybe only that action is refused ("Only on the computer."), maybe this
    /// device's access ended. The store asks the Hub to identify itself again to tell which.
    let aoDuvidar: @Sendable () -> Void

    func call(_ method: String, body: Data, timeout: TimeInterval) async throws -> Data {
        do {
            return try await base.call(method, body: body, timeout: timeout, onLink: link)
        } catch let error as HostError {
            guard await currentLink() == link else { throw HostError.unreachable }
            if let motivo = error.recusaDefinitiva(identificando: method == "hello") {
                aoRecusar(motivo)
            } else if case .http(403, _) = error {
                aoDuvidar()
            }
            throw error
        }
    }

    func call(_ method: String, body: Data, timeout: TimeInterval, onLink _: Int) async throws -> Data {
        try await call(method, body: body, timeout: timeout)
    }

    func currentLink() async -> Int? {
        await base.currentLink() == link ? link : nil
    }

    func screenRelayRequired() async throws -> Bool { try await base.screenRelayRequired() }
    func stream(_ request: HostStreamRequest) -> AsyncThrowingStream<Data, Error> { base.stream(request) }
    func states() -> AsyncStream<LinkState> { base.states() }
}

/// De quem é o espelho guardado (bots, conversas, rascunhos): só se reaproveita depois de uma queda com
/// a mesma conta, o mesmo computador e a mesma chave do servidor. Qualquer diferença recarrega do zero.
struct DonoDoEspelho: Equatable, Sendable {
    var conta: String?
    var computador: ComputerID
    var chave: String

    static func podeReaproveitar(_ antes: DonoDoEspelho, _ agora: DonoDoEspelho) -> Bool { antes == agora }
}

/// Os textos da queda curta, nos seis idiomas do desenho (chaves do desktop `conversa.mensagens.queda.*`).
public struct QuedaTextos: Equatable, Sendable {
    public let titulo: String
    /// Para leitor de tela.
    public let detalhe: String
    public let tentar: String
    public let tentando: String
    /// O campo de texto vazio, sem poder enviar.
    public let semEnvio: String
    /// A nota no cartão de aprovação.
    public let semAprovar: String

    /// Pelo primeiro idioma do aparelho; fora dos seis, inglês.
    public static func para(_ locale: Locale = Locale(identifier: Locale.preferredLanguages.first ?? "en")) -> QuedaTextos {
        let chave = switch locale.language.languageCode?.identifier {
        case "pt": "pt-BR"
        case "es": "es"
        case "fr": "fr"
        case "zh": "zh-CN"
        case "ar": "ar-AE"
        default: "en"
        }
        return porIdioma[chave] ?? en
    }

    public static let porIdioma: [String: QuedaTextos] = [
        "pt-BR": QuedaTextos(
            titulo: "Sem conexão — reconectando…",
            detalhe: "Suas conversas continuam aqui. Enviar e aprovar voltam quando reconectar; o que você digitar fica guardado.",
            tentar: "Tentar agora",
            tentando: "Tentando…",
            semEnvio: "Sem conexão: o texto fica guardado e você envia quando reconectar.",
            semAprovar: "Sem conexão: permitir e recusar voltam quando reconectar."
        ),
        "en": en,
        "es": QuedaTextos(
            titulo: "Sin conexión — reconectando…",
            detalhe: "Tus conversaciones siguen aquí. Enviar y aprobar vuelven al reconectar; lo que escribas se guarda.",
            tentar: "Intentar ahora",
            tentando: "Intentando…",
            semEnvio: "Sin conexión: el texto se guarda y lo envías al reconectar.",
            semAprovar: "Sin conexión: permitir y rechazar vuelven al reconectar."
        ),
        "fr": QuedaTextos(
            titulo: "Pas de connexion — reconnexion…",
            detalhe: "Vos conversations restent ici. L’envoi et l’approbation reviennent à la reconnexion ; ce que vous tapez est conservé.",
            tentar: "Réessayer maintenant",
            tentando: "Nouvelle tentative…",
            semEnvio: "Pas de connexion : le texte est conservé, envoyez-le à la reconnexion.",
            semAprovar: "Pas de connexion : autoriser et refuser reviennent à la reconnexion."
        ),
        "zh-CN": QuedaTextos(
            titulo: "无连接——正在重新连接…",
            detalhe: "对话仍保留在这里。重新连接后即可发送和批准；你输入的内容会被保留。",
            tentar: "立即重试",
            tentando: "正在重试…",
            semEnvio: "无连接：文字已保留，重新连接后即可发送。",
            semAprovar: "无连接：重新连接后即可允许或拒绝。"
        ),
        "ar-AE": QuedaTextos(
            titulo: "لا يوجد اتصال — جارٍ إعادة الاتصال…",
            detalhe: "تبقى محادثاتك هنا. يعود الإرسال والموافقة عند إعادة الاتصال، ويُحفظ ما تكتبه.",
            tentar: "حاول الآن",
            tentando: "جارٍ المحاولة…",
            semEnvio: "لا يوجد اتصال: يُحفظ النص ويمكنك إرساله عند إعادة الاتصال.",
            semAprovar: "لا يوجد اتصال: يعود السماح والرفض عند إعادة الاتصال."
        ),
    ]

    private static let en = QuedaTextos(
        titulo: "No connection — reconnecting…",
        detalhe: "Your conversations stay here. Sending and approving come back once it reconnects; what you type is kept.",
        tentar: "Try now",
        tentando: "Trying…",
        semEnvio: "No connection: your text is kept, send it once it reconnects.",
        semAprovar: "No connection: allowing and denying come back once it reconnects."
    )
}

/// Which link the person acted on: read with `BotStore.marcaDoLink` at the tap or when an
/// utterance begins, and handed to the action, which then goes out only on that link. Opaque on purpose.
public struct MarcaDoLink: Sendable, Equatable {
    let valor: Int
}

extension MarcaDoLink {
    /// No link at all: an action carrying it never goes out.
    static let nenhuma = MarcaDoLink(valor: -1)
}

/// The store's link generation, readable from any thread: the microphone's audio thread stamps
/// each buffer with it the moment the buffer is captured, before any hop to the main actor.
final class LinkAtual: Sendable {
    private let valor = OSAllocatedUnfairLock(initialState: 0)

    func definir(_ novo: Int) { valor.withLock { $0 = novo } }
    func marca() -> MarcaDoLink { MarcaDoLink(valor: valor.withLock { $0 }) }
}
