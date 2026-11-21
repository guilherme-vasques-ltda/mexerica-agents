// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import ActivityKit
import CodyncKit
import CodyncUI
import SwiftUI

struct ActivityGalleryView: View {
    /// The presentations shown: the State tab splits Lock Screen from the Dynamic Island.
    var forms = BotActivityPreview.Form.allCases
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.openURL) private var openURL
    @AppStorage("liveActivitiesEnabled") private var enabled = true
    @State private var allowed = ActivityAuthorizationInfo().areActivitiesEnabled
    @State private var form: BotActivityPreview.Form?
    @State private var phase = BotActivityPresentation.Phase.working
    @State private var startedAt = Date.now - 154

    private var island: Bool { !forms.contains(.lockScreen) }

    private var state: BotActivityPresentation {
        let status: String = switch phase {
        case .sending: "sending"
        case .queued: "queued"
        case .working, .stale: "working"
        case .needsInput: "needsInput"
        case .completed: "idle"
        case .failed: "error"
        case .waiting: "unknown"
        }
        return .init(status: status, activity: phase == .needsInput ? String(localized: "Revise as alterações propostas.") : String(localized: "Executando a suíte de testes."),
                     isStale: phase == .stale)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                VStack(alignment: .leading, spacing: 10) {
                    // Recorded from the switch itself: both galleries share the setting and stay alive.
                    Toggle(island ? String(localized: "Mostrar na Dynamic Island") : String(localized: "Mostrar Atividades ao Vivo"), isOn: Binding(get: { enabled }, set: { on in
                        enabled = on
                        Analytics.shared.capture(.settingChanged(.liveActivities, on: on))
                    }))
                        .toggleStyle(.codync)
                        .font(.subheadline.weight(.medium))
                    Text(island
                         ? String(localized: "A Dynamic Island mostra a mesma Atividade ao Vivo da Tela Bloqueada, então esta opção desativa as duas.")
                         : String(localized: "Começa quando você envia uma tarefa deste iPhone. Acompanhe o progresso e abra a conversa quando seu bot precisar de você."))
                        .font(.footnote).foregroundStyle(Palette.secondary)
                    if !allowed {
                        Text(String(localized: "As Atividades ao Vivo estão desativadas nos Ajustes do iOS."))
                            .font(.footnote).foregroundStyle(Palette.secondary)
                        Button(String(localized: "Abrir Ajustes do iOS")) { openURL(URL(string: UIApplication.openSettingsURLString)!) }
                            .buttonStyle(.plain)
                            .font(.footnote.weight(.medium))
                            .foregroundStyle(Palette.accent)
                    }
                }
                .padding(16).background(Palette.surface, in: RoundedRectangle(cornerRadius: 20))

                VStack(alignment: .leading, spacing: 14) {
                    HStack {
                        Text(String(localized: "Prévia")).font(.subheadline.weight(.medium))
                        Spacer()
                        Text(String(localized: "Tarefa de exemplo")).font(.caption).foregroundStyle(Palette.secondary)
                    }
                    if forms.count > 1 {
                        ChoicePicker(selection: Binding(get: { form ?? forms[0] }, set: { form = $0 }), options: forms.map { ($0, formTitle($0)) })
                    }
                    ChoicePicker(selection: $phase, options: [
                        (.sending, String(localized: "Enviando")), (.queued, String(localized: "Aguardando computador")),
                        (.working, String(localized: "Trabalhando")), (.needsInput, String(localized: "Precisa de você")), (.completed, String(localized: "Concluído")),
                        (.failed, String(localized: "Erro")), (.stale, String(localized: "Atualização atrasada")),
                    ])
                    if let bot = Bot.widgetPreview.first {
                        BotActivityPreview(bot: bot, state: state, form: form ?? forms[0], startedAt: startedAt)
                            .frame(maxWidth: .infinity, minHeight: 90)
                            .padding(12)
                            .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: 28))
                    }
                }
                VStack(alignment: .leading, spacing: 14) {
                    ForEach(forms, id: \.self) { explanation($0) }
                }
                Text(String(localized: "O iOS escolhe a apresentação conforme seu aparelho e as outras tarefas ativas. Estas prévias não iniciam uma Atividade ao Vivo. As aprovações são feitas na Mexerica."))
                    .font(.footnote).foregroundStyle(Palette.secondary)
            }
            .padding(18).frame(maxWidth: 560).frame(maxWidth: .infinity)
        }
        .background(Palette.background)

        .tint(Palette.accent)
        .onChange(of: enabled) { _, value in if !value { LiveActivities.shared.endAll() } }
        .onChange(of: scenePhase) { _, value in
            if value == .active { allowed = ActivityAuthorizationInfo().areActivitiesEnabled }
        }
    }

    private func formTitle(_ form: BotActivityPreview.Form) -> String {
        switch form {
        case .lockScreen: String(localized: "Tela Bloqueada")
        case .compact: String(localized: "Compacta")
        case .minimal: String(localized: "Mínima")
        case .expanded: String(localized: "Expandida")
        }
    }

    private func explanation(_ form: BotActivityPreview.Form) -> some View {
        let detail = switch form {
        case .lockScreen: String(localized: "O bot com seu estado, a etapa atual na cor correspondente, o tempo decorrido e uma esfera de pensamento. Quando o bot precisar de você, Revisar abre o pedido.")
        case .compact: String(localized: "O bot e uma esfera de pensamento ao lado da câmera, contornados na cor do estado. A esfera vira um sinal de conclusão ou aviso ao fim da tarefa.")
        case .minimal: String(localized: "Somente a esfera quando o iOS mostra várias Atividades ao Vivo.")
        case .expanded: String(localized: "Toque e segure a Dynamic Island para ver a etapa atual e o tempo decorrido, ou Revisar quando o bot precisar de você. Toque para abrir a conversa.")
        }
        return VStack(alignment: .leading, spacing: 4) {
            Text(formTitle(form)).font(.subheadline.weight(.medium)).accessibilityAddTraits(.isHeader)
            Text(detail).font(.footnote).foregroundStyle(Palette.secondary)
        }
    }
}

struct LockWidgetGalleryView: View {
    @State private var kind = "bots"

    private func familyTitle(_ family: AccessoryWidgetCard.Family) -> String {
        switch family {
        case .circular: String(localized: "Círculo")
        case .rectangular: String(localized: "Retângulo")
        case .inline: String(localized: "Em linha")
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                ChoicePicker(selection: $kind, options: [("bots", "Bots"), ("usage", String(localized: "Limites de uso"))])
                Text(String(localized: "Dados de exemplo")).font(.caption).foregroundStyle(Palette.secondary)
                ForEach(AccessoryWidgetCard.Family.allCases, id: \.self) { family in
                    VStack(alignment: .leading, spacing: 10) {
                        Text(familyTitle(family)).font(.subheadline.weight(.medium))
                        AccessoryWidgetCard(kind: kind == "bots" ? .bots : .usage, family: family,
                                            bots: Bot.widgetPreview, usage: .widgetPreview)
                            .frame(width: family == .circular ? 64 : family == .rectangular ? 160 : nil,
                                   height: family == .inline ? 24 : 64)
                            .padding(16).foregroundStyle(.white)
                            .background(.black, in: RoundedRectangle(cornerRadius: 20))
                            .environment(\.colorScheme, .dark)
                    }
                }
                Text(String(localized: "Toque e segure a Tela Bloqueada → Personalizar → Tela Bloqueada. Toque na área de widgets e escolha Mexerica. Widgets em linha ficam na data acima do relógio."))
                    .font(.footnote).foregroundStyle(Palette.secondary)
                Text(String(localized: "Escolha Bots para ver o estado das tarefas ou Limites de uso para ver o maior limite informado. O iOS aplica a cor e o estilo da Tela Bloqueada."))
                    .font(.footnote).foregroundStyle(Palette.secondary)
            }
            .padding(18).frame(maxWidth: 560).frame(maxWidth: .infinity)
        }
        .background(Palette.background)
        .page(String(localized: "Widgets da Tela Bloqueada"), pushed: true)
    }
}
