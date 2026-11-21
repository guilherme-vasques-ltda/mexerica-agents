// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import CodyncUI
import SwiftUI
import WidgetKit

/// The State tab's Widget page. The installed widgets and these previews share their rendering components.
struct WidgetGalleryView: View {
    @Environment(AccountStore.self) private var accounts
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var page: Page?
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.dynamicTypeSize) private var typeSize
    @State private var kind = "usage"
    @State private var providerID = "claude"
    @State private var size = "medium"
    @State private var hasWidget: Bool?
    @State private var widgetCheckFailed = false
    /// nil until asked, false once turned off in the Settings app. Not asked during onboarding: offered here.
    @State private var notificationsAllowed: Bool?
    @AppStorage(SharedStore.usageIconStyleKey, store: UserDefaults(suiteName: SharedStore.appGroup))
    private var usageIconStyle = UsageIconStyle.character.rawValue

    /// Sample data where there is some, else what the computer last reported.
    private var provider: UsageProvider? {
        (Usage.widgetPreview.providers + SharedStore.activeContext.usage.values.flatMap(\.providers)).first { $0.id == providerID }
    }
    private var providers: [(id: String, label: String)] {
        let reported = SharedStore.activeContext.usageProviders
        var names = Dictionary(uniqueKeysWithValues: reported.map { ($0.id, $0.name) })
        // Keep both built-in providers selectable even when the shared snapshot is empty/stale.
        for provider in Usage.widgetPreview.providers where names[provider.id] == nil {
            names[provider.id] = provider.name
        }
        return names.map { (id: $0.key, label: $0.value) }.sorted { $0.label < $1.label }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                HStack(spacing: 14) {
                    CharacterAvatar(shape: "hex", color: "gray", size: 42)
                        .frame(width: 58, height: 58)
                        .background(Palette.bubbleUser, in: RoundedRectangle(cornerRadius: 16))
                    VStack(alignment: .leading, spacing: 4) {
                        Text(String(localized: "Mexerica, de relance."))
                            .font(.title3.weight(.semibold)).tracking(-0.4)
                        Text(String(localized: "Seus bots e limites de uso em todos os lugares."))
                            .font(.footnote).foregroundStyle(Palette.secondary)
                    }
                }
                .padding(16)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Palette.surface, in: RoundedRectangle(cornerRadius: 22))

                VStack(alignment: .leading, spacing: 9) {
                    sectionLabel(String(localized: "Prepare tudo"))
                    VStack(spacing: 0) {
                        setupRow(String(localized: "Conectar um computador"), complete: !accounts.computers.isEmpty)
                        Rectangle().fill(Palette.border).frame(height: 0.5).padding(.leading, 42)
                        setupRow(String(localized: "Adicionar um widget da Mexerica"), complete: hasWidget == true,
                                 status: hasWidget == nil ? (widgetCheckFailed ? String(localized: "Não foi possível verificar") : String(localized: "Verificando")) : nil)
                        Rectangle().fill(Palette.border).frame(height: 0.5).padding(.leading, 42)
                        if SharedStore.remotePushEnabled {
                            Button(action: turnOnNotifications) {
                                setupRow(notificationsAllowed == false ? String(localized: "Ativar notificações nos Ajustes") : String(localized: "Ativar notificações"),
                                         complete: notificationsAllowed == true)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .disabled(notificationsAllowed == true)
                            .accessibilityHint(String(localized: "Saiba quando um bot terminar ou precisar de você."))
                        }
                    }
                    .background(Palette.surface, in: RoundedRectangle(cornerRadius: 18))
                    if widgetCheckFailed {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(String(localized: "Não foi possível verificar seus widgets. Você ainda pode adicionar um seguindo as instruções abaixo."))
                                .foregroundStyle(Palette.secondary)
                            Button(String(localized: "Verificar novamente")) { checkWidgets() }
                                .buttonStyle(.secondary)
                        }
                        .font(.caption)
                    } else {
                        Text(hasWidget == nil ? String(localized: "Verificando seus widgets…") : String(localized: "As etapas são marcadas automaticamente quando você as conclui."))
                            .font(.caption2).foregroundStyle(Palette.tertiary)
                    }
                }

                VStack(alignment: .leading, spacing: 14) {
                    HStack {
                        sectionLabel(String(localized: "Escolha seu widget"))
                        Spacer()
                        Text(String(localized: "Dados de exemplo")).font(.caption2).foregroundStyle(Palette.secondary)
                    }
                    if typeSize.isAccessibilitySize {
                        ChoicePicker(selection: $kind, options: Self.kinds)
                    } else {
                        SegmentedChoice(selection: $kind, options: Self.kinds)
                    }
                    if kind == "usage" {
                        if typeSize.isAccessibilitySize || providers.count > 3 {
                            ChoicePicker(selection: $providerID, options: providers)
                        } else {
                            SegmentedChoice(selection: $providerID, options: providers)
                        }
                    }
                    if kind != "team" {
                        ChoicePicker(selection: $size, options: [("small", String(localized: "Pequeno")), ("medium", String(localized: "Médio")), ("large", String(localized: "Grande"))])
                    }
                    VStack(spacing: 18) {
                        preview(wide: size != "small")
                            .frame(width: size == "small" || kind == "team" ? 158 : nil, height: size == "large" && kind != "team" ? 338 : 158)
                        previewDescription
                    }
                    .padding(14)
                    .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: 26))
                }

                VStack(alignment: .leading, spacing: 12) {
                    sectionLabel(String(localized: "Escolha a aparência"))
                    SegmentedChoice(selection: Binding(
                        get: { usageIconStyle },
                        set: { usageIconStyle = $0; WidgetCenter.shared.reloadTimelines(ofKind: "CodyncProviderUsage") }
                    ), options: [(UsageIconStyle.character.rawValue, String(localized: "Personagem")), (UsageIconStyle.original.rawValue, String(localized: "Ícone original"))])
                    Text(String(localized: "Aplica-se ao Uso no app e nos widgets."))
                        .font(.caption).foregroundStyle(Palette.secondary)
                }

                VStack(alignment: .leading, spacing: 12) {
                    sectionLabel(String(localized: "Acompanhe suas tarefas"))
                    Button { page = .lockScreen } label: {
                        Label(String(localized: "Widgets da Tela Bloqueada"), systemImage: "lock.rectangle").foregroundStyle(Palette.accent)
                    }
                    .buttonStyle(PressScale())
                }
                .font(.subheadline)
                .frame(maxWidth: .infinity, alignment: .leading)

                VStack(alignment: .leading, spacing: 12) {
                    sectionLabel(String(localized: "Adicionar um widget"))
                    WidgetSetupDemo()
                }
                Text(String(localized: "Os widgets mostram o relatório mais recente. Abra a Mexerica para atualizações ao vivo e aprovações."))
                    .font(.caption2).foregroundStyle(Palette.tertiary)
            }
            .padding(18)
            .frame(maxWidth: 560)
            .frame(maxWidth: .infinity)
        }
        .background(Palette.background)
        .hidesSystemNavigationBar()
        .navigationDestination(item: $page) { page in
            switch page {
            case .lockScreen: LockWidgetGalleryView()
            }
        }
        .task {
            checkWidgets()
            await checkNotifications()
        }
        .onChange(of: scenePhase) { _, phase in
            guard phase == .active else { return }
            checkWidgets()
            Task { await checkNotifications() }
        }
    }

    private enum Page: Hashable { case lockScreen }

    private static let kinds: [(id: String, label: String)] = [("usage", String(localized: "Uso por provedor")), ("bots", "Bots"), ("team", String(localized: "Organograma"))]

    private var widgetSizeTitle: String {
        switch kind == "team" ? "small" : size {
        case "small": String(localized: "Widget pequeno")
        case "large": String(localized: "Widget grande")
        default: String(localized: "Widget médio")
        }
    }

    private var previewAccessibilityLabel: String {
        switch kind == "team" ? "small" : size {
        case "small": String(localized: "Prévia do widget pequeno")
        case "large": String(localized: "Prévia do widget grande")
        default: String(localized: "Prévia do widget médio")
        }
    }

    private var previewDescription: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(widgetSizeTitle).font(.subheadline.weight(.medium))
            Text(kind == "usage" ? String(localized: "O pequeno mostra o limite mais próximo. O médio mostra duas janelas. O grande acrescenta um resumo e até quatro limites.")
                 : kind == "team" ? String(localized: "Quatro bots como personagens, com um ponto para cada um que estiver trabalhando. Toque e segure o widget e escolha Editar Widget para definir cada posição; posições vazias mostram primeiro os bots que precisam de você.")
                 : String(localized: "Veja quem precisa de você e quem ainda está trabalhando. O grande mostra até seis bots."))
                .font(.caption).foregroundStyle(Palette.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private func preview(wide: Bool) -> some View {
        Group {
            if kind == "usage", let provider {
                ProviderWidgetCard(provider: provider, layout: size == "large" ? .large : wide ? .medium : .small)
            } else if kind == "team" {
                BotsTeamCard(bots: Bot.widgetPreview)
            } else {
                BotsWidgetCard(bots: Bot.widgetPreview, wide: wide, large: size == "large")
            }
        }
        .padding(14)
        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 22))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(previewAccessibilityLabel)
    }

    private func sectionLabel(_ title: String) -> some View {
        Text(title).font(.caption.weight(.medium)).foregroundStyle(Palette.secondary)
            .accessibilityAddTraits(.isHeader)
    }

    private func setupRow(_ title: String, complete: Bool, status: String? = nil) -> some View {
        HStack(spacing: 10) {
            Image(systemName: complete ? "checkmark.circle.fill" : "circle")
                .foregroundStyle(complete ? Palette.added : Palette.tertiary)
                .font(.system(size: 15))
            Text(title).font(.footnote).foregroundStyle(complete ? Palette.secondary : Palette.text)
            Spacer()
        }
        .padding(14)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(title)
        .accessibilityValue(status ?? (complete ? String(localized: "Etapa concluída") : String(localized: "Não concluído")))
    }

    private func checkNotifications() async {
        let allowed = await PushRegistrar.shared.isAllowed()
        withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { notificationsAllowed = allowed }
    }

    /// Asks once; after a no, only the Settings app can turn them on.
    private func turnOnNotifications() {
        guard SharedStore.remotePushEnabled else { return }
        if notificationsAllowed == false {
            UIApplication.shared.open(URL(string: UIApplication.openNotificationSettingsURLString)!)
        } else {
            Task {
                _ = await PushRegistrar.shared.requestAuthorization(from: .widgets)
                await checkNotifications()
            }
        }
    }

    private func checkWidgets() {
        let animation = Motion.reduced(Motion.layout, reduceMotion)
        withAnimation(animation) { widgetCheckFailed = false }
        WidgetCenter.shared.getCurrentConfigurations { result in
            let installed = (try? result.get())?.contains { $0.kind.hasPrefix("Codync") }
            Task { @MainActor in
                withAnimation(animation) {
                    hasWidget = installed
                    widgetCheckFailed = installed == nil
                }
            }
        }
    }
}
