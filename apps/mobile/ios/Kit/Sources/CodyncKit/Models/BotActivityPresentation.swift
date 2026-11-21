// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

/// Presentation only; the ActivityKit payload and host push contract stay unchanged.
public struct BotActivityPresentation: Hashable, Sendable {
    public enum Phase: String, CaseIterable, Sendable {
        /// `sending` and `queued` are this phone's own (a message on its way, or waiting in the
        /// relay mailbox for an offline computer); the computer only pushes the others.
        case sending, queued, working, needsInput, completed, failed, stale, waiting
    }

    public let phase: Phase
    public let activity: String

    public init(status: String, activity: String, isStale: Bool = false) {
        let phase: Phase = switch status {
        case "sending": .sending
        case "queued": .queued
        case "working": .working
        case "needsInput": .needsInput
        case "idle", "completed": .completed
        case "error": .failed
        default: .waiting
        }
        self.phase = isStale && (phase == .working || phase == .needsInput || phase == .sending) ? .stale : phase
        self.activity = activity
    }

    public var title: String {
        switch phase {
        case .sending: String(localized: "Enviando", bundle: .module)
        case .queued: String(localized: "Aguardando seu computador", bundle: .module)
        case .working: String(localized: "Trabalhando", bundle: .module)
        case .needsInput: String(localized: "Precisa de você", bundle: .module)
        case .completed: String(localized: "Concluído", bundle: .module)
        case .failed: String(localized: "Falhou", bundle: .module)
        case .stale: String(localized: "Atualização atrasada", bundle: .module)
        case .waiting: String(localized: "Aguardando atualização", bundle: .module)
        }
    }

    /// The one line of text beside the orb: the bot's current step while it's live, else the state.
    public var caption: String {
        switch phase {
        case .working, .needsInput: activity.isEmpty ? title : activity
        case .sending, .queued, .completed, .failed, .stale, .waiting: title
        }
    }

    public var symbol: String {
        switch phase {
        case .completed: "checkmark"
        case .failed: "exclamationmark.triangle.fill"
        case .queued: "desktopcomputer"
        case .sending, .working, .needsInput, .stale, .waiting: "ellipsis"
        }
    }
}
