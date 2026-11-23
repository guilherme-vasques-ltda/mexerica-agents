import Foundation
import Testing
@testable import CodyncKit
@testable import CodyncUI

@MainActor @Test func forkAcknowledgementByNonceReplacesOptimisticOfflineMessageExactlyOnce() {
    let suite = "Fork.\(UUID())"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let store = BotStore(computer: Computer(id: UUID().uuidString, name: "Computer", signKey: "", boxKey: "", cloud: nil),
                         storage: SharedStore.Context(accountID: "account", suite: suite)) { _ in
        fatalError("Transport is not used by this test")
    }
    let nonce = "offline-nonce"
    let local = Entry(id: "local-\(nonce)", seq: .max, botId: "b", rev: 0, kind: "user", turn: 0,
                      data: EntryData(text: "hello", status: "waiting", clientNonce: nonce), createdAt: 1, updatedAt: 0)
    store.upsert(local)
    var acknowledged = local
    acknowledged.id = "server-entry"
    acknowledged.seq = 1
    acknowledged.rev = 1
    acknowledged.data.status = "sent"

    store.upsert(acknowledged)
    var response = acknowledged
    response.id = "bot-reply"
    response.kind = "agent"
    response.seq = 2
    response.rev = 2
    response.data = EntryData(text: "reply", status: "sent")
    response.data.final = true
    store.upsert(response)
    store.upsert(acknowledged)
    store.upsert(response)
    store.upsert(local)

    #expect(store.chat("b").map(\.id) == ["server-entry", "bot-reply"])
    #expect(store.chat("b").first?.data.clientNonce == nonce)
}

@MainActor @Test func forkOfflineDoesNotLookLikeAnEmptyLiveChat() {
    let suite = "Fork.\(UUID())"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let storage = SharedStore.Context(accountID: "fixture", suite: suite)
    let store = BotStore(computer: Computer(id: UUID().uuidString, name: "Fixture", signKey: "", boxKey: "", cloud: nil), storage: storage) { _ in
        fatalError("Transport is not used by this test")
    }
    #expect(store.chat("fixture").isEmpty)
    #expect(!store.hubUnavailable)
    store.connection = .computerOffline(lastSeen: nil)
    store.faixaDeQueda = true
    #expect(store.hubUnavailable)
    #expect(store.shownConnection == .connecting)
    #expect(!store.temAutoridade)
}

@MainActor @Test func forkRetiredStoreDropsOldChatAndPendingApproval() {
    let suite = "Fork.\(UUID())"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let storage = SharedStore.Context(accountID: "fixture", scope: "fixture-team", suite: suite)
    let store = BotStore(computer: Computer(id: UUID().uuidString, name: "Fixture", signKey: "", cloud: nil), storage: storage) { _ in
        fatalError("Transport is not used by this test")
    }
    let approval = Entry(id: "fixture-approval", seq: 1, botId: "fixture-bot", rev: 1, kind: "permission", turn: 0,
                         data: EntryData(status: "pending"), createdAt: 1, updatedAt: 0)
    store.upsert(approval)
    store.answering[approval.id] = "fixture-choice"
    store.setComposerDraft("Keep this draft", for: approval.botId)
    store.retire()
    store.removeComposerDrafts(for: approval.botId)
    #expect(storage.composerDrafts[store.computer.id]?[approval.botId] == "Keep this draft")
    storage.erase()
    store.upsert(approval)
    #expect(store.chat(approval.botId).isEmpty)
    #expect(store.answering.isEmpty)
}
