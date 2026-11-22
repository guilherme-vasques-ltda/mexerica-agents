import Foundation
import Testing
@testable import CodyncKit

@Test func botTitleFromHubIsBoundedByUTF8Bytes() throws {
    let decoder = JSONDecoder()
    let old = try decoder.decode(Bot.self, from: Data(#"{"id":"b1"}"#.utf8))
    #expect(old.title == "")
    let bot = try decoder.decode(Bot.self, from: Data(#"{"id":"b1","title":"Dev"}"#.utf8))
    #expect(bot.title == "Dev")
    let title = String(repeating: "é", count: 127) + "👩‍💻" + "fim"
    let data = try JSONSerialization.data(withJSONObject: ["id": "b1", "title": title])
    let bounded = try decoder.decode(Bot.self, from: data)
    #expect(bounded.title == String(repeating: "é", count: 127))
    #expect(bounded.title.utf8.count == 254)
}

@Test func snapshotOmitsTitleAndReadsOldPortrait() throws {
    let decoder = JSONDecoder()
    let bot = try decoder.decode(Bot.self, from: Data(#"{"id":"b1","title":"Roteiros"}"#.utf8))
    let snapshot = BotSnapshot(computerId: "c1", bot: bot)
    #expect(snapshot.bot.title.isEmpty)
    let data = try JSONEncoder().encode(snapshot)
    let object = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
    let portrait = try #require(object["bot"] as? [String: Any])
    #expect(portrait["title"] == nil)
    let old = try decoder.decode(BotSnapshot.self, from: Data(#"{"computerId":"c1","bot":{"id":"b1","name":"Antigo"}}"#.utf8))
    #expect(old.bot.name == "Antigo" && old.bot.title.isEmpty)
}

@Test func queueOrderDecodingAndSorting() throws {
    let decoder = JSONDecoder()
    func data(_ json: String) throws -> EntryData {
        try decoder.decode(EntryData.self, from: Data(json.utf8))
    }
    let old = try data(#"{"status":"queued"}"#)
    let reordered = try data(#"{"status":"queued","queueOrder":1}"#)
    #expect(old.queueOrder == nil && reordered.queueOrder == 1)
    #expect(try data(#"{"queueOrder":-1}"#).queueOrder == nil)
    #expect(try data(#"{"queueOrder":9007199254740992}"#).queueOrder == nil)
    #expect(try data(#"{"queueOrder":1.5}"#).queueOrder == nil)
    #expect(try decoder.decode(EntryData.self, from: JSONEncoder().encode(reordered)).queueOrder == 1)
    func entry(_ id: String, _ seq: Int64, _ data: EntryData) -> Entry {
        Entry(id: id, seq: seq, botId: "b", rev: 0, kind: "user", turn: 0, data: data, createdAt: 0, updatedAt: 0)
    }
    let a = entry("a", 1, old)
    let b = entry("b", 2, old)
    #expect(Entry.ordenadaPelaFila([b, a]).map(\.id) == ["a", "b"])
    let moved = entry("b", 2, try data(#"{"queueOrder":0}"#))
    #expect(Entry.ordenadaPelaFila([a, moved]).map(\.id) == ["b", "a"])
    let tied = entry("b", 2, reordered)
    #expect(Entry.ordenadaPelaFila([tied, a]).map(\.id) == ["a", "b"])
}
