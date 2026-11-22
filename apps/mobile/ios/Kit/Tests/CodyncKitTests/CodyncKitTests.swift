// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation
import Testing
@testable import CodyncKit

@Test func parsesHostEvents() throws {
    let url = try #require(Bundle.module.url(forResource: "events", withExtension: "txt", subdirectory: "Fixtures"))
    let lines = try String(contentsOf: url, encoding: .utf8).split(separator: "\n")
    let events = lines.compactMap { HostClient.parseEvent(Data($0.dropFirst(5).utf8)) }
    #expect(events.count == 6)
    guard case let .hello(hostId, rev, usage, _) = events[0] else { Issue.record("hello"); return }
    #expect(hostId == "h1" && rev == 42 && usage.providers.first?.windows.first?.percent == 8)
    guard case let .bot(bot) = events[1] else { Issue.record("bot"); return }
    #expect(bot.name == "Rex" && bot.command == nil && !bot.isWorking)
    guard case let .entry(perm) = events[2] else { Issue.record("perm"); return }
    #expect(perm.isChat && perm.data.options?.first?.kind == "allow_once")
    guard case let .entry(tool) = events[3] else { Issue.record("tool"); return }
    #expect(!tool.isChat && tool.data.diffs?.first?.added == 1)
    guard case let .botDeleted(id, _) = events[4] else { Issue.record("deleted"); return }
    #expect(id == "b2")
}

@Test func relativeTime() {
    let now = Date(timeIntervalSince1970: 1_000_000)
    #expect(RelativeTime.short(now.addingTimeInterval(-30), now: now) == String(localized: "agora", bundle: RelativeTime.resourceBundle))
    #expect(RelativeTime.short(now.addingTimeInterval(-600), now: now) == "10m")
    #expect(RelativeTime.short(now.addingTimeInterval(-7200), now: now) == "2h")
}

@Test func parsesScreenEvents() throws {
    let e = HostClient.parseEvent(Data(#"{"type":"screen","screen":{"enabled":true,"connected":true,"capture":true,"displays":[{"id":1,"name":"Built-in","width":1512,"height":982,"main":true}],"agentBot":"b1"}}"#.utf8))
    guard case let .screen(s) = e else { Issue.record("screen"); return }
    #expect(s.available && s.agentBot == "b1" && s.mainDisplay?.width == 1512 && !s.userControl)
}

@Test func screenViewportMapsAndZooms() {
    // A 16:10 display in a wider landscape phone view: letterboxed left and right.
    var v = ScreenViewport(display: CGSize(width: 1600, height: 1000), view: CGSize(width: 800, height: 400))
    #expect(v.scale == 0.4)
    #expect(v.crop == nil)
    #expect(v.toDisplay(CGPoint(x: 400, y: 200)) == CGPoint(x: 800, y: 500))
    #expect(v.frame(of: v.displayBounds) == CGRect(x: 80, y: 0, width: 640, height: 400))

    // Pinch keeps the point under the fingers still.
    let anchor = CGPoint(x: 600, y: 100)
    let before = v.toDisplay(anchor)
    v.zoom(by: 2, at: anchor)
    let after = v.toDisplay(anchor)
    #expect(abs(before.x - after.x) < 0.001 && abs(before.y - after.y) < 0.001)
    let crop = v.crop!
    #expect(v.displayBounds.contains(crop))
    #expect(abs(crop.width / crop.height - 2) < 0.001)

    // Panning never leaves the display.
    v.pan(by: CGPoint(x: 10_000, y: 10_000))
    #expect(v.visible.minX == 0 && v.visible.minY == 0)

    // The cursor pulls the view along.
    v.follow(CGPoint(x: 1590, y: 990))
    #expect(v.visible.maxX == 1600 && v.visible.maxY == 1000)

    v.zoom(by: 1000, at: .zero)
    #expect(v.zoom == v.maxZoom && v.scale == 4)
}

@Test func parsesClaudeResetText() throws {
    let taipei = try #require(TimeZone(identifier: "Asia/Taipei"))
    var cal = Calendar(identifier: .gregorian)
    cal.timeZone = taipei
    let now = try #require(cal.date(from: DateComponents(year: 2026, month: 9, day: 25, hour: 17, minute: 20)))
    let at = { (m: Int, d: Int, h: Int, min: Int, y: Int) in cal.date(from: DateComponents(year: y, month: m, day: d, hour: h, minute: min)) }
    #expect(UsageWindow.parseReset("Sep 25 at 7:30pm (Asia/Taipei)", now: now) == at(9, 25, 19, 30, 2026))
    #expect(UsageWindow.parseReset("Sep 26 at 12pm (Asia/Taipei)", now: now) == at(9, 26, 12, 0, 2026))
    #expect(UsageWindow.parseReset("Jan 2 at 12am (Asia/Taipei)", now: now) == at(1, 2, 0, 0, 2027))
    #expect(UsageWindow.parseReset("5am (Asia/Taipei)", now: now) == at(9, 26, 5, 0, 2026))
    #expect(UsageWindow.parseReset("soon") == nil)
}

// MARK: - Version compatibility (mirrors host/src/compat.rs)

@Test func versionsParseLikeTheHost() {
    #expect(AppVersion.parse("2.4.1") == [2, 4, 1])
    #expect(AppVersion.parse(" v2.4 ") == [2, 4, 0])
    #expect(AppVersion.parse("3") == [3, 0, 0])
    #expect(AppVersion.parse("2.4.0-beta.1+77") == [2, 4, 0])
    for bad in ["", "abc", "2.x", "2.4.0.1", "-1.0.0", "2..0", "+1.0.0", "２.4.0"] {
        #expect(AppVersion.parse(bad) == nil, "\(bad)")
    }
    #expect(AppVersion.isBelow("2.9.9", "2.10.0"))
    #expect(!AppVersion.isBelow("2.4.0", "2.4.0"))
    #expect(!AppVersion.isBelow("2.4.0-dev", "2.4.0"))
    // Unreadable versions never lock anyone out.
    #expect(!AppVersion.isBelow("", "2.4.0"))
    #expect(!AppVersion.isBelow("2.0.0", "garbage"))
}

@Test func mismatchNamesTheSideToUpdate() {
    #expect(VersionMismatch.check(app: "2.4.0", minHost: "2.3.0", hostVersion: "2.4.0", minApp: "2.3.0") == nil)
    #expect(VersionMismatch.check(app: "2.4.0", minHost: "2.3.0", hostVersion: "2.6.0", minApp: "2.5.0") == .updateApp(minimum: "2.5.0"))
    #expect(VersionMismatch.check(app: "2.6.0", minHost: "2.5.0", hostVersion: "2.4.0", minApp: "2.3.0") == .updateHost(version: "2.4.0", minimum: "2.5.0"))
    // Hosts from before minApp: only this app's floor applies.
    #expect(VersionMismatch.check(app: "2.4.0", minHost: "2.3.0", hostVersion: "2.3.1", minApp: nil) == nil)
    #expect(VersionMismatch.check(app: "2.4.0", minHost: "2.3.0", hostVersion: "2.2.3", minApp: nil) == .updateHost(version: "2.2.3", minimum: "2.3.0"))
    #expect(VersionMismatch.check(app: "", minHost: "2.3.0", hostVersion: "2.6.0", minApp: "2.5.0") == nil)
}

@Test func releaseOrderVectorsMatchTheSharedTable() {
    // Shared vectors with CompatibilityTest.kt (Android) — same cases, same expectations.
    let vectors: [(version: String, minimum: String, expected: Bool)] = [
        // The 0.x line (Mexerica) comes after every 2.x (inherited Codync).
        ("0.1.0", "2.20.1", false),
        ("0.1.0", "2.20.3", false),
        ("2.20.3", "0.1.0", true),
        ("0.1.0", "0.1.0", false),
        ("0.1.0", "0.2.0", true),
        ("0.9.0", "0.10.0", true),
        ("0.10.0", "0.9.0", false),
        // 1.x vs 2.x and the usual numeric order inside a line.
        ("1.9.9", "2.0.0", true),
        ("2.0.0", "1.9.9", false),
        ("2.9.9", "2.10.0", true),
        ("2.19.0", "2.20.1", true),
        ("2.4.0", "2.4.0", false),
        ("2.4.0-dev", "2.4.0", false),
        // Whitespace and line breaks are trimmed; one leading v drops.
        (" v2.3.9 ", "2.4.0", true),
        ("\n2.18.0\n", "2.20.1", true),
        ("v2.3.9", "2.4.0", true),
        // Pre-release and build suffixes are ignored.
        ("2.4.0-beta.1+77", "2.4.1", true),
        ("2.4.0+77", "2.4.1", true),
        // Missing parts count as 0.
        ("2.4", "2.4.1", true),
        ("3", "2.20.1", false),
        // Unreadable versions never block, in either position.
        ("", "2.4.0", false),
        ("2.0.0", "garbage", false),
        ("garbled", "0.1.0", false),
        ("0.1.0", "garbled", false),
        ("2..0", "2.4.1", false),
        ("2.4.0.1", "2.4.1", false),
        ("+1.0.0", "2.0.0", false),
        ("-1.0.0", "2.0.0", false),
        ("２.4.0", "2.5.0", false),
        // Documented decision: `vv` stays unreadable on mobile (the host strips every `v`;
        // like the desktop we strip one — the side that never blocks).
        ("vv2.18.0", "2.20.1", false),
        // 64-bit components: beyond Int32 still parses; beyond Int64 stays unreadable.
        ("2.2147483648.0", "3.0.0", true),
        ("2.9223372036854775807.0", "3.0.0", true),
        ("2.9223372036854775808.0", "0.1.0", false),
        ("0.1.0", "2.9223372036854775808.0", false),
    ]
    for (version, minimum, expected) in vectors {
        #expect(AppVersion.isBelow(version, minimum) == expected, "\(version) vs \(minimum)")
    }
}

@Test func checkVectorsMatchTheSharedTable() {
    // Shared vectors with CompatibilityTest.kt (Android) — same cases, same expectations.
    struct Row { let app, host: String; let minApp: String?; let expected: VersionMismatch? }
    let rows: [Row] = [
        // Same-release pair and mixed pairs within the floors connect; older 2.x is still refused.
        Row(app: "0.1.0", host: "0.1.0", minApp: "2.20.1", expected: nil),
        Row(app: "0.1.0", host: "2.20.3", minApp: "2.20.1", expected: nil),
        Row(app: "2.20.3", host: "0.1.0", minApp: "2.20.1", expected: nil),
        Row(app: "2.19.0", host: "0.1.0", minApp: "2.20.1", expected: .updateApp(minimum: "2.20.1")),
        Row(app: "0.1.0", host: "2.18.0", minApp: nil, expected: .updateHost(version: "2.18.0", minimum: "2.20.1")),
        // Line-break-padded versions are read (trimmed) and judged like any other.
        Row(app: "\n2.19.0\n", host: "0.1.0", minApp: "2.20.1", expected: .updateApp(minimum: "2.20.1")),
        // Unreadable versions never block: beyond Int64, `vv`, empty.
        Row(app: "2.9223372036854775808.0", host: "0.1.0", minApp: "2.20.1", expected: nil),
        Row(app: "vv2.19.0", host: "0.1.0", minApp: "2.20.1", expected: nil),
        Row(app: "", host: "0.1.0", minApp: "2.20.1", expected: nil),
    ]
    for row in rows {
        #expect(VersionMismatch.check(app: row.app, hostVersion: row.host, minApp: row.minApp) == row.expected, "\(row.app) vs \(row.host)")
    }
}

@Test func remindersShowOnlyNewerReleasesNotDismissed() {
    #expect(UpdateReminder.app(current: "2.4.0", store: "2.5.0", dismissed: nil) == "2.5.0")
    #expect(UpdateReminder.app(current: "2.4.0", store: "2.5.0", dismissed: "2.5.0") == nil)
    // Dismissing one release doesn't hide the next one.
    #expect(UpdateReminder.app(current: "2.4.0", store: "2.6.0", dismissed: "2.5.0") == "2.6.0")
    #expect(UpdateReminder.app(current: "2.5.0", store: "2.5.0", dismissed: nil) == nil)
    // A store that lags behind this build (TestFlight, review) or can't be read says nothing.
    #expect(UpdateReminder.app(current: "2.5.0", store: "2.4.0", dismissed: nil) == nil)
    #expect(UpdateReminder.app(current: "2.5.0", store: nil, dismissed: nil) == nil)
    #expect(UpdateReminder.app(current: "", store: "2.5.0", dismissed: nil) == nil)
}

@Test func entrySessionDecodesOldAndNewWireFormats() throws {
    let old = #"{"id":"e","seq":1,"botId":"b","rev":1,"kind":"user","turn":1,"data":{},"createdAt":1,"updatedAt":1}"#
    let new = String(old.dropLast()) + #", "sessionId":"s"}"#
    let decoder = JSONDecoder()
    #expect(try decoder.decode(Entry.self, from: Data(old.utf8)).sessionId == nil)
    #expect(try decoder.decode(Entry.self, from: Data(new.utf8)).sessionId == "s")
    let cached = try JSONEncoder().encode(decoder.decode(Entry.self, from: Data(old.utf8)))
    #expect(try decoder.decode(Entry.self, from: cached).sessionId == nil)
}

@Test func sessionTitleIsBoundedOnCharacterBoundaries() throws {
    let title = String(repeating: "🍊", count: 100)
    let json = #"{"id":"s","chatId":"b","principal":false,"parentSession":null,"depth":1,"title":"\#(title)","state":"open","revision":1}"#
    let session = try JSONDecoder().decode(BotSession.self, from: Data(json.utf8))
    #expect(session.title.utf8.count == 256)
    #expect(session.title.count == 64)
}
