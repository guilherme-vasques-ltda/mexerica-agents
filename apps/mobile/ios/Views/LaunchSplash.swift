// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import CodyncUI
import SwiftUI

/// Cold launch: the app-icon bot pops out of the system launch screen and looks around while the
/// computers connect and catch up, then zooms away onto a list that's already current.
/// After Clerk is ready, held at least `shortest` and at most `longest` for computer catch-up.
struct LaunchSplash: View {
    let stores: [BotStore]
    var accountReady = true
    let done: () -> Void
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var shown = false
    @State private var waited = false

    private static let shortest: Duration = .milliseconds(700)
    private static let longest: Duration = .milliseconds(1800)

    private var ready: Bool { accountReady && waited && stores.allSatisfy(\.caughtUp) }

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { context in
            let t = context.date.timeIntervalSinceReferenceDate
            Image("MexericaMascot")
                .resizable()
                .scaledToFit()
                .frame(width: 112, height: 112)
                .offset(y: reduceMotion ? 0 : sin(t * 2.4) * 4)
        }
        .scaleEffect(shown || reduceMotion ? 1 : 0.6)
        .opacity(shown ? 1 : 0)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Palette.background)
        .accessibilityLabel("Conectando")
        .task(id: accountReady) {
            withAnimation(.spring(duration: 0.5, bounce: 0.35)) { shown = true }
            waited = false
            guard accountReady else { return }
            do {
                try await Task.sleep(for: Self.shortest)
                waited = true
                try await Task.sleep(for: Self.longest - Self.shortest)
                done()
            } catch { /* The view left or Clerk's readiness changed. */ }
        }
        .onChange(of: ready, initial: true) { _, ready in if ready { done() } }
        // Leaving: the bot grows a little as everything fades through to the app.
        .transition(reduceMotion ? .opacity : .opacity.combined(with: .scale(scale: 1.15)))
    }
}
