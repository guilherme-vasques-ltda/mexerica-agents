#if DEBUG
import CodyncKit
import Foundation

// Ajustes, conta e computadores do redesenho (PLANO §4.1). Só em Debug: em Release este arquivo e os outros desta
// tela não existem, e os textos ficam aqui, em código (pt-BR e en), não nos catálogos, que entram em todos os builds.

// MARK: - Textos

/// Os textos da tela, em pt-BR e en (qualquer outro idioma cai no pt-BR, como o app). `ingles` vem do idioma do
/// aplicativo; os testes passam o idioma à mão.
public struct MXTextosDaConta: Equatable, Sendable {
    public let ingles: Bool

    public init(ingles: Bool = MXTextosDebug.ingles()) {
        self.ingles = ingles
    }

    private func t(_ pt: String, _ en: String) -> String { ingles ? en : pt }

    public var ajustes: String { t("Ajustes", "Settings") }
    public var voltar: String { t("Voltar", "Back") }
    public var suaConta: String { t("Sua conta", "Your account") }
    public var pareamentoLocal: String { t("Pareamento local", "Local pairing") }
    public var semLogin: String {
        t("Você não entrou. Entre pelo visual antigo (faixa no topo).", "You're not signed in. Sign in from the old look (top banner).")
    }
    public var abreAConta: String { t("Abre a conta: trocar de conta, entrar ou sair", "Opens the account: switch accounts or sign in") }

    public var computadores: String { t("Computadores", "Computers") }
    public var conectarOutro: String { t("Conectar outro computador", "Connect another computer") }
    public var conectarUm: String { t("Conectar um computador", "Connect a computer") }
    public var procurar: String { t("Procurar computadores", "Look for computers") }
    public var procurando: String { t("Procurando…", "Looking…") }
    public var semComputadorTitulo: String { t("Nenhum computador conectado", "No computer connected") }
    public var semComputadorTexto: String {
        t("Conecte um computador para ver e conversar com seus bots.", "Connect a computer to see and talk to your bots.")
    }
    public var semComputadorNaContaTexto: String {
        t("Abra a Mexerica no computador e entre com esta conta. Ele aparece aqui.",
          "Open Mexerica on the computer and sign in with this account. It shows up here.")
    }
    public var computadorDaContaNota: String {
        t("Abra a Mexerica no computador e entre com esta conta: ele aparece aqui.",
          "Open Mexerica on the computer and sign in with this account: it shows up here.")
    }
    public var abreOPareamento: String { t("Abre o pareamento: instalar e ler o código", "Opens pairing: install and scan the code") }
    public var atualizaALista: String { t("Atualiza a lista de computadores da sua conta", "Refreshes your account's computers") }

    public var pessoal: String { t("Pessoal", "Personal") }
    public var voz: String { t("Voz", "Voice") }
    public var memoria: String { t("Memória", "Memory") }
    public var uso: String { t("Uso", "Usage") }
    public var esteIphone: String { t("Este iPhone", "This iPhone") }
    public var avisos: String { t("Avisos", "Alerts") }
    public var idioma: String { t("Idioma", "Language") }
    public var abreOsAjustesDoIphone: String { t("Abre os Ajustes do iPhone", "Opens iPhone Settings") }
    public var nota: String {
        t("Motor, ferramentas, permissões e organograma de cada bot se ajustam no app do computador.",
          "Each bot's engine, tools, permissions and org chart are set in the computer app.")
    }

    public var maisAjustes: String { t("Mais ajustes", "More settings") }
    public var maisAjustesValor: String { t("Tela de hoje", "Today's screen") }
    public var maisAjustesDica: String {
        t("Abre os ajustes do app de hoje: bots ocultos, remover computador, recomeçar",
          "Opens today's settings: hidden bots, remove a computer, start over")
    }

    public var sair: String { t("Sair", "Sign out") }
    /// O mesmo aviso do `AccountSwitcherView` de hoje: sair apaga os computadores, as chaves e os caches deste iPhone.
    public func sairPergunta(_ email: String?) -> String {
        guard let email, !email.isEmpty else { return t("Sair desta conta?", "Sign out of this account?") }
        return t("Sair de \(email)?", "Sign out of \(email)?")
    }
    public var sairAviso: String {
        t("Este iPhone esquecerá os computadores da conta. Seus bots continuam neles.",
          "This iPhone will forget the account's computers. Your bots stay on them.")
    }
    public var cancelar: String { t("Cancelar", "Cancel") }
    public var ok: String { t("OK", "OK") }
    public var tentarDeNovo: String { t("Tentar de novo", "Try again") }
    public var dispensarAviso: String { t("Dispensar aviso", "Dismiss alert") }

    public var erroAoAtualizar: String {
        t("Não foi possível atualizar os computadores. Tente de novo.", "Couldn't refresh the computers. Try again.")
    }
    public var erroAoSair: String { t("Não foi possível sair agora. Tente de novo.", "Couldn't sign out right now. Try again.") }

    public var emBreve: String { t("Em breve", "Coming soon") }
    public var exemploDaFolha: String {
        t("Dados de exemplo: aqui abriria a tela de hoje.", "Example data: this would open today's screen.")
    }
    public var indisponivel: String { t("Indisponível", "Unavailable") }
    public var precisaDeComputador: String { t("Precisa de um computador conectado", "Needs a connected computer") }
    public var semDados: String { t("Sem dados ainda", "No data yet") }

    // Valores

    public var vozNoAparelho: String { t("Apple, neste iPhone", "Apple, on this iPhone") }
    public func usoValor(_ percentual: Int) -> String { t("\(percentual)% usado", "\(percentual)% used") }
    public var avisosLigados: String { t("Ligados", "On") }
    public var avisosDesligados: String { t("Desligados", "Off") }
    public var avisosNegados: String { t("Desativados no iPhone", "Off in iPhone Settings") }
    public func versao(_ numero: String) -> String {
        t("Versão \(numero) · baseado no Codync", "Version \(numero) · based on Codync")
    }

    // Estados do computador

    public var conectado: String { t("Conectado", "Connected") }
    public var conectando: String { t("Conectando…", "Connecting…") }
    public var desligado: String { t("Desligado", "Off") }
    public var semConexao: String { t("Sem conexão", "Can't connect") }
    public var semAcesso: String { t("Sem acesso", "No access") }
    public var precisaAtualizar: String { t("Precisa atualizar", "Needs update") }
    public var naoPareado: String { t("Não pareado", "Not paired") }
    public var emUso: String { t("em uso", "in use") }
    public var ligadoNaConta: String { t("Ligado · na sua conta", "On · in your account") }
    public var desligadoNaConta: String { t("Desligado · na sua conta", "Off · in your account") }
    public var pedidoAguardando: String { t("pedido aguardando", "request waiting") }
    public var copiaAntiga: String { t("Cópia antiga deste computador · remova em Mais ajustes", "Old copy of this computer · remove it in More settings") }
    public var pedirAcesso: String { t("Pedir acesso", "Ask for access") }
    public var verCodigo: String { t("Ver código", "Show code") }
    public var usarEste: String { t("Toque para usar este computador", "Tap to use this computer") }
    public var abreOPedido: String { t("Abre o pedido de acesso a este computador", "Opens the access request for this computer") }
    public var selecionado: String { t("Selecionado", "Selected") }

    /// Lido pelo VoiceOver depois de trocar de computador.
    public func agoraUsando(_ nome: String) -> String { t("Agora usando \(nome)", "Now using \(nome)") }

    /// "visto ontem às 18:40": hoje, ontem ou a data curta (sem inventar quando não se sabe).
    public func visto(_ data: Date, agora: Date = .now, calendario: Calendar = .current) -> String {
        let locale = Locale(identifier: ingles ? "en" : "pt-BR")
        let hora = Date.FormatStyle(date: .omitted, time: .shortened, locale: locale, calendar: calendario, timeZone: calendario.timeZone)
        if calendario.isDate(data, inSameDayAs: agora) {
            return t("visto hoje às \(data.formatted(hora))", "last seen today at \(data.formatted(hora))")
        }
        if calendario.isDate(data, inSameDayAs: calendario.date(byAdding: .day, value: -1, to: agora) ?? agora) {
            return t("visto ontem às \(data.formatted(hora))", "last seen yesterday at \(data.formatted(hora))")
        }
        let dia = Date.FormatStyle(date: .abbreviated, time: .omitted, locale: locale, calendar: calendario, timeZone: calendario.timeZone)
        return t("visto em \(data.formatted(dia))", "last seen on \(data.formatted(dia))")
    }

    /// A linha de estado do computador, em partes (a tela as junta com " · " e o VoiceOver com ", ").
    public func partes(_ estado: MXEstadoDoComputador, emUso uso: Bool, agora: Date = .now, calendario: Calendar = .current) -> [String] {
        var partes: [String]
        switch estado {
        case .conectado: partes = [conectado]
        case .conectando: partes = [conectando]
        case let .desligado(visto):
            partes = [desligado]
            if uso { partes.append(emUso) }
            if let visto { partes.append(self.visto(visto, agora: agora, calendario: calendario)) }
            return partes
        case .semConexao: partes = [semConexao]
        case .semAcesso: partes = [semAcesso]
        case .precisaAtualizar: partes = [precisaAtualizar]
        case .naoPareado: partes = [naoPareado]
        case let .daConta(ligado): return [ligado ? ligadoNaConta : desligadoNaConta]
        case let .pedidoEmAndamento(ligado, detalhe):
            return [detalhe ?? [ligado ? ligadoNaConta : desligadoNaConta, pedidoAguardando].joined(separator: " · ")]
        case .copiaAntiga: return [copiaAntiga]
        }
        if uso { partes.append(emUso) }
        return partes
    }

    public func estado(_ estado: MXEstadoDoComputador, emUso: Bool, agora: Date = .now, calendario: Calendar = .current) -> String {
        partes(estado, emUso: emUso, agora: agora, calendario: calendario).joined(separator: " · ")
    }
}

// MARK: - Dados

/// Como está um computador na lista (texto, ícone e se dá para usar).
public enum MXEstadoDoComputador: Equatable, Sendable {
    case conectado
    case conectando
    /// O computador está desligado; `visto` é a última vez que a nuvem o viu, se ela sabe.
    case desligado(visto: Date?)
    case semConexao
    case semAcesso
    case precisaAtualizar
    case naoPareado
    /// Computador da conta que este iPhone ainda não usa.
    case daConta(ligado: Bool)
    /// Pedido de acesso em andamento; `detalhe` é a frase que o app de hoje já mostra (traz o código a digitar).
    case pedidoEmAndamento(ligado: Bool, detalhe: String?)
    /// A mesma máquina sob uma identidade antiga: só se limpa, nos "Mais ajustes".
    case copiaAntiga

    /// O ícone que acompanha o texto: nunca só a cor diz o estado.
    public var marca: MXMarcaDoEstado {
        switch self {
        case .conectado: .ok
        case .semConexao, .semAcesso, .precisaAtualizar, .copiaAntiga: .alerta
        case .conectando, .desligado, .naoPareado, .daConta, .pedidoEmAndamento: .vazio
        }
    }
}

public enum MXMarcaDoEstado: Equatable, Sendable {
    case ok, vazio, alerta
}

/// O que tocar na linha do computador faz.
public enum MXAcaoDaLinha: Equatable, Sendable {
    /// Trocar para este computador.
    case usar
    /// Abrir o pedido de acesso (computador da conta ainda sem acesso).
    case pedirAcesso
    /// Abrir o pedido em andamento (mostra o código).
    case verCodigo
    /// Nada (já é o que está em uso, ou só se resolve nos Mais ajustes).
    case nenhuma
}

public struct MXComputadorLinha: Identifiable, Equatable, Sendable {
    public let id: String
    public var nome: String
    public var estado: MXEstadoDoComputador
    public var emUso: Bool
    public var acao: MXAcaoDaLinha

    public init(id: String, nome: String, estado: MXEstadoDoComputador, emUso: Bool = false, acao: MXAcaoDaLinha) {
        self.id = id
        self.nome = nome
        self.estado = estado
        self.emUso = emUso
        self.acao = acao
    }
}

/// O que "Conectar outro computador" faz.
public enum MXConectar: Equatable, Sendable {
    /// Pareamento por QR (build sem conta): abre o pareamento do app de hoje.
    case parear
    /// Com conta, os computadores vêm da conta: não há código a ler; a linha atualiza a lista.
    case atualizar
}

public enum MXAvisosDoIphone: Equatable, Sendable {
    case ligados, desligados
    /// O iOS não deixa avisar: só os Ajustes do iPhone mudam isso.
    case negadoNoIphone
}

public enum MXVozValor: Equatable, Sendable {
    case noAparelho
    case provedor(String)
    case indisponivel
}

public enum MXUsoValor: Equatable, Sendable {
    case percentual(Int)
    case semDados
    case indisponivel
}

/// Tudo o que a tela mostra, já resolvido (sem texto: a tela escolhe o idioma).
public struct MXContaDados: Equatable, Sendable {
    public var email: String?
    /// Entrou numa conta.
    public var logada: Bool
    /// O build usa conta (com login); sem ela só há pareamento local.
    public var configurada: Bool
    public var computadores: [MXComputadorLinha]
    public var conectar: MXConectar
    /// Nulo: este build não tem avisos remotos, e a linha não aparece.
    public var avisos: MXAvisosDoIphone?
    public var voz: MXVozValor
    public var uso: MXUsoValor
    /// O nome do idioma do app, escrito nele mesmo ("Português (Brasil)").
    public var idioma: String
    public var versao: String
    /// A lista está sendo atualizada.
    public var atualizando: Bool

    public init(email: String? = nil, logada: Bool = false, configurada: Bool = false, computadores: [MXComputadorLinha] = [],
                conectar: MXConectar = .parear, avisos: MXAvisosDoIphone? = nil, voz: MXVozValor = .indisponivel,
                uso: MXUsoValor = .indisponivel, idioma: String = "", versao: String = "", atualizando: Bool = false) {
        self.email = email
        self.logada = logada
        self.configurada = configurada
        self.computadores = computadores
        self.conectar = conectar
        self.avisos = avisos
        self.voz = voz
        self.uso = uso
        self.idioma = idioma
        self.versao = versao
        self.atualizando = atualizando
    }

    public var semComputador: Bool { computadores.isEmpty }
    public var emUso: MXComputadorLinha? { computadores.first(where: \.emUso) }
}

/// O que não deu certo (a tela escreve a frase; o texto bruto do Hub ou da conta nunca sai).
public enum MXErroDaConta: Equatable, Sendable {
    case atualizar
    case sair
}

/// Para onde a tela leva por baixo (o app de hoje, vestido de ponte): cada um abre numa folha.
public enum MXDestinoDaConta: Hashable, Identifiable, Sendable {
    case conectar
    case acesso(String)
    case voz
    case uso
    case maisAjustes

    public var id: String {
        switch self {
        case .conectar: "conectar"
        case let .acesso(id): "acesso-\(id)"
        case .voz: "voz"
        case .uso: "uso"
        case .maisAjustes: "mais-ajustes"
        }
    }
}

// MARK: - Regras

public enum MXContaRegras {
    /// O estado de um computador deste iPhone a partir da conexão que o store mostra (a que já esconde a queda curta).
    public static func estado(conexao: BotStore.Connection, precisaAtualizar: Bool, copiaAntiga: Bool) -> MXEstadoDoComputador {
        if copiaAntiga { return .copiaAntiga }
        switch conexao {
        case .online: return precisaAtualizar ? .precisaAtualizar : .conectado
        case .connecting: return .conectando
        case let .computerOffline(visto): return .desligado(visto: visto)
        case .offline: return .semConexao
        case .unauthorized: return .semAcesso
        case .unpaired: return .naoPareado
        }
    }

    /// O estado de um computador da conta que este iPhone ainda não usa.
    public static func estadoDaConta(ligado: Bool, copiaAntiga: Bool, pedidoEmAndamento: Bool, detalhe: String?) -> MXEstadoDoComputador {
        if copiaAntiga { return .copiaAntiga }
        if pedidoEmAndamento { return .pedidoEmAndamento(ligado: ligado, detalhe: detalhe) }
        return .daConta(ligado: ligado)
    }

    /// O que tocar na linha faz: trocar, se ainda não é o computador em uso e dá para trocar; a cópia antiga só se limpa
    /// nos Mais ajustes.
    public static func acao(estado: MXEstadoDoComputador, emUso: Bool) -> MXAcaoDaLinha {
        if emUso { return .nenhuma }
        switch estado {
        case .copiaAntiga: return .nenhuma
        case .daConta: return .pedirAcesso
        case .pedidoEmAndamento: return .verCodigo
        default: return .usar
        }
    }

    /// Qual computador está em uso: o escolhido, se ainda existe; senão o último que o app de hoje usou; senão o primeiro.
    public static func emUso(escolhido: String?, ultimo: String?, ids: [String]) -> String? {
        for candidato in [escolhido, ultimo] {
            if let candidato, ids.contains(candidato) { return candidato }
        }
        return ids.first
    }

    /// "62%" no valor de Uso: o limite mais apertado entre os provedores.
    public static func uso(providers: [UsageProvider]) -> MXUsoValor {
        guard let maior = providers.flatMap(\.windows).map(\.percent).max() else { return .semDados }
        return .percentual(Int(maior.rounded()))
    }

    /// O nome do idioma do app escrito nele mesmo ("Português (Brasil)", "English").
    public static func nomeDoIdioma(_ identificador: String?) -> String {
        let id = identificador ?? "pt-BR"
        let locale = Locale(identifier: id)
        guard let nome = locale.localizedString(forIdentifier: id) else { return id }
        return nome.prefix(1).uppercased() + nome.dropFirst()
    }

    /// O texto de VoiceOver de uma linha de computador: nome, estado, "selecionado" se em uso.
    public static func rotuloAcessivel(_ linha: MXComputadorLinha, textos: MXTextosDaConta, agora: Date = .now,
                                        calendario: Calendar = .current) -> (rotulo: String, valor: String) {
        (linha.nome, textos.partes(linha.estado, emUso: linha.emUso, agora: agora, calendario: calendario).joined(separator: ", "))
    }
}
#endif
