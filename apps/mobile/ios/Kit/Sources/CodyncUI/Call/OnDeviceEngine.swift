// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import AVFoundation
import CodyncKit
import NaturalLanguage
import Observation
import os
import Speech

private let log = Logger(subsystem: "app.mexerica.ios", category: "Call")

/// A hands-free voice loop with one bot, all on the phone: speech is transcribed on device,
/// sent as an ordinary message, and the bot's final replies are read aloud. The host sees text only.
@MainActor
@Observable
final class OnDeviceEngine: VoiceEngine {
    private(set) var phase: VoicePhase = .starting {
        didSet { onActivityChanged?(!ended && phase.isActive) }
    }
    var provider: VoiceProvider? { nil }
    /// Keep the transport attached while audio is active, even without a visible UI update.
    @ObservationIgnored var onActivityChanged: ((Bool) -> Void)?
    /// What you're saying right now (live caption).
    private(set) var heard = ""
    /// The reply being read aloud.
    private(set) var said = ""
    /// Microphone loudness, 0…1, while listening.
    private(set) var level: Float = 0
    var muted = false {
        didSet {
            guard muted != oldValue, phase == .listening else { return }
            if muted { stopListening() } else { listen() }
        }
    }

    /// The link right now, read on the microphone's audio thread.
    private let marcar: @Sendable () -> MarcaDoLink
    /// The text and the link its audio was captured on.
    private let send: (String, MarcaDoLink) -> Void
    /// The link of this listening session; a buffer captured on another one restarts the session.
    private var marcaDaEscuta = MarcaDoLink.nenhuma
    /// An utterance crossed a link change: nothing is recognized until it ends (a pause); only an
    /// utterance that begins after that counts.
    private var descartando = false
    private var fimDoDescarte: Task<Void, Never>?
    /// Louder than this, someone is speaking (the meter's 0…1 scale).
    private static let limiarDeFala: Float = 0.3
    /// Whether someone is mid-utterance in this session, kept through short pauses.
    private var deteccao = FalaEmCurso(limiar: 0.3, pausa: 1.5)
    /// The language picked in the call settings, else the phone's.
    private let recognizer = UserDefaults.standard.string(forKey: OnDeviceEngine.languageKey)
        .flatMap { $0.isEmpty ? nil : SFSpeechRecognizer(locale: Locale(identifier: $0)) } ?? SFSpeechRecognizer()
    private let engine = AVAudioEngine()
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    /// Bumped per utterance so late callbacks from a cancelled recognition are dropped.
    private var generation = 0
    private var silence: Task<Void, Never>?
    private let synthesizer = AVSpeechSynthesizer()
    private var speechDone: SpeechDone?
    private var ended = false

    /// Call settings (the gear), kept across calls.
    static let pauseKey = "callPause"
    static let rateKey = "callRate"
    /// On-device recognition language (a locale identifier); empty follows the system.
    static let languageKey = "callLanguage"

    /// Settings options: the system's language, then every language the recognizer supports, by name.
    static var languageOptions: [(String, String)] {
        let named = SFSpeechRecognizer.supportedLocales().map { ($0.identifier, Locale.current.localizedString(forIdentifier: $0.identifier) ?? $0.identifier) }
        return [("", String(localized: "Sistema", bundle: .module))] + named.sorted { $0.1.localizedStandardCompare($1.1) == .orderedAscending }
    }

    /// A pause this long ends what you're saying and sends it.
    private var endOfUtterance: Duration {
        let seconds = UserDefaults.standard.double(forKey: Self.pauseKey)
        return .milliseconds(Int((seconds > 0 ? seconds : 1.5) * 1000))
    }

    init(marcar: @escaping @Sendable () -> MarcaDoLink, send: @escaping (String, MarcaDoLink) -> Void) {
        self.marcar = marcar
        self.send = send
        let done = SpeechDone { [weak self] in Task { @MainActor in self?.finishedSpeaking() } }
        synthesizer.delegate = done
        speechDone = done
    }

    func start() async {
        guard await AVAudioApplication.requestRecordPermission() else {
            return fail(String(localized: "Permita o acesso ao microfone para a Mexerica nos Ajustes para falar com seus bots.", bundle: .module))
        }
        guard await Self.speechAuthorized() else {
            return fail(String(localized: "Permita o Reconhecimento de Fala para a Mexerica nos Ajustes para falar com seus bots.", bundle: .module))
        }
        guard let recognizer, recognizer.isAvailable else {
            return fail(String(localized: "O reconhecimento de fala não está disponível para este idioma agora.", bundle: .module))
        }
        guard !ended else { return }
            do {
                let audio = AVAudioSession.sharedInstance()
                try audio.setCategory(.playAndRecord, mode: .voiceChat, options: [.defaultToSpeaker, .allowBluetoothHFP])
                try audio.setActive(true)
            } catch {
                return fail(String(localized: "Não foi possível iniciar o áudio: \(error.localizedDescription)", bundle: .module))
            }
        listen()
    }

    func end() {
        ended = true
        onActivityChanged?(false)
        stopListening()
        fimDoDescarte?.cancel()
        fimDoDescarte = nil
        synthesizer.stopSpeaking(at: .immediate)
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    /// Reads a reply aloud (the bot's final message), pausing the microphone meanwhile.
    func speak(reply markdown: String) {
        let text = SpokenText.from(markdown)
        guard !ended, !text.isEmpty, phase != .starting, !(phase.isFailed) else { return }
        stopListening()
        said = text
        phase = .speaking
        let utterance = AVSpeechUtterance(string: text)
        utterance.voice = Self.voice(for: text)
        let rate = UserDefaults.standard.float(forKey: Self.rateKey)
        utterance.rate = rate > 0 ? rate : AVSpeechUtteranceDefaultSpeechRate
        synthesizer.speak(utterance)
    }

    func announce(_ notice: String) { speak(reply: notice) }

    /// Cuts the reply short and goes back to listening.
    func interrupt() {
        guard phase == .speaking else { return }
        synthesizer.stopSpeaking(at: .immediate)
        listen()
    }

    // MARK: listening

    private func listen() {
        phase = .listening
        heard = ""
        guard !muted, !ended, let recognizer else { return }
        if descartando { return medirDescarte() }
        generation += 1
        let sessao = generation
        let marcar = marcar
        // Read right before the microphone delivers this session's first buffer.
        let inicio = marcar()
        marcaDaEscuta = inicio
        deteccao = FalaEmCurso(limiar: Self.limiarDeFala, pausa: endOfUtterance / .seconds(1))
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true
        if recognizer.supportsOnDeviceRecognition { request.requiresOnDeviceRecognition = true }
        self.request = request
        let input = engine.inputNode
        input.removeTap(onBus: 0)
        do {
            let format = try MicTap.format(of: input)
            input.installTap(onBus: 0, bufferSize: 1024, format: format, block: Self.tap(request) { [weak self] level in
                // Stamped as captured, on the audio thread.
                let marca = marcar()
                Task { @MainActor in
                    self?.ouviu(level, sessao: sessao)
                    if marca != inicio { self?.linkMudou(durante: sessao) }
                }
            })
            engine.prepare()
            try engine.start()
        } catch {
            log.error("audio engine: \(error.localizedDescription, privacy: .public)")
            return fail(String(localized: "Não foi possível usar o microfone: \(error.localizedDescription)", bundle: .module))
        }
        task = recognizer.recognitionTask(with: request, resultHandler: Self.results(self, generation: generation))
    }

    /// The link changed during this session: what was heard so far is dropped, unsent. In silence,
    /// listening starts over on the new link; mid-utterance, the rest of that utterance is dropped too
    /// and listening starts over only after it ends.
    private func linkMudou(durante sessao: Int) {
        guard sessao == generation, phase == .listening, !ended else { return }
        let falando = deteccao.atravessou(ouvido: heard)
        heard = ""
        stopListening()
        if falando { descartando = true }
        listen()
    }

    /// One buffer's loudness in this session: the meter, and whether an utterance is going on.
    private func ouviu(_ level: Float, sessao: Int) {
        guard sessao == generation else { return }
        self.level = level
        deteccao.ouviu(nivel: level, em: Date())
    }

    /// Only the microphone's loudness, no recognition, until the utterance being dropped ends.
    private func medirDescarte() {
        let input = engine.inputNode
        input.removeTap(onBus: 0)
        do {
            let format = try MicTap.format(of: input)
            input.installTap(onBus: 0, bufferSize: 1024, format: format, block: Self.medidor { [weak self] level in
                Task { @MainActor in self?.ouviuDuranteDescarte(level) }
            })
            engine.prepare()
            try engine.start()
        } catch {
            log.error("audio engine: \(error.localizedDescription, privacy: .public)")
            return fail(String(localized: "Não foi possível usar o microfone: \(error.localizedDescription)", bundle: .module))
        }
    }

    private func ouviuDuranteDescarte(_ level: Float) {
        guard descartando, !ended, !muted else { return }
        self.level = level
        if level > Self.limiarDeFala {
            fimDoDescarte?.cancel()
            fimDoDescarte = nil
            return
        }
        guard fimDoDescarte == nil else { return }
        let pausa = endOfUtterance
        fimDoDescarte = Task { [weak self] in
            try? await Task.sleep(for: pausa)
            guard !Task.isCancelled, let self, self.descartando else { return }
            self.fimDoDescarte = nil
            self.descartando = false
            if self.engine.isRunning { self.engine.stop() }
            self.engine.inputNode.removeTap(onBus: 0)
            self.listen()
        }
    }

    private func stopListening() {
        silence?.cancel()
        silence = nil
        generation += 1
        task?.cancel()
        task = nil
        request?.endAudio()
        request = nil
        if engine.isRunning { engine.stop() }
        engine.inputNode.removeTap(onBus: 0)
        level = 0
    }

    fileprivate func recognized(_ text: String, final: Bool, generation: Int) {
        guard generation == self.generation, phase == .listening else { return }
        heard = text
        silence?.cancel()
        if final { return submit() }
        silence = Task {
            try? await Task.sleep(for: endOfUtterance)
            guard !Task.isCancelled else { return }
            submit()
        }
    }

    private func submit() {
        let text = heard.trimmingCharacters(in: .whitespacesAndNewlines)
        let marca = marcaDaEscuta
        stopListening()
        if !text.isEmpty { send(text, marca) }
        // Keep listening while the bot works: more messages fold into its next turn.
        listen()
    }

    private func finishedSpeaking() {
        guard phase == .speaking else { return }
        listen()
    }

    private func fail(_ message: String) {
        stopListening()
        phase = .failed(message)
    }

    // MARK: audio-thread closures (built outside the main actor)

    private nonisolated static func tap(_ request: SFSpeechAudioBufferRecognitionRequest,
                                        meter: @escaping @Sendable (Float) -> Void) -> AVAudioNodeTapBlock {
        { buffer, _ in
            request.append(buffer)
            guard let samples = buffer.floatChannelData?[0], buffer.frameLength > 0 else { return }
            let n = Int(buffer.frameLength)
            var sum: Float = 0
            for i in 0..<n { sum += samples[i] * samples[i] }
            // -50 dB (quiet room) … 0 dB mapped to 0…1.
            let db = 10 * log10(max(sum / Float(n), 1e-10))
            meter(min(1, max(0, (db + 50) / 50)))
        }
    }

    /// The loudness tap alone, for while an utterance is being dropped.
    private nonisolated static func medidor(_ meter: @escaping @Sendable (Float) -> Void) -> AVAudioNodeTapBlock {
        { buffer, _ in
            guard let samples = buffer.floatChannelData?[0], buffer.frameLength > 0 else { return }
            let n = Int(buffer.frameLength)
            var sum: Float = 0
            for i in 0..<n { sum += samples[i] * samples[i] }
            let db = 10 * log10(max(sum / Float(n), 1e-10))
            meter(min(1, max(0, (db + 50) / 50)))
        }
    }

    private nonisolated static func results(_ session: OnDeviceEngine, generation: Int) -> @Sendable (SFSpeechRecognitionResult?, (any Error)?) -> Void {
        { result, _ in
            guard let result else { return }
            let text = result.bestTranscription.formattedString
            let final = result.isFinal
            Task { @MainActor in session.recognized(text, final: final, generation: generation) }
        }
    }

    private nonisolated static func speechAuthorized() async -> Bool {
        await withCheckedContinuation { done in
            SFSpeechRecognizer.requestAuthorization { done.resume(returning: $0 == .authorized) }
        }
    }

    // MARK: voice

    /// A voice in the reply's own language (replies may be English while the phone is in Chinese).
    private static func voice(for text: String) -> AVSpeechSynthesisVoice? {
        guard let language = NLLanguageRecognizer.dominantLanguage(for: text) else { return nil }
        let code = switch language {
        case .traditionalChinese: "zh-TW"
        case .simplifiedChinese: "zh-CN"
        default: language.rawValue
        }
        let voices = AVSpeechSynthesisVoice.speechVoices().filter { $0.language.hasPrefix(code) }
        // Prefer the phone's own region for the language, then the best quality installed.
        let preferred = voices.filter { $0.language == Locale.current.identifier(.bcp47) }
        return (preferred.isEmpty ? voices : preferred).max { $0.quality.rawValue < $1.quality.rawValue }
    }
}

/// Synthesizer delegate: a finished utterance hands back to listening. Cancelled ones don't.
private final class SpeechDone: NSObject, AVSpeechSynthesizerDelegate, Sendable {
    let onFinish: @Sendable () -> Void

    init(onFinish: @escaping @Sendable () -> Void) { self.onFinish = onFinish }

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        onFinish()
    }
}

/// Whether someone is mid-utterance, from the microphone's loudness alone: from the first buffer
/// louder than `limiar` until a complete pause (quiet for `pausa` seconds). A short pause, or a
/// transcription that hasn't arrived yet, doesn't end it.
struct FalaEmCurso {
    let limiar: Float
    let pausa: TimeInterval
    private(set) var falando = false
    private var silencioDesde: Date?

    init(limiar: Float, pausa: TimeInterval) {
        self.limiar = limiar
        self.pausa = pausa
    }

    mutating func ouviu(nivel: Float, em instante: Date) {
        if nivel > limiar {
            falando = true
            silencioDesde = nil
            return
        }
        guard falando else { return }
        if let desde = silencioDesde {
            if instante.timeIntervalSince(desde) >= pausa {
                falando = false
                silencioDesde = nil
            }
        } else {
            silencioDesde = instante
        }
    }

    /// When the link changes: whether an utterance crosses it (something recognized, or speech not
    /// yet ended by a complete pause), so all of it must be dropped.
    func atravessou(ouvido: String) -> Bool {
        falando || !ouvido.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}
