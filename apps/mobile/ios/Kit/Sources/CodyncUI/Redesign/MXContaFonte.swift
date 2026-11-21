#if DEBUG
import CodyncKit
import Observation
import SwiftUI

// A fonte de Ajustes, conta e computadores (PLANO §4.1). Só Debug. Fica ao lado da `MXFonte` das conversas, não dentro
// dela: a `MXFonte` é de um computador só (reconstruída ao trocar), e esta é da conta e vive por cima da troca.

/// O que a tela de Ajustes lê e pede. Duas implementações: `MXContaReal` (o `AccountStore` e o app de hoje por baixo) e
/// `MXContaExemplo` (dados de exemplo para as cenas de captura).
@MainActor
public protocol MXContaFonte: AnyObject {
    var dados: MXContaDados { get }
    /// O que não deu certo na última ação (a tela escreve a frase).
    var erro: MXErroDaConta? { get }
    func limparErro()
    /// Troca para este computador.
    func usar(_ id: String)
    /// Atualiza a lista de computadores da conta.
    func atualizar() async
    /// Sai da conta (a tela já confirmou).
    func sair() async
    /// Abre a conta do app de hoje (trocar de conta, entrar).
    func abrirConta()
    /// Liga ou desliga os avisos deste iPhone.
    func alternarAvisos() async
    /// O que a ponte mostra para este destino (o app de hoje, numa folha).
    func vista(_ destino: MXDestinoDaConta) -> AnyView
    /// A tela abriu: pergunta ao iOS sobre avisos e atualiza os computadores da conta.
    func aoAbrir() async
    /// O app voltou à frente (talvez dos Ajustes do iPhone): pergunta de novo ao iOS sobre avisos.
    func aoVoltarAoApp() async
}

/// O que o app de hoje sabe da sessão (login), lido pela tela sem depender do Clerk.
@MainActor
public protocol MXSessaoDaConta: AnyObject {
    var email: String? { get }
    var isSignedIn: Bool { get }
    var isConfigured: Bool { get }
}

/// O que só o app tem (login, pareamento, telas de hoje): a casca recebe, não importa.
public struct MXPonteDoApp {
    public var sessao: any MXSessaoDaConta
    public var abrirConta: @MainActor () -> Void
    public var sair: @MainActor () async -> Void
    /// A tela de hoje para o destino, no computador em uso (nulo se não há).
    public var vista: @MainActor (MXDestinoDaConta, String?) -> AnyView
    /// Nulo até perguntarem ao iOS; falso se negou.
    public var permissaoDeAvisos: @MainActor () async -> Bool?
    public var pedirPermissaoDeAvisos: @MainActor () async -> Bool
    /// O interruptor de avisos mudou: manda aos computadores o que ele diz agora.
    public var avisosMudaram: @MainActor (Bool) -> Void

    public init(sessao: any MXSessaoDaConta, abrirConta: @escaping @MainActor () -> Void, sair: @escaping @MainActor () async -> Void,
                vista: @escaping @MainActor (MXDestinoDaConta, String?) -> AnyView,
                permissaoDeAvisos: @escaping @MainActor () async -> Bool?,
                pedirPermissaoDeAvisos: @escaping @MainActor () async -> Bool,
                avisosMudaram: @escaping @MainActor (Bool) -> Void) {
        self.sessao = sessao
        self.abrirConta = abrirConta
        self.sair = sair
        self.vista = vista
        self.permissaoDeAvisos = permissaoDeAvisos
        self.pedirPermissaoDeAvisos = pedirPermissaoDeAvisos
        self.avisosMudaram = avisosMudaram
    }
}

// MARK: - Real

/// A conta de verdade: lê o `AccountStore` (computadores, nuvem, pedidos de acesso) e a sessão do app de hoje, e age
/// pelos mesmos caminhos que a `SettingsView` e a `AccountSwitcherView` de hoje.
@MainActor
@Observable
public final class MXContaReal: MXContaFonte {
    private let contas: AccountStore
    private let ponte: MXPonteDoApp
    private let defaults: UserDefaults
    /// O computador que o dono escolheu nesta abertura (vence o último usado, se ainda existe).
    public private(set) var escolhido: ComputerID?
    /// A casca foi reconstruída por uma troca de computador: reabre os Ajustes, para o dono ver a marca mudar de lugar.
    public var reabrirNosAjustes = false
    public private(set) var erro: MXErroDaConta?
    private var atualizando = false
    private var permissao: Bool??
    /// Sobe quando qualquer preferência do `UserDefaults` muda (por exemplo, "Notificações" nos Mais ajustes, que grava a
    /// mesma chave): é o que faz a tela reler `avisosLigados`, porque o `UserDefaults` em si não é observável.
    private var versaoDasPreferencias = 0
    @ObservationIgnored nonisolated(unsafe) private var observador: (any NSObjectProtocol)?

    public static let chaveDeAvisos = "notificationsEnabled"

    public init(contas: AccountStore, ponte: MXPonteDoApp, defaults: UserDefaults = .standard) {
        self.contas = contas
        self.ponte = ponte
        self.defaults = defaults
        observador = NotificationCenter.default.addObserver(forName: UserDefaults.didChangeNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.versaoDasPreferencias += 1 }
        }
    }

    deinit {
        if let observador { NotificationCenter.default.removeObserver(observador) }
    }

    /// A preferência do iPhone, sempre lida da fonte compartilhada (a mesma chave do app de hoje): nunca guarda cópia.
    var avisosLigados: Bool {
        _ = versaoDasPreferencias
        return defaults.object(forKey: Self.chaveDeAvisos) as? Bool ?? true
    }

    /// O computador em uso: o escolhido, se ainda existe; senão o último do app de hoje; senão o primeiro.
    public var emUsoId: ComputerID? {
        MXContaRegras.emUso(escolhido: escolhido, ultimo: contas.storage.lastComputerId, ids: contas.computers.map(\.id))
    }

    public var storeEmUso: BotStore? { emUsoId.flatMap { contas.store(for: $0) } }

    public var dados: MXContaDados {
        let emUso = emUsoId
        let nuvem = Dictionary(contas.cloudComputers.map { ($0.computerId, $0) }, uniquingKeysWith: { primeiro, _ in primeiro })
        var linhas: [MXComputadorLinha] = contas.computers.compactMap { computador in
            guard let store = contas.store(for: computador.id) else { return nil }
            var estado = MXContaRegras.estado(conexao: store.shownConnection, precisaAtualizar: store.mismatch != nil,
                                              copiaAntiga: contas.isStale(computador.id))
            // A nuvem sabe de quando foi a última vez que viu o computador, mesmo sem o relay dizer.
            if case .desligado(nil) = estado, let visto = nuvem[computador.id]?.lastSeenAt {
                estado = .desligado(visto: Date(timeIntervalSince1970: Double(visto) / 1000))
            }
            let uso = computador.id == emUso
            return MXComputadorLinha(id: computador.id, nome: store.hostName, estado: estado, emUso: uso,
                                     acao: MXContaRegras.acao(estado: estado, emUso: uso))
        }
        // Computadores da conta que este iPhone ainda não usa (pedir acesso, ver código, cópia antiga).
        for computador in contas.cloudComputers where !contas.computers.contains(where: { $0.id == computador.computerId }) {
            let estado = MXContaRegras.estadoDaConta(
                ligado: computador.isOnline, copiaAntiga: contas.isOlderCopy(computador),
                pedidoEmAndamento: contas.pendingAccess[computador.computerId] != nil,
                detalhe: contas.accessRowDetail(computador.computerId))
            linhas.append(MXComputadorLinha(id: computador.computerId, nome: computador.name, estado: estado,
                                            acao: MXContaRegras.acao(estado: estado, emUso: false)))
        }
        let armazem = storeEmUso
        let conectado = armazem?.mantemNaTela == true
        let voz: MXVozValor
        if !conectado {
            voz = .indisponivel
        } else if let provedor = VoiceProvider(rawValue: defaults.string(forKey: VoiceSettings.engineKey) ?? "") {
            voz = .provedor(provedor.name)
        } else {
            voz = .noAparelho
        }
        return MXContaDados(
            email: ponte.sessao.email,
            logada: ponte.sessao.isSignedIn,
            configurada: ponte.sessao.isConfigured,
            computadores: linhas,
            conectar: ponte.sessao.isConfigured ? .atualizar : .parear,
            avisos: SharedStore.remotePushEnabled ? avisosDoIphone : nil,
            voz: voz,
            uso: !conectado ? .indisponivel : MXContaRegras.uso(providers: armazem?.usage.providers ?? []),
            idioma: MXContaRegras.nomeDoIdioma(Bundle.main.preferredLocalizations.first),
            versao: Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "",
            atualizando: atualizando)
    }

    var avisosDoIphone: MXAvisosDoIphone {
        if case .some(.some(false)) = permissao { return .negadoNoIphone }
        return avisosLigados ? .ligados : .desligados
    }

    /// Pergunta ao iOS se pode avisar (a tela chama ao abrir e ao voltar ao app).
    public func carregarPermissao() async {
        permissao = .some(await ponte.permissaoDeAvisos())
    }

    public func usar(_ id: String) {
        guard id != emUsoId, contas.computers.contains(where: { $0.id == id }) else { return }
        escolhido = id
        // O mesmo "último computador" que o app de hoje usa: voltar ao visual antigo abre no mesmo.
        contas.storage.lastComputerId = id
        reabrirNosAjustes = true
    }

    public func atualizar() async {
        guard !atualizando else { return }
        atualizando = true
        defer { atualizando = false }
        contas.lastError = nil
        // A busca solicitada pelo usuário recebe o resultado da nuvem (o refresh de fundo engole falha transitória): a lista que já
        // estava na tela fica, e a falha vira o aviso traduzido com "Tentar de novo". O texto bruto da nuvem nunca sai.
        let deuCerto = await contas.refreshCloudReportingResult()
        if !deuCerto || contas.lastError != nil {
            contas.lastError = nil
            erro = .atualizar
        } else if erro == .atualizar {
            erro = nil
        }
    }

    public func sair() async {
        await ponte.sair()
        if ponte.sessao.isSignedIn { erro = .sair }
    }

    public func abrirConta() { ponte.abrirConta() }

    public func alternarAvisos() async {
        // O iOS negou: só os Ajustes do iPhone mudam isso (a tela leva até lá), e a preferência fica como está.
        if case .some(.some(false)) = permissao { return }
        let ligar = !avisosLigados
        defaults.set(ligar, forKey: Self.chaveDeAvisos)
        if ligar, case .some(.none) = permissao {
            // Ainda não perguntaram ao iOS: é agora, e a pergunta é do iOS, não do app.
            _ = await ponte.pedirPermissaoDeAvisos()
            await carregarPermissao()
        }
        ponte.avisosMudaram(ligar)
    }

    public func limparErro() { erro = nil }

    public func vista(_ destino: MXDestinoDaConta) -> AnyView { ponte.vista(destino, emUsoId) }

    public func aoAbrir() async {
        await carregarPermissao()
        if ponte.sessao.isConfigured { await atualizar() }
    }

    public func aoVoltarAoApp() async { await carregarPermissao() }
}

// MARK: - Exemplo

/// As cenas de captura da tela de Ajustes (só a casca de exemplo): `-MexericaCena conta-varios`, `conta-um`, `conta-sem`.
public enum MXCenaDaConta: String, Sendable, CaseIterable {
    case varios = "conta-varios"
    case um = "conta-um"
    case sem = "conta-sem"

    public static var doProcesso: MXCenaDaConta? {
        let processo = ProcessInfo.processInfo
        return daLinhaDeComando(processo.arguments, ambiente: processo.environment)
    }

    static func daLinhaDeComando(_ argumentos: [String], ambiente: [String: String]) -> MXCenaDaConta? {
        if let i = argumentos.firstIndex(of: "-MexericaCena"), argumentos.indices.contains(i + 1) {
            return MXCenaDaConta(rawValue: argumentos[i + 1])
        }
        return ambiente["MEXERICA_CENA"].flatMap(MXCenaDaConta.init(rawValue:))
    }
}

/// Dados de exemplo (o mockup do PLANO §5: "MacBook de exemplo", "Mac mini de exemplo", `voce@exemplo.com`). Não falam
/// com nada: trocar de computador só muda a marca, e as folhas mostram um aviso de exemplo.
@MainActor
@Observable
public final class MXContaExemplo: MXContaFonte {
    private var linhas: [MXComputadorLinha]
    public private(set) var erro: MXErroDaConta?
    private var avisosLigados = true

    public init(cena: MXCenaDaConta = .varios) {
        switch cena {
        case .varios:
            linhas = [
                MXComputadorLinha(id: "exemplo-macbook", nome: "MacBook de exemplo", estado: .conectado, emUso: true, acao: .nenhuma),
                MXComputadorLinha(id: "exemplo-macmini", nome: "Mac mini de exemplo",
                                  estado: .desligado(visto: Self.ontemAs1840()), acao: .usar),
                MXComputadorLinha(id: "exemplo-estudio", nome: "Mac Studio de exemplo", estado: .daConta(ligado: true), acao: .pedirAcesso),
            ]
        case .um:
            linhas = [MXComputadorLinha(id: "exemplo-macbook", nome: "MacBook de exemplo", estado: .conectado, emUso: true, acao: .nenhuma)]
        case .sem:
            linhas = []
        }
    }

    private static func ontemAs1840() -> Date {
        let calendario = Calendar.current
        let ontem = calendario.date(byAdding: .day, value: -1, to: .now) ?? .now
        return calendario.date(bySettingHour: 18, minute: 40, second: 0, of: ontem) ?? ontem
    }

    public var dados: MXContaDados {
        MXContaDados(email: "voce@exemplo.com", logada: true, configurada: true, computadores: linhas, conectar: .atualizar,
                     avisos: avisosLigados ? .ligados : .desligados, voz: linhas.isEmpty ? .indisponivel : .noAparelho,
                     uso: linhas.isEmpty ? .indisponivel : .percentual(62), idioma: MXContaRegras.nomeDoIdioma(Bundle.main.preferredLocalizations.first),
                     versao: "1.0")
    }

    public func usar(_ id: String) {
        guard linhas.contains(where: { $0.id == id && $0.acao == .usar }) else { return }
        for i in linhas.indices {
            linhas[i].emUso = linhas[i].id == id
            linhas[i].acao = MXContaRegras.acao(estado: linhas[i].estado, emUso: linhas[i].emUso)
        }
    }

    public func atualizar() async {}
    public func aoAbrir() async {}
    public func aoVoltarAoApp() async {}
    public func sair() async {}
    public func abrirConta() {}
    public func alternarAvisos() async { avisosLigados.toggle() }
    public func limparErro() { erro = nil }

    public func vista(_ destino: MXDestinoDaConta) -> AnyView {
        AnyView(MXEmBreveTela(titulo: MXTextosDaConta().ajustes, texto: MXTextosDaConta().exemploDaFolha, voltar: {}))
    }
}

// MARK: - Ambiente

extension EnvironmentValues {
    /// A conta da casca; nula nas telas que não têm Ajustes (os testes).
    @Entry var mxConta: (any MXContaFonte)? = nil

    /// O tema do sistema, lido por fora da casca (que fica no claro até a etapa 7): só os Ajustes o seguem.
    @Entry var mxEsquemaDoSistema: ColorScheme = .light
}
#endif
