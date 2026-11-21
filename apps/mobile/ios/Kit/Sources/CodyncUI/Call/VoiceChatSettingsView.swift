// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import AVFoundation
import CodyncKit
import SwiftUI

/// Voice chat settings: which voice carries calls (Apple's on this device, or OpenAI / Gemini on the
/// user's own key kept by the computer), the mode, models and voice. Opened from a call's gear and
/// from the computer's row in settings; the computer is the `BotStore` in the environment.
public struct VoiceChatSettingsView: View {
    @AppStorage(VoiceSettings.engineKey) private var engine = "device"
    @AppStorage(OnDeviceEngine.rateKey) private var rate = Double(AVSpeechUtteranceDefaultSpeechRate)
    @AppStorage(OnDeviceEngine.languageKey) private var language = ""
    @State private var languages = OnDeviceEngine.languageOptions
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init() {}

    public var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Conversa por voz", bundle: .module))
            CardForm {
                CardSection(String(localized: "Voz", bundle: .module), footer: engineFooter) {
                    ValueRow(String(localized: "Mecanismo", bundle: .module)) {
                        ChoicePicker(selection: $engine.animation(Motion.reduced(Motion.layout, reduceMotion)),
                                     options: [("device", String(localized: "No aparelho", bundle: .module))] + VoiceProvider.allCases.map { ($0.rawValue, $0.name) })
                    }
                    if let provider = VoiceProvider(rawValue: engine) {
                        ModeRows(provider: provider).id(provider)
                    } else {
                        ValueRow(String(localized: "Idioma", bundle: .module), detail: String(localized: "O idioma que você fala nas chamadas.", bundle: .module)) {
                            ChoicePicker(selection: $language, options: languages)
                        }
                        ValueRow(String(localized: "Velocidade da leitura", bundle: .module)) {
                            ChoicePicker(selection: $rate, options: [
                                (0.42, String(localized: "Mais lenta", bundle: .module)), (Double(AVSpeechUtteranceDefaultSpeechRate), String(localized: "Normal", bundle: .module)), (0.56, String(localized: "Mais rápida", bundle: .module)),
                            ])
                        }
                        PauseRow()
                    }
                }
                if let provider = VoiceProvider(rawValue: engine) {
                    ProviderVoiceSettings(provider: provider).id(provider)
                }
            }
        }
        .background(Palette.background)
        .tracked(.screenViewed(.voiceSettings))
    }

    private var engineFooter: String {
        guard let provider = VoiceProvider(rawValue: engine) else {
            return String(localized: "Fala da Apple neste aparelho. Vale a partir da próxima chamada.", bundle: .module)
        }
        return String(localized: "Sua própria chave de \(provider.name), cobrada por \(provider.name). Vale a partir da próxima chamada.", bundle: .module)
    }
}

/// A provider's mode, and the pause that ends what you say when it's Speech.
private struct ModeRows: View {
    @AppStorage private var mode: String
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    init(provider: VoiceProvider) {
        _mode = AppStorage(wrappedValue: "realtime", VoiceSettings.modeKey(provider))
    }

    var body: some View {
        ValueRow(String(localized: "Modo", bundle: .module), detail: mode == "speech" ? String(localized: "Envia quando você faz uma pausa e lê as respostas em voz alta.", bundle: .module) : String(localized: "Uma conversa ao vivo. Fale mesmo durante as respostas.", bundle: .module)) {
            ChoicePicker(selection: $mode.animation(Motion.reduced(Motion.layout, reduceMotion)),
                         options: [("realtime", String(localized: "Tempo real", bundle: .module)), ("speech", String(localized: "Fala", bundle: .module))])
        }
        if mode == "speech" { PauseRow() }
    }
}

/// How long a silence ends what you say, before it's sent.
private struct PauseRow: View {
    @Environment(\.locale) private var locale
    @AppStorage(OnDeviceEngine.pauseKey) private var pause = 1.5

    var body: some View {
        ValueRow(String(localized: "Enviar após uma pausa de", bundle: .module), detail: String(localized: "Uma pausa maior dá tempo para pensar no meio da frase.", bundle: .module)) {
            ChoicePicker(selection: $pause, options: [1.0, 1.5, 2.5].map { (id: $0, label: "\($0.formatted(.number.precision(.fractionLength(0...1)).locale(locale))) s") })
        }
    }
}

/// One provider: mode, the key the computer keeps, models, voice and this month's minutes.
private struct ProviderVoiceSettings: View {
    let provider: VoiceProvider
    @Environment(BotStore.self) private var store
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @AppStorage private var mode: String
    /// nil while the computer hasn't answered yet.
    @State private var saved: Bool?
    @State private var replacing = false
    @State private var key = ""
    @State private var busy = false
    @State private var result: (ok: Bool, text: String)?
    @State private var voice = ""
    @State private var model = ""
    @State private var transcribeModel = ""
    @State private var speechModel = ""
    @State private var models = VoiceModels(models: [], transcribe: [], speech: [])

    init(provider: VoiceProvider) {
        self.provider = provider
        _mode = AppStorage(wrappedValue: "realtime", VoiceSettings.modeKey(provider))
    }

    private var speechMode: Bool { mode == "speech" }
    private var hasKey: Bool { saved == true }

    var body: some View {
        CardSection(provider.name, footer: hasKey ? nil : String(localized: "Adicione uma chave para escolher entre os modelos de \(provider.name).", bundle: .module)) {
            VStack(alignment: .leading, spacing: 8) {
                if hasKey && !replacing {
                    savedKeyRow
                } else if Self.canEditKey {
                    keyEntry
                } else if saved == false {
                    missingKeyRow
                }
                if let result {
                    Label(result.text, systemImage: result.ok ? "checkmark.circle.fill" : "exclamationmark.circle.fill")
                        .font(.footnote)
                        .foregroundStyle(result.ok ? Palette.secondary : Palette.danger)
                        .transition(.opacity)
                }
            }
            Group {
                if speechMode {
                    ValueRow(String(localized: "Fala para texto", bundle: .module)) { picker($transcribeModel, models.transcribe) }
                    ValueRow(String(localized: "Texto para fala", bundle: .module)) { picker($speechModel, models.speech) }
                } else {
                    ValueRow(String(localized: "Modelo", bundle: .module)) { picker($model, models.models) }
                }
                ValueRow(String(localized: "Voz", bundle: .module)) { ChoicePicker(selection: $voice, options: provider.voices.map { ($0, $0) }) }
                ValueRow(String(localized: "Neste mês", bundle: .module), value: "≈ \(VoiceUsage.minutes(for: provider).formatted()) min")
            }
            .disabled(!hasKey)
            .opacity(hasKey ? 1 : 0.55)
        }
        .animation(Motion.reduced(Motion.layout, reduceMotion), value: replacing)
        .animation(Motion.reduced(Motion.fade, reduceMotion), value: result?.text)

        Color.clear.frame(height: 0)
            .task { await load() }
            .onChange(of: voice) { _, v in UserDefaults.standard.set(v, forKey: VoiceSettings.voiceKey(provider)) }
            .onChange(of: model) { _, m in VoiceSettings.save(m, default: VoiceSettings.defaults(provider).realtime, forKey: VoiceSettings.modelKey(provider)) }
            .onChange(of: transcribeModel) { _, m in
                VoiceSettings.save(m, default: VoiceSettings.defaults(provider).transcribe, forKey: VoiceSettings.transcribeKey(provider))
            }
            .onChange(of: speechModel) { _, m in
                VoiceSettings.save(m, default: VoiceSettings.defaults(provider).speech, forKey: VoiceSettings.speechKey(provider))
            }
    }

    /// The key is on the computer: say so, and offer test / replace / remove.
    private var savedKeyRow: some View {
        HStack(spacing: 10) {
            Image(systemName: "key.fill")
                .foregroundStyle(Palette.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                Text(String(localized: "Chave salva", bundle: .module)).foregroundStyle(Palette.text).lineLimit(1)
                Text(String(localized: "Fica em \(store.hostName)", bundle: .module)).font(.caption).foregroundStyle(Palette.tertiary).lineLimit(1)
            }
            Spacer(minLength: 8)
            if busy {
                Spinner()
            } else {
                IconButton(String(localized: "Testar chave", bundle: .module), systemImage: "checkmark.seal", action: test)
                if Self.canEditKey {
                    IconButton(String(localized: "Substituir chave", bundle: .module), systemImage: "pencil") { replacing = true; result = nil }
                    IconButton(String(localized: "Remover chave", bundle: .module), systemImage: "trash", action: remove)
                }
            }
        }
    }

    /// Keys are entered on the computer itself (the host takes `setVoiceKey` from loopback only).
        private static let canEditKey = false

    /// On the phone: where to add the key.
    private var missingKeyRow: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "key")
                .foregroundStyle(Palette.tertiary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(String(localized: "Nenhuma chave de \(provider.name) em \(store.hostName)", bundle: .module)).foregroundStyle(Palette.text)
                Text(String(localized: "Adicione-a na Mexerica nesse computador: Computadores e dispositivos, depois Conversa por voz.", bundle: .module))
                    .font(.caption)
                    .foregroundStyle(Palette.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    /// Paste a key; the computer checks it with the provider before keeping it.
    private var keyEntry: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                SecureField(String(localized: "Chave de API", bundle: .module), text: $key, prompt: Text(provider.keyPrompt))
                    .plainTextInput()
                    .textFieldStyle(.plain)
                    .onSubmit(test)
                if busy {
                    Spinner()
                } else {
                    Button(String(localized: "Salvar", bundle: .module), action: test)
                        .buttonStyle(.primary)
                        .disabled(key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            HStack {
                WebLink(String(localized: "Obter uma chave de \(provider.name)", bundle: .module), url: provider.keyPage).font(.footnote)
                Spacer()
                if replacing {
                    Button(String(localized: "Cancelar", bundle: .module)) { replacing = false; key = ""; result = nil }
                        .buttonStyle(.plain)
                        .font(.footnote)
                        .foregroundStyle(Palette.secondary)
                }
            }
        }
    }

    private func picker(_ selection: Binding<String>, _ list: [String]) -> some View {
        // The current choice stays listed before the provider's list arrives.
        let options = list.contains(selection.wrappedValue) ? list : [selection.wrappedValue] + list
        return ChoicePicker(selection: selection, options: options.map { ($0, $0) })
    }

    private func load() async {
        voice = VoiceSettings.voice(provider)
        model = VoiceSettings.model(provider)
        transcribeModel = VoiceSettings.transcribeModel(provider)
        speechModel = VoiceSettings.speechModel(provider)
        if let data = UserDefaults.standard.data(forKey: VoiceSettings.modelsKey(provider)),
           let cached = try? JSONDecoder().decode(VoiceModels.self, from: data) {
            models = cached
        }
        guard let client = store.client else {
            saved = false
            result = (false, String(localized: "Sem conexão com \(store.hostName).", bundle: .module))
            return
        }
        do {
            let status = try await client.voiceStatus()
            VoiceSettings.remember(status)
            saved = status.configured(provider)
        } catch {
            saved = false
            result = (false, error.localizedDescription)
            return
        }
        if hasKey { await loadModels(client) }
    }

    private func loadModels(_ client: HostClient) async {
        guard let list = try? await client.voiceModels(provider) else { return }
        models = list
        // A newer default moves every choice the user hasn't made themselves.
        VoiceSettings.remember(list.defaults, for: provider)
        model = VoiceSettings.model(provider)
        transcribeModel = VoiceSettings.transcribeModel(provider)
        speechModel = VoiceSettings.speechModel(provider)
        UserDefaults.standard.set(try? JSONEncoder().encode(list), forKey: VoiceSettings.modelsKey(provider))
    }

    /// A pasted key is checked with the provider by the computer before it's kept. With a key
    /// saved, Test starts a call credential with the chosen model and voice.
    private func test() {
        let typed = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !typed.isEmpty || hasKey else { return }
        run { client in
            if typed.isEmpty || !replacing && hasKey {
                if speechMode {
                    _ = try await client.voiceSpeak(provider, model: speechModel, voice: voice, text: "OK")
                } else {
                    _ = try await client.voiceSession(provider, model: model, voice: voice)
                }
                result = (true, String(localized: "Chave funcionando", bundle: .module))
            } else {
                saved = try await client.setVoiceKey(typed, for: provider).configured(provider)
                key = ""
                replacing = false
                result = (true, String(localized: "A chave funciona e está salva em \(store.hostName)", bundle: .module))
            }
            await loadModels(client)
        }
    }

    private func remove() {
        run { client in
            saved = try await client.setVoiceKey("", for: provider).configured(provider)
            result = nil
        }
    }

    private func run(_ work: @escaping @MainActor (HostClient) async throws -> Void) {
        guard !busy else { return }
        guard let client = store.client else {
            result = (false, String(localized: "Sem conexão com \(store.hostName).", bundle: .module))
            return
        }
        busy = true
        result = nil
        Task {
            do {
                try await work(client)
            } catch {
                result = (false, error.localizedDescription)
            }
            busy = false
        }
    }
}
