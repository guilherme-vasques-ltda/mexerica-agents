import Foundation
import Testing
@testable import CodyncKit

private final class FixtureMailInboxTransport: MailInboxTransport, @unchecked Sendable {
    var statusResponses: [String: Result<MailInboxStatus, Error>] = [:]
    var listResponses: [String: Result<MailInboxListThreadsResponse, Error>] = [:]
    var openResponses: [String: Result<MailInboxOpenThreadResponse, Error>] = [:]
    var recordedCalls: [String] = []
    var delayNanoseconds: UInt64 = 0
    var onCallEntered: (@Sendable () -> Void)?

    func mailInboxStatus(spaceId: String) async throws -> MailInboxStatus {
        recordedCalls.append("status:\(spaceId)")
        if let callback = onCallEntered {
            onCallEntered = nil
            callback()
        }
        if delayNanoseconds > 0 { try await Task.sleep(nanoseconds: delayNanoseconds) }
        guard let result = statusResponses[spaceId] else {
            throw MailInboxError.accessDenied
        }
        return try result.get()
    }

    func mailInboxListThreads(
        spaceId: String,
        mailboxId: String,
        cursor: String?
    ) async throws -> MailInboxListThreadsResponse {
        recordedCalls.append("list:\(spaceId):\(mailboxId):\(cursor ?? "nil")")
        if let callback = onCallEntered {
            onCallEntered = nil
            callback()
        }
        if delayNanoseconds > 0 { try await Task.sleep(nanoseconds: delayNanoseconds) }
        let key = "\(spaceId):\(mailboxId):\(cursor ?? "page1")"
        guard let result = listResponses[key] else {
            throw MailInboxError.accessDenied
        }
        return try result.get()
    }

    func mailInboxOpenThread(
        spaceId: String,
        mailboxId: String,
        threadId: String
    ) async throws -> MailInboxOpenThreadResponse {
        recordedCalls.append("open:\(spaceId):\(mailboxId):\(threadId)")
        if let callback = onCallEntered {
            onCallEntered = nil
            callback()
        }
        if delayNanoseconds > 0 { try await Task.sleep(nanoseconds: delayNanoseconds) }
        let key = "\(spaceId):\(mailboxId):\(threadId)"
        guard let result = openResponses[key] else {
            throw MailInboxError.accessDenied
        }
        return try result.get()
    }
}

private func makeFixtureTransport() -> FixtureMailInboxTransport {
    let transport = FixtureMailInboxTransport()

    transport.statusResponses["space-alpha"] = .success(
        .ready(mailboxes: [
            MailInboxMailbox(mailboxId: "mbox-empty", label: "Caixa Vazia"),
            MailInboxMailbox(mailboxId: "mbox-inbox", label: "Caixa Principal")
        ])
    )
    transport.statusResponses["space-beta"] = .success(
        .ready(mailboxes: [
            MailInboxMailbox(mailboxId: "mbox-beta", label: "Equipe Beta")
        ])
    )

    transport.listResponses["space-alpha:mbox-empty:page1"] = .success(
        MailInboxListThreadsResponse(threads: [], nextCursor: nil)
    )
    transport.listResponses["space-alpha:mbox-inbox:page1"] = .success(
        MailInboxListThreadsResponse(
            threads: [
                MailInboxThreadSummary(
                    threadId: "thr-100",
                    subject: "Fatura mensal",
                    lastMessageAt: "2026-11-22T14:00:00Z"
                )
            ],
            nextCursor: "cur-page-2"
        )
    )
    transport.listResponses["space-alpha:mbox-inbox:cur-page-2"] = .success(
        MailInboxListThreadsResponse(
            threads: [
                MailInboxThreadSummary(
                    threadId: "thr-099",
                    subject: "Boas-vindas",
                    lastMessageAt: "2026-11-21T09:30:00Z"
                )
            ],
            nextCursor: nil
        )
    )

    transport.openResponses["space-alpha:mbox-inbox:thr-100"] = .success(
        MailInboxOpenThreadResponse(
            threadId: "thr-100",
            subject: "Fatura mensal",
            messages: [
                MailInboxMessage(
                    messageId: "msg-1",
                    fromDisplay: "Financeiro Lab",
                    fromDomain: "billing.example.invalid",
                    sentAt: "2026-11-22T13:50:00Z",
                    bodyText: "Confira o resumo em https://portal.example.invalid/fatura"
                ),
                MailInboxMessage(
                    messageId: "msg-2",
                    fromDisplay: "Contato Sem Dominio",
                    fromDomain: nil,
                    sentAt: "2026-11-22T14:00:00Z",
                    bodyText: "Recebido, obrigado."
                )
            ],
            truncated: true
        )
    )

    return transport
}

@Test @MainActor func featureGateDisabledFailsClosedAsUnavailable() async {
    let transport = makeFixtureTransport()
    let store = ForkMailInboxStore(transport: transport, featureEnabled: false)

    await store.loadStatus(spaceId: "space-alpha")
    #expect(store.status == .unavailable)
    #expect(store.lastError == .unavailable)
    #expect(!store.isEmptyInbox)

    await store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-inbox")
    #expect(store.lastError == .unavailable)

    await store.openThread(spaceId: "space-alpha", mailboxId: "mbox-inbox", threadId: "thr-100")
    #expect(store.lastError == .unavailable)
    #expect(transport.recordedCalls.isEmpty)
}

@Test @MainActor func emptySpaceIdFailsWithInvalidRequestAndNeverDefaults() async {
    let transport = makeFixtureTransport()
    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)

    await store.loadStatus(spaceId: "   ")
    #expect(store.lastError == .invalidRequest)
    #expect(store.activeSpaceId == nil)
    #expect(transport.recordedCalls.isEmpty)
}

@Test @MainActor func listsPaginatesAndOpensThreadWithSeparateDomainAndTruncatedFlag() async throws {
    let transport = makeFixtureTransport()
    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)

    await store.loadStatus(spaceId: "space-alpha")
    #expect(store.status == .ready(mailboxes: [
        MailInboxMailbox(mailboxId: "mbox-empty", label: "Caixa Vazia"),
        MailInboxMailbox(mailboxId: "mbox-inbox", label: "Caixa Principal")
    ]))

    await store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-inbox")
    #expect(store.threads.map(\.threadId) == ["thr-100"])
    #expect(store.nextCursor == "cur-page-2")
    #expect(!store.isEmptyInbox)

    await store.loadNextPage()
    #expect(store.threads.map(\.threadId) == ["thr-100", "thr-099"])
    #expect(store.nextCursor == nil)

    await store.openThread(spaceId: "space-alpha", mailboxId: "mbox-inbox", threadId: "thr-100")
    let opened = try #require(store.openThread)
    #expect(opened.truncated == true)
    #expect(opened.messages.count == 2)
    #expect(opened.messages[0].fromDisplay == "Financeiro Lab")
    #expect(opened.messages[0].fromDomain == "billing.example.invalid")
    #expect(opened.messages[0].senderPresentation.hasDeclaredDomain == true)
    #expect(opened.messages[0].senderPresentation.isVerifiedIdentity == false)
    #expect(opened.messages[1].fromDomain == nil)
    #expect(opened.messages[1].senderPresentation.hasDeclaredDomain == false)
    #expect(opened.messages[1].senderPresentation.isVerifiedIdentity == false)
}

@Test @MainActor func emptyMailboxOnlyCountsAsEmptyOnSuccessfulFirstPage() async {
    let transport = makeFixtureTransport()
    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)

    await store.loadStatus(spaceId: "space-alpha")
    await store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-empty")
    #expect(store.isEmptyInbox)

    // Hub offline / network timeout becomes unavailable and is NOT empty inbox
    struct TimeoutError: Error {}
    transport.statusResponses["space-offline"] = .failure(TimeoutError())
    await store.loadStatus(spaceId: "space-offline")
    #expect(store.status == .unavailable)
    #expect(store.lastError == .unavailable)
    #expect(!store.isEmptyInbox)

    // Access denied is distinct from empty inbox
    await store.loadStatus(spaceId: "space-unknown")
    #expect(store.lastError == .accessDenied)
    #expect(!store.isEmptyInbox)
}

@Test @MainActor func cursorInvalidRestartsFromPageOneWithoutMixingPreviousPages() async {
    let transport = makeFixtureTransport()
    transport.listResponses["space-alpha:mbox-inbox:cur-page-2"] = .failure(MailInboxError.cursorInvalid)

    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)
    await store.loadStatus(spaceId: "space-alpha")
    await store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-inbox")
    #expect(store.threads.map(\.threadId) == ["thr-100"])

    // Update page 1 so we prove page 1 replaced prior state without duplicating thr-100
    transport.listResponses["space-alpha:mbox-inbox:page1"] = .success(
        MailInboxListThreadsResponse(
            threads: [
                MailInboxThreadSummary(
                    threadId: "thr-200",
                    subject: "Pagina 1 reiniciada",
                    lastMessageAt: "2026-11-23T10:00:00Z"
                )
            ],
            nextCursor: nil
        )
    )

    await store.loadNextPage()
    #expect(store.lastError == nil)
    #expect(store.threads.map(\.threadId) == ["thr-200"])
    #expect(store.nextCursor == nil)
}

@Test @MainActor func sanitizesBidiCfTagSeparatorsAndIdnaDomains() async throws {
    let transport = makeFixtureTransport()
    let bidiSubject = "Aviso \u{202E}gpj.exe\u{202C}\u{200B}\u{E0061}\u{2028}importante\u{0007}"
    let bidiSender = "Remetente\u{2066}Falso\u{2069}\u{200E}\u{200F}\u{061C}\u{200C}\u{FEFF}"
    let body = "Linha 1\r\nLinha 2 \u{202E}inversa\u{202C}\u{2028}Linha 3\u{2029}Tag\u{E007F}\u{200B}\nשלום"

    transport.openResponses["space-alpha:mbox-inbox:thr-bidi"] = .success(
        MailInboxOpenThreadResponse(
            threadId: "thr-bidi",
            subject: bidiSubject,
            messages: [
                MailInboxMessage(
                    messageId: "msg-bidi",
                    fromDisplay: bidiSender,
                    fromDomain: "café\u{200B}.example.invalid",
                    sentAt: "2026-11-23T08:00:00Z",
                    bodyText: body
                ),
                MailInboxMessage(
                    messageId: "msg-bad-domain",
                    fromDisplay: "Invalido",
                    fromDomain: "evil/path.example.invalid",
                    sentAt: "2026-11-23T08:05:00Z",
                    bodyText: "ok"
                )
            ],
            truncated: false
        )
    )

    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)
    await store.openThread(spaceId: "space-alpha", mailboxId: "mbox-inbox", threadId: "thr-bidi")

    let opened = try #require(store.openThread)
    #expect(opened.subject == "Aviso gpj.exeimportante")
    #expect(opened.messages[0].fromDisplay == "RemetenteFalso")
    #expect(opened.messages[0].fromDomain == "xn--caf-dma.example.invalid")
    #expect(opened.messages[0].bodyText == "Linha 1\nLinha 2 inversa\nLinha 3\nTag\nשלום")
    #expect(opened.messages[1].fromDomain == nil)

    #expect(MailInboxSanitizer.sanitizeDomain("good.-bad.example.invalid") == nil)
    #expect(MailInboxSanitizer.sanitizeDomain("good.bad-.example.invalid") == nil)
    #expect(MailInboxSanitizer.sanitizeDomain("\(String(repeating: "a", count: 64)).example.invalid") == nil)
    let longDomain = Array(repeating: String(repeating: "a", count: 50), count: 5).joined(separator: ".") + ".example.invalid"
    #expect(longDomain.count > 253)
    #expect(MailInboxSanitizer.sanitizeDomain(longDomain) == nil)
}

@Test @MainActor func loadStatusInParallelWithLoadFirstPageDoesNotStall() async {
    let transport = makeFixtureTransport()
    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)
    await store.loadStatus(spaceId: "space-alpha")

    transport.delayNanoseconds = 40_000_000
    async let statusTask: Void = store.loadStatus(spaceId: "space-alpha")
    async let pageTask: Void = store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-inbox")
    _ = await (statusTask, pageTask)

    #expect(!store.isLoadingStatus)
    #expect(store.status?.isReady == true)
    #expect(store.threads.map(\.threadId) == ["thr-100"])
}

@Test @MainActor func openThreadSwitchesMailboxOnSameSpaceAndClearsPreviousList() async {
    let transport = makeFixtureTransport()
    transport.openResponses["space-alpha:mbox-empty:thr-other-box"] = .success(
        MailInboxOpenThreadResponse(
            threadId: "thr-other-box",
            subject: "Outra caixa",
            messages: [],
            truncated: false
        )
    )

    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)
    await store.loadStatus(spaceId: "space-alpha")
    await store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-inbox")
    #expect(store.threads.map(\.threadId) == ["thr-100"])

    await store.openThread(spaceId: "space-alpha", mailboxId: "mbox-empty", threadId: "thr-other-box")
    #expect(!store.isLoadingThreadDetail)
    #expect(store.selectedMailboxId == "mbox-empty")
    #expect(store.threads.isEmpty)
    #expect(store.nextCursor == nil)
    #expect(store.openThread?.threadId == "thr-other-box")
}

@Test @MainActor func loadFirstPageDiscardsInFlightLoadNextPageOnSameMailbox() async {
    let transport = makeFixtureTransport()
    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)

    await store.loadStatus(spaceId: "space-alpha")
    await store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-inbox")
    #expect(store.threads.map(\.threadId) == ["thr-100"])

    transport.delayNanoseconds = 60_000_000
    var nextPageTask: Task<Void, Never>?
    await withCheckedContinuation { (enteredCont: CheckedContinuation<Void, Never>) in
        transport.onCallEntered = {
            enteredCont.resume()
        }
        nextPageTask = Task {
            await store.loadNextPage()
        }
    }
    transport.delayNanoseconds = 0
    transport.listResponses["space-alpha:mbox-inbox:page1"] = .success(
        MailInboxListThreadsResponse(
            threads: [
                MailInboxThreadSummary(
                    threadId: "thr-fresh",
                    subject: "Recarregada",
                    lastMessageAt: "2026-11-23T11:00:00Z"
                )
            ],
            nextCursor: nil
        )
    )

    await store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-inbox")
    await nextPageTask?.value

    #expect(store.threads.map(\.threadId) == ["thr-fresh"])
    #expect(store.nextCursor == nil)
}

@Test @MainActor func openThreadDiscardsStaleResponseFromPreviousMailboxOrOlderRequest() async {
    let transport = makeFixtureTransport()
    transport.openResponses["space-alpha:mbox-inbox:thr-other"] = .success(
        MailInboxOpenThreadResponse(
            threadId: "thr-other",
            subject: "Segundo fio",
            messages: [],
            truncated: false
        )
    )

    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)
    await store.loadStatus(spaceId: "space-alpha")
    await store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-inbox")

    transport.delayNanoseconds = 60_000_000
    let firstOpen = Task {
        await store.openThread(spaceId: "space-alpha", mailboxId: "mbox-inbox", threadId: "thr-100")
    }
    await Task.yield()
    transport.delayNanoseconds = 0
    await store.openThread(spaceId: "space-alpha", mailboxId: "mbox-inbox", threadId: "thr-other")
    await firstOpen.value

    #expect(store.openThread?.threadId == "thr-other")
}

@Test @MainActor func contextChangeDiscardsInFlightResponseAndClearsSensitiveState() async {
    let transport = makeFixtureTransport()
    transport.delayNanoseconds = 60_000_000

    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)
    let initialGeneration = store.generation
    var taskRef: Task<Void, Never>?

    await withCheckedContinuation { (enteredCont: CheckedContinuation<Void, Never>) in
        transport.onCallEntered = {
            enteredCont.resume()
        }
        taskRef = Task {
            await store.loadStatus(spaceId: "space-alpha")
        }
    }
    transport.onCallEntered = nil

    let beforeReset = store.generation
    store.resetForContextChange()

    await taskRef?.value
    #expect(store.generation > initialGeneration)
    #expect(store.generation == beforeReset + 1)
    #expect(store.activeSpaceId == nil)
    #expect(store.status == nil)
    #expect(store.threads.isEmpty)
    #expect(store.openThread == nil)
    #expect(!store.shouldMaskInAppSwitcher)
}

@Test @MainActor func appSwitcherMaskAndLogRedactionProtectSensitiveFields() async {
    let transport = makeFixtureTransport()
    let store = ForkMailInboxStore(transport: transport, featureEnabled: true)
    #expect(!store.shouldMaskInAppSwitcher)

    await store.loadStatus(spaceId: "space-alpha")
    await store.loadFirstPage(spaceId: "space-alpha", mailboxId: "mbox-inbox")
    await store.openThread(spaceId: "space-alpha", mailboxId: "mbox-inbox", threadId: "thr-100")

    #expect(store.shouldMaskInAppSwitcher)

    let dumped = [
        String(describing: store),
        String(describing: store.status!),
        String(describing: store.threads[0]),
        String(describing: store.openThread!),
        String(describing: store.openThread!.messages[0])
    ].joined(separator: "\n")

    for secret in [
        "Fatura mensal",
        "Financeiro Lab",
        "billing.example.invalid",
        "portal.example.invalid",
        "mbox-inbox",
        "thr-100",
        "msg-1",
        "cur-page-2"
    ] {
        #expect(!dumped.contains(secret))
    }

    store.resetForContextChange()
    #expect(!store.shouldMaskInAppSwitcher)
}
