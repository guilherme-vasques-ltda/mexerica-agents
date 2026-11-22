// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

/// Read from the containing app or extension, so the shared package follows its build configuration.
public enum AppEnvironment: String, Sendable {
    case main
    case dev

    public static let current = AppEnvironment(
        rawValue: Bundle.main.object(forInfoDictionaryKey: "CodyncEnvironment") as? String ?? "main"
    ) ?? .main

    /// Missing or unknown signing configuration always uses the restricted mode.
    public static let signing = Bundle.main.object(forInfoDictionaryKey: "MexericaSigning") as? String ?? "free"
    public static var supportsAppleSignIn: Bool { signing == "full" }

    public static func allowsRemotePush(signing: String, relayURL: String) -> Bool {
        guard signing == "full", let url = URL(string: relayURL),
              url.scheme == "https", let host = url.host, !host.isEmpty,
              url.user == nil, url.password == nil else { return false }
        return true
    }

    public var appGroup: String { self == .dev ? "group.app.mexerica.ios.dev" : "group.app.mexerica.ios" }
    public var urlScheme: String { self == .dev ? "donnabot-dev" : "donnabot" }

    public func link(_ destination: String) -> URL {
        // All callers supply a fixed app route.
        URL(string: "\(urlScheme)://\(destination)")!
    }
}
