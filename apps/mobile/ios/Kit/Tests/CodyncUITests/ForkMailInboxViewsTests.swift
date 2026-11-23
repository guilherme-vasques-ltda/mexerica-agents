import Foundation
import SwiftUI
import Testing
@testable import CodyncKit
@testable import CodyncUI

@MainActor
struct ForkMailInboxViewsTests {
    private let spaceA = ForkMailInboxFixtureTransport.spaceA
    private let spaceB = ForkMailInboxFixtureTransport.spaceB
    private let emptyMailboxId = ForkMailInboxFixtureTransport.emptyMailbox.mailboxId
    private let threadMailboxId = ForkMailInboxFixtureTransport.threadMailbox.mailboxId

    private func enabledStore(_ transport: ForkMailInboxFixtureTransport) -> ForkMailInboxStore {
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
        #expect(ForkMailInboxPresentationState(store: store) == .unavailable)
        #expect(await transport.callCount == 0)
    }

    @Test func connectionStatesStayDistinctFromLoading() async {
        let transport = ForkMailInboxFixtureTransport()
        let store = enabledStore(transport)
        #expect(ForkMailInboxPresentationState(store: store) == .loading)

        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store) == .loading)

        await transport.setAvailability(.notConnected)
        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store) == .notConnected)

        await transport.setAvailability(.reconnectRequired)
        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store) == .reconnectRequired)

        await transport.setAvailability(.offline)
        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store) == .unavailable)
    }

    @Test func accessDeniedDoesNotRevealAnotherWorkspaceMailbox() async {
        let store = enabledStore(ForkMailInboxFixtureTransport())
        await store.loadFirstPage(spaceId: spaceA, mailboxId: threadMailboxId)
        #expect(ForkMailInboxPresentationState(store: store) == .accessDenied)
    }

    @Test func emptyInboxOnlyAfterSuccessfulEmptyFirstPage() async throws {
        let store = enabledStore(ForkMailInboxFixtureTransport())
        await store.loadStatus(spaceId: spaceA)
        await store.loadFirstPage(spaceId: spaceA, mailboxId: emptyMailboxId)
        guard case .emptyInbox(let selection) = ForkMailInboxPresentationState(store: store) else {
            Issue.record("Expected emptyInbox after a successful empty first page")
            return
        }
        #expect(selection.selectedMailboxId == emptyMailboxId)
        #expect(selection.mailboxes.count == 1)
    }

    @Test func fixtureFlowPagesOpensAndClearsContentOnRevocation() async throws {
        let transport = ForkMailInboxFixtureTransport()
        let store = enabledStore(transport)

        await store.loadStatus(spaceId: spaceB)
        await store.loadFirstPage(spaceId: spaceB, mailboxId: threadMailboxId)
        let firstPage = try #require(threads(ForkMailInboxPresentationState(store: store)))
        #expect(firstPage.rows.count == 1)
        #expect(firstPage.canLoadMore)
        #expect(firstPage.rows[0].subject == "Assunto fictício de teste")
        #expect(firstPage.rows[0].lastMessageAt != nil)

        await store.loadNextPage()
        let secondPage = try #require(threads(ForkMailInboxPresentationState(store: store)))
        #expect(secondPage.rows.count == 2)
        #expect(!secondPage.canLoadMore)
        #expect(secondPage.rows[1].subject == nil)

        await store.openThread(spaceId: spaceB, mailboxId: threadMailboxId, threadId: "fixture-thread-1")
        let opened = try #require(store.openThread)
        #expect(opened.messages.count == 2)
        #expect(!opened.truncated)

        await transport.setAvailability(.notConnected)
        await store.loadStatus(spaceId: spaceB)
        #expect(ForkMailInboxPresentationState(store: store) == .notConnected)
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
