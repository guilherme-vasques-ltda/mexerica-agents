// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation
import Testing
@testable import CodyncKit

@Test func routinesDecodeMixedTriggersAndRunHistory() throws {
    let data = Data(#"{"routines":[{"id":"r","botId":"b","name":"Check","instruction":"Check status","triggers":[{"type":"cron","expression":"20 14 * * 0","timeZone":"Asia/Taipei"},{"type":"webhook"}],"enabled":true,"createdAt":0,"updatedAt":0,"triggerDescriptions":["Every Sunday","When a webhook fires"],"nextRunAt":1000}],"runs":[{"id":"run","routineId":"r","botId":"b","status":"pending","createdAt":0,"event":{"source":"schedule"}}]}"#.utf8)
    let listing = try JSONDecoder().decode(RoutineListing.self, from: data)
    #expect(listing.routines[0].triggers[0].timeZone == "Asia/Taipei")
    #expect(listing.routines[0].triggers[1].type == "webhook")
    #expect(listing.runs[0].isActive)
}

@Test func routineTranscriptLinkSurvivesRoundTrip() throws {
    let data = Data(#"{"text":"Created routine","routineId":"r","runId":"run","style":"info"}"#.utf8)
    let entry = try JSONDecoder().decode(EntryData.self, from: data)
    let restored = try JSONDecoder().decode(EntryData.self, from: JSONEncoder().encode(entry))
    #expect(restored.routineId == "r")
    #expect(restored.runId == "run")
}

@Test func routineScheduleResponseCarriesServerPreviewWithoutClientRules() throws {
    let data = Data(#"{"draft":{"kind":"cron","amount":"1","unit":3600,"calendarStyle":"days","weekday":1,"selectedDays":[1,3,5],"monthDay":1,"minuteStep":15,"hour":23,"minute":47,"at":0,"expression":"47 23 * * 1,3,5","zone":"Asia/Taipei","original":[]},"triggers":[{"type":"cron","expression":"47 23 * * 1,3,5","timeZone":"Asia/Taipei"}],"summary":"Server schedule description","nextRunAt":1790470800000,"warning":null}"#.utf8)
    let preview = try JSONDecoder().decode(RoutineSchedulePreview.self, from: data)
    #expect(preview.summary == "Server schedule description")
    #expect(preview.draft.selectedDays == [1, 3, 5])
    #expect(preview.nextRunAt == 1_790_470_800_000)
    let encoded = try JSONEncoder().encode(preview.draft)
    #expect(try JSONDecoder().decode(RoutineScheduleDraft.self, from: encoded) == preview.draft)
}

@Test func routineDateControlPreservesWireMilliseconds() {
    var draft = RoutineScheduleDraft()
    draft.at = 1_779_000_000_001
    let date = draft.date
    draft.date = date
    #expect(draft.at == 1_779_000_000_001)
}

@Test func routineTimeOfDayRoundTripsHourAndMinute() throws {
    var draft = RoutineScheduleDraft()
    draft.hour = 23
    draft.minute = 47
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = .gmt
    #expect(calendar.dateComponents([.hour, .minute], from: draft.timeOfDay) == DateComponents(hour: 23, minute: 47))
    draft.timeOfDay = try #require(calendar.date(from: DateComponents(year: 2030, month: 3, day: 9, hour: 6, minute: 5)))
    #expect(draft.hour == 6)
    #expect(draft.minute == 5)
}

@Test func routineWebhookDecodesWithAndWithoutTheCloud() throws {
    let cloud = Data(#"{"url":"https://api.codync.dev/v1/hooks/c/r","localUrl":"http://127.0.0.1:19222/hooks/routines/r","key":"k","connected":true}"#.utf8)
    let local = Data(#"{"url":null,"localUrl":"http://127.0.0.1:19222/hooks/routines/r","key":"k","connected":false}"#.utf8)
    #expect(try JSONDecoder().decode(RoutineWebhook.self, from: cloud).url == "https://api.codync.dev/v1/hooks/c/r")
    let off = try JSONDecoder().decode(RoutineWebhook.self, from: local)
    #expect(off.url == nil)
    #expect(off.localUrl.hasSuffix("/hooks/routines/r"))
}

@Test func routineTimeoutReadsAsADuration() {
    let locale = Locale(identifier: "en_US")
    #expect(Routine.timeoutText("3600", locale: locale) == "1 hour")
    #expect(Routine.timeoutText("7200", locale: locale) == "2 hours")
    #expect(Routine.timeoutText("1800", locale: locale) == "30 minutes")
    #expect(Routine.timeoutText("60", locale: locale) == "1 minute")
    #expect(Routine.timeoutText("90", locale: locale) == "90 seconds")
    #expect(Routine.timeoutText("1", locale: locale) == "1 second")
    #expect(Routine.timeoutText("1.5", locale: locale) == "1.5s")
    #expect(Routine.timeoutText("", locale: locale) == "s")
}

@Test func timeZoneSearchIsFuzzyAndRanksWordStartsFirst() {
    let zones = ["America/New_York", "Asia/Taipei", "Asia/Tokyo", "Europe/Berlin", "America/Argentina/Buenos_Aires"]
    #expect(TimeZones.search("taipei", in: zones) == ["Asia/Taipei"])
    #expect(TimeZones.search("TAIPEI", in: zones) == ["Asia/Taipei"])
    #expect(TimeZones.search("new york", in: zones) == ["America/New_York"])
    #expect(TimeZones.search("new_york", in: zones) == ["America/New_York"])
    #expect(TimeZones.search("asia tai", in: zones) == ["Asia/Taipei"])
    #expect(TimeZones.search("buenos-aires", in: zones) == ["America/Argentina/Buenos_Aires"])
    // A name that starts with the query, then word starts, then text inside a word, then letters in order.
    #expect(TimeZones.search("a", in: zones) == ["America/Argentina/Buenos_Aires", "America/New_York", "Asia/Taipei", "Asia/Tokyo"])
    #expect(TimeZones.search("ai", in: zones) == ["America/Argentina/Buenos_Aires", "Asia/Taipei", "America/New_York", "Asia/Tokyo"])
    #expect(TimeZones.search("nyk", in: zones) == ["America/New_York"])
    #expect(TimeZones.search("  ", in: zones) == zones)
    #expect(TimeZones.search("xyz", in: zones).isEmpty)
    #expect(TimeZones.offset("Asia/Taipei") == "GMT+8")
}

@Test func routineRepeatKeepsTheFiveChoicesAndOpensTheRestAsCustom() {
    #expect(RoutineScheduleDraft.repeatChoices.map(\.id) == ["hourly", "daily", "weekdays", "weekly", "custom"])
    var draft = RoutineScheduleDraft()
    for style in ["hourly", "daily", "weekdays", "weekly", "custom"] {
        draft.calendarStyle = style
        #expect(draft.forMobileEditor().calendarStyle == style)
    }
    for style in ["minutes", "days", "monthly"] {
        draft.calendarStyle = style
        #expect(draft.forMobileEditor().calendarStyle == "custom")
    }
    for (kind, shown) in [("cron", "cron"), ("webhook", "webhook"), ("interval", "keep"), ("once", "keep")] {
        draft.kind = kind
        #expect(draft.forMobileEditor().kind == shown)
    }
}

@Test func customStartsFromTheCronTheHostCompiled() {
    var draft = RoutineScheduleDraft()
    draft.calendarStyle = "weekdays"
    draft.expression = "0 8 * * *"
    #expect(draft.repeating("custom", compiled: "0 9 * * 1-5").expression == "0 9 * * 1-5")
    #expect(draft.repeating("custom", compiled: nil).expression == "")
    #expect(draft.repeating("daily", compiled: "0 9 * * 1-5").expression == "0 8 * * *")
    #expect(draft.repeating("weekdays", compiled: "x") == draft)
    let cron = RoutineTrigger(type: "cron", expression: "0 9 * * 1-5", timeZone: "UTC")
    #expect(RoutineSchedulePreview(draft: draft, triggers: [cron], summary: "").cronExpression == "0 9 * * 1-5")
    #expect(RoutineSchedulePreview(draft: draft, triggers: [RoutineTrigger(type: "webhook")], summary: "").cronExpression == nil)
}

// MARK: Hub #980/#981: agenda, gatilhos intactos e recusas

private func routineJSON(_ extra: String = "", triggers: String = #"[{"type":"cron","expression":"05 9 * * *","timeZone":"UTC"}]"#) -> Data {
    Data(#"{"id":"r","botId":"b","name":"Check","instruction":"Check","triggers":\#(triggers),"enabled":true,"createdAt":0,"updatedAt":0,"triggerDescriptions":["Every day at 9:05"]\#(extra)}"#.utf8)
}

private let pt = Locale(identifier: "pt_BR")
private let en = Locale(identifier: "en_US")

@Test func routineAgendaIsDecodedAndShownInPortugueseOnly() throws {
    let routine = try JSONDecoder().decode(Routine.self, from: routineJSON(#","agenda":"Todo dia às 9h e às 15h""#))
    #expect(routine.agenda == "Todo dia às 9h e às 15h")
    #expect(routine.scheduleText(locale: pt) == "Todo dia às 9h e às 15h")
    // In English the host's own descriptions stay: the Hub writes `agenda` in Portuguese only.
    #expect(routine.scheduleText(locale: en) == "Every day at 9:05")
}

@Test func routineWithoutAgendaFromAnOlderHubKeepsTheDescriptions() throws {
    let routine = try JSONDecoder().decode(Routine.self, from: routineJSON())
    #expect(routine.agenda == nil)
    #expect(routine.scheduleText(locale: pt) == "Every day at 9:05")
    for odd in [#""""#, #""   ""#, #""linha\nquebrada""#, "\"" + String(repeating: "x", count: 301) + "\"", "null"] {
        let r = try JSONDecoder().decode(Routine.self, from: routineJSON(",\"agenda\":\(odd)"))
        #expect(r.scheduleText(locale: pt) == "Every day at 9:05")
    }
}

@Test func schedulePreviewShowsResumoWhenThereIsOne() throws {
    let draft = String(decoding: try JSONEncoder().encode(RoutineScheduleDraft()), as: UTF8.self)
    let base = #"{"draft":\#(draft),"triggers":[],"summary":"Every day at 9:00""#
    let with = try JSONDecoder().decode(RoutineSchedulePreview.self, from: Data((base + #","resumo":"Todo dia às 9h"}"#).utf8))
    #expect(with.resumo == "Todo dia às 9h")
    #expect(with.summaryText(locale: pt) == "Todo dia às 9h")
    #expect(with.summaryText(locale: en) == "Every day at 9:00")
    let without = try JSONDecoder().decode(RoutineSchedulePreview.self, from: Data((base + "}").utf8))
    #expect(without.summaryText(locale: pt) == "Every day at 9:00")
}

@Test func savingWithoutEditingTheScheduleSendsTheOriginalTriggers() throws {
    let routine = try JSONDecoder().decode(Routine.self, from: routineJSON())
    var opened = RoutineScheduleDraft()
    opened.expression = "5 9 * * *"
    let loaded = opened.forMobileEditor()
    // Same draft: the routine's own triggers go back, not what the form would compile.
    #expect(RoutineSavePlan.make(current: loaded, loaded: loaded, loadedTriggers: routine.triggers) == .triggers(routine.triggers))
    // Edited: the form values.
    var edited = loaded
    edited.hour = 10
    #expect(RoutineSavePlan.make(current: edited, loaded: loaded, loadedTriggers: routine.triggers) == .draft(edited))
    // Back to the initial state: intact again.
    edited.hour = loaded.hour
    #expect(RoutineSavePlan.make(current: edited, loaded: loaded, loadedTriggers: routine.triggers) == .triggers(routine.triggers))
    // New routine, no triggers, or a schedule that never loaded: always the form.
    #expect(RoutineSavePlan.make(current: loaded, loaded: loaded, loadedTriggers: []) == .draft(loaded))
    #expect(RoutineSavePlan.make(current: loaded, loaded: nil, loadedTriggers: routine.triggers) == .draft(loaded))
}

/// Keeps the last call's JSON body and answers with `result`.
private final class CapturingTransport: HostTransport, @unchecked Sendable {
    private let lock = NSLock()
    private var last: [String: Any] = [:]
    private let result: Result<Data, Error>
    init(_ result: Result<Data, Error>) { self.result = result }
    var body: [String: Any] { lock.withLock { last } }
    func call(_ method: String, body: Data, timeout: TimeInterval) async throws -> Data {
        lock.withLock { last = (try? JSONSerialization.jsonObject(with: body) as? [String: Any]) ?? [:] }
        return try result.get()
    }
    func stream(_ request: HostStreamRequest) -> AsyncThrowingStream<Data, Error> { AsyncThrowingStream { $0.finish() } }
    func states() -> AsyncStream<LinkState> { AsyncStream { $0.finish() } }
}

private let savedRoutine = Data(("{\"routine\":" + String(decoding: routineJSON(), as: UTF8.self) + "}").utf8)

@Test func renamingSendsTheTriggersByteForByteEvenWhenNotCanonical() async throws {
    for expression in ["05 9 * * *", "0  9,15 * * *", "0 09 * * 01"] {
        let json = routineJSON(triggers: #"[{"type":"cron","expression":"\#(expression)","timeZone":"UTC"},{"type":"webhook"}]"#)
        let routine = try JSONDecoder().decode(Routine.self, from: json)
        let transport = CapturingTransport(.success(savedRoutine))
        _ = try await HostClient(transport: transport).saveRoutine(botId: "b", id: routine.id, name: "Novo nome",
            instruction: routine.instruction, triggers: routine.triggers, timeoutSeconds: "3600")
        let sent = try #require(transport.body["triggers"] as? [[String: Any]])
        #expect(sent.count == 2)
        #expect(sent[0]["expression"] as? String == expression)
        #expect(sent[0]["timeZone"] as? String == "UTC")
        #expect(sent[1]["type"] as? String == "webhook")
        #expect(transport.body["schedule"] == nil)
        #expect(transport.body["name"] as? String == "Novo nome")
    }
}

@Test func anInvalidTimeoutGoesAsTypedAndIsNeverReplacedByADefault() async throws {
    let routine = try JSONDecoder().decode(Routine.self, from: routineJSON())
    for typed in ["abc", "0", "", " 90"] {
        let transport = CapturingTransport(.success(savedRoutine))
        _ = try await HostClient(transport: transport).saveRoutine(botId: "b", id: "r", name: "n", instruction: "i",
            triggers: routine.triggers, timeoutSeconds: typed)
        #expect(transport.body["timeoutSeconds"] as? String == typed)
    }
}

private func refusal(_ raw: String?) async -> String {
    let transport = CapturingTransport(.failure(HostError.refused(HubRefusal(raw))))
    do {
        _ = try await HostClient(transport: transport).saveRoutine(botId: "b", id: "r", name: "n", instruction: "i",
            triggers: [], timeoutSeconds: "3600")
        return "no error"
    } catch { return error.localizedDescription }
}

@Test func knownHubRefusalsBecomeLocalPhrases() async {
    let duplicate = await refusal("Dois horários iguais: 15:00. Troque um deles ou tire um.")
    #expect(duplicate.contains("15:00") && duplicate != RoutineRefusal.generic)
    // A label that is not a time is not repeated back.
    let cron = await refusal("Dois horários iguais: 0 9,15 * * 1-5. Troque um deles ou tire um.")
    #expect(!cron.contains("9,15") && cron != RoutineRefusal.generic)
    let many = await refusal("A rotina comporta até 20 gatilhos; esta tem 23. Tire alguns ou divida em outra rotina.")
    #expect(many.contains("23") && many != RoutineRefusal.generic)
    for known in ["A rotina precisa de pelo menos um gatilho.",
                  "timeoutSeconds must be an integer between 1 and 86400",
                  "routine name or instruction is empty or too long"] {
        let shown = await refusal(known)
        #expect(shown != RoutineRefusal.generic)
        // The English wording of the Hub is never what shows.
        #expect(shown != known || known == "A rotina precisa de pelo menos um gatilho.")
    }
}

@Test func absentEmptyUnknownOrInternalHubTextGetsTheGenericPhraseWithoutLeaking() async {
    let detalhe = "starting `npx x`: No such file or directory (os error 2) at /Users/user/.env token=sk-123 192.0.2.4"
    for raw in [nil, "", "   ", "unknown bot", detalhe,
                "Dois horários iguais: 15:00. Troque um deles ou tire um. " + detalhe,
                "contexto: Dois horários iguais: 15:00. Troque um deles ou tire um.",
                "A rotina comporta até 20 gatilhos; esta tem 23. Tire alguns ou divida em outra rotina. mais",
                String(repeating: "x", count: 5000)] {
        let shown = await refusal(raw)
        #expect(shown == RoutineRefusal.generic)
        #expect(!shown.contains("npx") && !shown.contains("/Users") && !shown.contains("sk-") && !shown.contains("192."))
    }
}

@Test func routineCallsKeepStatus401And403Fixed() async {
    for (status, expected) in [(401, "Este iPhone não tem mais acesso a este computador."), (403, "O computador recusou este pedido.")] {
        let transport = CapturingTransport(.failure(HostError.http(status, ChannelTransport.httpMessage(status))))
        do {
            _ = try await HostClient(transport: transport).saveRoutine(botId: "b", id: "r", name: "n", instruction: "i",
                triggers: [], timeoutSeconds: "3600")
            Issue.record("expected an error")
        } catch { #expect(error.localizedDescription == expected) }
    }
}
