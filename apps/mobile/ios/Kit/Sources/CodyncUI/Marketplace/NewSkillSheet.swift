// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

struct NewSkillSheet: View {
    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss
    @State private var name = ""
    @State private var summary = ""
    @State private var instructions = ""
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Nova skill", bundle: .module)) {
                if saving {
                    Spinner()
                } else {
                    IconButton(String(localized: "Salvar", bundle: .module), systemImage: "checkmark") {
                        saving = true
                        Task {
                            do {
                                try await model.addSkill(name: name, description: summary, instructions: instructions)
                                dismiss()
                            } catch {
                                self.error = error.localizedDescription
                            }
                            saving = false
                        }
                    }
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || instructions.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            form
        }
        .background(Palette.background)
    }

    private var form: some View {
        CardForm {
            CardSection(footer: String(localized: "O bot vê o nome e quando usá-la e lê as instruções apenas quando uma tarefa corresponde.", bundle: .module)) {
                TextField(String(localized: "Nome", bundle: .module), text: $name)
                TextField(String(localized: "Quando usar", bundle: .module), text: $summary, axis: .vertical).lineLimit(2...4)
            }
            CardSection(String(localized: "Instruções", bundle: .module)) {
                TextField(String(localized: "Passo a passo, em palavras simples…", bundle: .module), text: $instructions, axis: .vertical)
                    .lineLimit(6...20)
            }
            if let error {
                CardSection { Text(error).foregroundStyle(Palette.danger) }
            }
        }
        .textFieldStyle(.plain)
    }
}
