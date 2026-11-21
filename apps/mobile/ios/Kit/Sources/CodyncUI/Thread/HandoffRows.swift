// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// The bot's chat starts over: what the new conversation carries, which the user can edit, or turn down to carry on.
struct HandoffCardRow: View {
    let entry: Entry
    let card: HandoffCard
    @Environment(BotStore.self) private var model
    @State private var expanded = false
    @State private var editing = false

    var body: some View {
        if card.isDraft {
            VStack(alignment: .leading, spacing: 10) {
                Label(entry.data.text ?? "", systemImage: "arrow.triangle.branch")
                    .font(.footnote)
                    .foregroundStyle(Palette.secondary)
                Disclosure(isExpanded: $expanded) {
                    carried
                } label: {
                    Text(String(localized: "O que leva", bundle: .module)).font(.footnote.weight(.medium)).foregroundStyle(Palette.text)
                }
                HStack(spacing: 8) {
                    if model.answering[entry.id] != nil {
                        Spinner()
                    } else {
                        Button(String(localized: "Manter esta conversa", bundle: .module)) { model.keepConversation(entry) }
                            .buttonStyle(SecondaryButtonStyle())
                        IconButton(String(localized: "Editar o que leva", bundle: .module), systemImage: "pencil") {
                            withAnimation(Motion.layout) { editing = true }
                        }
                    }
                }
            }
            .padding(12)
            .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: 14))
            .padding(.trailing, 40)
            .codyncSheet(isPresented: $editing) { HandoffEditorView(entry: entry, text: card.text ?? "") }
        } else {
            Disclosure(isExpanded: $expanded) {
                carried
            } label: {
                Label(entry.data.text ?? "", systemImage: "arrow.triangle.branch")
                    .font(.footnote)
                    .foregroundStyle(Palette.secondary)
            }
        }
    }

    private var carried: some View {
        Text(card.text ?? "")
            .font(.footnote.monospaced())
            .foregroundStyle(Palette.secondary)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Edits what the next conversation starts with.
struct HandoffEditorView: View {
    let entry: Entry
    @State private var text: String
    @State private var saving = false
    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss

    init(entry: Entry, text: String) {
        self.entry = entry
        _text = State(initialValue: text)
    }

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Editar transferência", bundle: .module)) {
                if saving {
                    Spinner()
                } else {
                    IconButton(String(localized: "Salvar", bundle: .module), systemImage: "checkmark") {
                        saving = true
                        // The link of this tap, not of whenever the task runs.
                        let marca = model.marcaDoLink
                        Task {
                            if await model.editHandoff(entry, text: trimmed, marca: marca) { dismiss() }
                            saving = false
                        }
                    }
                    .disabled(trimmed.isEmpty)
                }
            }
            CardForm {
                CardSection(footer: String(localized: "Sua próxima mensagem inicia uma nova conversa com estas informações.", bundle: .module)) {
                    TextField(String(localized: "O que a nova conversa deve saber", bundle: .module), text: $text, axis: .vertical)
                        .lineLimit(10...24)
                        .font(.footnote.monospaced())
                }
            }
            .textFieldStyle(.plain)
        }
        .background(Palette.background)
    }

    private var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }
}
