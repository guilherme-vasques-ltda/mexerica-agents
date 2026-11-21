// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// The skill a details sheet shows: an installed one, or one from the catalog.
struct SkillRef: Identifiable {
    let id: String
    let name: String
    let description: String
    /// A catalog skill's folder in anthropics/skills; `nil` for an installed skill.
    var source: String?
    var installed = true

    init(_ skill: InstalledSkill) {
        (id, name, description) = (skill.id, skill.name, skill.description)
    }

    init(_ skill: MarketSkill, installed: Bool) {
        (id, name, description, source, self.installed) = ("catalog/\(skill.source)", skill.name, skill.description, skill.source, installed)
    }

    /// What the host's `skillDetails` looks it up by.
    var request: [String: String] { source.map { ["source": $0] } ?? ["id": id] }
}

/// A skill's details (desktop `SkillSheet`): what it tells the bot, its `SKILL.md`, with Add
/// while it isn't installed.
struct SkillSheet: View {
    let skill: SkillRef
    @Environment(BotStore.self) private var model
    @State private var markdown: String?
    @State private var error: String?
    @State private var adding = false

    private var added: Bool {
        guard let source = skill.source else { return true }
        return skill.installed || model.installedSkills.contains { $0.id == source.lowercased() }
    }

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(skill.name) {
                if added {
                    Image(systemName: "checkmark")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(Palette.secondary)
                        .frame(width: 32)
                        .accessibilityLabel(String(localized: "Adicionada", bundle: .module))
                } else if adding {
                    Spinner()
                } else {
                    IconButton(String(localized: "Adicionar", bundle: .module), systemImage: "plus") { add() }
                }
            }
            CardForm {
                if !skill.description.isEmpty {
                    CardSection {
                        Text(skill.description).font(.subheadline).foregroundStyle(Palette.secondary)
                    }
                }
                CardSection(String(localized: "Instruções", bundle: .module), footer: added ? String(localized: "Os bots leem estas instruções antes de usar a skill.", bundle: .module) : String(localized: "Adicione para permitir que os bots usem estas instruções.", bundle: .module)) {
                    if let markdown {
                        if markdown.isEmpty {
                            Text(String(localized: "Nenhuma instrução.", bundle: .module)).foregroundStyle(Palette.secondary)
                        } else {
                            MarkdownText(markdown)
                        }
                    } else if error == nil {
                        Spinner()
                    }
                }
                if let error {
                    CardSection {
                        Text(error).font(.footnote).foregroundStyle(Palette.danger)
                    }
                }
            }
        }
        .background(Palette.background)
        .task {
            do {
                markdown = try await model.skillDetails(skill)
            } catch {
                self.error = error.localizedDescription
            }
        }
    }

    private func add() {
        guard let source = skill.source else { return }
        adding = true
        error = nil
        Task {
            do {
                try await model.installSkill(source: source)
            } catch {
                self.error = error.localizedDescription
            }
            adding = false
        }
    }
}
