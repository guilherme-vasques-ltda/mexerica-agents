// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import AuthenticationServices
import ClerkKit
import CodyncKit
import CodyncUI
import Foundation
import Observation

/// Clerk owns OAuth, session restoration and Keychain persistence. An account session
/// never grants access to a computer by itself: each one still approves this device (spec §4.2).
@MainActor
@Observable
final class AccountSession {
    private let clerk: Clerk?
    /// Account mode is mandatory in Main Release, including a broken configuration.
    let isConfigured: Bool
    private(set) var configurationError: String?
    var requiresLogin: Bool { AccountConfiguration.requiresLogin(configured: isConfigured, signedIn: isSignedIn) }
    private var pendingSignIn: SignIn?
    private(set) var needsEmailCode = false
    private(set) var isBusy = false
    var errorMessage: String?

    var showSwitcher = false
    /// Every call that can change the active session goes through `changeSession`, which lets the active
    /// account clean up first; the app attaches that clean-up (`AccountTransition.attach`).
    let transition = AccountTransition()

    struct Account: Identifiable {
        let id: String
        let sessionID: String
        let email: String
        let avatarURL: URL?
    }

    var userID: String? {
        guard let clerk, clerk.isLoaded, let session = clerk.session,
              session.status == .active, session.expireAt > .now else { return nil }
        return clerk.user?.id
    }
    var workspaceSessionID: String? { userID == nil ? nil : clerk?.session?.id }
    private let enablesWorkspaceDirectory: Bool
    var usesWorkspaceDirectory: Bool { isConfigured && enablesWorkspaceDirectory }

    func loadWorkspaceMemberships() async throws -> ForkWorkspaceMemberships {
        guard usesWorkspaceDirectory, let sessionID = workspaceSessionID, let user = clerk?.user,
              let settings = clerk?.environment?.organizationSettings else {
            throw SessionError.signedOut
        }
        let memberships = try await ForkClerkWorkspaceDirectory().load(user: user, organizationsEnabled: settings.enabled)
        try Task.checkCancellation()
        guard workspaceSessionID == sessionID, userID == user.id else { throw SessionError.signedOut }
        return memberships
    }
    var accounts: [Account] {
        var seen = Set<String>()
        return (clerk?.auth.sessions ?? []).compactMap { session in
            guard session.status == .active, session.expireAt > .now,
                  let user = session.user, seen.insert(user.id).inserted else { return nil }
            return Account(id: user.id, sessionID: session.id,
                           email: user.primaryEmailAddress?.emailAddress ?? "Mexerica account",
                           avatarURL: URL(string: user.imageUrl).flatMap { $0.scheme == "https" ? $0 : nil })
        }
    }
    var supportsMultipleAccounts: Bool { clerk?.environment?.authConfig.singleSessionMode == false }

    /// Clerk restores the persisted Keychain session during configuration. Don't offer a fresh sign-in
    /// until that restore has completed, or an update can briefly look like a signed-out install.
    var isReady: Bool { clerk?.isLoaded ?? true }
    /// The Codync cloud (accounts and the encrypted relay); nil in builds without one.
    let cloudURL: URL?
    var isSignedIn: Bool { userID != nil }
    var email: String? { clerk?.user?.primaryEmailAddress?.emailAddress }
    var avatarURL: URL? {
        guard let image = clerk?.user?.imageUrl, let url = URL(string: image), url.scheme == "https" else { return nil }
        return url
    }

    init() {
        #if DEBUG
        let isDebug = true
        let env = ProcessInfo.processInfo.environment
        #else
        let isDebug = false
        let env: [String: String] = [:]
        #endif
        let configData = Bundle.main.url(forResource: "AccountConfig", withExtension: "plist")
            .flatMap { try? Data(contentsOf: $0) }
        let config = configData
            .flatMap { try? PropertyListDecoder().decode(AccountConfiguration.self, from: $0) }
        enablesWorkspaceDirectory = configData.flatMap {
            try? PropertyListDecoder().decode(ForkClerkWorkspaceDirectory.Configuration.self, from: $0)
        }?.workspaceDirectoryEnabled == true
        let cloudValue = AccountConfiguration.value(config?.cloudURL, debugOverride: env["CODYNC_CLOUD_URL"], isDebug: isDebug)
        cloudURL = URL(string: cloudValue)
            .flatMap { Pairing.isCloudURL($0) ? $0 : nil }
        let key = AccountConfiguration.value(config?.clerkPublishableKey, debugOverride: env["CODYNC_CLERK_PUBLISHABLE_KEY"], isDebug: isDebug)
        isConfigured = AccountConfiguration.isConfigured(cloudURL: cloudValue, publishableKey: key,
                                                        environment: AppEnvironment.current.rawValue, isDebug: isDebug)
        guard isConfigured else {
            clerk = nil
            return
        }
        guard config != nil, cloudURL != nil, let tokenRequest = try? ClerkTokenRequest(publishableKey: key, origin: config?.clerkTokenOrigin ?? "") else {
            clerk = nil
            configurationError = "Configuração de login inválida. Confira AccountConfig.plist, cloudURL, clerkPublishableKey e clerkTokenOrigin."
            return
        }
        let scheme = Bundle.main.bundleIdentifier ?? "app.mexerica.ios"
        clerk = Clerk.configure(publishableKey: key, options: .init(
            telemetryEnabled: false,
            redirectConfig: .init(redirectUrl: "\(scheme)://callback", callbackUrlScheme: scheme),
            middleware: .init(request: [TokenOriginMiddleware(policy: tokenRequest)])
        ))
        transition.isAccountActive = { [weak self] in self?.isSignedIn ?? false }
    }

    /// The only way this type calls Clerk to sign in, switch or sign out: through `transition`, so the
    /// active account cleans up while its session still works. Any new such call goes here too.
    @discardableResult
    private func changeSession<T>(_ kind: AccountTransition.Kind = .change,
                                  _ change: @MainActor () async throws -> T) async throws -> T {
        try await transition.run(kind, change)
    }

    /// Force the native POST: the SDK cache can contain a login/restore token issued without Origin.
    /// Clerk owns credentials; only the resulting session JWT goes to our cloud.
    func sessionToken() async throws -> String {
        guard let clerk, isSignedIn, let sessionID = clerk.session?.id else { throw SessionError.signedOut }
        guard let token = try await clerk.auth.getToken(.init(skipCache: true)), clerk.session?.id == sessionID, isSignedIn else {
            throw SessionError.signedOut
        }
        return token
    }

    /// The Codync cloud for `userID`'s context; nil when signed out or the build has no cloud.
    /// Requests stop working once another account becomes active, so one account's calls never carry another's token.
    func cloudClient(for userID: String?, identity: DeviceIdentity?) -> CloudClient? {
        guard let userID, let cloudURL, isConfigured, !usesWorkspaceDirectory,
              let sessionID = workspaceSessionID else { return nil }
        return CloudClient(baseURL: cloudURL, identity: identity) { [weak self] in
            guard let self, await self.userID == userID, await self.workspaceSessionID == sessionID else { throw SessionError.signedOut }
            let token = try await self.sessionToken()
            guard await self.userID == userID, await self.workspaceSessionID == sessionID else { throw SessionError.signedOut }
            return token
        }
    }

    enum SessionError: LocalizedError {
        case signedOut
        var errorDescription: String? { "Sign in again to reach your account." }
    }

    enum SignInProvider: String {
        case google = "Google"
        case apple = "Apple"
    }

    func signIn(provider: SignInProvider = .google) async {
        guard !isBusy, isReady else { return }
        guard provider != .apple || AppEnvironment.supportsAppleSignIn else { return }
        guard let clerk else {
            errorMessage = "Configuração de login indisponível. Confira o plist local."
            return
        }
        let tracked: AnalyticsEvent.SignInProvider = provider == .apple ? .apple : .google
        Analytics.shared.capture(.signInStarted(tracked))
        isBusy = true
        errorMessage = nil
        defer { isBusy = false }
        do {
            // Existing-account sign-in only: never transfer an unknown identity to sign-up.
            let result: TransferFlowResult
            switch provider {
            case .google:
                let ephemeral = isSignedIn
                result = try await changeSession(.signInWithCallback) {
                    try await clerk.auth.signInWithOAuth(provider: .google, prefersEphemeralWebBrowserSession: ephemeral, transferable: false)
                }
            case .apple:
                result = try await changeSession(.signInWithCallback) { try await clerk.auth.signInWithApple(transferable: false) }
            }
            switch result {
            case .signIn(let signIn):
                try await finishSignIn(signIn)
            case .signUp:
                errorMessage = "Esta conta ainda não tem acesso. Entre com uma conta já liberada."
            }
            Analytics.shared.capture(errorMessage == nil ? .signedIn(tracked) : .signInFailed(tracked, reason: .verification))
        } catch where Self.isCancellation(error) {
            Analytics.shared.capture(.signInFailed(tracked, reason: .cancelled))
        } catch {
            errorMessage = "Couldn't sign in. Check your connection and try again."
            Analytics.shared.capture(.signInFailed(tracked, reason: .error))
        }
    }

    func signIn(email: String, password: String) async {
        guard !isBusy, isReady, let clerk else { return }
        isBusy = true
        errorMessage = nil
        pendingSignIn = nil
        needsEmailCode = false
        defer { isBusy = false }
        do {
            let signIn = try await changeSession { try await clerk.auth.signInWithPassword(identifier: email, password: password) }
            try await finishSignIn(signIn)
        } catch {
            errorMessage = "Não foi possível entrar. Confira e-mail, senha e conexão."
        }
    }

    /// Production uses Device Trust on a new iPhone; password alone can require an email code.
    private func finishSignIn(_ signIn: SignIn) async throws {
        if signIn.status == .complete {
            pendingSignIn = nil
            needsEmailCode = false
        } else if (signIn.status == .needsClientTrust || signIn.status == .needsSecondFactor),
                  signIn.supportedSecondFactors?.contains(where: { $0.strategy == .emailCode }) == true {
            pendingSignIn = try await signIn.sendMfaEmailCode()
            needsEmailCode = true
        } else {
            errorMessage = "A conta exige uma verificação adicional não disponível nesta versão. A sessão continua bloqueada."
        }
    }

    func verifyEmailCode(_ code: String) async {
        guard !isBusy, let pendingSignIn else { return }
        isBusy = true
        errorMessage = nil
        defer { isBusy = false }
        do {
            let result = try await changeSession { try await pendingSignIn.verifyMfaCode(code, type: .emailCode) }
            if result.status == .complete {
                self.pendingSignIn = nil
                needsEmailCode = false
            } else {
                errorMessage = "A verificação ainda não foi concluída."
            }
        } catch {
            errorMessage = "Código inválido ou expirado. Tente novamente ou entre de novo."
        }
    }

    func cancelEmailCode() {
        guard !isBusy else { return }
        pendingSignIn = nil
        needsEmailCode = false
        errorMessage = nil
    }

    /// The user closed the sign-in sheet.
    private static func isCancellation(_ error: any Error) -> Bool {
        switch error {
        case let error as ASWebAuthenticationSessionError: error.code == .canceledLogin
        case let error as ASAuthorizationError: error.code == .canceled
        default: error is CancellationError
        }
    }

    func signOut() async {
        guard !isBusy, let clerk, let sessionID = clerk.session?.id else { return }
        isBusy = true
        errorMessage = nil
        defer { isBusy = false }
        do {
            try await changeSession { try await clerk.auth.signOut(sessionId: sessionID) }
            pendingSignIn = nil
            needsEmailCode = false
            showSwitcher = false
        } catch {
            errorMessage = "Couldn't sign out. Check your connection and try again."
        }
    }

    /// Every account on this device, for starting over.
    func signOutAll() async -> Bool {
        guard !isBusy, isReady else {
            errorMessage = "Aguarde a operação da conta terminar antes de começar de novo."
            return false
        }
        guard let clerk else {
            if isConfigured { errorMessage = "Corrija a configuração de login antes de sair das contas." }
            return !isConfigured
        }
        isBusy = true
        errorMessage = nil
        defer { isBusy = false }
        do {
            try await changeSession {
                for session in clerk.auth.sessions where session.status == .active {
                    try await clerk.auth.signOut(sessionId: session.id)
                }
            }
            guard !clerk.auth.sessions.contains(where: { $0.status == .active }), clerk.session?.status != .active else {
                errorMessage = "Ainda há uma sessão ativa. Tente sair novamente antes de apagar os dados."
                return false
            }
            pendingSignIn = nil
            needsEmailCode = false
            return true
        } catch {
            errorMessage = "Não foi possível sair de todas as contas. Os dados locais foram preservados. Confira a conexão e tente novamente."
            return false
        }
    }

    func switchAccount(_ account: Account) async {
        guard !isBusy, let clerk, account.id != userID else { return }
        isBusy = true
        errorMessage = nil
        defer { isBusy = false }
        do {
            // Before B becomes active: A's cloud client only works while A is.
            try await changeSession { try await clerk.auth.setActive(sessionId: account.sessionID) }
        } catch {
            errorMessage = "Couldn't switch accounts. Check your connection or sign in again."
        }
    }

    func handle(_ url: URL) async {
        guard let clerk else { return }
        // Part of a browser sign-in running now (no queue, no deadlock), or a change of its own.
        do { _ = try await changeSession(.callback) { try await clerk.handle(url) } }
        catch { errorMessage = "Couldn't finish signing in. Please try again." }
    }

}

private struct TokenOriginMiddleware: ClerkRequestMiddleware {
    let policy: ClerkTokenRequest
    func prepare(_ request: inout URLRequest) async throws { policy.prepare(&request) }
}
