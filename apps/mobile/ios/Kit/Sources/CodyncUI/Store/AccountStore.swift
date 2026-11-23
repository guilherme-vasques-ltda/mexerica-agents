// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import Foundation
import Observation
import os

private let log = Logger(subsystem: "app.mexerica.ios", category: "Account")

/// One bot across an account context's computers, for lists that mix them.
public struct RosterItem: Identifiable, Sendable {
    public var ref: BotReference
    public var bot: Bot
    public var computer: Computer
    public var id: BotReference { ref }
}

/// Approved by the cloud with every check passed, waiting for the person to type here the code the Mac
/// shows once it saved this device. Nothing is saved until that code matches.
public struct AccessConfirmPrompt: Equatable, Sendable {
    public var requestId: String
    /// Wrong codes still allowed before the request is withdrawn.
    public var attemptsLeft: Int
}

/// What typing the Mac's code did.
public enum AccessConfirmResult: Equatable, Sendable {
    /// The code matched: the computer is saved and connecting.
    case saved
    /// Not six digits yet: not counted as a try.
    case incomplete
    /// Nothing saved; the person can try again.
    case wrong(attemptsLeft: Int)
    /// The last allowed try was wrong: the request was withdrawn, nothing saved.
    case tooManyAttempts
    /// Nothing is waiting for a code anymore (expired, cancelled, account switched, or the code can't
    /// be checked): nothing saved; `lastError` says why when there's something to say.
    case ended
}

/// Account → computers → bots: one `BotStore` per computer this context can reach
/// (QR-paired or approved through the account),
/// plus the account's computers from the cloud that this device can still ask access to.
///
/// Multiple computers are a future direction, not a current priority: the product targets one
/// computer today. This structure stays because the relay, accounts and SSH are built on it and it
/// works unchanged with a single computer; don't extend or polish multi-computer features for now.
@MainActor
@Observable
public final class AccountStore {
    public let storage: SharedStore.Context
    public var accountId: String? { storage.accountID }
    public private(set) var computers: [Computer] = []
    public private(set) var stores: [ComputerID: BotStore] = [:]
    /// Everything in the account, including computers this device isn't authorized on yet.
    public private(set) var cloudComputers: [CloudComputer] = []
    /// Access requests waiting for approval on the computer. The iPhone shows the ticket's code;
    /// the person types it on the Mac.
    public private(set) var pendingAccess: [ComputerID: AccessTicket] = [:]
    /// Requests the Mac approved, waiting for its confirmation code to be typed here: any screen
    /// shows this and calls `confirmAccess(_:code:)`, or `cancelAccess(_:)` to give up and start over.
    public private(set) var accessConfirm: [ComputerID: AccessConfirmPrompt] = [:]
    /// Wrong codes allowed per request before it is withdrawn.
    public static let confirmAttempts = 5
    public var lastError: String?
    public var selection: BotReference? {
        didSet { if let id = selection?.computerId, !retired { storage.lastComputerId = id } }
    }

    public var onBotUpdated: (@MainActor (BotReference, Bot) -> Void)?
    public var onConnected: (@MainActor (BotStore) -> Void)?
    public var onSent: (@MainActor (BotReference, Bot, SendProgress) -> Void)?
    /// The combined roster changed: a bot came, changed or went, or a computer did.
    public var onRosterChanged: (@MainActor ([RosterItem]) -> Void)?

    private let cloud: CloudClient?
    private let accountConfigured: Bool
    private let computerOrder: ComputerOrderStore
    private let makeStore: @MainActor (Computer) -> BotStore
    /// Persisted in the context (channel route).
    private var saved: [Computer]
    /// Internal (not private) so tests can wait for a poll to end.
    var accessPolls: [ComputerID: Task<Void, Never>] = [:]
    /// Bumped by each account list request and each local change to the list: a list that comes back
    /// after a newer one was asked for (or the list changed here) is out of date.
    private var cloudListVersion = 0
    /// Computers being taken out of the account: a list that comes back meanwhile leaves them out.
    private var removing: Set<ComputerID> = []
    /// Access requests still waiting for their code, joined by a second ask.
    private var asks: [ComputerID: Task<AccessAttempt, any Error>] = [:]
    /// One per pending ticket, memory only: what the Mac's confirmation code is checked against, and what
    /// saving needs. Dropped with the ticket, on a new request and on retire.
    /// Internal (not private) so tests can see it's gone.
    struct PendingConfirmation {
        let confirmation: AccessConfirmation
        let target: CloudComputer
        /// The approval's own expiry, once the cloud said "approved".
        var approvalExpiresAt: Int64?
        /// The grant the cloud made when the Mac approved: revoked if this step ends without a match.
        var grantId: String?
    }
    var confirmations: [ComputerID: PendingConfirmation] = [:]
    /// Revocations of approved grants that never got their code, per computer. A new request waits for
    /// them, or the cloud would answer 409. Internal (not private) so tests can wait for them.
    var grantRevocations: [ComputerID: Task<Void, Never>] = [:]
    /// Tests advance this wait explicitly, after each processed approval response.
    var waitForApprovalPoll: @MainActor () async throws -> Void = { try await Task.sleep(for: .seconds(2)) }
    /// How long an account change waits for `settleBeforeLeaving`; tests replace it with a signal.
    var leaveDeadline: @MainActor () async -> Void = { try? await Task.sleep(for: .seconds(3)) }
    /// Current time in epoch ms; tests replace it to cross a ticket's expiry without a real clock.
    var nowMs: @MainActor () -> Int64 = { Int64(Date.now.timeIntervalSince1970 * 1000) }
    private var isActive = true
    private var retired = false
    /// Computers saved just now (paired or approved): their first transport tolerates early 403s.
    private var recemGravados: Set<ComputerID> = []

    public convenience init(storage: SharedStore.Context, cloud: CloudClient?, loadSavedComputers: Bool = true, accountConfigured: Bool = false) {
        self.init(storage: storage, cloud: cloud, loadSavedComputers: loadSavedComputers, accountConfigured: accountConfigured) { computer in
            BotStore(computer: computer, storage: storage)
        }
    }

    init(storage: SharedStore.Context, cloud: CloudClient?, loadSavedComputers: Bool = true, accountConfigured: Bool = false,
         makeStore: @escaping @MainActor (Computer) -> BotStore) {
        self.storage = storage
        self.cloud = cloud
        self.accountConfigured = accountConfigured || cloud != nil || storage.accountID != nil
        computerOrder = ComputerOrderStore(storage: storage)
        self.makeStore = makeStore
        saved = loadSavedComputers ? storage.computers.filter(\.isConsistent) : []
        if let cloud {
            saved = saved.map { computer in
                var value = computer
                value.route = value.route ?? .relayOnly
                value.cloud = cloud.baseURL
                return value
            }
        }
        for computer in saved { open(computer) }
        computerOrder.onChanged = { [weak self] in self?.refreshList() }
        refreshList()
    }

    // MARK: derived

    /// Every computer's visible bots, computer by computer, each in its roster order.
    public var roster: [RosterItem] {
        computers.flatMap { computer -> [RosterItem] in
            guard let store = stores[computer.id] else { return [] }
            return store.roster.map {
                RosterItem(ref: BotReference(accountId: accountId, computerId: computer.id, botId: $0.id), bot: $0, computer: store.computer)
            }
        }
    }

    public func store(for id: ComputerID) -> BotStore? { stores[id] }

    // MARK: computers

    /// QR pairing (§4.1); the computer is saved in this context and connected.
    public func pair(_ pairing: Pairing, deviceName: String, platform: String) async throws -> Computer {
        guard !retired else { throw CancellationError() }
        guard !accountConfigured else {
            throw CloudError(status: 0, code: "accountRequired")
        }
        let identity = try DeviceIdentity.load(context: storage)
        var computer = try await HostConnector.pair(pairing, identity: identity, deviceName: deviceName, platform: platform)
        guard !retired else { throw CancellationError() }
        // Picked on this device: re-pairing keeps them.
        if let previous = saved.first(where: { $0.id == computer.id }) {
            computer.color = previous.color
            computer.route = previous.route
        }
        save(computer)
        open(computer)
        return computer
    }

    public func forget(_ id: ComputerID) {
        saved.removeAll { $0.id == id }
        persist()
        close(id)
        accessPolls.removeValue(forKey: id)?.cancel()
        setPending(id, nil)
        if selection?.computerId == id { selection = nil }
        refreshList()
    }

    /// Saved with the computer on this device; its channel reconnects over the new order right away.
    public func setRoute(_ id: ComputerID, _ route: ConnectionRoute) {
        let value: ConnectionRoute? = route == .automatic ? (cloud == nil ? nil : .relayOnly) : route
        if let i = saved.firstIndex(where: { $0.id == id }) {
            saved[i].route = value
            persist()
        }
        stores[id]?.updateComputer { $0.route = value }
        stores[id]?.restartStream()
        refreshList()
    }

    public func setColor(_ id: ComputerID, _ color: String) {
        if let i = saved.firstIndex(where: { $0.id == id }) {
            saved[i].color = color
            persist()
        }
        stores[id]?.updateComputer { $0.color = color }
        refreshList()
    }

    /// Moves a computer section on this device only.
    public func move(_ id: ComputerID, to target: ComputerID) {
        computerOrder.move(id, to: target, available: computers.map(\.id))
    }

    // MARK: account (§4.2 B)

    public func refreshCloud() async {
        _ = await refreshCloudReportingResult()
    }

    /// The same refresh, but it says whether the list was fetched. Background refreshes keep swallowing transient
    /// failures (see `refreshCloud`); a refresh the person asked for needs the answer, so it can say it failed.
    /// The cached list stays either way. Nothing to ask (no cloud, retired, or a newer refresh took over) is not a failure.
    public func refreshCloudReportingResult() async -> Bool {
        guard let cloud, !retired else { return true }
        cloudListVersion += 1
        let version = cloudListVersion
        do {
            let list = try await cloud.computers().filter { !removing.contains($0.computerId) }
            guard !retired, version == cloudListVersion else { return true }
            if list != cloudComputers { Motion.animate { cloudComputers = list } }
            return true
        } catch let error as CloudError where !error.isTransient {
            guard !retired, version == cloudListVersion else { return true }
            lastError = error.localizedDescription
            return false
        } catch {
            // Runs on every launch and foreground: a dropped connection, a busy cloud or a token
            // not ready yet fixes itself on the next refresh, so only answers that stay reach the dialog.
            log.info("cloud refresh failed")
            return false
        }
    }

    /// Asks the computer for access; the code arrives in `pendingAccess`: this iPhone shows it and the
    /// person types it on the Mac. The request runs here, not in the caller: it outlives a closed sheet, so the
    /// computer never waits on a request nobody reveals, and asking again while it runs joins it
    /// instead of committing a second one.
    /// Approval is then picked up in the background and the computer joins `stores`.
    public func requestAccess(_ id: ComputerID) async throws {
        guard !retired else { throw CancellationError() }
        if let ask = asks[id], !ask.isCancelled {
            _ = try await ask.value
            return
        }
        guard let cloud, let target = cloudComputers.first(where: { $0.computerId == id }) else {
            throw CloudError(status: 404, code: "computerNotInAccount")
        }
        accessPolls[id]?.cancel()
        // A new request: nothing from the previous one can confirm anything (an approved one is revoked).
        dropConfirmation(id)
        let revoking = grantRevocations[id]
        let ask = Task { () async throws -> AccessAttempt in
            // A grant still being revoked would make the cloud refuse the new request (409).
            await revoking?.value
            return try await cloud.requestAccess(id, signKey: target.signKey)
        }
        asks[id] = ask
        accessPolls[id] = Task { [weak self] in
            await withTaskCancellationHandler {
                let result = await ask.result
                guard let self else { return }
                if self.asks[id] == ask { self.asks[id] = nil }
                let attempt: AccessAttempt
                do {
                    attempt = try result.get()
                } catch {
                    // With the sheet closed nothing else sees it; an open sheet shows it too.
                    log.info("access request failed")
                    return
                }
                guard !self.retired, !Task.isCancelled else { return }
                Analytics.shared.capture(.accessRequested)
                let ticket = attempt.ticket
                self.setPending(id, ticket)
                // After the ticket: setting a new ticket drops what belonged to the one before.
                self.confirmations[id] = PendingConfirmation(confirmation: attempt.confirmation, target: target)
                await self.awaitApproval(ticket, target: target, cloud: cloud)
            } onCancel: {
                ask.cancel()
            }
        }
        _ = try await ask.value
    }

    /// An offline account computer named like one this device uses or sees online: most likely the same
    /// machine under an earlier identity (its host keys were reset), left behind in the account.
    /// Seen after that one joined the account, it ran beside it: another machine with the same name.
    public func isOlderCopy(_ computer: CloudComputer) -> Bool {
        guard !computer.isOnline else { return false }
        let seen = computer.lastSeenAt
        // The current copy may be QR-paired without being listed in the account.
        return computers.contains { $0.id != computer.computerId && $0.name == computer.name && replaced(lastSeen: seen, by: $0.id) }
            || cloudComputers.contains { other in
                other.computerId != computer.computerId && other.name == computer.name
                    && (stores[other.computerId] != nil || other.isOnline) && replaced(lastSeen: seen, by: other.computerId)
            }
    }

    /// A computer this device uses that won't come back as it is: it turned this device away (access
    /// revoked, or its identity changed after a reset or reinstall), or it's offline while a computer
    /// with its name is reachable that joined the account after it was last seen, the same machine
    /// under a new identity. Removing it is the fix.
    public func isStale(_ id: ComputerID) -> Bool {
        guard let store = stores[id] else { return false }
        switch store.connection {
        case .unauthorized:
            return true
        case .computerOffline, .offline:
            let name = store.computer.name
            let seen = lastSeen(store)
            return stores.values.contains {
                $0.computer.id != id && $0.computer.name == name && $0.connection == .online && replaced(lastSeen: seen, by: $0.computer.id)
            } || cloudComputers.contains {
                $0.computerId != id && $0.name == name && $0.isOnline && replaced(lastSeen: seen, by: $0.computerId)
            }
        default:
            return false
        }
    }

    /// A same-named computer last seen (epoch ms) before `id` joined the account was replaced by it;
    /// seen after, it ran beside it: another machine. Without either time, the name alone decides.
    private func replaced(lastSeen seen: Int64?, by id: ComputerID) -> Bool {
        guard let seen, let claimed = cloudComputers.first(where: { $0.computerId == id })?.claimedAt else { return true }
        return seen <= claimed
    }

    /// When the account or, for a computer only paired here, the relay last saw it.
    private func lastSeen(_ store: BotStore) -> Int64? {
        if let seen = cloudComputers.first(where: { $0.computerId == store.computer.id })?.lastSeenAt { return seen }
        guard case let .computerOffline(date?) = store.connection else { return nil }
        return Int64(date.timeIntervalSince1970 * 1000)
    }

    /// Access requests appear and settle with an animation, like connection changes.
    /// A request that ends or is replaced takes its confirmation step with it.
    private func setPending(_ id: ComputerID, _ ticket: AccessTicket?) {
        if ticket == nil || ticket?.requestId != pendingAccess[id]?.requestId { dropConfirmation(id) }
        guard pendingAccess[id] != ticket else { return }
        Motion.animate { pendingAccess[id] = ticket }
    }

    /// Takes a computer out of the account (its grants and pending requests go with it).
    /// Its row goes at once, so a second tap finds nothing to remove; no list brings it back while the
    /// removal runs, nor one asked for before it finished. A failed removal puts it back.
    public func removeFromAccount(_ id: ComputerID) async {
        guard !retired, let cloud, let index = cloudComputers.firstIndex(where: { $0.computerId == id }) else { return }
        let removed = cloudComputers[index]
        removing.insert(id)
        Motion.animate { _ = cloudComputers.remove(at: index) }
        do {
            try await cloud.removeComputer(id)
            guard !retired else { return }
            setPending(id, nil)
        } catch {
            guard !retired else { return }
            lastError = error.localizedDescription
            if !cloudComputers.contains(where: { $0.computerId == id }) {
                Motion.animate { cloudComputers.insert(removed, at: min(index, cloudComputers.count)) }
            }
        }
        removing.remove(id)
        cloudListVersion += 1
        await refreshCloud()
    }

    private func dropConfirmation(_ id: ComputerID) {
        // Ending after the Mac approved: the cloud still holds an active grant for this iPhone, and a new
        // request would get 409 until it goes. Revoked once, here, best effort. Without a grant seen
        // here, the Mac may still have approved it on the server: that is checked once instead.
        if let pending = confirmations[id] {
            if let grant = pending.grantId {
                revokeApprovedGrant(id, grantId: grant)
            } else {
                reconcileGrant(id, pending.confirmation.approvalCheck)
            }
        }
        confirmations[id] = nil
        if accessConfirm[id] != nil { Motion.animate { accessConfirm[id] = nil } }
    }

    /// Called right before the account changes (switch or sign out), while this account's session still
    /// works: its cloud client stops answering once another account is active, so cleaning up later
    /// (`retire`) could reach nothing. Every open request ends here; a grant seen here is revoked, any
    /// other is checked once (`dropConfirmation`). Then it waits for every clean-up of this account, those
    /// already running too (a request cancelled just before, its read still out), at most `leaveDeadline`,
    /// and lets the change go on whatever is left; nothing is queued again, nothing saved from the answers.
    public func settleBeforeLeaving() async {
        guard !retired else { return }
        for id in Array(pendingAccess.keys) {
            accessPolls.removeValue(forKey: id)?.cancel()
            setPending(id, nil)
        }
        // Each computer's queue is chained, so its last task ends after all before it; finished ones return at once.
        let work = Array(grantRevocations.values)
        guard !work.isEmpty else { return }
        let (done, signal) = AsyncStream<Void>.makeStream()
        let waiter = Task {
            for task in work { await task.value }
            signal.yield(())
        }
        let timer = Task { [weak self] in
            await self?.leaveDeadline()
            signal.yield(())
        }
        var iterator = done.makeAsyncIterator()
        _ = await iterator.next(isolation: #isolation)
        // The clean-up itself goes on (best effort); only the waiting stops.
        waiter.cancel()
        timer.cancel()
        signal.finish()
    }

    /// A request that showed its code ended before this iPhone saw an approval, which may have happened on
    /// the server meanwhile (the person typed the code on the Mac, then cancelled here). One look at the
    /// request, in the same queue as revocations: if it was approved for this very request, computer and
    /// host nonce, its grant is revoked once. Nothing is saved from this answer; a failure isn't retried.
    private func reconcileGrant(_ id: ComputerID, _ check: AccessApprovalCheck) {
        guard let cloud else { return }
        let previous = grantRevocations[id]
        grantRevocations[id] = Task {
            await previous?.value
            guard let status = try? await cloud.accessStatus(check.requestId),
                  let grant = check.grant(in: status) else { return }
            do {
                try await cloud.revokeGrant(id, grantId: grant)
            } catch {
                log.info("approved grant revoke failed")
            }
        }
    }

    /// The account owner's revocation of a grant this iPhone never confirmed. Best effort: a failure is
    /// logged, never retried, and only delays the next request of this computer by that one call.
    private func revokeApprovedGrant(_ id: ComputerID, grantId: String) {
        guard let cloud else { return }
        let previous = grantRevocations[id]
        grantRevocations[id] = Task {
            await previous?.value
            do {
                try await cloud.revokeGrant(id, grantId: grantId)
            } catch {
                log.info("approved grant revoke failed")
            }
        }
    }

    /// The code the Mac showed, typed here. Saves the computer only if it matches the code this
    /// iPhone computes from its own request (host key, both keys and nonces, request id), compared in
    /// constant time, with both deadlines checked again now. A code from the network is never used.
    /// The last allowed wrong code withdraws the request (`cancelAccess`).
    public func confirmAccess(_ id: ComputerID, code typed: String) async -> AccessConfirmResult {
        guard !retired, let cloud, let ticket = pendingAccess[id], var prompt = accessConfirm[id],
              prompt.requestId == ticket.requestId, let pending = confirmations[id],
              pending.confirmation.isBound(to: ticket), let approvalExpiresAt = pending.approvalExpiresAt else { return .ended }
        guard AccessConfirmation.normalize(typed) != nil else { return .incomplete }
        // Either deadline may have passed while the person was typing: the request is over, nothing saved;
        // it is withdrawn and its grant revoked.
        guard nowMs() < ticket.expiresAt, nowMs() < approvalExpiresAt else {
            lastError = "O pedido expirou. Peça acesso novamente."
            _ = await withdraw(id, reason: .expired)
            return .ended
        }
        switch pending.confirmation.verify(typed) {
        case .unusable:
            lastError = AccessConfirmText.text(.unusable)
            _ = await withdraw(id, reason: .cancelled)
            return .ended
        case .mismatch:
            prompt.attemptsLeft -= 1
            guard prompt.attemptsLeft > 0 else {
                lastError = AccessConfirmText.text(.tooMany)
                _ = await withdraw(id, reason: .cancelled)
                return .tooManyAttempts
            }
            accessConfirm[id] = prompt
            return .wrong(attemptsLeft: prompt.attemptsLeft)
        case .match:
            accessPolls.removeValue(forKey: id)?.cancel()
            // Confirmed: the grant stays. Taken out first, so ending the request doesn't revoke it.
            confirmations[id] = nil
            setPending(id, nil)
            // Pin the key the code was computed with; the first hello brings its mailbox key and addresses.
            var computer = Computer(id: ticket.computerId, name: pending.target.name, signKey: ticket.signKey,
                                    cloud: cloud.baseURL, device: pending.target.device)
            computer.route = .relayOnly
            save(computer)
            open(computer)
            Analytics.shared.capture(.computerPaired(via: .account, computer: computer.id))
            await refreshCloud()
            return .saved
        }
    }

    /// Withdraws a pending access request; the computer drops it from its approval list. After the Mac
    /// approved (waiting for its code), the grant is revoked too.
    public func cancelAccess(_ id: ComputerID) async {
        if let error = await withdraw(id, reason: .cancelled) { lastError = error.localizedDescription }
    }

    /// Ends a pending request here and withdraws it in the cloud (DELETE of the request); one approved
    /// and waiting for the Mac's code also has its grant revoked (`dropConfirmation`). Returns the
    /// withdrawal's error, so callers that already said why it ended keep their message.
    private func withdraw(_ id: ComputerID, reason: AnalyticsEvent.AccessFailure) async -> (any Error)? {
        guard let ticket = pendingAccess[id] else { return nil }
        accessPolls.removeValue(forKey: id)?.cancel()
        setPending(id, nil)
        Analytics.shared.capture(.accessRequestFailed(reason))
        do {
            try await cloud?.cancelAccess(ticket.requestId)
            return nil
        } catch {
            return error
        }
    }

    /// Ends, from its own poll, a request the cloud already approved: withdrawn in the cloud (in its own
    /// task, since the poll ends here) and its grant revoked once, through `setPending`.
    private func endApproved(_ ticket: AccessTicket, cloud: CloudClient, message: String) {
        lastError = message
        let requestId = ticket.requestId
        Task { _ = try? await cloud.cancelAccess(requestId) }
        setPending(ticket.computerId, nil)
    }

    private func awaitApproval(_ ticket: AccessTicket, target: CloudComputer, cloud: CloudClient) async {
        while !Task.isCancelled, !retired {
            do { try await waitForApprovalPoll() } catch { return }
            guard !Task.isCancelled, !retired else { return }
            guard nowMs() < ticket.expiresAt else {
                // Ran out after the cloud approved (waiting for the Mac's code): withdrawn and revoked too.
                if confirmations[ticket.computerId]?.grantId != nil {
                    endApproved(ticket, cloud: cloud, message: "O pedido expirou. Peça acesso novamente.")
                    return
                }
                lastError = "O pedido expirou. Peça acesso novamente."
                setPending(ticket.computerId, nil)
                return
            }
            guard let status = try? await cloud.accessStatus(ticket.requestId) else { continue }
            // Ended meanwhile (cancelled, replaced, account switched): that end already took care of the
            // approval, if any (revocation or reconciliation); this answer is not acted on.
            guard !retired, !Task.isCancelled, pendingAccess[ticket.computerId]?.requestId == ticket.requestId else { return }
            switch status.status {
            case .pending:
                continue
            case .approved:
                // Already waiting for the Mac's code: keep polling, so a withdrawal or expiry ends it.
                if accessConfirm[ticket.computerId]?.requestId == ticket.requestId { continue }
                // The Mac approved only after the person typed the code shown here: nothing is saved
                // unless every check below passes and then the Mac's code is typed here.
                // Identity first: this request, this computer, this host nonce and a well-formed grant.
                guard status.requestId == ticket.requestId, status.computerId == ticket.computerId,
                      status.hostNonce == ticket.hostNonce, ticket.hostNonce != nil,
                      let grant = status.grantId, RelayCrypto.isAccessRequestId(grant) else { continue }
                // From here the cloud holds a grant for this iPhone. It is kept with the attempt now,
                // before any time check or wait, so every end from here on (cancel, expiry, account
                // switch, new request, a failed check below) revokes it, once (`dropConfirmation`, `retire`).
                // Without this request's own material there is nothing to check a code against: fail closed.
                guard var pending = confirmations[ticket.computerId], pending.confirmation.isBound(to: ticket) else {
                    revokeApprovedGrant(ticket.computerId, grantId: grant)
                    endApproved(ticket, cloud: cloud, message: AccessConfirmText.text(.unusable))
                    return
                }
                pending.grantId = grant
                confirmations[ticket.computerId] = pending
                // Then time: an approval without a deadline, or one that arrived already past it (or past
                // the ticket's), ends the request and revokes its grant.
                guard let approvalExpiresAt = status.expiresAt else {
                    endApproved(ticket, cloud: cloud, message: AccessConfirmText.text(.unusable))
                    return
                }
                guard nowMs() < approvalExpiresAt, nowMs() < ticket.expiresAt else {
                    endApproved(ticket, cloud: cloud, message: "O pedido expirou. Peça acesso novamente.")
                    return
                }
                pending.approvalExpiresAt = approvalExpiresAt
                confirmations[ticket.computerId] = pending
                // The list is asked apart from the other guards: whatever it answers (an error too),
                // the ticket's expiry is checked right after it returns.
                let list = try? await cloud.computers()
                // Ended meanwhile (cancelled, replaced, account switched): that end already revoked the grant.
                guard !retired, !Task.isCancelled,
                      pendingAccess[ticket.computerId]?.requestId == ticket.requestId else { return }
                // The ticket or the approval may have run out while the list was loading: the request
                // ends, never left hanging, and its grant is revoked.
                guard nowMs() < ticket.expiresAt, nowMs() < approvalExpiresAt else {
                    endApproved(ticket, cloud: cloud, message: "O pedido expirou. Peça acesso novamente.")
                    return
                }
                // An account can hold several computers, but this one must appear exactly once:
                // a duplicated id is an ambiguous answer, not something to pick from. A list that
                // failed, or any mismatch, ends the request too: its grant must not stay behind.
                guard let list, list.filter({ $0.computerId == ticket.computerId }).count == 1,
                      let current = list.first(where: { $0.computerId == ticket.computerId }),
                      current.signKey == ticket.signKey, current.isGranted else {
                    endApproved(ticket, cloud: cloud, message: AccessConfirmText.text(.unusable))
                    return
                }
                // A cloud could fake all of the above: nothing is saved here. The person types
                // the code the Mac shows once it saved this device; `confirmAccess` saves on a match.
                let prompt = AccessConfirmPrompt(requestId: ticket.requestId, attemptsLeft: Self.confirmAttempts)
                Motion.animate { accessConfirm[ticket.computerId] = prompt }
                continue
            case .refused, .unknown:
                setPending(ticket.computerId, nil)
                lastError = status.status == .refused
                    ? "A nuvem recusou este pedido. Nenhum vínculo foi salvo."
                    : "O resultado do pedido não foi confirmado. Confira no Mac antes de pedir acesso novamente."
                return
            case .denied, .expired, .cancelled:
                setPending(ticket.computerId, nil)
                let reason: AnalyticsEvent.AccessFailure = switch status.status {
                case .denied: .denied
                case .expired: .expired
                default: .cancelled
                }
                Analytics.shared.capture(.accessRequestFailed(reason))
                lastError = switch reason {
                case .denied: "\(target.name) recusou o pedido."
                case .expired: "O pedido expirou. Peça acesso novamente."
                case .cancelled: "O pedido foi cancelado: \(target.name) ou este iPhone saiu da sua conta."
                }
                // Withdrawn by the account: the list shows what's left of it.
                if reason == .cancelled { await refreshCloud() }
                return
            }
        }
    }

    // MARK: lifecycle

    public func setActive(_ active: Bool) {
        guard !retired else { return }
        isActive = active
        for store in stores.values { store.setActive(active) }
    }

    /// Account switched or signed out: nothing from here writes into storage or calls back anymore.
    public func retire() {
        guard !retired else { return }
        retired = true
        computerOrder.retire()
        for store in stores.values { store.retire() }
        for poll in accessPolls.values { poll.cancel() }
        accessPolls = [:]
        for ask in asks.values { ask.cancel() }
        asks = [:]
        // Only this store's own requests: a replacement store keeps its own `pendingAccess`.
        pendingAccess = [:]
        // Attempts still open end with the account: grants seen here are revoked, others checked once.
        for (id, pending) in confirmations {
            if let grant = pending.grantId {
                revokeApprovedGrant(id, grantId: grant)
            } else {
                reconcileGrant(id, pending.confirmation.approvalCheck)
            }
        }
        confirmations = [:]
        accessConfirm = [:]
        onBotUpdated = nil
        onConnected = nil
        onSent = nil
        onRosterChanged = nil
        selection = nil
        saved = []
        stores = [:]
        computers = []
        cloudComputers = []
        lastError = nil
    }

    // MARK: plumbing

    private func open(_ computer: Computer) {
        close(computer.id)
        let store = makeStore(computer)
        let id = computer.id
        // Only a computer this store just saved, stamped now; a saved one reopened gets no tolerance.
        if recemGravados.remove(id) != nil { store.vinculoGravadoEm = store.monotonico() }
        store.onBotUpdated = { [weak self] bot in
            guard let self, !self.retired else { return }
            self.onBotUpdated?(BotReference(accountId: self.accountId, computerId: id, botId: bot.id), bot)
        }
        store.onSent = { [weak self] bot, progress in
            guard let self, !self.retired else { return }
            self.onSent?(BotReference(accountId: self.accountId, computerId: id, botId: bot.id), bot, progress)
        }
        store.onConnected = { [weak self] store in
            guard let self, !self.retired else { return }
            self.onConnected?(store)
        }
        store.onComputerChanged = { [weak self] computer in self?.computerChanged(computer) }
        store.onRosterChanged = { [weak self] in self?.rosterChanged() }
        stores[id] = store
        if isActive { store.setActive(true) }
    }

    private func close(_ id: ComputerID) {
        stores.removeValue(forKey: id)?.retire()
    }

    private func computerChanged(_ computer: Computer) {
        guard !retired else { return }
        if let i = saved.firstIndex(where: { $0.id == computer.id }) {
            saved[i] = computer
            persist()
        }
        refreshList()
    }

    private func save(_ computer: Computer) {
        recemGravados.insert(computer.id)
        saved.removeAll { $0.id == computer.id }
        saved.insert(computer, at: 0)
        persist()
        refreshList()
    }

    private func persist() {
        guard !retired else { return }
        storage.computers = saved
    }

    private func refreshList() {
        let byID = Dictionary(saved.map { ($0.id, $0) }, uniquingKeysWith: { _, newest in newest })
        let ordered = computerOrder.state.orderedIDs(saved.map(\.id)).compactMap { byID[$0] }
        if saved != ordered {
            saved = ordered
            persist()
        }
        if ordered != computers { Motion.animate { computers = ordered } }
        rosterChanged()
    }

    private func rosterChanged() {
        guard !retired else { return }
        onRosterChanged?(roster)
    }
}
