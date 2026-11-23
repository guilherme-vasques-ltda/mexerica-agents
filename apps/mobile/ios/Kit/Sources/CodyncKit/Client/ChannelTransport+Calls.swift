// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

// MARK: calls and streams

extension ChannelTransport {
    public nonisolated func call(_ method: String, body: Data, timeout: TimeInterval) async throws -> Data {
        try await perform(method, body: body, timeout: timeout)
    }

    public nonisolated func call(_ method: String, body: Data, timeout: TimeInterval, onLink link: Int) async throws -> Data {
        try await perform(method, body: body, timeout: timeout, onLink: link)
    }

    public func currentLink() async -> Int? { readyLink() }

    /// The encrypted channel that is ready now, if any: a new one after every handshake, even on the
    /// same relay socket.
    func readyLink() -> Int? {
        guard !closed, case .ready = state, let link, link.sealer != nil else { return nil }
        return link.channel
    }

    private func nextId() -> UInt32 {
        nextRequestId = nextRequestId == .max ? 1 : nextRequestId + 1
        return nextRequestId
    }

    /// Waits (bounded) while connecting; throws what the link state means otherwise.
    func ready(within limit: TimeInterval) async throws {
        let deadline = ContinuousClock.now + .seconds(max(limit, 0))
        while true {
            if closed { throw HostError.unreachable }
            switch state {
            case .ready where link?.sealer != nil: return
            case .ready, .connecting, .workspaceInvalidated:
                guard ContinuousClock.now < deadline else { throw HostError.unreachable }
                await stateChange(until: deadline)
            case let .hostOffline(lastSeen): throw HostError.computerOffline(lastSeen: lastSeen)
            case let .unauthorized(message): throw HostError.unauthorized(message)
            case .failed: throw needsUpgrade ? HostError.upgradeRequired : HostError.unreachable
            }
        }
    }

    /// With `onLink`, never waits: the call goes out on that channel or fails at once.
    func perform(_ method: String, body: Data, timeout: TimeInterval, onLink expected: Int? = nil, workspaceProbe: Int? = nil) async throws -> Data {
        if let workspaceProbe {
            guard link?.channel == workspaceProbe, link?.sealer != nil else { throw HostError.unreachable }
        } else if let expected {
            guard readyLink() == expected else { throw HostError.unreachable }
        } else {
            let generation = channelGeneration
            try await ready(within: timeout)
            guard generation == 0 || generation == channelGeneration else { throw HostError.unreachable }
        }
        var b: Any = body.isEmpty ? NSNull() : try JSONSerialization.jsonObject(with: body, options: .fragmentsAllowed)
        if workspaceProbe == nil, let workspace {
            guard b is NSNull || b is [String: Any] else { throw HostError.unreachable }
            var stamped = b as? [String: Any] ?? [:]
            stamped["workspaceEpoch"] = workspace.workspaceEpoch
            b = stamped
        }
        let id = nextId()
        let payload = try JSONSerialization.data(withJSONObject: ["id": id, "m": method, "b": b] as [String: Any])
        let channel = link?.channel
        let result: Data = try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (c: CheckedContinuation<Data, Error>) in
                calls[id] = c
                do {
                    // Checked again right where it is sealed: nothing runs between this and the send.
                    if let expected, readyLink() != expected { throw HostError.unreachable }
                    try sendInner(payload)
                } catch {
                    calls.removeValue(forKey: id)?.resume(throwing: error)
                    return
                }
                Task {
                    try? await Task.sleep(for: .seconds(timeout))
                    self.abandon(id, error: HostError.unreachable)
                }
            }
        } onCancel: {
            Task { await self.abandon(id, error: CancellationError()) }
        }
        guard link?.channel == channel, link?.sealer != nil else { throw HostError.unreachable }
        return result
    }

    /// Timed out or cancelled: the host drops the answer (an already started mutation still runs).
    private func abandon(_ id: UInt32, error: Error) {
        guard let c = calls.removeValue(forKey: id) else { return }
        c.resume(throwing: error)
        try? sendInner(JSONSerialization.data(withJSONObject: ["id": id, "cancel": true]))
    }

    private func sendInner(_ message: Data) throws {
        guard var sealer = link?.sealer, let link else { throw HostError.unreachable }
        let frames = try sealer.seal(message)
        self.link?.sealer = sealer
        for frame in frames {
            link.outbox.yield(#"{"t":"f","c":\#(frame.c),"d":"\#(frame.d.base64URL)"}"#)
        }
        // Counters must never wrap: replace the channel (§3.5).
        if sealer.exhausted {
            restartRequested = true
            link.socket.close(code: 4011)
        }
    }

    public nonisolated func stream(_ request: HostStreamRequest) -> AsyncThrowingStream<Data, Error> {
        stream(request, onLink: nil)
    }

    public nonisolated func stream(_ request: HostStreamRequest, onLink expected: Int?) -> AsyncThrowingStream<Data, Error> {
        AsyncThrowingStream { continuation in
            let key = UUID()
            let task = Task { await self.openStream(request, onLink: expected, key: key, continuation) }
            continuation.onTermination = { _ in
                task.cancel()
                Task { await self.closeStream(key) }
            }
        }
    }

    private func openStream(_ request: HostStreamRequest, onLink expected: Int?, key: UUID, _ continuation: AsyncThrowingStream<Data, Error>.Continuation) async {
        do {
            if let expected {
                guard readyLink() == expected else { throw HostError.unreachable }
            } else {
                let generation = channelGeneration
                try await ready(within: 10)
                guard generation == 0 || generation == channelGeneration else { throw HostError.unreachable }
            }
            // The consumer left while we waited: `closeStream` already ran and found nothing to stop.
            try Task.checkCancellation()
            let id = nextId()
            let kind: String
            var body: [String: Any]
            switch request {
            case let .events(since, client):
                kind = "events"
                body = ["since": since, "client": client]
            case let .term(term):
                kind = "term"
                body = ["term": term]
            }
            if let workspace { body["workspaceEpoch"] = workspace.workspaceEpoch }
            let sub: [String: Any] = ["id": id, "sub": kind, "b": body]
            streams[id] = continuation
            streamIds[key] = id
            try sendInner(JSONSerialization.data(withJSONObject: sub))
        } catch {
            continuation.finish(throwing: error)
        }
    }

    /// The consumer went away: stop the host's stream.
    private func closeStream(_ key: UUID) {
        guard let id = streamIds.removeValue(forKey: key), streams.removeValue(forKey: id) != nil else { return }
        try? sendInner(JSONSerialization.data(withJSONObject: ["id": id, "cancel": true]))
    }
}
