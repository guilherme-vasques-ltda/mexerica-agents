// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import AVFoundation
import CodyncKit
import Foundation
import Observation
import os
@preconcurrency import WebRTC

private let log = Logger(subsystem: "app.mexerica.ios", category: "Call")

/// OpenAI Realtime over WebRTC, phone ↔ OpenAI directly: the microphone is a WebRTC audio track,
/// the model's voice plays from the remote track, and events (tools, replies) go over `oai-events`.
/// The user's key stays on the computer, which mints the client secret; the host never sees audio.
@MainActor
@Observable
final class OpenAIRealtimeEngine: VoiceEngine {
    private(set) var phase: VoicePhase = .starting {
        didSet { onActivityChanged?(!ended && phase.isActive) }
    }
    private(set) var level: Float = 0
    var muted = false {
        didSet {
            mic?.isEnabled = !muted
            if muted { level = 0 }
        }
    }
    var provider: VoiceProvider? { .openAI }
    @ObservationIgnored var onActivityChanged: ((Bool) -> Void)?

    /// A short-lived credential for the provider, minted by the computer that holds the key.
    private let credential: @MainActor () async throws -> String
    private let op: CallOperator
    private var pc: RTCPeerConnection?
    private var events: RTCDataChannel?
    private var mic: RTCAudioTrack?
    private var delegate: RealtimeEvents?
    private var ended = false
    /// A response is being generated; new replies wait for it to finish.
    private var responding = false
    /// The computer's data waiting for the model; delivering it ends the turn's authority.
    private var pending: FilaParaOModelo
    private var updateCall: String?
    /// A forced turn gets one chance until new input or bot_updates arrives.
    private var rearmRequested = false
    private var needsResponse = false
    private var dropTimer: Task<Void, Never>?

    init(credential: @escaping @MainActor () async throws -> String, op: CallOperator) {
        self.credential = credential
        self.op = op
        pending = FilaParaOModelo(op: op)
    }

    func start() async {
        guard await AVAudioApplication.requestRecordPermission() else {
            return fail(String(localized: "Permita o acesso ao microfone para a Mexerica nos Ajustes para falar com seus bots.", bundle: .module))
        }
        do {
            let secret = try await credential()
            guard !ended else { return }
            try await connect(secret: secret)
        } catch {
            guard !ended else { return }
            fail(error.localizedDescription)
        }
    }

    private func connect(secret: String) async throws {
            let audio = RTCAudioSessionConfiguration.webRTC()
            audio.categoryOptions = [.defaultToSpeaker, .allowBluetoothHFP]
            RTCAudioSessionConfiguration.setWebRTC(audio)

        let config = RTCConfiguration()
        config.sdpSemantics = .unifiedPlan
        config.bundlePolicy = .maxBundle
        let delegate = RealtimeEvents(owner: self)
        guard let pc = Self.factory.peerConnection(with: config, constraints: Self.noConstraints, delegate: delegate) else {
            throw VoiceError( String(localized: "Não foi possível iniciar o áudio da chamada.", bundle: .module))
        }
        self.delegate = delegate
        self.pc = pc
        // The microphone starts delivering audio from here: what is said in this call carries this link.
        op.audioComecou()
        let source = Self.factory.audioSource(with: Self.noConstraints)
        let mic = Self.factory.audioTrack(with: source, trackId: "mic")
        mic.isEnabled = !muted
        pc.add(mic, streamIds: ["call"])
        self.mic = mic
        let events = pc.dataChannel(forLabel: "oai-events", configuration: RTCDataChannelConfiguration())
        events?.delegate = delegate
        self.events = events

        let offer = try await pc.offer(for: Self.noConstraints)
        try await pc.setLocalDescription(offer)
        var request = URLRequest(url: URL(string: "https://api.openai.com/v1/realtime/calls")!)
        request.httpMethod = "POST"
        request.setValue("Bearer \(secret)", forHTTPHeaderField: "Authorization")
        request.setValue("application/sdp", forHTTPHeaderField: "Content-Type")
        request.httpBody = Data(offer.sdp.utf8)
        request.timeoutInterval = 15
        let (data, response) = try await URLSession.shared.data(for: request)
        guard !ended else { return }
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else {
            let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            let message = (json?["error"] as? [String: Any])?["message"] as? String
            throw VoiceError( message ?? "OpenAI answered \(status).")
        }
        try await pc.setRemoteDescription(RTCSessionDescription(type: .answer, sdp: String(decoding: data, as: UTF8.self)))
        dropTimer = Task { [weak self] in
            try? await Task.sleep(for: .seconds(15))
            guard let self, !Task.isCancelled, self.phase == .starting else { return }
            self.fail(String(localized: "Não foi possível conectar à OpenAI.", bundle: .module))
        }
    }

    func end() {
        ended = true
        onActivityChanged?(false)
        dropTimer?.cancel()
        events?.close()
        pc?.close()
        events = nil
        pc = nil
        mic = nil
        delegate = nil
    }

    func interrupt() {
        guard phase == .speaking else { return }
        send(["type": "response.cancel"])
        send(["type": "output_audio_buffer.clear"])
        phase = .listening
    }

    func speak(reply: String) {
        guard !ended, !phase.isFailed else { return }
        rearmRequested = false
        pending.resposta(reply)
        flush()
    }

    func announce(_ notice: String) {
        guard !ended, !phase.isFailed else { return }
        rearmRequested = false
        pending.aviso(notice)
        flush()
    }

    // MARK: events

    fileprivate func opened() {
        guard !ended else { return }
        dropTimer?.cancel()
        let tools: [[String: Any]] = CallOperator.tools.map {
            ["type": "function", "name": $0.name, "description": $0.description, "parameters": $0.parameters]
        }
        send([
            "type": "session.update",
            "session": [
                "type": "realtime",
                "instructions": op.instructions,
                "tools": tools,
                "audio": ["input": ["turn_detection": ["type": "semantic_vad"]]],
            ] as [String: Any],
        ])
        phase = .listening
        responding = true
        rearmRequested = true
        send(["type": "response.create", "response": ["tool_choice": ["type": "function", "name": "bot_updates"]]])
    }

    func received(_ data: Data) {
        guard !ended, let msg = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return }
        switch msg["type"] as? String {
        case "input_audio_buffer.speech_started":
            op.falaDetectada()
            rearmRequested = false
            level = muted ? 0 : 0.7
        case "input_audio_buffer.speech_stopped": level = 0
        // GA confirms a created item with `conversation.item.added` and finalizes it with
        // `conversation.item.done`; the beta name is still accepted.
        case "conversation.item.added", "conversation.item.done", "conversation.item.created":
            if let item = (msg["item"] as? [String: Any])?["id"] as? String { op.confirmado(item) }
        case "output_audio_buffer.started": phase = .speaking
        case "output_audio_buffer.stopped", "output_audio_buffer.cleared": if phase == .speaking { phase = .listening }
        case "response.created": responding = true
        case "response.done":
            responding = false
            if (msg["response"] as? [String: Any])?["status"] as? String == "failed" {
                needsResponse = false
                return
            }
            flush()
        case "response.function_call_arguments.done":
            guard let id = msg["call_id"] as? String, let name = msg["name"] as? String else { return }
            if name == "bot_updates" {
                rearmRequested = false
                if let previous = updateCall, previous != id {
                    send(["type": "conversation.item.create", "item": ["type": "function_call_output", "call_id": previous, "output": #"{"messages":[]}"#]])
                }
                updateCall = id
                flush()
                return
            }
            let args = (msg["arguments"] as? String).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) } as? [String: Any]
            // Our own id for the result: speech the server reports before confirming it may predate it.
            let item = Self.novoItem()
            let output = op.call(name, arguments: args ?? [:], confirmacao: item)
            send(["type": "conversation.item.create", "item": ["id": item, "type": "function_call_output", "call_id": id, "output": output]])
            needsResponse = true
            flush()
        case "error":
            let error = msg["error"] as? [String: Any]
            log.error("realtime: \(error?["message"] as? String ?? "?", privacy: .public)")
        default: break
        }
    }

    fileprivate func iceChanged(_ state: RTCIceConnectionState) {
        guard !ended else { return }
        switch state {
        case .connected, .completed: dropTimer?.cancel()
        case .disconnected:
            dropTimer?.cancel()
            dropTimer = Task { [weak self] in
                try? await Task.sleep(for: .seconds(5))
                guard let self, !Task.isCancelled else { return }
                self.fail(String(localized: "A conexão com a OpenAI foi perdida.", bundle: .module))
            }
        case .failed: fail(String(localized: "A conexão com a OpenAI foi perdida.", bundle: .module))
        default: break
        }
    }

    /// Bot replies are results of a real outstanding tool call, never user speech.
    private func flush() {
        guard !responding, phase.isActive, events?.readyState == .open else { return }
        if let id = updateCall, !pending.isEmpty {
            let item = Self.novoItem()
            let data = (try? JSONSerialization.data(withJSONObject: ["messages": pending.entregar(confirmacao: item)])) ?? Data("{}".utf8)
            send(["type": "conversation.item.create", "item": [
                "id": item, "type": "function_call_output", "call_id": id, "output": String(decoding: data, as: UTF8.self),
            ] as [String: Any]])
            updateCall = nil
            needsResponse = true
        }
        if updateCall == nil, !pending.isEmpty {
            guard !rearmRequested else { return }
            rearmRequested = true
            needsResponse = false
            responding = true
            send(["type": "response.create", "response": ["tool_choice": ["type": "function", "name": "bot_updates"]]])
            return
        }
        guard needsResponse else { return }
        needsResponse = false
        responding = true
        send(["type": "response.create"])
    }

    private func send(_ event: [String: Any]) {
        guard let events, events.readyState == .open,
              let data = try? JSONSerialization.data(withJSONObject: event) else { return }
        events.sendData(RTCDataBuffer(data: data, isBinary: false))
    }

    private func fail(_ message: String) {
        end()
        phase = .failed(message)
    }

    private static let factory: RTCPeerConnectionFactory = {
        RTCInitializeSSL()
        return RTCPeerConnectionFactory()
    }()

    private static let noConstraints = RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: nil)

    /// A client-chosen item id (up to 32 characters), echoed back in the item's confirmation event.
    private static func novoItem() -> String {
        "mx_" + String(UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(29))
    }
}

/// WebRTC callbacks arrive on its signaling thread; this hops them to the main actor.
private final class RealtimeEvents: NSObject, RTCPeerConnectionDelegate, RTCDataChannelDelegate, Sendable {
    private let owner: WeakEngine

    @MainActor init(owner: OpenAIRealtimeEngine) { self.owner = WeakEngine(owner) }

    func peerConnection(_ peerConnection: RTCPeerConnection, didChange stateChanged: RTCSignalingState) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didAdd stream: RTCMediaStream) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didRemove stream: RTCMediaStream) {}
    func peerConnectionShouldNegotiate(_ peerConnection: RTCPeerConnection) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didChange newState: RTCIceGatheringState) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didGenerate candidate: RTCIceCandidate) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didRemove candidates: [RTCIceCandidate]) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didOpen dataChannel: RTCDataChannel) {}

    func peerConnection(_ peerConnection: RTCPeerConnection, didChange newState: RTCIceConnectionState) {
        let owner = owner
        Task { @MainActor in owner.value?.iceChanged(newState) }
    }

    func dataChannelDidChangeState(_ dataChannel: RTCDataChannel) {
        guard dataChannel.readyState == .open else { return }
        let owner = owner
        Task { @MainActor in owner.value?.opened() }
    }

    func dataChannel(_ dataChannel: RTCDataChannel, didReceiveMessageWith buffer: RTCDataBuffer) {
        let data = buffer.data
        let owner = owner
        Task { @MainActor in owner.value?.received(data) }
    }
}

private final class WeakEngine: Sendable {
    @MainActor weak var value: OpenAIRealtimeEngine?
    @MainActor init(_ value: OpenAIRealtimeEngine) { self.value = value }
}
