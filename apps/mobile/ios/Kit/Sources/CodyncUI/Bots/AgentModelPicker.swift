// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// Models come from the selected computer's agent, never a bundled provider list.
/// Fast mode is a second card row, shown while the agent offers it or while it's on
/// (so it can be turned off).
struct AgentModelPicker: View {
    let backend: String
    @Binding var selection: String?
    /// The selected model's settings (effort, fast…); they belong to one model.
    @Binding var options: [String: String]
    @Binding var fastMode: Bool
    /// The saved bot: whether its own sessions offer Fast mode, and why the agent refused it.
    var bot: Bot?
    @Environment(BotStore.self) private var store
    @State private var catalog: AgentModels?
    @State private var catalogScope: String?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var loading = false
    @State private var failure: String?
    @State private var retry = 0

    private var requestKey: String {
        "\(store.computer.id)/\(backend)/\(store.connection == .online)/\(retry)"
    }

    /// The agent's own "Default (recommended)" entry is the same choice as leaving the model unset,
    /// so it folds into the single Default row instead of showing twice.
    private var agentDefault: AgentModels.Model? {
        catalog?.models.first { $0.name.lowercased().hasPrefix("default") }
    }

    private var modelChoices: [(String, String)] {
        let available = (catalog?.models ?? []).filter { $0.id != agentDefault?.id }
        let current = available.first { $0.id == catalog?.currentModelId }?.name
        var result = [("", agentDefault?.name ?? current.map { String(localized: "Padrão · \($0)", bundle: .module) } ?? String(localized: "Padrão", bundle: .module))]
        result += available.map { ($0.id, $0.name) }
        if let selection, !selection.isEmpty, selection != agentDefault?.id,
           !available.contains(where: { $0.id == selection }) {
            result.append((selection, selection))
        }
        return result
    }

    private var value: Binding<String> {
        Binding(
            get: { selection == agentDefault?.id ? "" : selection ?? "" },
            set: {
                let model = $0.isEmpty ? nil : $0
                guard model != selection else { return }
                selection = model
                options = [:]
            }
        )
    }

    /// Settings the selected model offers.
    private var modelOptions: [AgentModels.Option] {
        catalog?.models.first { $0.id == value.wrappedValue }?.options ?? []
    }

    private func optionValue(_ id: String) -> Binding<String> {
        Binding(get: { options[id] ?? "" }, set: { options[id] = $0.isEmpty ? nil : $0 })
    }

    private func choices(_ option: AgentModels.Option) -> [(String, String)] {
        let fallback = option.values.first { $0.id == option.default }?.name
        return [("", fallback.map { String(localized: "Padrão · \($0)", bundle: .module) } ?? String(localized: "Padrão", bundle: .module))] + option.values.map { ($0.id, $0.name) }
    }

    private var showsFastMode: Bool {
        (backend != "custom" && catalog?.fastMode == true) || fastMode
            || (bot?.backend == backend && bot?.fastModeShown == true)
    }

    var body: some View {
        modelRow
            .onChange(of: backend) { _, _ in
                selection = nil
                options = [:]
            }
            .task(id: requestKey) { await load() }
        if showsFastMode {
            SwitchRow(String(localized: "Modo rápido", bundle: .module), detail: bot?.fastModeRefusal ?? String(localized: "Respostas mais rápidas com maior consumo nos modelos que oferecem essa opção.", bundle: .module),
                      isOn: $fastMode)
                .transition(.opacity)
        }
    }

    private var modelRow: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(String(localized: "Modelo", bundle: .module))
                    .foregroundStyle(Palette.text)
                    .fixedSize()
                Spacer(minLength: 0)
                HStack(spacing: 4) {
                    if backend != "custom" { refreshControl }
                    if backend == "custom" || (!loading && catalog?.models.isEmpty != false) {
                        TextField(String(localized: "Padrão", bundle: .module), text: value)
                            .plainTextInput()
                            .multilineTextAlignment(.trailing)
                            .textFieldStyle(.plain)
                            .frame(minWidth: 0, maxWidth: 180)
                            .outlinedPill()
                    } else {
                        ChoicePicker(selection: value, options: modelChoices, fitsAvailableWidth: true)
                    }
                }
            }
            ForEach(modelOptions) { option in
                HStack(spacing: 8) {
                    Text(option.name)
                        .foregroundStyle(Palette.text)
                        .fixedSize()
                    Spacer(minLength: 0)
                    ChoicePicker(selection: optionValue(option.id), options: choices(option), fitsAvailableWidth: true)
                }
                .transition(.opacity)
            }
            if backend != "custom", !loading {
                if let failure {
                    Text(failure)
                        .font(.caption)
                        .foregroundStyle(Palette.warning)
                } else if catalog?.models.isEmpty == true {
                    Text(String(localized: "Este motor não informa uma lista de modelos. Mantenha Padrão ou digite o ID de um modelo.", bundle: .module))
                        .font(.caption)
                        .foregroundStyle(Palette.secondary)
                }
            }
            Text(String(localized: "Alterar o modelo inicia uma nova sessão do motor. O histórico da conversa é mantido.", bundle: .module))
                .font(.caption)
                .foregroundStyle(Palette.secondary)
        }
        .animation(Motion.reduced(Motion.fade, reduceMotion), value: modelOptions.map(\.id))
    }

    /// Loading and refresh share one fixed slot, so the row never grows for loading.
    private var refreshControl: some View {
        ZStack {
            IconButton(failure == nil ? String(localized: "Atualizar lista de modelos", bundle: .module) : String(localized: "Tentar carregar a lista de modelos novamente", bundle: .module), systemImage: "arrow.clockwise") {
                retry += 1
            }
            .opacity(loading ? 0 : 1)
            .disabled(loading)
            .accessibilityHidden(loading)
            if loading {
                Spinner(size: 12)
                    .accessibilityLabel(String(localized: "Carregando modelos disponíveis", bundle: .module))
                    .transition(.opacity)
            }
        }
        .frame(width: 36, height: 36)
        .animation(Motion.reduced(Motion.fade, reduceMotion), value: loading)
    }

    @MainActor private func load() async {
        let scope = "\(store.computer.id)/\(backend)"
        if catalogScope != scope {
            catalog = nil
            catalogScope = scope
        }
        failure = nil
        loading = false
        guard backend != "custom" else { return }
        // Only a link the Hub identified itself on: never the client of a dropped one.
        guard store.temAutoridade, let client = store.client else {
            failure = String(localized: "Conecte-se a este computador para carregar os modelos. Você ainda pode digitar o ID de um modelo.", bundle: .module)
            return
        }
        loading = true
        do {
            let response = try await client.agentModels(backend)
            try Task.checkCancellation()
            withAnimation(Motion.reduced(Motion.fade, reduceMotion)) { catalog = response }
        } catch {
            guard !Task.isCancelled else { return }
            failure = String(localized: "Não foi possível carregar modelos: \(error.localizedDescription)", bundle: .module)
        }
        loading = false
    }
}
