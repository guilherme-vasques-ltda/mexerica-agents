// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

/// The computer's remote screen: whether phones can view/control it, and who's in control.
public struct ScreenState: Codable, Equatable, Sendable {
    public var enabled = false
    /// The screen helper is running.
    public var connected = false
    public var platform = ""
    /// App name shown in the computer’s system permission settings.
    public var permissionApp: String?
    /// Screen recording is permitted.
    public var capture = false
    /// Input injection is permitted.
    public var input = false
    public var displays: [ScreenDisplay] = []
    /// A phone took over: bots may only look.
    public var userControl = false
    /// The bot using the computer right now.
    public var agentBot: String?
    /// Bots with computer use can act here (Windows: always; elsewhere Remote screen is on).
    public var computerUse = false
    public var viewers = 0

    public init() {}

    public var available: Bool { enabled && connected && capture }
    public var controlReady: Bool { platform == "windows" ? computerUse : available && input }
    public var mainDisplay: ScreenDisplay? { displays.first(where: \.main) ?? displays.first }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? false
        connected = try c.decodeIfPresent(Bool.self, forKey: .connected) ?? false
        platform = try c.decodeIfPresent(String.self, forKey: .platform) ?? ""
        permissionApp = try c.decodeIfPresent(String.self, forKey: .permissionApp)
        capture = try c.decodeIfPresent(Bool.self, forKey: .capture) ?? false
        input = try c.decodeIfPresent(Bool.self, forKey: .input) ?? false
        displays = try c.decodeIfPresent([ScreenDisplay].self, forKey: .displays) ?? []
        userControl = try c.decodeIfPresent(Bool.self, forKey: .userControl) ?? false
        agentBot = try c.decodeIfPresent(String.self, forKey: .agentBot)
        computerUse = try c.decodeIfPresent(Bool.self, forKey: .computerUse) ?? false
        viewers = try c.decodeIfPresent(Int.self, forKey: .viewers) ?? 0
    }
}

public struct ScreenDisplay: Codable, Hashable, Identifiable, Sendable {
    public var id: UInt32
    public var name: String
    /// Size in points: the coordinate space of screen input.
    public var width: Double
    public var height: Double
    public var main: Bool
}

extension ScreenState {
    /// Remote clients explain setup; only the computer itself can request OS permissions.
    public var accessGuidance: String { accessGuidance(bundle: .module) }

    func accessGuidance(bundle: Bundle) -> String {
        if platform == "windows" {
            return String(localized: "A visualização remota da tela ainda não está disponível no Windows. Os bots ainda podem usar aplicativos nesse computador.", bundle: bundle)
        }
        let setup = String(localized: "No computador, abra Mexerica → Ajustes → Acesso ao computador. As permissões são configuradas separadamente em cada computador.", bundle: bundle)
        if !enabled { return String(localized: "A tela remota está desativada. \(setup)", bundle: bundle) }
        if !connected { return String(localized: "Mexerica Screen não está em execução. \(setup)", bundle: bundle) }
        if !capture || (platform == "linux" && !input) {
            if platform == "linux" { return String(localized: "Escolha uma tela e permita o controle na janela de compartilhamento do sistema. \(setup)", bundle: bundle) }
            return String(localized: "Permita a gravação da tela para \(permissionApp ?? "Mexerica Screen"). \(setup)", bundle: bundle)
        }
        if !input { return String(localized: "Permita o controle do computador para \(permissionApp ?? "Mexerica Screen"). O macOS pode chamar isso de Controle de Dispositivos e Acesso a Dados ou Acessibilidade. \(setup)", bundle: bundle) }
        return String(localized: "O acesso ao computador está pronto.", bundle: bundle)
    }
}
