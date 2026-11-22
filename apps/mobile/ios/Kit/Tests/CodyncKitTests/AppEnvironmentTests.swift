// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation
import Testing
@testable import CodyncKit

struct AppEnvironmentTests {
    @Test func developmentWidgetsAndDataStaySeparateFromProduction() {
        #expect(AppEnvironment.main.appGroup == "group.app.mexerica.ios")
        #expect(AppEnvironment.main.link("computers").absoluteString == "donnabot://computers")
        #expect(AppEnvironment.dev.appGroup == "group.app.mexerica.ios.dev")
        #expect(AppEnvironment.dev.appGroup != AppEnvironment.main.appGroup)
        #expect(AppEnvironment.dev.link("computers").absoluteString == "donnabot-dev://computers")
        #expect(AppEnvironment.dev.link("usage").scheme != AppEnvironment.main.link("usage").scheme)
    }
}

@Test func remotePushRequiresFullSigningAndAnExplicitRelay() {
    for signing in ["free", "", "unknown"] {
        #expect(!AppEnvironment.allowsRemotePush(signing: signing, relayURL: "https://push.example.com"))
    }
    for relay in ["", "   ", "http://push.example.com", "https://", "https://user:pass@push.example.com"] {
        #expect(!AppEnvironment.allowsRemotePush(signing: "full", relayURL: relay))
    }
    #expect(AppEnvironment.allowsRemotePush(signing: "full", relayURL: "https://push.example.com"))
    // The package test bundle has no relay or signing configuration: default must be closed.
    #expect(SharedStore.relayURL.isEmpty)
    #expect(!SharedStore.remotePushEnabled)
    #expect(!AppEnvironment.supportsAppleSignIn)
}
