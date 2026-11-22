// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation
import Testing
@testable import CodyncKit

struct ScreenAccessTests {
    private func bundle(_ language: String) throws -> Bundle {
        let path = try #require(RelativeTime.resourceBundle.path(forResource: language, ofType: "lproj"))
        return try #require(Bundle(path: path))
    }

    @Test func oldHostScreenStatusStillDecodes() throws {
        let state = try JSONDecoder().decode(ScreenState.self, from: Data("{\"enabled\":true}".utf8))
        #expect(state.permissionApp == nil)
        #expect(!state.available)
        let english = try bundle("en")
        let portuguese = try bundle("pt-BR")
        #expect(state.accessGuidance(bundle: english).contains("Settings → Computer access"))
        #expect(state.accessGuidance(bundle: portuguese).contains("Ajustes → Acesso ao computador"))
    }

    @Test func guidanceUsesTheActualPermissionAppAndPlatform() throws {
        let english = try bundle("en")
        var state = ScreenState()
        state.enabled = true
        state.connected = true
        state.permissionApp = "CodyncDevScreen"
        #expect(state.accessGuidance(bundle: english).contains("CodyncDevScreen"))
        state.capture = true
        #expect(state.available)
        #expect(!state.controlReady)
        #expect(state.accessGuidance(bundle: english).contains("Device Control and Data Access"))
        state.platform = "linux"
        #expect(state.accessGuidance(bundle: english).contains("system sharing dialog"))
        state.platform = "windows"
        #expect(state.accessGuidance(bundle: english).contains("not available on Windows"))
        state.computerUse = true
        #expect(state.controlReady)
    }
}

@Test func compactDurationsAndBotCountsUseEachCatalogLanguage() throws {
    func bundle(_ language: String) throws -> Bundle {
        let path = try #require(RelativeTime.resourceBundle.path(forResource: language, ofType: "lproj"))
        return try #require(Bundle(path: path))
    }
    let now = Date(timeIntervalSince1970: 1_700_000_000)
    for language in ["pt-BR", "en"] {
        let localized = try bundle(language)
        #expect(RelativeTime.short(now.addingTimeInterval(-600), now: now, bundle: localized) == "10m")
        #expect(RelativeTime.short(now.addingTimeInterval(-172_800), now: now, bundle: localized) == "2d")
        #expect(RelativeTime.until(now.addingTimeInterval(5_700), now: now, bundle: localized) == "1h 35m")
        #expect(RelativeTime.until(now.addingTimeInterval(172_800), now: now, bundle: localized) == "2d")
        #expect(String(localized: "\(1) bot", bundle: localized) == "1 bot")
        #expect(String(localized: "\(2) bot", bundle: localized) == "2 bots")
        #expect(String(localized: "bots (rótulo plural)", bundle: localized) == "bots")
        let expected = language == "pt-BR" ? "2 precisam de você" : "2 need you"
        #expect(String(localized: "\(2) precisa de você", bundle: localized) == expected)
    }
}
