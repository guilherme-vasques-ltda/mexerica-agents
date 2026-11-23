// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation
import os

private let log = Logger(subsystem: "app.mexerica.ios", category: "Channel")

// MARK: incoming

extension ChannelTransport {
    struct Wire: Decodable {
        var t: String
        var ek: String?
        var sig: String?
        var code: String?
        var stableCode: String?
        var c: UInt64?
        var d: String?
        var online: Bool?
        var lastSeenAt: Int64?
        var nonce: String?
        var state: String?
        var items: [QueuedItem]?
    }

    static func decodeWire(_ text: String) throws -> Wire {
        try JSONDecoder().decode(Wire.self, from: Data(text.utf8))
    }

    static func helloMessage(_ hs: RelayCrypto.Handshake, identity: DeviceIdentity, pair: Bool) throws -> String {
        struct Hello: Encodable { var t = "hello"; var v = 1; var dk: String; var ek: String; var n: String; var sig: String; var pair: Bool? }
        let hello = Hello(dk: identity.publicKey, ek: hs.ekD.base64URL, n: hs.n.base64URL,
                          sig: try identity.sign(hs.signInput).base64URL, pair: pair ? true : nil)
        return String(decoding: try JSONEncoder().encode(hello), as: UTF8.self)
    }

    /// Handles one socket message; returns an outcome when the socket must end.
    func handle(_ text: String, generation: Int) -> Outcome? {
        guard link?.generation == generation else { return nil }
        link?.lastReceived = .now
        guard let wire = try? Self.decodeWire(text) else { return nil }
        switch wire.t {
        case "presence":
            if wire.online == true {
                // The host (re)joined: any previous channel is gone; start a fresh handshake.
                dropChannel(HostError.unreachable)
                dropMailbox()
                return startHandshake(generation)
            }
            if workspaceVerificationEnded {
                return .stop(.failed(HostError.unreachable.localizedDescription))
            }
            lastSeen = wire.lastSeenAt.map(Date.init(milliseconds:))
            dropChannel(HostError.computerOffline(lastSeen: lastSeen))
            dropMailbox()
            link?.handshake = nil
            link?.sealer = nil
            link?.opener = nil
            setState(.hostOffline(lastSeen: lastSeen))
        case "welcome":
            guard let hs = link?.handshake, let route = link?.route,
                  let ek = wire.ek.flatMap(Data.init(base64URL:)), let sig = wire.sig.flatMap(Data.init(base64URL:)) else {
                return .retry(HostError.unreachable.localizedDescription)
            }
            link?.handshake = nil
            do {
                install(try hs.finish(ekH: ek, sig: sig), route: route)
            } catch {
                return .stop(.unauthorized(Self.identityChanged))
            }
        case "reject":
            if pairingCode == nil, Self.isPlaintextWorkspaceReject(wire) {
                return recheckPlaintextRejection()
            }
            if wire.code == "unsupportedVersion" { return upgrade() }
            return Self.rejection(wire) ?? .retry("Este computador recusou a conexão.")
        case "f":
            guard var opener = link?.opener, let c = wire.c, let d = wire.d.flatMap(Data.init(base64URL:)) else {
                link?.socket.close(code: 4002)
                return .retry(String(localized: "Erro de protocolo", bundle: .module))
            }
            do {
                let message = try opener.open(c: c, d: d)
                link?.opener = opener
                if let message { handleInner(message) }
            } catch RelayCryptoError.tooLarge {
                link?.socket.close(code: 4013)
                return .retry(String(localized: "Mensagem grande demais", bundle: .module))
            } catch {
                link?.socket.close(code: 4002)
                return .retry(String(localized: "Erro de protocolo", bundle: .module))
            }
        case "mbox.ok":
            if let nonce = wire.nonce { puts.removeValue(forKey: nonce)?.resume() }
        case "mbox.err":
            if let nonce = wire.nonce {
                puts.removeValue(forKey: nonce)?.resume(throwing: wire.code == "hostOnline" ? MailboxError.hostOnline : MailboxError.rejected(wire.code ?? "invalid"))
            }
        case "mbox.cancelled":
            if let nonce = wire.nonce { cancels.removeValue(forKey: nonce)?.resume(returning: .cancelled) }
        case "mbox.gone":
            if let nonce = wire.nonce { cancels.removeValue(forKey: nonce)?.resume(returning: wire.state == "delivering" ? .delivering : .unknown) }
        case "mbox.items":
            if let key = lists.keys.first, let continuation = lists.removeValue(forKey: key) {
                if let items = wire.items { continuation.resume(returning: items) }
                else { continuation.resume(throwing: HostError.unreachable) }
            }
        case "mbox.delivered":
            if let nonce = wire.nonce { broadcast(.delivered(nonce: nonce)) }
        case "mbox.failed":
            if let nonce = wire.nonce { broadcast(.failed(nonce: nonce, code: wire.code ?? "invalid")) }
        case "mbox.expired":
            if let nonce = wire.nonce { broadcast(.expired(nonce: nonce)) }
        default:
            break
        }
        return nil
    }

    private func startHandshake(_ generation: Int) -> Outcome? {
        guard let link else { return nil }
        do {
            let hs = try RelayCrypto.Handshake(computer: computer, deviceKey: identity.deviceKey)
            self.link?.handshake = hs
            self.link?.sealer = nil
            self.link?.opener = nil
            if case .ready = state { setState(.connecting) }
            if case .hostOffline = state { setState(.connecting) }
            link.outbox.yield(try Self.helloMessage(hs, identity: identity, pair: pairingCode != nil))
        } catch {
            return .stop(.unauthorized(Self.identityChanged))
        }
        // No welcome within 10 s: give up on this socket.
        Task {
            try? await Task.sleep(for: .seconds(10))
            self.handshakeDeadline(generation)
        }
        return nil
    }

    private func handshakeDeadline(_ generation: Int) {
        guard let link, link.generation == generation, link.handshake != nil else { return }
        link.socket.close(code: 1000)
    }

    func install(_ keys: RelayCrypto.ChannelKeys, route: HostRoute) {
        channelGeneration += 1
        link?.channel = channelGeneration
        link?.sealer = RelayCrypto.FrameSealer(key: keys.d2h)
        link?.opener = RelayCrypto.FrameOpener(key: keys.h2d)
        refusals = 0
        if pairingCode == nil {
            let channel = channelGeneration
            Task { await self.prepareWorkspace(channel: channel, route: route) }
        } else {
            setState(.ready(route))
        }
    }

    private func broadcast(_ event: MailboxEvent) {
        for c in mailboxObservers.values { c.yield(event) }
    }

    /// Inner RPC (§6.5): `ok`/`err` answer calls, `ev`/`end`/`err` feed streams.
    private func handleInner(_ message: Data) {
        guard let obj = try? JSONSerialization.jsonObject(with: message) as? [String: Any],
              let rawId = obj["id"] as? NSNumber else { return }
        let id = rawId.uint32Value
        if let ev = obj["ev"] {
            guard streams[id] != nil else { return }
            if let event = ev as? [String: Any], event["type"] as? String == "hello", let workspace {
                guard let value = event["workspace"],
                      let data = try? JSONSerialization.data(withJSONObject: value, options: .fragmentsAllowed),
                      (try? JSONDecoder().decode(ForkPairedWorkspace.self, from: data)) == workspace else {
                    retireWorkspace(.workspaceEpochMismatch)
                    return
                }
            }
            if let data = try? JSONSerialization.data(withJSONObject: ev, options: .fragmentsAllowed) { streams[id]?.yield(data) }
        } else if obj["end"] as? Bool == true {
            streams.removeValue(forKey: id)?.finish()
            streamIds = streamIds.filter { $0.value != id }
        } else if let err = obj["err"] as? [String: Any] {
            let status = (err["status"] as? NSNumber)?.intValue ?? 500
            guard calls[id] != nil || streams[id] != nil else { return }
            let issue = ForkWorkspaceFailure.http(status: status, code: err["code"] as? String)?.issue
            if let issue, issue == .workspaceEpochMismatch || issue == .workspaceForbidden {
                retireWorkspace(issue)
                return
            }
            // A 400 keeps the Hub's text unseen, for routine calls to recognise; everything else is fixed by status.
            let error: Error
            if issue == .leaseExpired {
                error = ForkWorkspaceIssue.leaseExpired
            } else if status == 400 {
                error = HostError.refused(HubRefusal(err["message"] as? String))
            } else {
                error = HostError.http(status, Self.httpMessage(status))
            }
            if let call = calls.removeValue(forKey: id) {
                call.resume(throwing: error)
            } else if let stream = streams.removeValue(forKey: id) {
                stream.finish(throwing: error)
                streamIds = streamIds.filter { $0.value != id }
            }
        } else if obj.keys.contains("ok") {
            let data = (try? JSONSerialization.data(withJSONObject: obj["ok"] ?? NSNull(), options: .fragmentsAllowed)) ?? Data("null".utf8)
            calls.removeValue(forKey: id)?.resume(returning: data)
        }
    }

    static func httpMessage(_ status: Int) -> String {
        switch status {
        case 401: "Este iPhone não tem mais acesso a este computador."
        case 403: "O computador recusou este pedido."
        case 404: "O computador não encontrou o que foi pedido."
        case 429: "Muitos pedidos em pouco tempo. Tente de novo daqui a pouco."
        default: "Não foi possível concluir. Tente de novo."
        }
    }

    /// Ends everything riding on the channel (not the mailbox, which lives on the relay socket).
    func dropChannel(_ error: Error) {
        let c = calls
        let s = streams
        calls = [:]
        streams = [:]
        streamIds = [:]
        for call in c.values { call.resume(throwing: error) }
        // Route change or re-handshake: the store resubscribes from its own rev.
        for stream in s.values { stream.finish(throwing: error) }
    }

    func dropMailbox() {
        let p = puts, x = cancels, l = lists
        puts = [:]
        cancels = [:]
        lists = [:]
        for put in p.values { put.resume(throwing: HostError.unreachable) }
        for cancel in x.values { cancel.resume(returning: .unknown) }
        for list in l.values { list.resume(returning: []) }
    }

    // MARK: inner hello (§7.5 step 5)

    func refreshComputer(channel: Int) async {
        guard let data = try? await perform("hello", body: Data(), timeout: 20, onLink: channel),
              readyLink() == channel, let hello = try? JSONDecoder().decode(Hello.self, from: data) else { return }
        merge(hello)
    }

    func merge(_ hello: Hello) {
        // The welcome signature already proved the key; a hello naming another computer is ignored.
        guard hello.computerId == nil || hello.computerId == computer.id,
              hello.signKey == nil || hello.signKey == computer.signKey else {
            log.error("hello names another computer")
            return
        }
        var c = computer
        if !hello.name.isEmpty { c.name = hello.name }
        if let bk = hello.boxKey, Data(base64URL: bk)?.count == 32 { c.boxKey = bk }
        if let urls = hello.urls { c.urls = urls.filter(Pairing.isDirectURL) }
        // Account relay is configured locally; a loopback-only Hub's hello cannot clear it.
        if c.route != .relayOnly {
            c.cloud = hello.cloud.flatMap(URL.init(string:)).flatMap { Pairing.isCloudURL($0) ? $0 : nil }
        }
        if let device = hello.device { c.device = device }
        guard c != computer else { return }
        computer = c
        for o in computerObservers.values { o.yield(c) }
    }
}
