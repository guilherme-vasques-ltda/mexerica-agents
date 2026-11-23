// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import CodyncUI
import SwiftUI
import Observation
import UserNotifications
import WidgetKit

@main
struct CodyncApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var app = AppStore.shared
    /// What the App Store has: greys out an update it doesn't have yet, reminds of newer ones.
    @State private var updates = AppUpdates()
    @Environment(\.scenePhase) private var scenePhase
    @AppStorage("onboardingCompleted") private var onboardingCompleted = false
    /// Covers Clerk restoration before deciding between the account and the welcome.
    @State private var launching = true
    init() {
        // A tapped widget bot arrives as an intent, not a URL.
        OpenBotIntent.open = { AppStore.shared.open($0) }
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .id("\(app.contextID)#\(app.generation)")
                .environment(app)
                .environment(app.accounts)
                .environment(app.account)
                .environment(updates)
                .tint(Palette.accent)
                // No scroll indicators anywhere in the app.
                .scrollIndicators(.never)
                .codyncSheet(isPresented: Binding(
                    get: { !app.account.requiresLogin && (onboardingCompleted || app.account.isSignedIn) && app.account.showSwitcher },
                    set: { app.account.showSwitcher = $0 }
                )) {
                    // Holds the pushes inside the sheet (Computers & settings, Widgets); no bar shows.
                    NavigationStack { AccountSwitcherView() }
                        .environment(app)
                        .environment(app.accounts)
                        .environment(app.account)
                        .tint(Palette.accent)
                }
                .onChange(of: app.workspaceSession, initial: true) { _, _ in
                    app.syncAccountContext()
                }
                .task(id: app.workspaceRefreshKey(foreground: scenePhase == .active)) {
                    guard scenePhase == .active else { return }
                    await app.refreshWorkspacesWhileActive()
                }
                .overlay {
                    if launching || !app.account.isReady {
                        LaunchSplash(stores: app.accounts.computers.compactMap { app.accounts.store(for: $0.id) },
                                     accountReady: app.account.isReady) {
                            guard app.account.isReady else { return }
                            withAnimation(.easeOut(duration: 0.35)) { launching = false }
                        }
                    }
                }
                #if DEBUG
                // Prévia do redesenho iOS v1 (só Debug/DevDebug): `-MexericaRedesign` (exemplo, sem login),
                // `-MexericaRedesignReal` (dados do computador da conta) ou o interruptor "Novo visual (teste)" dos
                // Ajustes (mesmos dados reais); `MXCascaDoApp` decide e não mostra nada se nenhum estiver ligado.
                .overlay { MXCascaDoApp(contas: app.accounts, ponte: MXPonteApp.criar(app: app)) }
                #endif
                .onOpenURL { url in app.open(url) }
                .onChange(of: scenePhase, initial: true) { _, phase in
                    switch phase {
                    case .active: Analytics.shared.becameActive()
                    case .background: BackgroundTime.run("Analytics") { await Analytics.shared.enteredBackground() }
                    default: break
                    }
                    // Only leaving for the background disconnects; `.inactive` (Control Center,
                    // app switcher, system prompts) comes and goes too often to drop the link.
                    app.accounts.setActive(phase != .background && !app.account.requiresLogin)
                    // Back in the app: a computer may have joined the account meanwhile.
                    if phase == .active {
                        let accounts = app.accounts
                        Task { await accounts.refreshCloud() }
                        Task { await updates.refresh() }
                    }
                }
                #if DEBUG
                .task {
                    // Simulator/UI-test pairing: SIMCTL_CHILD_CODYNC_PAIR_URL=donnabot://pair?...
                    if let s = ProcessInfo.processInfo.environment["CODYNC_PAIR_URL"], let p = Pairing(string: s) {
                        _ = try? await app.pair(p, via: .link)
                    }
                    if ProcessInfo.processInfo.environment["CODYNC_OPEN_USAGE"] != nil { app.tab = .state; app.showUsage = true }
                    if ProcessInfo.processInfo.environment["CODYNC_STATE_SURFACE"] != nil { app.tab = .state }
                    // Screenshots: CODYNC_OPEN_URL=donnabot://bot/<botId>?scope=local&computer=<id>, without the system prompt.
                    if let s = ProcessInfo.processInfo.environment["CODYNC_OPEN_URL"], let url = URL(string: s) {
                        try? await Task.sleep(for: .seconds(1))
                        app.open(url)
                    }
                    await LiveActivities.shared.previewIfRequested()
                    if ProcessInfo.processInfo.environment["CODYNC_OPEN_SCREEN"] != nil {
                        try? await Task.sleep(for: .seconds(2))
                        app.currentStore?.screenRequest = ScreenRequest()
                    }
                }
                #endif
        }
    }
}

enum AppTab: Hashable { case bots, state }

/// One `AccountStore` per account context: switching accounts retires the old one so
/// in-flight work can't land on another user's screen or storage.
@MainActor
@Observable
final class AppStore {
    static let shared = AppStore()
    let account: AccountSession
    private(set) var accounts: AccountStore
    /// The signed-in account's cloud; nil when signed out or the build has no cloud.
    private(set) var cloud: CloudClient?
    private(set) var contextID: String
    /// Rebuilds the UI when the scope is replaced, even when its storage ID is unchanged.
    private(set) var generation = 0
    var workspaceSelection: ForkWorkspaceSelection?
    var workspaceDirectoryUnavailable = false
    var appliedWorkspaceSession: ForkWorkspaceSession?
    private var accountStartup: Task<Void, Never>?
    var tab = AppTab.bots
    var showComputers = false
    /// The usage limits sheet over the State tab.
    var showUsage = false
    /// The computer whose marketplace is open.
    var marketplace: ComputerID?
    /// Keep contexts from a partially successful logout until Start over can finish safely.
    private var resetAccountIDs = Set<String>()

    private init() {
        let session = AccountSession()
        let storage = Self.initialStorage(for: session)
        SharedStore.clearLegacyAttachments()
        SharedStore.activate(storage)
        account = session
        Analytics.shared.userID = { [session] in session.userID }
        Analytics.shared.foregroundState = {
            let allowed = await PushRegistrar.shared.isAllowed()
            // Unreadable (WidgetKit's error carries nothing to act on): `app_opened` leaves the count out.
            let widgets = try? await WidgetCenter.shared.currentConfigurations().count
            return (allowed.map { $0 ? .allowed : .denied } ?? .notAsked, widgets)
        }
        CrashReports.shared.start()
        contextID = storage.id
        (accounts, cloud) = Self.makeAccounts(storage, session: session, loadContext: !session.usesWorkspaceDirectory && !session.requiresLogin)
        appliedWorkspaceSession = workspaceSession
        if session.usesWorkspaceDirectory, let userID = session.userID {
            workspaceSelection = ForkWorkspaceSelection(userId: userID)
        }
        startAccountCloud()
        // The current account's open access requests end while its session still works.
        session.transition.attach { [weak self] in self?.accounts }
    }

    var storage: SharedStore.Context { accounts.storage }

    /// The computer the Usage tab, widgets and "open screen" links act on: the last active one.
    var currentStore: BotStore? {
        accounts.storage.lastComputerId.flatMap(accounts.store(for:)) ?? accounts.computers.first.flatMap { accounts.store(for: $0.id) }
    }

    func replaceContext(_ storage: SharedStore.Context, loadContext: Bool, erase: [SharedStore.Context] = []) {
        accountStartup?.cancel()
        accounts.retire()
        PushRegistrar.shared.deactivate()
        LiveActivities.shared.endAll()
        accounts.storage.bots = []
        SharedStore.activate(SharedStore.Context(accountID: nil, scope: "inactive"))
        for context in erase { context.erase() }
        storage.bots = []
        SharedStore.activate(storage)
        contextID = storage.id
        generation += 1
        tab = .bots
        showComputers = false
        showUsage = false
        marketplace = nil
        (accounts, cloud) = Self.makeAccounts(storage, session: account, loadContext: loadContext)
        startAccountCloud()
        BotsWidgetFeed.reset()
        WidgetCenter.shared.reloadAllTimelines()
    }

    /// Signing out forgets this account on this iPhone: its device keys, computers and caches (spec §3.2).
    func signOut() async {
        guard let userID = account.userID else { return }
        // Sent first so it still counts toward the account.
        Analytics.shared.capture(.signedOut)
        await account.signOut()
        guard account.userID != userID, !account.accounts.contains(where: { $0.id == userID }) else { return }
        // Retire its stores first: a retiring store saves its cache one last time.
        syncAccountContext()
        SharedStore.eraseWorkspaceScopes(userID: userID)
        SharedStore.Context(accountID: userID).erase()
    }

    /// Settings → Start over: signs out of every account and forgets every computer, key and cache
    /// on this iPhone, then shows the welcome again. The computers keep their bots and chats.
    func startOver() async {
        resetAccountIDs.formUnion(account.accounts.map(\.id))
        if let userID = account.userID { resetAccountIDs.insert(userID) }
        guard await account.signOutAll() else {
            accounts.lastError = account.errorMessage
            return
        }
        workspaceSelection = nil
        let storage = SharedStore.Context(accountID: nil)
        UserDefaults.standard.set(false, forKey: "onboardingCompleted")
        account.showSwitcher = false
        appliedWorkspaceSession = workspaceSession
        replaceContext(Self.initialStorage(for: account), loadContext: false, erase: [storage])
        for id in resetAccountIDs {
            SharedStore.Context(accountID: id).erase()
            SharedStore.eraseWorkspaceScopes(userID: id)
        }
        resetAccountIDs.removeAll()
    }

    /// A scanned, pasted or opened pairing link; one that can't be read is recorded before it's shown.
    func pairing(from link: String) throws -> Pairing {
        do { return try Pairing.parse(link) } catch { throw pairingFailed(error) }
    }

    /// `via`: `qr` scanned or pasted here, `link` opened from the Camera app.
    func pair(_ pairing: Pairing, via: AnalyticsEvent.PairVia) async throws -> Computer {
        // QR remains a development path only when account configuration is empty.
        guard !account.isConfigured else { throw CloudError(status: 0, code: "accountRequired") }
        do {
            let computer = try await accounts.pair(pairing, deviceName: UIDevice.current.name, platform: "ios")
            Analytics.shared.capture(.computerPaired(via: via, computer: computer.id))
            return computer
        } catch {
            throw pairingFailed(error)
        }
    }

    /// Records why pairing failed (nothing when it was called off) and hands the error on.
    private func pairingFailed(_ error: any Error) -> any Error {
        if let reason = AnalyticsEvent.PairingFailure(error) { Analytics.shared.capture(.pairingFailed(reason)) }
        return error
    }

    func open(_ url: URL) {
        if url.scheme == Bundle.main.bundleIdentifier {
            Task { await account.handle(url) }
            return
        }
        guard !account.requiresLogin, url.scheme == AppEnvironment.current.urlScheme else { return }
        guard !showsWorkspaceStatus else { return }
        switch url.host() {
        case "pair":
            // Scanned with the Camera app: pair right away.
            let accounts = accounts
            Task {
                do { _ = try await pair(try pairing(from: url.absoluteString), via: .link) } catch { accounts.lastError = error.localizedDescription }
            }
        case "bot":
            guard let ref = storage.reference(from: url), accounts.store(for: ref.computerId) != nil else { return }
            tab = .bots
            accounts.selection = ref
        case "plugins":
            accounts.selection = nil
            tab = .bots
            marketplace = currentStore?.computer.id
        case "usage":
            accounts.selection = nil
            tab = .state
            showUsage = true
        case "computers":
            accounts.selection = nil
            tab = .bots
            showComputers = true
        case "screen":
            currentStore?.screenRequest = ScreenRequest()
        default:
            break
        }
    }

    private static func makeAccounts(_ storage: SharedStore.Context, session: AccountSession, loadContext: Bool) -> (AccountStore, CloudClient?) {
        let identity = !loadContext || storage.accountID == nil ? nil : try? DeviceIdentity.load(context: storage)
        let cloud = loadContext ? session.cloudClient(for: storage.accountID, identity: identity) : nil
        let accounts = AccountStore(storage: storage, cloud: cloud, loadSavedComputers: loadContext,
                                    accountConfigured: session.isConfigured)
        accounts.onConnected = { store in
            PushRegistrar.shared.syncDevice(with: store)
            store.onUsageChanged = { _, _ in WidgetCenter.shared.reloadTimelines(ofKind: "CodyncUsage") }
        }
        accounts.onBotUpdated = { ref, bot in LiveActivities.shared.update(ref, bot: bot, storage: storage) }
        accounts.onRosterChanged = { roster in
            BotsWidgetFeed.update(roster, storage: storage)
            if roster.isEmpty {
                LiveActivities.shared.endAll()
                WidgetCenter.shared.reloadAllTimelines()
            }
        }
        accounts.onSent = { [weak accounts] ref, bot, progress in
            guard let store = accounts?.store(for: ref.computerId) else { return }
            LiveActivities.shared.sent(ref, bot: bot, progress: progress, store: store)
        }
        return (accounts, cloud)
    }

    private func startAccountCloud() {
        guard let cloud else { return }
        accountStartup = Task { [weak accounts] in
            do {
                try Task.checkCancellation()
                try await cloud.registerDevice(name: UIDevice.current.name, platform: "ios")
            } catch {
                guard !Task.isCancelled else { return }
                accounts?.lastError = "Não foi possível registrar este iPhone na nuvem. Confira a conexão e a configuração Clerk e abra o app novamente."
                return
            }
            guard !Task.isCancelled else { return }
            await accounts?.refreshCloud()
        }
    }

    /// Takes back this iPhone's account access to a computer (its grant), and forgets the computer here.
    /// Also the way out when an earlier install was approved but this one never checked the code.
    func revokeAccess(_ id: ComputerID) async {
        guard let cloud else { return }
        let accounts = accounts
        do {
            let identity = try DeviceIdentity.load(context: accounts.storage)
            if let device = try await cloud.devices().first(where: { $0.deviceKey == identity.publicKey }) {
                for grant in try await cloud.grants(id) where grant.deviceId == device.deviceId {
                    try await cloud.revokeGrant(id, grantId: grant.grantId)
                }
            }
            accounts.forget(id)
            await accounts.refreshCloud()
        } catch {
            accounts.lastError = error.localizedDescription
        }
    }

    /// Forgets a computer here. Approved through the account, its grant goes too, or it would come
    /// straight back as an account row; a failed revoke (offline) still forgets it, and its error says
    /// why the grant stayed.
    func removeComputer(_ id: ComputerID) async {
        let accounts = accounts
        if accounts.cloudComputers.first(where: { $0.computerId == id })?.isGranted == true { await revokeAccess(id) }
        // A revoke that went through forgot it already.
        if accounts.computers.contains(where: { $0.id == id }) { accounts.forget(id) }
    }
}

final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        let open = UNNotificationAction(identifier: "openConversation", title: "Open conversation", options: [.foreground])
        let review = UNNotificationAction(identifier: "reviewRequest", title: "Review request", options: [.foreground])
        UNUserNotificationCenter.current().setNotificationCategories([
            UNNotificationCategory(identifier: "done", actions: [open], intentIdentifiers: []),
            UNNotificationCategory(identifier: "needsInput", actions: [review], intentIdentifiers: []),
            UNNotificationCategory(identifier: "failed", actions: [open], intentIdentifiers: []),
        ])
        #if DEBUG
        PushRegistrar.shared.verifyDeliveryIfRequested()
        #endif
        return true
    }

    /// Portrait, except while the screen viewer is open.
    func application(_ application: UIApplication, supportedInterfaceOrientationsFor window: UIWindow?) -> UIInterfaceOrientationMask {
        OrientationLock.mask
    }

    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        Task { @MainActor in PushRegistrar.shared.didRegister(token: deviceToken) }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                          withCompletionHandler completionHandler: @escaping @Sendable () -> Void) {
        let info = response.notification.request.content.userInfo
        let ctx = info["ctx"] as? String
        let computerId = info["computerId"] as? String
        let botId = info["botId"] as? String
        let kind = AnalyticsEvent.NotificationKind(category: response.notification.request.content.categoryIdentifier)
        // UIKit saves application state from this completion. The async delegate
        // bridge can complete off the main thread even after MainActor.run.
        Task { @MainActor in
            defer { completionHandler() }
            if let kind { Analytics.shared.capture(.notificationOpened(kind)) }
            guard let ctx, let computerId, let botId else { return }
            // Only open it in the account it was sent to, on a computer that account still has.
            let accounts = AppStore.shared.accounts
            guard ctx == accounts.storage.id, accounts.store(for: computerId) != nil else { return }
            AppStore.shared.tab = .bots
            accounts.selection = BotReference(accountId: accounts.accountId, computerId: computerId, botId: botId)
        }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        // The chat already shows it live.
        []
    }
}

/// Background time for work that has to finish after the app leaves the screen (the analytics
/// batch); without it iOS suspends the app with the work half done.
@MainActor
private final class BackgroundTime {
    private var id = UIBackgroundTaskIdentifier.invalid

    static func run(_ name: String, _ work: @escaping @MainActor @Sendable () async -> Void) {
        let time = BackgroundTime()
        let task = Task { await work() }
        // Out of time: give the work up rather than have iOS end the app for overrunning.
        time.id = UIApplication.shared.beginBackgroundTask(withName: name) { task.cancel(); time.end() }
        Task { await task.value; time.end() }
    }

    private func end() {
        guard id != .invalid else { return }
        UIApplication.shared.endBackgroundTask(id)
        id = .invalid
    }
}

/// Keeps the Bots widget's snapshot current; asks WidgetKit to redraw only when
/// something the widget shows changes (who is working / needs you / unread).
@MainActor
enum BotsWidgetFeed {
    private struct Snapshot: Equatable {
        let computerId: ComputerID
        let id: String
        let name: String
        let shape: String
        let color: String
        let avatar: BotAvatar
        let avatarState: MexeriquinhaState
        let status: String
        let activity: String
        let message: String?
        let lastAt: Int64

        init(_ bot: Bot, computerId: ComputerID) {
            self.computerId = computerId
            id = bot.id; name = bot.name; shape = bot.avatarShape; color = bot.avatarColor
            avatar = bot.avatar; avatarState = bot.avatarState()
            status = bot.status; activity = bot.activity; message = bot.lastMessage; lastAt = bot.lastAt
        }
    }
    private static var shown: [Snapshot]?
    private static var scope: String?

    static func reset() { shown = nil; scope = nil }

    static func update(_ roster: [RosterItem], storage: SharedStore.Context) {
        guard storage.id == SharedStore.activeContext.id else { return }
        storage.bots = roster.prefix(BotSnapshot.maxBots).map { BotSnapshot(computerId: $0.ref.computerId, bot: $0.bot) }
        let currentScope = storage.id
        let signature = roster.filter { !$0.bot.hidden }.map { Snapshot($0.bot, computerId: $0.ref.computerId) }
        guard signature != shown || scope != currentScope else { return }
        shown = signature
        scope = currentScope
        WidgetCenter.shared.reloadTimelines(ofKind: "CodyncBots")
        WidgetCenter.shared.reloadTimelines(ofKind: "CodyncTeam")
        ControlCenter.shared.reloadControls(ofKind: "CodyncLeadBot")
    }
}
