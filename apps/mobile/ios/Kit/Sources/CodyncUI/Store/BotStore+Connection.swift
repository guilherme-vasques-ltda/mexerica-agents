// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import Foundation

extension BotStore {
    /// Permanently detach a store (account switch, computer removed). Late async
    /// callbacks still hold it, but it writes nothing and calls no hooks anymore.
    public func retire() {
        guard !retired else { return }
        let calls = Array(voiceCalls.values)
        voiceCalls.removeAll()
        setActive(false)
        retired = true
        fileDownloads.retire()
        for call in calls { call.end() }
        saveTask?.cancel()
        dropTimer?.cancel()
        prazoTask?.cancel()
        prazoTask = nil
        // Let go at once (signed out, account switched, computer removed): no grace, no deadline.
        estadoConexao = .desconectado(.encerradoNoApp)
        faixaDeQueda = false
        onConnected = nil
        onBotUpdated = nil
        onUsageChanged = nil
        onSent = nil
        onRosterChanged = nil
        onComputerChanged = nil
        clienteBase = nil
        clienteAutorizado = nil
        selection = nil
        screenRequest = nil
        bots = [:]
        entries = [:]
        usage = Usage()
        screen = nil
        hello = nil
        installedConnectors = []
        installedSkills = []
        answering = [:]
        composerDrafts = [:]
        composerPasted = [:]
        outgoingFiles = [:]
        routineDrafts = [:]
    }

    public func setActive(_ active: Bool) {
        guard !retired else { return }
        isActive = active
        if active {
            // Returning to the foreground must not interrupt a live call's sends.
            let reabre = voiceCalls.isEmpty || streamTask == nil
            switch estadoConexao {
            case .conectado where reabre:
                // The link was closed on purpose in the background: the new one has no authority until
                // the Hub identifies itself on it.
                mudarEstado(.reconectando(desde: agora()))
            case .reconectando:
                // A drop that was already on keeps its start; the deadline is checked right away.
                verificarPrazo()
            default:
                break
            }
            if reabre { restartStream() }
        } else if voiceCalls.isEmpty {
            // Disconnect so the host knows we're gone and sends pushes instead.
            stopTransport()
            saveCache()
            prazoTask?.cancel()
            prazoTask = nil
        }
    }

    public func restartStream() {
        guard isActive || !voiceCalls.isEmpty, !retired else { return }
        stopTransport()
        if connection != .online { setConnection(.connecting) }
        if estadoConexao.estaReconectando, prazoTask == nil { armarPrazo() }
        streamTask = Task { [weak self] in await self?.runStream() }
    }

    /// "Try now": a fresh attempt instead of waiting out the reconnect backoff (up to 30 s).
    /// It doesn't move the five-minute deadline.
    public func tentarAgora() {
        guard podeTentarAgora else { return }
        // Limited here, not only on screen: at most one fresh attempt every `intervaloTentarAgora`.
        let instante = monotonico()
        if let ultima = ultimaTentativa, ultima.duration(to: instante) < Self.intervaloTentarAgora { return }
        ultimaTentativa = instante
        restartStream()
    }

    // MARK: short drop

    func mudarEstado(_ novo: EstadoConexao) {
        guard !retired, novo != estadoConexao else { return }
        let voltou = novo == .conectado
        let eraQueda = estadoConexao.estaReconectando
        Motion.animate {
            estadoConexao = novo
            if !novo.estaReconectando { faixaDeQueda = false }
        }
        if voltou { linkId += 1 }
        if novo.estaReconectando {
            if !eraQueda { inicioDaQueda = monotonico() }
            armarPrazo()
        } else {
            inicioDaQueda = nil
            prazoTask?.cancel()
            prazoTask = nil
        }
    }

    /// Wakes when the strip is due (`dropGrace`) and when the deadline is, reading `agora` each time.
    func armarPrazo() {
        prazoTask?.cancel()
        prazoTask = nil
        guard !retired, estadoConexao.estaReconectando else { return }
        let espera = esperar
        prazoTask = Task { [weak self] in
            while !Task.isCancelled {
                self?.verificarPrazo()
                guard let falta = self?.faltaParaOProximoPasso() else { return }
                do { try await espera(falta) } catch { return }
            }
        }
    }

    private func faltaParaOProximoPasso() -> Duration? {
        guard !retired, estadoConexao.estaReconectando, let inicio = inicioDaQueda else { return nil }
        let passou = inicio.duration(to: monotonico())
        let passo: Duration = faixaDeQueda ? .seconds(EstadoConexao.prazo) : Self.dropGrace
        // At least a second: a clock that didn't move never spins this loop.
        return max(passo - passou, .seconds(1))
    }

    /// Applies the monotonic clock: the strip after the grace, `.desconectado(.prazoLongo)` at the deadline.
    func verificarPrazo() {
        guard !retired, estadoConexao.estaReconectando, let inicio = inicioDaQueda else { return }
        let passou = inicio.duration(to: monotonico())
        let depois = estadoConexao.depoisDe(passou)
        if depois != estadoConexao {
            mudarEstado(depois)
        } else if !faixaDeQueda, passou >= Self.dropGrace {
            Motion.animate { faixaDeQueda = true }
        }
    }

    /// A definitive refusal from any call: let go at once and stop retrying until the person asks again.
    func soltarPorRecusa(_ motivo: String) {
        guard !retired else { return }
        stopTransport()
        setConnection(.unauthorized(motivo))
    }

    /// A 403 answering one action: authority is suspended at once, every time, and the Hub asked to identify
    /// itself again on the same channel. A 401/403 or another computer there ends access at once; a good
    /// answer gives the authority back and leaves only that action refused. At most one check is pending;
    /// within `intervaloReverificacao` of the last one it is delayed to the end of that window, not skipped.
    func reverificarAcesso() {
        guard !retired else { return }
        // A check already pending covers this 403 too; authority stays suspended until it answers.
        guard reverificacao == nil else {
            clienteAutorizado = nil
            return
        }
        guard let cliente = clienteAutorizado, let listener = refusalListener else { return }
        clienteAutorizado = nil
        let instante = monotonico()
        let atraso: Duration = ultimaReverificacao.map { max(Self.intervaloReverificacao - $0.duration(to: instante), .zero) } ?? .zero
        let espera = esperar
        reverificacao = Task { [weak self] in
            if atraso > .zero {
                do { try await espera(atraso) } catch { return }
            }
            guard let self, self.refusalListener == listener else { return }
            defer {
                if self.refusalListener == listener { self.reverificacao = nil }
            }
            guard await cliente.transport.currentLink() != nil, self.refusalListener == listener else { return }
            self.ultimaReverificacao = self.monotonico()
            let resposta: Result<Hello, Error>
            do {
                resposta = .success(try await cliente.hello())
            } catch {
                resposta = .failure(error)
            }
            guard self.refusalListener == listener, await cliente.transport.currentLink() != nil,
                  self.refusalListener == listener else { return }
            switch resposta {
            case let .success(h):
                if let id = h.computerId, id != self.computer.id {
                    self.soltarPorRecusa(HostError.identidadeMudou)
                } else if let key = h.signKey, key != self.computer.signKey {
                    self.soltarPorRecusa(HostError.identidadeMudou)
                } else {
                    self.clienteAutorizado = cliente
                }
            case let .failure(error):
                if let motivo = (error as? HostError)?.recusaDefinitiva(identificando: true) {
                    self.soltarPorRecusa(motivo)
                } else {
                    // Not identified (the link is failing): a fresh link identifies itself from scratch.
                    self.restartStream()
                }
            }
        }
    }

    /// Whether the next transport may tolerate early 403s; whoever asks consumes the mark.
    func consumirVinculoRecente() -> Bool {
        guard let gravado = vinculoGravadoEm else { return false }
        vinculoGravadoEm = nil
        return gravado.duration(to: monotonico()) < Self.prazoVinculoRecente
    }

    /// The link ended: nothing keeps the authority it had, and no action started on it may continue.
    private func encerrarAutoridade() {
        clienteAutorizado = nil
        refusalListener = nil
        linkId += 1
        // A pending check belonged to that link; the next one identifies itself from scratch.
        reverificacao?.cancel()
        reverificacao = nil
    }

    /// Audio owns this subscription, so background calls don't depend on SwiftUI updates.
    func beginVoiceCall(_ id: UUID, botId: String, speak: @escaping @MainActor (String) -> Void,
                        announce: (@MainActor (String) -> Void)? = nil, end: @escaping @MainActor () -> Void) {
        guard !retired, voiceCalls[id] == nil else { return }
        voiceCalls[id] = VoiceCall(botId: botId, startRev: rev, speak: speak, announce: announce ?? speak, end: end)
        if streamTask == nil { restartStream() }
    }

    func endVoiceCall(_ id: UUID) {
        guard voiceCalls.removeValue(forKey: id) != nil else { return }
        if !isActive && voiceCalls.isEmpty {
            stopTransport()
            saveCache()
        }
    }

    private func stopTransport() {
        streamTask?.cancel()
        streamTask = nil
        eventsTask?.cancel()
        eventsTask = nil
        dropTimer?.cancel()
        dropTimer = nil
        heldDrop = nil
        encerrarAutoridade()
        if let remote = transport as? any RemoteTransport {
            Task { await remote.shutdown() }
        }
        transport = nil
    }

    private func runStream() async {
        var backoff: Double = 1
        var transport: any HostTransport
        // Single use: only the first transport after a fresh link, within its deadline, is tolerant.
        var tolerante = consumirVinculoRecente()
        while true {
            do {
                let conectar = tolerante ? (conectarRecemVinculado ?? makeTransport) : makeTransport
                tolerante = false
                transport = try await conectar(computer)
                break
            } catch let error as HostError where error.recusaDefinitiva(identificando: true) != nil {
                if !Task.isCancelled, !retired { setConnection(.unauthorized(error.recusaDefinitiva(identificando: true) ?? HostError.semAcesso)) }
                return
            } catch {
                guard !Task.isCancelled, !retired else { return }
                setConnection(.offline(error.localizedDescription))
                try? await Task.sleep(for: .seconds(backoff))
                backoff = min(backoff * 2, 30)
                guard !Task.isCancelled else { return }
            }
        }
        guard !Task.isCancelled, !retired else {
            if let remote = transport as? any RemoteTransport { await remote.shutdown() }
            return
        }
        self.transport = transport
        let client = HostClient(transport: transport)
        clienteBase = client
        if let remote = transport as? any RemoteTransport { watch(remote) }
        for await state in transport.states() {
            guard !Task.isCancelled, !retired else { return }
            switch state {
            case let .ready(route):
                setHostRoute(route)
                if eventsTask == nil {
                    if connection != .online { setConnection(.connecting) }
                    eventsTask = Task { [weak self] in await self?.runEvents(client) }
                    // Mailbox outcomes that happened while this app wasn't listening (§10.2).
                    if let remote = transport as? any RemoteTransport {
                        Task { [weak self] in await self?.reconcileQueued(remote) }
                    }
                }
            case .connecting:
                stopEvents()
                setConnection(.connecting)
            case let .hostOffline(lastSeen):
                stopEvents()
                setConnection(.computerOffline(lastSeen: lastSeen))
                if let remote = transport as? any RemoteTransport {
                    Task { [weak self] in await self?.reconcileQueued(remote) }
                }
            case let .unauthorized(message):
                stopEvents()
                setConnection(.unauthorized(message))
            case let .failed(message):
                stopEvents()
                setConnection(.offline(message))
            }
        }
    }

    /// Connection changes animate wherever they show (banners, headers, captions, rows).
    /// Give initial connection failures a second to recover and online drops five seconds.
    /// Relay presence can lag a reconnect, so "computer offline" gets the same grace.
    /// Authorization failures still show immediately.
    func setConnection(_ new: Connection) {
        // The screen's state follows at once; only the strip waits for the grace below.
        mudarEstado(EstadoConexao.proximo(estadoConexao, nova: new, agora: agora()))
        let transient = switch new {
        case .connecting, .offline, .computerOffline: true
        default: false
        }
        let recovering = new == .online && (connection != .online || heldDrop != nil)
        let initialFailure = connection == .connecting && transient && new != .connecting
        if transient && (connection == .online || initialFailure) {
            let grace = connection == .online ? Self.dropGrace : Self.initialConnectionGrace
            heldDrop = new
            if dropTimer == nil {
                dropTimer = Task { [weak self] in
                    try? await Task.sleep(for: grace)
                    guard !Task.isCancelled, let self, let held = self.heldDrop else { return }
                    self.dropTimer = nil
                    self.heldDrop = nil
                    Motion.animate { self.connection = held }
                }
            }
            return
        }
        dropTimer?.cancel()
        dropTimer = nil
        heldDrop = nil
        if new != connection { Motion.animate { connection = new } }
        if recovering {
            for botId in Set(readingViews.values) { markRead(botId) }
        }
    }

    private func setHostRoute(_ new: HostRoute?) {
        guard new != hostRoute else { return }
        Motion.animate { hostRoute = new }
    }

    private func stopEvents() {
        eventsTask?.cancel()
        eventsTask = nil
        encerrarAutoridade()
        setHostRoute(nil)
    }

    /// The channel's side streams: merged computer info and mailbox outcomes.
    private func watch(_ remote: any RemoteTransport) {
        let updates = remote.computerUpdates()
        let mailbox = remote.mailboxEvents()
        Task { [weak self] in
            for await computer in updates {
                guard let self, !self.retired else { return }
                // The color and route are picked on this device; the transport's copy may be older.
                self.updateComputer { current in
                    let (color, route) = (current.color, current.route)
                    current = computer
                    current.color = color
                    current.route = route
                }
            }
        }
        Task { [weak self] in
            for await event in mailbox {
                guard let self, !self.retired else { return }
                self.applyMailbox(event)
            }
        }
    }
}
