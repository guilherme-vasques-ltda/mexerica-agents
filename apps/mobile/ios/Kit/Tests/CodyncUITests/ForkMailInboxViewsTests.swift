import Foundation
import SwiftUI
import Testing
@testable import CodyncKit
@testable import CodyncUI

private actor GatedMailInboxTransport: MailInboxTransport {
    enum Operation: Sendable {
        case status
        case list
    }

    let base = ForkMailInboxFixtureTransport()
    private var held: Operation?
    private var waiter: CheckedContinuation<Void, Never>?
    private(set) var hasArrived = false

    func hold(_ operation: Operation) {
        held = operation
    }

    func release() {
        held = nil
        waiter?.resume()
        waiter = nil
    }

    func waitUntilArrived() async {
        while !hasArrived {
            await Task.yield()
        }
    }

    func mailInboxStatus(spaceId: String) async throws -> MailInboxStatus {
        await gateIfHeld(.status)
        return try await base.mailInboxStatus(spaceId: spaceId)
    }

    func mailInboxListThreads(spaceId: String, mailboxId: String, cursor: String?) async throws -> MailInboxListThreadsResponse {
        await gateIfHeld(.list)
        return try await base.mailInboxListThreads(spaceId: spaceId, mailboxId: mailboxId, cursor: cursor)
    }

    func mailInboxOpenThread(spaceId: String, mailboxId: String, threadId: String) async throws -> MailInboxOpenThreadResponse {
        try await base.mailInboxOpenThread(spaceId: spaceId, mailboxId: mailboxId, threadId: threadId)
    }

    private func gateIfHeld(_ operation: Operation) async {
        guard held == operation else { return }
        hasArrived = true
        await withCheckedContinuation { waiter = $0 }
    }
}

@MainActor
struct ForkMailInboxViewsTests {
    private let spaceA = ForkMailInboxFixtureTransport.spaceA
    private let spaceB = ForkMailInboxFixtureTransport.spaceB
    private let emptyMailboxId = ForkMailInboxFixtureTransport.emptyMailbox.mailboxId
    private let threadMailboxId = ForkMailInboxFixtureTransport.threadMailbox.mailboxId

    private func enabledStore(_ transport: MailInboxTransport) -> ForkMailInboxStore {
        ForkMailInboxStore(transport: transport, featureEnabled: true)
    }

    private func threads(_ state: ForkMailInboxPresentationState) -> ForkMailInboxThreadsPresentation? {
        if case .threads(let value) = state { return value }
        return nil
    }

    @Test func featureDisabledIsUnavailableAndNeverCallsTransport() async {
        let transport = ForkMailInboxFixtureTransport()
        let store = ForkMailInboxStore(transport: transport)
        await store.loadStatus(spaceId: spaceB)
        await store.loadFirstPage(spaceId: spaceB, mailboxId: threadMailboxId)
        #expect(ForkMailInboxPresentationState(store: store, spaceId: spaceB) == .unavailable)
        #expect(await transport.callCount == 0)
    }

    @Test func connectionStatesStayDistinctFromLoading() async {
        let transport = ForkMailInboxFixtureTransport()
        let store = enabledStore(transport)
        #expect(ForkMailInboxPresentationState(store: store, spaceId: spaceB) == .loading)

        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store, spaceId: spaceB) == .loading)

        await transport.setAvailability(.notConnected)
        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store, spaceId: spaceB) == .notConnected)

        await transport.setAvailability(.reconnectRequired)
        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store, spaceId: spaceB) == .reconnectRequired)

        await transport.setAvailability(.offline)
        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store, spaceId: spaceB) == .unavailable)
    }

    @Test func accessDeniedDoesNotRevealAnotherWorkspaceMailbox() async {
        let store = enabledStore(ForkMailInboxFixtureTransport())
        await store.loadFirstPage(spaceId: spaceA, mailboxId: threadMailboxId)
        #expect(ForkMailInboxPresentationState(store: store, spaceId: spaceA) == .accessDenied)
    }

    @Test func emptyInboxOnlyAfterSuccessfulEmptyFirstPage() async throws {
        let store = enabledStore(ForkMailInboxFixtureTransport())
        await store.loadStatus(spaceId: spaceA)
        await store.loadFirstPage(spaceId: spaceA, mailboxId: emptyMailboxId)
        guard case .emptyInbox(let selection) = ForkMailInboxPresentationState(store: store, spaceId: spaceA) else {
            Issue.record("Expected emptyInbox after a successful empty first page")
            return
        }
        #expect(selection.selectedMailboxId == emptyMailboxId)
        #expect(selection.mailboxes.count == 1)
    }

    @Test func presentationNeverProjectsCachedDataFromAnotherSpace() async throws {
        let store = enabledStore(ForkMailInboxFixtureTransport())
        await store.loadStatus(spaceId: spaceB)
        await store.loadFirstPage(spaceId: spaceB, mailboxId: threadMailboxId)
        #expect(threads(ForkMailInboxPresentationState(store: store, spaceId: spaceB)) != nil)

        let model = ForkMailInboxScreenModel(store: store, spaceId: spaceA)
        #expect(model.presentation == .loading)
        #expect(!model.shouldMaskInAppSwitcher)
    }

    @Test func modelShowsLoadingWhileStatusRequestIsSuspended() async throws {
        let transport = GatedMailInboxTransport()
        let store = enabledStore(transport)
        let model = ForkMailInboxScreenModel(store: store, spaceId: spaceB)
        await transport.hold(.status)

        let refresh = Task { await model.refresh() }
        await transport.waitUntilArrived()
        #expect(model.presentation == .loading)
        #expect(model.isBusy)

        await transport.release()
        await refresh.value
        #expect(threads(model.presentation) != nil)
        #expect(!model.isBusy)
    }

    @Test func staleSuspendedOperationIsDiscardedAfterSpaceSwap() async throws {
        let transport = GatedMailInboxTransport()
        let store = enabledStore(transport)
        let staleModel = ForkMailInboxScreenModel(store: store, spaceId: spaceB)
        await transport.hold(.list)

        let selection = Task { await staleModel.selectMailbox(threadMailboxId) }
        await transport.waitUntilArrived()

        let currentModel = ForkMailInboxScreenModel(store: store, spaceId: spaceA)
        await store.loadStatus(spaceId: spaceA)

        await transport.release()
        await selection.value

        #expect(staleModel.presentation == .loading)
        #expect(!staleModel.shouldMaskInAppSwitcher)
        #expect(currentModel.presentation == .loading)
        #expect(store.activeSpaceId == spaceA)
    }

    @Test func failedReopenDoesNotResurrectPreviousThreadBody() async throws {
        let transport = ForkMailInboxFixtureTransport()
        let store = enabledStore(transport)
        let model = ForkMailInboxScreenModel(store: store, spaceId: spaceB)
        await model.refresh()

        await model.open(threadId: "fixture-thread-1")
        #expect(model.thread != nil)
        #expect(model.isThreadPresented)

        model.isThreadPresented = false
        await transport.setAvailability(.offline)
        await model.open(threadId: "fixture-thread-1")
        #expect(model.thread == nil)
        #expect(!model.isThreadPresented)
    }

    @Test func leavingClearsOpenThreadAndListForTheNextSession() async throws {
        let store = enabledStore(ForkMailInboxFixtureTransport())
        let model = ForkMailInboxScreenModel(store: store, spaceId: spaceB)
        await model.refresh()
        await model.open(threadId: "fixture-thread-1")
        #expect(model.thread != nil)

        model.leave()
        #expect(model.thread == nil)
        #expect(!model.isThreadPresented)
        #expect(store.threads.isEmpty)
        #expect(store.openThread == nil)
        #expect(model.presentation == .loading)
    }

    @Test func fixtureFlowPagesOpensAndClearsContentOnRevocation() async throws {
        let transport = ForkMailInboxFixtureTransport()
        let store = enabledStore(transport)

        await store.loadStatus(spaceId: spaceB)
        await store.loadFirstPage(spaceId: spaceB, mailboxId: threadMailboxId)
        let firstPage = try #require(threads(ForkMailInboxPresentationState(store: store, spaceId: spaceB)))
        #expect(firstPage.rows.count == 1)
        #expect(firstPage.canLoadMore)
        #expect(firstPage.rows[0].subject == "Assunto fictício de teste")
        #expect(firstPage.rows[0].lastMessageAt != nil)

        await store.loadNextPage()
        let secondPage = try #require(threads(ForkMailInboxPresentationState(store: store, spaceId: spaceB)))
        #expect(secondPage.rows.count == 2)
        #expect(!secondPage.canLoadMore)
        #expect(secondPage.rows[1].subject == nil)

        await store.openThread(spaceId: spaceB, mailboxId: threadMailboxId, threadId: "fixture-thread-1")
        let opened = try #require(store.openThread)
        #expect(opened.messages.count == 2)
        #expect(!opened.truncated)

        await transport.setAvailability(.notConnected)
        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store, spaceId: spaceB) == .notConnected)
        #expect(store.openThread == nil)
        #expect(store.threads.isEmpty)
        #expect(!store.shouldMaskInAppSwitcher)
    }

    @Test func senderDomainStaysSeparateAndNeverClaimsVerification() async throws {
        let store = enabledStore(ForkMailInboxFixtureTransport())
        await store.loadStatus(spaceId: spaceB)
        await store.loadFirstPage(spaceId: spaceB, mailboxId: threadMailboxId)
        await store.openThread(spaceId: spaceB, mailboxId: threadMailboxId, threadId: "fixture-thread-1")
        let response = try #require(store.openThread)
        let presentation = ForkMailInboxThreadPresentation(response: response)

        #expect(presentation.messages[0].senderName == "Remetente Fictício")
        #expect(presentation.messages[0].senderDomain == "remetente.example.invalid")
        #expect(presentation.messages[1].senderName == nil)
        #expect(presentation.messages[1].senderDomain == "example.invalid")
        #expect(response.messages.allSatisfy { !$0.senderPresentation.isVerifiedIdentity })
    }

    @Test func bodyStaysInertPlainText() async throws {
        let store = enabledStore(ForkMailInboxFixtureTransport())
        await store.loadStatus(spaceId: spaceB)
        await store.loadFirstPage(spaceId: spaceB, mailboxId: threadMailboxId)
        await store.openThread(spaceId: spaceB, mailboxId: threadMailboxId, threadId: "fixture-thread-1")
        let presentation = ForkMailInboxThreadPresentation(response: try #require(store.openThread))

        #expect(presentation.messages[0].body == "Primeira mensagem fictícia.\nSegunda linha.")
        #expect(presentation.messages[1].body == "Resposta fictícia com [link](https://example.invalid) e *texto*.")
    }

    @Test func truncatedFlagIsCarriedToThePresentation() {
        let response = MailInboxOpenThreadResponse(threadId: "fixture-thread-1", subject: nil, messages: [], truncated: true)
        #expect(ForkMailInboxThreadPresentation(response: response).truncated)
    }

    @Test func appSwitcherMaskCoversSensitiveContentOutsideActivePhase() async {
        let store = enabledStore(ForkMailInboxFixtureTransport())
        #expect(!store.shouldMaskInAppSwitcher)
        #expect(!ForkMailInboxPresentationState.shouldShowPrivacyMask(
            scenePhase: .inactive,
            shouldMaskInAppSwitcher: store.shouldMaskInAppSwitcher
        ))

        await store.loadStatus(spaceId: spaceB)
        #expect(store.shouldMaskInAppSwitcher)
        #expect(ForkMailInboxPresentationState.shouldShowPrivacyMask(
            scenePhase: .inactive,
            shouldMaskInAppSwitcher: store.shouldMaskInAppSwitcher
        ))
        #expect(ForkMailInboxPresentationState.shouldShowPrivacyMask(
            scenePhase: .background,
            shouldMaskInAppSwitcher: store.shouldMaskInAppSwitcher
        ))
        #expect(!ForkMailInboxPresentationState.shouldShowPrivacyMask(
            scenePhase: .active,
            shouldMaskInAppSwitcher: store.shouldMaskInAppSwitcher
        ))
    }
}
