import Foundation
import Testing
@testable import CodyncKit

@Test func snapshotBoundsUntrustedFieldsAndKeepsExtensionData() throws {
    let huge = String(repeating: "x", count: 1_000_000)
    var bot = try JSONDecoder().decode(Bot.self, from: Data(#"{"id":"b1","name":"Berga","kind":"group","members":["b2"],"status":"working","activity":"Lendo","hidden":true,"createdAt":1,"lastAt":2,"startedAt":3,"hasPendingQuestions":true}"#.utf8))
    bot.description = huge; bot.cwd = huge; bot.command = huge; bot.lastMessage = huge
    bot.modelOptions = [huge: huge]; bot.connectors = [huge]; bot.skills = [huge]
    bot.members = Array(repeating: huge, count: 10_000)
    bot.name = huge; bot.activity = huge
    let snapshot = BotSnapshot(computerId: "c1", bot: bot)
    let data = try JSONEncoder().encode(snapshot)
    #expect(data.count < 65_536)
    let restored = try JSONDecoder().decode(BotSnapshot.self, from: data)
    #expect(restored.bot.name.utf8.count == 128 && restored.bot.activity.utf8.count == 2048)
    #expect(restored.bot.members.count == 16 && restored.bot.members.allSatisfy { $0.utf8.count == 64 })
    #expect(restored.bot.kind == "group" && restored.bot.status == "working" && restored.bot.hidden)
    #expect(restored.bot.createdAt == 1 && restored.bot.lastAt == 2 && restored.bot.startedAt == 3 && restored.bot.hasPendingQuestions)
    #expect(restored.bot.description.isEmpty && restored.bot.lastMessage == nil && restored.bot.modelOptions.isEmpty)
    let composed = "e" + String(repeating: "\u{301}", count: 500)
    bot.name = composed + "fim"
    #expect(BotSnapshot(computerId: "c1", bot: bot).bot.name == "Bot")
}

@Test func legacySnapshotAndInvalidCacheAreSafe() throws {
    let suite = "CodyncTests.\(UUID().uuidString)"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let defaults = try #require(UserDefaults(suiteName: suite))
    let store = SharedStore.Context(accountID: nil, suite: suite)
    let legacy = #"[{"computerId":"c1","bot":{"id":"b1","name":"Berga","members":["b2"],"description":"antigo","status":"working","activity":"Lendo"}}]"#
    defaults.set(Data(legacy.utf8), forKey: "bots")
    #expect(store.bots.first?.bot.name == "Berga" && store.bots.first?.bot.members == ["b2"])
    #expect(store.bots.first?.bot.description == "")
    let rewritten = try #require(defaults.data(forKey: "bots"))
    #expect(!String(decoding: rewritten, as: UTF8.self).contains("antigo"))
    _ = store.bots
    #expect(defaults.data(forKey: "bots") == rewritten)
    defaults.set(Data("inválido".utf8), forKey: "bots")
    #expect(store.bots.isEmpty)
    #expect(try JSONDecoder().decode([BotSnapshot].self, from: #require(defaults.data(forKey: "bots"))).isEmpty)
    let bot = try JSONDecoder().decode(Bot.self, from: Data(#"{"id":"b1"}"#.utf8))
    store.bots = Array(repeating: BotSnapshot(computerId: "c1", bot: bot), count: 201)
    #expect(store.bots.count == 100)
}

@Test func snapshotKeepsPrefixWithinByteCeiling() throws {
    let bot = try JSONDecoder().decode(Bot.self, from: Data(#"{"id":"b1","activity":"hello"}"#.utf8))
    let snapshots = (0..<100).map { BotSnapshot(computerId: "c\($0)", bot: bot) }
    let two = try JSONEncoder().encode(Array(snapshots.prefix(2))).count
    let data = try #require(SharedStore.Context.encodeBots(snapshots, maxBytes: two))
    #expect(try JSONDecoder().decode([BotSnapshot].self, from: data).map(\.computerId) == ["c0", "c1"])
    let full = try #require(SharedStore.Context.encodeBots(snapshots, maxBytes: 4 * 1024 * 1024))
    #expect(full.count <= 4 * 1024 * 1024)
    #expect(try JSONDecoder().decode([BotSnapshot].self, from: full).count == 100)
}

@Test func snapshotBoundsIdentifiersAndMultibyteCharacters() throws {
    var bot = try JSONDecoder().decode(Bot.self, from: Data(#"{"id":"b"}"#.utf8))
    bot.id = String(repeating: "é", count: 130)
    bot.kind = String(repeating: "é", count: 33)
    bot.name = String(repeating: "🍊", count: 33)
    bot.members = [String(repeating: "é", count: 33)]
    bot.avatarColor = String(repeating: "x", count: 1000)
    bot.avatarShape = "invalid shape"
    let snapshot = BotSnapshot(computerId: String(repeating: "é", count: 130), bot: bot)
    #expect(snapshot.computerId.utf8.count == 256)
    #expect(snapshot.bot.id.utf8.count == 256)
    #expect(snapshot.bot.kind.utf8.count == 64)
    #expect(snapshot.bot.name.utf8.count == 128)
    #expect(snapshot.bot.members[0].utf8.count == 64)
    #expect(snapshot.bot.avatarColor == "blue" && snapshot.bot.avatarShape == "blob")
}

@Test func oversizedLegacyCacheIsDiscardedBeforeDecoding() throws {
    let suite = "CodyncTests.\(UUID().uuidString)"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let defaults = try #require(UserDefaults(suiteName: suite))
    let store = SharedStore.Context(accountID: nil, suite: suite)
    defaults.set(Data(repeating: 32, count: 4 * 1024 * 1024 + 1), forKey: "bots")
    #expect(store.bots.isEmpty)
    #expect(try JSONDecoder().decode([BotSnapshot].self, from: #require(defaults.data(forKey: "bots"))).isEmpty)
}
