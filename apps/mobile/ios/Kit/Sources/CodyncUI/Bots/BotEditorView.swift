// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

public struct EditorRequest: Identifiable {
    public let id = UUID()
    public var draft: BotDraft

    public init(_ draft: BotDraft) { self.draft = draft }
}

/// Edit a bot in a modal (iPhone): the settings form plus Close / Save.
public struct BotEditorView: View {
    @Environment(BotStore.self) private var model
    @Environment(\.dismissModal) private var dismiss
    @State var draft: BotDraft
    @State private var saving = false
    @State private var error: String?

    public init(draft: BotDraft) {
        _draft = State(initialValue: draft)
    }

    public var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Ajustes", bundle: .module)) {
                if saving {
                    Spinner()
                } else {
                    IconButton(String(localized: "Salvar", bundle: .module), systemImage: "checkmark") { save() }
                        .disabled(!draft.isValid || saving || model.connection != .online)
                }
            }
            BotSettingsForm(draft: $draft, error: error)
        }
        .background(Palette.background)
        // A change made elsewhere (a bot that manages the team, another device) replaces each field not edited here.
        .onChange(of: draft.id.flatMap { model.bots[$0] }.map(BotDraft.init)) { base, fresh in
            if let base, let fresh { draft = draft.rebased(from: base, onto: fresh) }
        }
    }

    private func save() {
        guard draft.isValid, !saving, model.connection == .online else { return }
        saving = true
        error = nil
        Task {
            do {
                _ = try await model.save(draft.normalized)
                dismiss()
            } catch {
                self.error = error.localizedDescription
            }
            saving = false
        }
    }
}

/// Which settings a form shows: all of them (the editor), what changes often (instructions, model,
/// fast mode: the details sheet's Details page) or the rest (its Settings page).
enum FormPart {
    case all, often, rarely
}

/// Bot settings, in sections in the Settings style (`CardSection`). `scrolls` is off where a
/// surrounding view scrolls (the details sheet).
struct BotSettingsForm: View {
    @Binding var draft: BotDraft
    let error: String?
    var scrolls = true
    var part: FormPart = .all
    @Environment(BotStore.self) private var model
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.locale) private var locale
    @State private var pickingFolder = false
    @State private var viewingSkill: SkillRef?

    var body: some View {
        Group {
            if scrolls {
                ScrollView { sections.padding(20) }
                    .scrollDismissesKeyboard(.interactively)
            } else {
                sections
            }
        }
        .font(.body)
        .background(Palette.background)
        .task { await model.refreshPlugins() }
        .codyncSheet(item: $viewingSkill) { SkillSheet(skill: $0) }
        .codyncSheet(isPresented: $pickingFolder) {
            FolderPicker(start: draft.cwd.isEmpty ? model.hello?.home : draft.cwd) {
                draft.cwd = $0
                pickingFolder = false
            }
        }
    }

    /// What changes often: on the details sheet's Details page.
    private var oftenSections: some View {
        VStack(alignment: .leading, spacing: 30) {
            CardSection(String(localized: "Instruções", bundle: .module)) { instructions }
            CardSection(String(localized: "Motor", bundle: .module)) {
                agentPicker
                modelPicker
            }
            if let error {
                Text(error).font(.footnote).foregroundStyle(Palette.danger)
            }
        }
    }

    private var instructions: some View {
        Field(String(localized: "Instruções permanentes", bundle: .module), detail: String(localized: "Regras que sempre se aplicam. Faça pedidos específicos de tarefas na conversa.", bundle: .module)) {
            TextField(String(localized: "Ex.: revisa PRs. Nunca faz push sem perguntar.", bundle: .module), text: $draft.description, axis: .vertical)
                .lineLimit(3...8)
                .fieldBox()
        }
    }

    private var agentPicker: some View {
        VStack(alignment: .leading, spacing: 8) {
            ValueRow(String(localized: "Motor", bundle: .module)) {
                ChoicePicker(selection: $draft.backend,
                             options: (model.hello?.backends ?? []).map { ($0.id, $0.available ? $0.name : String(localized: "\($0.name) (não instalado)", bundle: .module)) } + [("custom", String(localized: "Comando personalizado", bundle: .module))],
                             image: { id in AgentIcon.image(registry: model.hello?.backends.first { $0.id == id }?.registry ?? id) })
            }
            if let b = model.hello?.backends.first(where: { $0.id == draft.backend }), !b.available {
                Text(b.installHint).font(.caption).foregroundStyle(Palette.warning)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            if draft.backend == "custom" {
                TextField(String(localized: "Comando ACP, ex.: meu-motor --acp", bundle: .module), text: Binding(get: { draft.command ?? "" }, set: { draft.command = $0.isEmpty ? nil : $0 }))
                    .font(.callout.monospaced())
                    .plainTextInput()
                    .fieldBox()
            }
        }
    }

    private var modelPicker: some View {
        AgentModelPicker(backend: draft.backend, selection: $draft.model, options: $draft.modelOptions,
                         fastMode: Binding(get: { draft.fastMode ?? false }, set: { on in
                             withAnimation(Motion.reduced(Motion.fade, reduceMotion)) { draft.fastMode = on }
                         }), bot: draft.id.flatMap { model.bots[$0] })
    }

    @ViewBuilder private var sections: some View {
        if part == .often { oftenSections } else { allSections }
    }

    private var allSections: some View {
        VStack(alignment: .leading, spacing: 30) {
            CardSection(String(localized: "Perfil", bundle: .module)) {
                VStack(alignment: .leading, spacing: 12) {
                    ValueRow(String(localized: "Avatar", bundle: .module)) {
                        // The bot's own Mexeriquinha (the dotted shape in the draft is only the host's rough copy).
                        Group {
                            if let bot = draft.id.flatMap({ model.bots[$0] }) { CharacterAvatar(bot: bot, size: 28, animated: false) }
                            else { MexeriquinhaView(avatar: .padrao, size: 28) }
                        }
                            .padding(5)
                            .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    }
                    Text("Edite o avatar no app do Mac", bundle: .module)
                        .font(.caption).foregroundStyle(Palette.secondary)
                }
                Field(String(localized: "Nome", bundle: .module)) {
                    TextField(String(localized: "Nome", bundle: .module), text: $draft.name).fieldBox()
                }
                if part == .all { instructions }
                if let bot = draft.id.flatMap({ model.bots[$0] }), let creator = bot.createdBy {
                    ValueRow(String(localized: "Criado por", bundle: .module), value: "\(model.bots[creator]?.name ?? String(localized: "Um bot", bundle: .module)) · \(Date(milliseconds: bot.createdAt).formatted(.dateTime.day().month(.abbreviated).year().locale(locale)))")
                }
            }

            // On the details sheet the agent and its model are on the Details page; what's left is where it works.
            CardSection(part == .all ? String(localized: "Motor", bundle: .module) : String(localized: "Espaço de trabalho", bundle: .module)) {
                ValueRow(String(localized: "Computador", bundle: .module)) {
                    Text(model.hostName).lineLimit(1)
                }
                if part == .all {
                    agentPicker
                    modelPicker
                }
                ValueRow(String(localized: "Espaço de trabalho", bundle: .module), detail: draft.cwd.isEmpty
                         ? String(localized: "Este bot tem seu próprio espaço para arquivos. Ele pode trabalhar em outras pastas quando você pedir.", bundle: .module)
                         : String(localized: "Este projeto é a pasta inicial padrão. O bot pode trabalhar em outro lugar quando você pedir.", bundle: .module)) {
                    DropdownMenu {
                        [
                            MenuItem(String(localized: "Espaço de trabalho pessoal", bundle: .module), selected: draft.cwd.isEmpty) {
                                withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { draft.cwd = "" }
                            },
                            MenuItem(String(localized: "Escolher pasta do projeto…", bundle: .module), selected: !draft.cwd.isEmpty) {
                                withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { pickingFolder = true }
                            },
                        ]
                    } label: {
                        HStack(spacing: 6) {
                            Text(draft.cwd.isEmpty ? String(localized: "Pessoal", bundle: .module) : (draft.cwd as NSString).lastPathComponent).lineLimit(1)
                            Image(systemName: "chevron.down").font(.caption2.weight(.semibold)).foregroundStyle(Palette.secondary)
                        }
                        .foregroundStyle(Palette.text)
                        .outlinedPill()
                    }
                    .help(draft.cwd.isEmpty ? String(localized: "Um espaço de trabalho permanente reservado para este bot", bundle: .module) : draft.cwd)
                }
                ValueRow("Autonomia", detail: draft.permission == "auto"
                         ? String(localized: "Pedidos de ferramentas são aprovados automaticamente. As configurações do próprio motor, como as regras de permissão do Claude Code, continuam valendo.", bundle: .module)
                         : String(localized: "Você receberá um cartão de aprovação e uma notificação sempre que o bot pedir.", bundle: .module)) {
                    ChoicePicker(selection: Binding(get: { draft.permission }, set: {
                        draft.permission = $0
                        draft.permissionChosenByUser = true
                    }), options: [("ask", "Pedir"), ("auto", "Liberar")])
                }
            }

            CardSection(String(localized: "Atividade", bundle: .module)) {
                SwitchRow(String(localized: "Notificações", bundle: .module), detail: String(localized: "Receba uma notificação quando este bot terminar ou precisar de você", bundle: .module),
                          isOn: Binding(get: { draft.notify ?? true }, set: { draft.notify = $0 }))
                if model.screen != nil, part == .all {
                    SwitchRow(String(localized: "Usar o computador", bundle: .module), detail: model.screen?.controlReady != true
                        ? String(localized: "Permita que este bot use aplicativos. Nesse computador, abra Mexerica → Ajustes → Acesso ao computador para configurar as permissões primeiro.", bundle: .module)
                        : model.screen?.enabled == true
                        ? String(localized: "Permita que este bot veja a tela e use o mouse e o teclado. Você pode acompanhar e assumir o controle pelo celular.", bundle: .module)
                        : model.screen?.computerUse == true
                        ? String(localized: "Permita que este bot veja a tela e use o mouse e o teclado.", bundle: .module)
                        : String(localized: "Permita que este bot veja a tela e use o mouse e o teclado. Nesse computador, abra Mexerica → Ajustes → Acesso ao computador para configurar as permissões primeiro.", bundle: .module),
                              isOn: Binding(get: { draft.computer ?? false }, set: { draft.computer = $0 }))
                }
                // Only from a host that has it (its bots say).
                if model.bots.values.contains(where: { $0.manageTeam != nil }) {
                    SwitchRow(String(localized: "Gerenciar bots", bundle: .module), detail: String(localized: "Permita que este bot crie bots e altere as configurações de qualquer bot, incluindo as próprias. Os bots criados por ele também poderão fazer isso.", bundle: .module),
                              isOn: Binding(get: { draft.manageTeam ?? false }, set: { draft.manageTeam = $0 }))
                }
            }

            PluginToggles(
                title: String(localized: "Conectores", bundle: .module),
                empty: String(localized: "Nenhum conector ainda. Adicione GitHub, Linear, Notion e outros em Extensões.", bundle: .module),
                items: model.installedConnectors.map { ($0.id, $0.name, $0.command ?? $0.url ?? "") },
                selection: Binding(get: { draft.connectors ?? [] }, set: { draft.connectors = $0 })
            )
            PluginToggles(
                title: String(localized: "Skills", bundle: .module),
                empty: String(localized: "Nenhuma skill ainda. Obtenha algumas em Extensões ou escreva a sua.", bundle: .module),
                items: model.installedSkills.map { ($0.id, $0.name, $0.description) },
                selection: Binding(get: { draft.skills ?? [] }, set: { draft.skills = $0 }),
                info: { id in viewingSkill = model.installedSkills.first { $0.id == id }.map(SkillRef.init) }
            )
            if let error {
                Text(error).font(.footnote).foregroundStyle(Palette.danger)
            }
        }
    }
}

/// A labeled input inside a section row: label (and an optional note) over the field.
struct Field<Content: View>: View {
    let label: String
    var detail: String?
    @ViewBuilder let content: Content
    @Environment(\.noteLines) private var noteLines

    init(_ label: String, detail: String? = nil, @ViewBuilder content: () -> Content) {
        self.label = label
        self.detail = detail
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            VStack(alignment: .leading, spacing: 3) {
                Text(label).foregroundStyle(Palette.text)
                if let detail {
                    Text(detail).font(.caption).foregroundStyle(Palette.secondary).lineSpacing(2)
                        .lineLimit(noteLines)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            content
        }
    }
}

/// An on/off row: title and a short note, the switch on the right.
struct SwitchRow: View {
    let title: String
    var detail: String?
    @Binding var isOn: Bool
    @Environment(\.noteLines) private var noteLines

    init(_ title: String, detail: String? = nil, isOn: Binding<Bool>) {
        self.title = title
        self.detail = detail
        _isOn = isOn
    }

    var body: some View {
        Toggle(isOn: $isOn) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title).foregroundStyle(Palette.text)
                if let detail {
                    Text(detail).font(.caption).foregroundStyle(Palette.secondary).lineSpacing(2)
                        .lineLimit(noteLines)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .toggleStyle(.codync)
    }
}

extension View {
    /// A rounded input box: the page color in a filled card, filled in an outlined one.
    func fieldBox(fill: Color? = nil) -> some View {
        modifier(FieldBox(fill: fill))
    }
}

private struct FieldBox: ViewModifier {
    let fill: Color?
    @Environment(\.cardStyle) private var cardStyle

    func body(content: Content) -> some View {
        content
            .textFieldStyle(.plain)
            .padding(.horizontal, 14)
            .padding(.vertical, 11)
            .background(fill ?? (cardStyle == .outlined ? Palette.surface : Palette.background),
                        in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}

extension BotDraft {
    /// A saved bot keeps a name; only a new one goes without (the host names it after its first conversations).
    var isValid: Bool { !name.trimmingCharacters(in: .whitespaces).isEmpty }

    /// What the host should get: no stale command unless the agent is custom.
    var normalized: BotDraft {
        var d = self
        if d.backend != "custom" { d.command = nil }
        return d
    }
}
