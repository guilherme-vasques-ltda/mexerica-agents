import Foundation

/// Public build configuration only. Credentials belong to Clerk's Keychain, never a plist.
public struct AccountConfiguration: Decodable, Sendable {
    public var clerkPublishableKey: String?
    public var clerkTokenOrigin: String?
    public var cloudURL: String?

    /// Main Release requires an account even when its configuration is missing or broken.
    public static func isConfigured(cloudURL: String, publishableKey: String, environment: String, isDebug: Bool) -> Bool {
        (!isDebug && environment != "dev") ||
            (!cloudURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
             !publishableKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
    }

    public static func value(_ bundled: String?, debugOverride: String?, isDebug: Bool) -> String {
        ((isDebug ? debugOverride : nil) ?? bundled ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public enum ConfigurationError: LocalizedError, Equatable {
        case missingLocalFile(environment: String)
        case missingKey
        case missingCloud
        public var errorDescription: String? {
            switch self {
            case .missingLocalFile(let environment):
                return "apps/mobile/shared/Config/\(environment).local.plist não encontrado: crie o arquivo com a configuração da sua instalação; ele é ignorado pelo Git."
            case .missingKey:
                return "clerkPublishableKey vazia: preencha a chave publicável no .local.plist do ambiente em apps/mobile/shared/Config."
            case .missingCloud:
                return "Release de main exige cloudURL preenchida no plist do ambiente."
            }
        }
    }

    /// Release e qualquer build de aparelho exigem conta; Debug de simulador e Dev seguem permitindo configuração vazia.
    public func validateForBuild(environment: String, isDebug: Bool, isDevice: Bool = false, localFileExists: Bool = true) throws {
        guard environment != "dev", !isDebug || isDevice else { return }
        guard localFileExists else { throw ConfigurationError.missingLocalFile(environment: environment) }
        if Self.value(clerkPublishableKey, debugOverride: nil, isDebug: false).isEmpty {
            throw ConfigurationError.missingKey
        }
        guard !Self.value(cloudURL, debugOverride: nil, isDebug: false).isEmpty else {
            throw ConfigurationError.missingCloud
        }
    }

    /// Shared by the iOS build tool and tests. An empty local value deliberately disables a field.
    public static func merge(base: Data, local: Data?) throws -> Data {
        func dictionary(_ data: Data) throws -> [String: Any] {
            guard let value = try PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any] else {
                throw CocoaError(.propertyListReadCorrupt)
            }
            return value
        }
        var values = try dictionary(base)
        if let local { values.merge(try dictionary(local)) { _, replacement in replacement } }
        return try PropertyListSerialization.data(fromPropertyList: values, format: .xml, options: 0)
    }

    /// A configured build never lets onboarding history or saved computers bypass sign-in.
    /// Empty development configuration deliberately preserves the original QR flow.
    public static func requiresLogin(configured: Bool, signedIn: Bool) -> Bool {
        configured && !signedIn
    }
}

/// ClerkKit 1.5.6 already builds/authenticates the native token POST. It omits Origin.
/// Add it only to session-token issuance, never OAuth, sign-in or other API requests.
public struct ClerkTokenRequest: Sendable {
    let origin: String
    let frontend: URL

    public init(publishableKey: String, origin: String) throws {
        guard let tokenOrigin = Self.httpsOrigin(origin),
              publishableKey.hasPrefix("pk_live_") || publishableKey.hasPrefix("pk_test_") else {
            throw CocoaError(.fileReadCorruptFile)
        }
        var encoded = String(publishableKey.dropFirst(8))
        encoded += String(repeating: "=", count: (4 - encoded.count % 4) % 4)
        guard let data = Data(base64Encoded: encoded), let decoded = String(data: data, encoding: .utf8),
              decoded.hasSuffix("$"), let frontend = Self.httpsOrigin("https://" + decoded.dropLast()) else {
            throw CocoaError(.fileReadCorruptFile)
        }
        self.origin = tokenOrigin.absoluteString
        self.frontend = frontend
    }

    private static func httpsOrigin(_ value: String) -> URL? {
        guard let url = URL(string: value), let parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
              parts.scheme == "https", parts.host?.isEmpty == false, parts.port == nil,
              parts.user == nil, parts.password == nil, parts.path.isEmpty,
              parts.query == nil, parts.fragment == nil else { return nil }
        return url
    }

    public func prepare(_ request: inout URLRequest) {
        guard let url = request.url, url.scheme == frontend.scheme, url.host == frontend.host,
              url.port == nil, url.user == nil, url.password == nil,
              request.httpMethod == "POST" else { return }
        let path = url.path.split(separator: "/")
        guard path.count == 5, path[0] == "v1", path[1] == "client", path[2] == "sessions",
              path[3].hasPrefix("sess_"), path[4] == "tokens" else { return }
        request.httpShouldHandleCookies = false
        request.setValue(origin, forHTTPHeaderField: "Origin")
    }
}
