// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// A new bot, profile first (Grok Bot's Create New Bot): its avatar and name. Everything else
/// starts on the usual defaults and is changed afterwards in the bot's Settings.
public struct NewBotView: View {
    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss
    @State private var draft = BotDraft()
    @State private var saving = false
    @State private var error: String?
    private let computers: [(id: ComputerID, label: String)]
    @Binding private var computer: ComputerID

    /// `computers` can take the bot; the caller swaps the `BotStore` to match `computer`.
    public init(computers: [(id: ComputerID, label: String)], computer: Binding<ComputerID>) {
        self.computers = computers
        _computer = computer
    }

    public var body: some View {
        CreateForm(title: String(localized: "Novo bot", bundle: .module), placeholder: String(localized: "Dê um nome ao seu bot", bundle: .module), name: $draft.name, saving: saving,
                   canCreate: !saving && model.connection == .online, error: error, create: save) {
            // The new bot starts as the default mascot; its avatar is set on the Mac (the line below).
            MexeriquinhaView(avatar: .padrao, size: 96)
        } content: {
            Text("Edite o avatar no app do Mac", bundle: .module)
                .font(.caption).foregroundStyle(Palette.secondary)
            if computers.count > 1 || model.connection != .online {
                VStack(alignment: .leading, spacing: 6) {
                    if computers.count > 1 {
                        ValueRow(String(localized: "Computador", bundle: .module)) {
                            ChoicePicker(selection: $computer, options: computers, fitsAvailableWidth: true)
                                .accessibilityLabel(String(localized: "Computador", bundle: .module))
                        }
                    }
                    if model.connection != .online {
                        Text(computers.count > 1
                            ? String(localized: "Conecte este computador para criar o bot ou escolha outro computador.", bundle: .module)
                            : String(localized: "Conecte este computador para criar o bot.", bundle: .module))
                            .font(.caption).foregroundStyle(Palette.warning)
                    }
                }
            }
        }
    }

    private func save() {
        guard !saving, model.connection == .online else { return }
        saving = true
        error = nil
        var draft = draft
        draft.name = draft.name.trimmingCharacters(in: .whitespacesAndNewlines)
        // No engine picker here: the Hub resolves D42 from its current catalog.
        // Connectors/accounts stay unset (off); skills keep the Hub's defaults.
        draft.backend = ""
        Task {
            do {
                let bot = try await model.save(draft)
                dismiss()
                model.selection = bot.id
            } catch {
                withAnimation(Motion.fade) { self.error = error.localizedDescription }
            }
            saving = false
        }
    }
}

/// A new group chat, laid out like a new bot: who's in it and its name. What it works on is set
/// afterwards in the group editor.
public struct NewGroupView: View {
    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss
    @State private var name = ""
    @State private var members: [String] = []
    @State private var saving = false
    @State private var error: String?

    public init() {}

    private var picked: [Bot] { members.compactMap { model.bots[$0] } }
    /// "Alice, Bob" when no name is typed.
    private var defaultName: String { picked.map(\.name).joined(separator: ", ") }

    public var body: some View {
        CreateForm(title: String(localized: "Nova conversa em grupo", bundle: .module), placeholder: picked.isEmpty ? String(localized: "Dê um nome ao grupo", bundle: .module) : defaultName, name: $name,
                   saving: saving, canCreate: !members.isEmpty && !saving, error: error, create: save) {
            GroupAvatar(members: picked, size: 96)
                .animation(Motion.layout, value: members)
        } content: {
            let bots = model.roster.filter { !$0.isGroup }
            VStack(spacing: 14) {
                if bots.isEmpty {
                    Text(String(localized: "Nenhum bot ainda. Crie um bot primeiro.", bundle: .module)).font(.subheadline).foregroundStyle(Palette.secondary)
                } else {
                    WrapLayout(spacing: 8) {
                        ForEach(bots) { tile($0) }
                    }
                }
                Text(String(localized: "Todos respondem por vez, a menos que você mencione alguém com @.", bundle: .module))
                    .font(.caption)
                    .foregroundStyle(Palette.tertiary)
                    .multilineTextAlignment(.center)
            }
        }
    }

    /// A bot to pick: its avatar, ringed once picked, with its name under it.
    private func tile(_ bot: Bot) -> some View {
        let chosen = members.contains(bot.id)
        return Button { toggle(bot.id) } label: {
            VStack(spacing: 4) {
                CharacterAvatar(bot: bot, size: 44, animated: false).choiceRing(chosen)
                Text(bot.name)
                    .font(.caption)
                    .foregroundStyle(chosen ? Palette.text : Palette.secondary)
                    .lineLimit(1)
            }
            .frame(width: 64)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(bot.name)
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }

    private func toggle(_ id: String) {
        withAnimation(Motion.layout) {
            if members.contains(id) { members.removeAll { $0 == id } } else { members.append(id) }
        }
    }

    private func save() {
        guard !members.isEmpty, !saving else { return }
        saving = true
        error = nil
        let typed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let name = typed.isEmpty ? defaultName : typed
        Task {
            do {
                _ = try await model.createGroup(name: name, members: members)
                dismiss()
            } catch {
                withAnimation(Motion.fade) { self.error = error.localizedDescription }
            }
            saving = false
        }
    }
}

/// The create dialogs' layout: a large preview, a centered name field (Return creates), the
/// pickers, then any save error.
private struct CreateForm<Preview: View, Content: View>: View {
    let title: String
    let placeholder: String
    @Binding var name: String
    let saving: Bool
    let canCreate: Bool
    let error: String?
    let create: () -> Void
    @ViewBuilder let preview: Preview
    @ViewBuilder let content: Content
    @FocusState private var naming: Bool

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(title) {
                if saving {
                    Spinner()
                } else {
                    IconButton(String(localized: "Criar", bundle: .module), systemImage: "checkmark", action: create)
                        .disabled(!canCreate)
                }
            }
            ScrollView {
                VStack(spacing: 28) {
                    VStack(spacing: 16) {
                        preview
                        TextField(placeholder, text: $name)
                            .textFieldStyle(.plain)
                            .font(.title2.weight(.semibold))
                            .foregroundStyle(Palette.text)
                            .multilineTextAlignment(.center)
                            .submitLabel(.done)
                            .focused($naming)
                            .onSubmit(create)
                    }
                    content
                    if let error {
                        Text(error)
                            .font(.footnote)
                            .foregroundStyle(Palette.danger)
                            .multilineTextAlignment(.center)
                            .transition(.opacity)
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.horizontal, 20)
                .padding(.top, 8)
                .padding(.bottom, 24)
            }
            .scrollDismissesKeyboard(.interactively)
        }
        .background(Palette.background)
        .onAppear { naming = true }
    }
}
