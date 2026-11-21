// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// A bot's memory: what all your bots know about you, what this one knows, its dreams, and its harness's own notes
/// when the user kept them on.
struct MemoryCard: View {
    let botId: String
    @Environment(BotStore.self) private var host
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @Environment(\.locale) private var locale
    @State private var memory: MemoryStore?
    @State private var section: Section = .bot
    @State private var editor: MemoryEditTarget?
    @State private var confirmClear = false
    @State private var aboutHarness = false
    @State private var openDreams: Set<DreamRun.ID> = []

    enum Section: Hashable { case yours, bot, dreams, harness }

    private var bot: Bot? { host.bots[botId] }
    /// A long memory stops at a few lines, fewer when the screen is short (landscape); tapping it shows the rest
    /// (an entry opens its editor, a dream unfolds).
    private var lines: Int { verticalSizeClass == .compact ? 2 : 5 }

    var body: some View {
        VStack(spacing: 10) {
            TabChoice(selection: Binding(get: { section }, set: { value in
                withAnimation(Motion.layout) { section = value }
                // Claude Code writes its notes outside Codync, so nothing moves `memoryRev`: read them again on the way in.
                if value == .harness { Task { await memory?.load() } }
            }), options: sections(memory?.view))
            CardSection {
                if let memory { contents(memory) }
                else { Text(String(localized: "Carregando memória…", bundle: .module)).foregroundStyle(Palette.secondary) }
            }
        }
        .tracked(.screenViewed(.memory))
        // Memory changes anywhere move the bot's `memoryRev`: fetch again.
        .task(id: "\(botId)-\(bot?.memoryRev ?? 0)-\(bot?.sharedMemory ?? false)-\(bot?.harnessMemory ?? false)") {
            let store = memory ?? MemoryStore(botId: botId) { try await host.ready() }
            memory = store
            await store.load()
        }
        // Claude Code's section goes when its memory is turned off (on another device too).
        .onChange(of: memory?.view.map { $0.projectMemory == nil } ?? false) { _, gone in
            if gone && section == .harness { section = .bot }
        }
        .codyncSheet(isPresented: $aboutHarness) {
            if let bot { HarnessMemorySheet(harness: host.backendName(bot.backend), readable: bot.backend == "claude") }
        }
        .codyncSheet(item: $editor) { target in
            if let memory { MemoryEditorView(memory: memory, target: target) }
        }
        .codyncDialog(section == .yours ? String(localized: "Esquecer tudo que seus bots compartilham sobre você?", bundle: .module) : String(localized: "Esquecer tudo que este bot lembra?", bundle: .module),
                      isPresented: $confirmClear) {
            [DialogAction(String(localized: "Esquecer tudo", bundle: .module), destructive: true) {
                Task { await memory?.mutate("clearMemory", shared: section == .yours) }
            }]
        }
    }

    @ViewBuilder private func contents(_ memory: MemoryStore) -> some View {
        if let error = memory.error { Text(error).foregroundStyle(Palette.danger).textSelection(.enabled) }
        if let view = memory.view {
            switch section {
            case .yours, .bot: layer(memory, view: view)
            case .dreams: dreams(memory, view.dreams)
            case .harness: harness(view.projectMemory)
            }
        } else if memory.error == nil {
            Text(String(localized: "Carregando memória…", bundle: .module)).foregroundStyle(Palette.secondary)
        }
    }

    private func sections(_ view: MemoryView?) -> [(id: Section, label: String)] {
        [(Section.yours, String(localized: "Sobre você", bundle: .module)), (.bot, String(localized: "Este bot", bundle: .module)), (.dreams, String(localized: "Sonhos", bundle: .module))]
            + (view?.projectMemory != nil ? [(.harness, host.backendName(bot?.backend ?? "claude"))] : [])
    }

    @ViewBuilder private func layer(_ memory: MemoryStore, view: MemoryView) -> some View {
        let shared = section == .yours
        let layer = shared ? view.yours : view.bot
        if shared, let bot {
            Toggle(String(localized: "Usar o que seus bots compartilham", bundle: .module), isOn: Binding(get: { bot.sharedMemory ?? view.shared }, set: { host.setSharedMemory(bot, $0) }))
                .toggleStyle(.codync)
        }
        if !shared, let bot, let on = bot.harnessMemory {
            let name = host.backendName(bot.backend)
            HStack(spacing: 4) {
                Text(String(localized: "Memória própria de \(name)", bundle: .module)).lineLimit(1).layoutPriority(1)
                IconButton(String(localized: "Sobre a memória própria de \(name)", bundle: .module), systemImage: "info.circle") {
                    withAnimation(Motion.layout) { aboutHarness = true }
                }
                Toggle(isOn: Binding(get: { on }, set: { host.setHarnessMemory(bot, $0) })) { EmptyView() }
                    .toggleStyle(.codync)
                    .disabled(memory.busy)
                    .accessibilityLabel(String(localized: "Memória própria de \(name)", bundle: .module))
            }
        }
        HStack(alignment: .top, spacing: 8) {
            Text(shared
                 ? String(localized: "Seu nome, fuso horário, pessoas e preferências duradouras, compartilhados pelos seus bots. \(layer.used) de \(layer.cap) em uso.", bundle: .module)
                 : String(localized: "O que este bot aprendeu sobre você. \(layer.used) de \(layer.cap) em uso.", bundle: .module))
                .font(.footnote).foregroundStyle(Palette.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            IconButton(String(localized: "Adicionar memória", bundle: .module), systemImage: "plus") {
                withAnimation(Motion.layout) { editor = MemoryEditTarget(entry: nil, shared: shared, text: "") }
            }
            .disabled(memory.busy)
            moreMenu(memory)
        }
        if layer.entries.isEmpty {
            empty("brain", shared ? String(localized: "Nada compartilhado ainda.", bundle: .module) : String(localized: "Nada ainda. O que este bot aprender sobre você aparecerá aqui, e você poderá alterar tudo.", bundle: .module))
        }
        ForEach(layer.entries) { entry in
            HStack(alignment: .top, spacing: 8) {
                Button {
                    withAnimation(Motion.layout) { editor = MemoryEditTarget(entry: entry.id, shared: shared, text: entry.text) }
                } label: {
                    VStack(alignment: .leading, spacing: 4) {
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            if entry.pinned { Image(systemName: "pin.fill").font(.caption2).foregroundStyle(Palette.secondary) }
                            Text(entry.text).foregroundStyle(entry.archived ? Palette.tertiary : Palette.text).lineLimit(lines)
                        }
                        Text(entry.archived
                             ? String(localized: "\(author(entry)) · \(Date(milliseconds: entry.updatedAt).formatted(.relative(presentation: .named).locale(locale))) · arquivada", bundle: .module)
                             : String(localized: "\(author(entry)) · \(Date(milliseconds: entry.updatedAt).formatted(.relative(presentation: .named).locale(locale)))", bundle: .module))
                            .font(.caption).foregroundStyle(Palette.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                entryMenu(memory, entry, shared: shared)
            }
            .padding(.vertical, 6)
        }
        if let files = view.files { MemoryFilesList(memory: memory, files: files, shared: shared) }
    }

    @ViewBuilder private func dreams(_ memory: MemoryStore, _ runs: [DreamRun]) -> some View {
        Text(String(localized: "Entre conversas, o bot organiza a memória usando suas próprias palavras. Cada execução pode ser desfeita.", bundle: .module))
            .font(.footnote).foregroundStyle(Palette.secondary)
        if runs.isEmpty { empty("moon", String(localized: "Nenhum sonho ainda.", bundle: .module)) }
        ForEach(runs) { run in
            HStack(alignment: .top, spacing: 8) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(run.undone
                         ? String(localized: "\(Date(milliseconds: run.at).formatted(.relative(presentation: .named).locale(locale))) · desfeito", bundle: .module)
                         : Date(milliseconds: run.at).formatted(.relative(presentation: .named).locale(locale)))
                        .fontWeight(.semibold)
                    let limit = openDreams.contains(run.id) ? nil : lines
                    ForEach(run.added, id: \.id) { Text("+ \($0.text)").lineLimit(limit) }
                    ForEach(run.archived, id: \.id) { Text("− \($0.text)").foregroundStyle(Palette.tertiary).lineLimit(limit) }
                    ForEach(run.files ?? [], id: \.version) { Label(String(localized: "Adicionado a \($0.name)", bundle: .module), systemImage: "doc.text").foregroundStyle(Palette.secondary) }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture { withAnimation(Motion.layout) { openDreams.formSymmetricDifference([run.id]) } }
                if !run.undone {
                    IconButton(String(localized: "Desfazer este sonho", bundle: .module), systemImage: "arrow.uturn.backward") {
                        Task { await memory.mutate("undoDream", id: run.id) }
                    }
                    .disabled(memory.busy)
                }
            }
            .padding(.vertical, 6)
        }
    }

    @ViewBuilder private func harness(_ project: MemoryView.Project?) -> some View {
        if let project {
            Text(String(localized: "Notas próprias do Claude Code, somente para leitura aqui.", bundle: .module))
                .font(.footnote).foregroundStyle(Palette.secondary)
            Text(project.path).font(.caption).foregroundStyle(Palette.tertiary).textSelection(.enabled)
            if project.text.isEmpty { empty("brain", String(localized: "Nada ainda. As notas do Claude Code aparecerão aqui quando ele salvar alguma.", bundle: .module)) }
            else { Text(project.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
        }
    }

    private func empty(_ icon: String, _ text: String) -> some View {
        VStack(spacing: 8) {
            Image(systemName: icon).font(.title3).foregroundStyle(Palette.tertiary)
            Text(text).multilineTextAlignment(.center).foregroundStyle(Palette.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 18)
    }

    private func author(_ entry: MemoryEntry) -> String {
        switch entry.author {
        case "user": String(localized: "Você escreveu", bundle: .module)
        case "bot": String(localized: "\(host.bots[entry.authorBot ?? ""]?.name ?? String(localized: "Um bot", bundle: .module)) lembrou", bundle: .module)
        case "dream": String(localized: "Sonhando", bundle: .module)
        default: String(localized: "Movido da memória antiga", bundle: .module)
        }
    }

    private func moreMenu(_ memory: MemoryStore) -> some View {
        DropdownMenu {
            [MenuItem(String(localized: "Atualizar", bundle: .module), icon: "arrow.clockwise") { Task { await memory.load() } },
             MenuItem(String(localized: "Esquecer tudo aqui", bundle: .module), icon: "trash", destructive: true, divider: true) {
                 withAnimation(Motion.layout) { confirmClear = true }
             }]
        } label: { menuLabel(String(localized: "Mais", bundle: .module)) }
    }

    private func entryMenu(_ memory: MemoryStore, _ entry: MemoryEntry, shared: Bool) -> some View {
        DropdownMenu {
            [MenuItem(String(localized: "Editar", bundle: .module), icon: "pencil") {
                withAnimation(Motion.layout) { editor = MemoryEditTarget(entry: entry.id, shared: shared, text: entry.text) }
            },
             MenuItem(entry.pinned ? String(localized: "Desafixar", bundle: .module) : String(localized: "Fixar", bundle: .module), icon: entry.pinned ? "pin.slash" : "pin") {
                 Task { await memory.mutate("pinMemory", id: entry.id, pinned: !entry.pinned) }
             },
             MenuItem(entry.archived ? String(localized: "Restaurar", bundle: .module) : String(localized: "Arquivar", bundle: .module), icon: entry.archived ? "tray.and.arrow.up" : "archivebox") {
                 Task { await memory.mutate("archiveMemory", id: entry.id, archived: !entry.archived) }
             },
             MenuItem(String(localized: "Esquecer", bundle: .module), icon: "trash", destructive: true, divider: true) {
                 Task { await memory.mutate("forgetMemory", id: entry.id) }
             }]
        } label: { menuLabel(String(localized: "Ações da memória", bundle: .module)) }
        .disabled(memory.busy)
    }

    private func menuLabel(_ title: String) -> some View {
        Image(systemName: "ellipsis")
            .font(.body.weight(.medium))
            .foregroundStyle(Palette.secondary)
            .frame(width: 32, height: 32)
            .contentShape(Rectangle())
            .accessibilityLabel(title)
    }
}
