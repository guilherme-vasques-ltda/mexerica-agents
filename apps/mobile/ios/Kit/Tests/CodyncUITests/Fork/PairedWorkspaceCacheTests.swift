import CodyncKit
import Foundation
import Testing
@testable import CodyncUI

@MainActor @Test func pairedWorkspaceInvalidationClearsOnlyActiveComputer() {
    let suite = "Fork.\(UUID())"
    let context = SharedStore.Context(accountID: "fixture", suite: suite)
    defer {
        context.erase()
        UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
    }
    let first = Computer(id: UUID().uuidString, name: "First", signKey: "", cloud: nil)
    let second = Computer(id: UUID().uuidString, name: "Second", signKey: "", cloud: nil)
    func makeStore(_ computer: Computer) -> BotStore {
        BotStore(computer: computer, storage: context) { _ in fatalError("No transport in cache test") }
    }
    let active = makeStore(first)
    let unaffected = makeStore(second)
    let pending = Entry(id: "local-old", seq: .max, botId: "bot", rev: 0, kind: "user", turn: 0,
                        data: EntryData(text: "pending", status: "waiting", clientNonce: "old"), createdAt: 0, updatedAt: 0)
    for store in [active, unaffected] {
        store.upsert(pending)
        store.setComposerDraft("draft", for: "bot")
        store.saveCache()
    }
    active.answering["approval"] = "yes"
    active.clearPairedWorkspace()
    #expect(active.chat("bot").isEmpty)
    #expect(active.answering.isEmpty)
    #expect(context.composerDrafts[first.id]?["bot"] == nil)
    #expect(context.composerDrafts[second.id]?["bot"] == "draft")
    #expect(makeStore(first).chat("bot").isEmpty)
    #expect(makeStore(second).chat("bot").map(\.id) == [pending.id])
}
