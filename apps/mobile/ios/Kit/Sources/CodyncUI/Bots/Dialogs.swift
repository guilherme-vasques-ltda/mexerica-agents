// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

public extension View {
    /// The one "Delete <bot>?" confirmation, used wherever a bot can be deleted.
    /// Setting `bot` presents it; `onDeleted` runs after the store deletes it.
    func deleteBotConfirmation(_ bot: Binding<Bot?>, onDeleted: @escaping () -> Void = {}) -> some View {
        modifier(DeleteBotConfirmation(bot: bot, onDeleted: onDeleted))
    }

}

private struct DeleteBotConfirmation: ViewModifier {
    @Binding var bot: Bot?
    let onDeleted: () -> Void
    @Environment(BotStore.self) private var model

    func body(content: Content) -> some View {
        content.codyncDialog(
            String(localized: "Excluir \(bot?.name ?? "bot")?", bundle: .module),
            isPresented: Binding(get: { bot != nil }, set: { if !$0 { bot = nil } }),
            message: bot?.isGroup == true ? String(localized: "Os bots e suas próprias conversas continuam.", bundle: .module) : String(localized: "Os arquivos alterados no seu computador permanecem como estão.", bundle: .module)
        ) {
            // Captured now: the dialog clears `bot` before running the action.
            let target = bot
            return [DialogAction(String(localized: "Excluir bot e sua conversa", bundle: .module), destructive: true) {
                if let target { model.delete(target) }
                onDeleted()
            }]
        }
    }
}
