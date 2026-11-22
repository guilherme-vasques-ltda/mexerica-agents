// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// Optional distribution action; Mexerica currently provides no download destination.
public struct AppUpdateAction: Sendable {
    let action: @MainActor @Sendable () -> Void

    public init(_ action: @escaping @MainActor @Sendable () -> Void) { self.action = action }

    @MainActor public func callAsFunction() { action() }
}

public extension EnvironmentValues {
    @Entry var appUpdate: AppUpdateAction?
}

/// Says which side has to update before this app and a computer can work together, and offers
/// the update where this app can start it. Shown instead of the composer and above the
/// computer's bots (docs/reference/compatibility.md).
public struct UpdateNeededCard: View {
    let store: BotStore
    let mismatch: VersionMismatch
    @Environment(\.appUpdate) private var appUpdate

    public init(store: BotStore, mismatch: VersionMismatch) {
        self.store = store
        self.mismatch = mismatch
    }

    public var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "arrow.down.circle")
                .font(.title3)
                .foregroundStyle(Palette.warning)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 6) {
                Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(Palette.text)
                Text(detail)
                    .font(.footnote)
                    .foregroundStyle(Palette.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if let action {
                    Button(action.title, action: action.run)
                        .buttonStyle(.primary)
                        .padding(.top, 4)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .accessibilityElement(children: .contain)
    }

    var title: String { UpdateNeededText.title(mismatch, host: store.hostName) }
    var detail: String {
        let text = UpdateNeededText.detail(mismatch, host: store.hostName, hostVersion: store.hostVersion?.version,
                                           // With a button here, instructions for that computer would only confuse.
                                           os: store.hello?.os, instructions: action == nil)
        return text
    }

    private var action: (title: String, run: () -> Void)? {
        switch mismatch {
        case .updateApp:
            guard let appUpdate else { return nil }
            return (String(localized: "Atualizar Mexerica", bundle: .module), { appUpdate() })
        case .updateHost:
            // The computer updates itself; the card says how.
            return nil
        }
    }
}

/// The words every client uses for a version mismatch (the desktop app and the terminal client
/// mirror them).
public enum UpdateNeededText {
    public static func title(_ mismatch: VersionMismatch, host: String) -> String {
        switch mismatch {
        case .updateApp: String(localized: "Atualize este app", bundle: .module)
        case .updateHost: String(localized: "Atualize a Mexerica em \(host)", bundle: .module)
        }
    }

    public static func detail(_ mismatch: VersionMismatch, host: String, hostVersion: String?, os: String?,
                              instructions: Bool = true) -> String {
        switch mismatch {
        case let .updateApp(minimum):
            if let hostVersion {
                return String(localized: "\(host) usa a Mexerica \(hostVersion) e precisa que este app seja \(minimum) ou mais recente. Suas conversas estão seguras no computador.", bundle: .module)
            }
            return String(localized: "\(host) precisa que este app seja \(minimum) ou mais recente. Suas conversas estão seguras no computador.", bundle: .module)
        case let .updateHost(version, minimum):
            if instructions {
                return String(localized: "\(host) usa a Mexerica \(version); este app precisa da versão \(minimum) ou mais recente. Atualize a Mexerica nesse computador.", bundle: .module)
            }
            return String(localized: "\(host) usa a Mexerica \(version); este app precisa da versão \(minimum) ou mais recente.", bundle: .module)
        }
    }
}

/// Update reminder state retained for a future Mexerica distribution channel.
/// No network lookup or store listing is configured.
@MainActor
@Observable
public final class AppUpdates {
    /// No published version is configured yet.
    public private(set) var store: String?
    private var dismissedApp: String? = UserDefaults.standard.string(forKey: "dismissedAppUpdate")

    public init() {}

    /// No store listing is configured for Mexerica yet; never query the upstream app.
    public func refresh() async {}

    /// A newer app release to remind about, unless dismissed.
    public var newerApp: String? {
        UpdateReminder.app(current: AppVersion.current, store: store, dismissed: dismissedApp)
    }

    public func dismissApp() {
        Motion.animate { dismissedApp = store }
        UserDefaults.standard.set(store, forKey: "dismissedAppUpdate")
    }
}

/// A quiet, dismissible note that a newer release is out (everything still works).
public struct UpdateReminderCard: View {
    let title: String
    let detail: String
    let actionTitle: String?
    let action: () -> Void
    let dismiss: () -> Void

    public init(title: String, detail: String, actionTitle: String? = nil,
                action: @escaping () -> Void = {}, dismiss: @escaping () -> Void) {
        self.title = title
        self.detail = detail
        self.actionTitle = actionTitle
        self.action = action
        self.dismiss = dismiss
    }

    public var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "arrow.down.circle.fill")
                .font(.title3.weight(.medium))
                .symbolRenderingMode(.hierarchical)
                .foregroundStyle(Palette.text)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 6) {
                Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(Palette.text)
                Text(detail)
                    .font(.footnote)
                    .foregroundStyle(Palette.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if let actionTitle {
                    Button(actionTitle, action: action)
                        .buttonStyle(.secondary)
                        .padding(.top, 4)
                }
            }
            Spacer(minLength: 0)
            IconButton(String(localized: "Dispensar", bundle: .module), systemImage: "xmark", action: dismiss)
                .padding(.top, -6)
                .padding(.trailing, -6)
        }
        .padding(14)
        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .accessibilityElement(children: .contain)
    }
}

/// The App Store has a newer app (iPhone; needs `AppUpdates` in the environment).
public struct AppUpdateReminder: View {
    @Environment(AppUpdates.self) private var appUpdates: AppUpdates?
    @Environment(\.appUpdate) private var appUpdate

    public init() {}

    public var body: some View {
        if let appUpdates, let version = appUpdates.newerApp {
            UpdateReminderCard(title: String(localized: "Mexerica \(version) está disponível", bundle: .module),
                               detail: String(localized: "Você tem a versão \(AppVersion.current).", bundle: .module),
                               actionTitle: appUpdate == nil ? nil : String(localized: "Atualizar app", bundle: .module),
                               action: { appUpdate?() },
                               dismiss: appUpdates.dismissApp)
                .padding(.vertical, 4)
                .transition(.opacity)
        }
    }
}
