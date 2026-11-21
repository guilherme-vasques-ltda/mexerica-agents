// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// One layer's long-form notes, under its entries: name, description, size and date, marked past a cap.
struct MemoryFilesList: View {
    let memory: MemoryStore
    let files: MemoryFiles
    let shared: Bool
    @State private var open: MemoryFileTarget?
    @Environment(\.noteLines) private var noteLines

    var body: some View {
        let folder = shared ? files.yours : files.bot
        let live = folder.files.filter { !$0.deleted }
        let gone = folder.files.filter(\.deleted)
        HStack(spacing: 8) {
            Text(String(localized: "Notas", bundle: .module)).font(.footnote.weight(.semibold)).foregroundStyle(Palette.secondary)
            Spacer(minLength: 0)
            IconButton(String(localized: "Nova nota", bundle: .module), systemImage: "doc.badge.plus") {
                withAnimation(Motion.layout) { open = MemoryFileTarget(name: nil, shared: shared) }
            }
            .disabled(memory.busy)
        }
        .padding(.top, 6)
        .codyncSheet(item: $open) { target in MemoryFileView(memory: memory, target: target) }
        if live.isEmpty {
            Text(String(localized: "Notas mais longas, uma por assunto. O bot lê uma nota quando o assunto aparece.", bundle: .module))
                .font(.footnote).foregroundStyle(Palette.tertiary)
        } else if live.count > files.folderCap {
            Text(String(localized: "\(live.count) nota, acima do limite de \(files.folderCap). Ajuste o limite.", bundle: .module))
                .font(.footnote).foregroundStyle(Palette.attention)
        }
        ForEach(live) { file in row(file, cap: files.cap) }
        ForEach(gone) { file in row(file, cap: files.cap) }
    }

    private func row(_ file: MemoryFile, cap: Int) -> some View {
        Button {
            withAnimation(Motion.layout) { open = MemoryFileTarget(name: file.name, shared: shared) }
        } label: {
            VStack(alignment: .leading, spacing: 4) {
                Label(file.name, systemImage: file.deleted ? "doc" : "doc.text")
                    .foregroundStyle(file.deleted ? Palette.tertiary : Palette.text)
                if let description = file.description, !file.deleted {
                    Text(description).font(.footnote).foregroundStyle(Palette.secondary).lineLimit(noteLines)
                }
                Text(caption(file, cap: cap))
                    .font(.caption)
                    .foregroundStyle(file.tokens > cap && !file.deleted ? Palette.attention : Palette.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.vertical, 6)
    }

    private func caption(_ file: MemoryFile, cap: Int) -> String {
        let when = Date(milliseconds: file.updatedAt).formatted(.relative(presentation: .named))
        if file.deleted { return String(localized: "Excluída \(when)", bundle: .module) }
        if file.tokens > cap {
            return String(localized: "\(file.tokens) token, acima do limite de \(cap) · \(when)", bundle: .module)
        }
        return String(localized: "\(file.tokens) token · \(when)", bundle: .module)
    }
}

/// What a note sheet opens: a file, or (`name` nil) a new one.
struct MemoryFileTarget: Identifiable {
    let id = UUID()
    let name: String?
    let shared: Bool
}

/// One note: read it, edit it, delete it, or bring back an earlier version.
struct MemoryFileView: View {
    let memory: MemoryStore
    let target: MemoryFileTarget
    @State private var detail: MemoryFileDetail?
    /// Editing, with the text being edited. Not an optional binding: the editor still reads it while it animates out.
    @State private var editing = false
    @State private var draft = ""
    @State private var name = ""
    @State private var confirmDelete = false
    @Environment(BotStore.self) private var host
    @Environment(\.dismissModal) private var dismiss

    private static var template: String { "---\ndescription: \(String(localized: "Quando ler esta nota", bundle: .module))\n---\n" }

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(target.name ?? String(localized: "Nova nota", bundle: .module)) { actions }
            CardForm {
                if let error = memory.error {
                    CardSection { Text(error).foregroundStyle(Palette.danger).textSelection(.enabled) }
                }
                if editing {
                    editor
                } else if let detail {
                    reader(detail)
                } else {
                    CardSection { Text(String(localized: "Carregando…", bundle: .module)).foregroundStyle(Palette.secondary) }
                }
            }
        }
        .background(Palette.background)
        .task {
            if let file = target.name {
                detail = await memory.file(file, shared: target.shared)
            } else {
                draft = Self.template
                editing = true
            }
        }
        .codyncDialog(String(localized: "Excluir \(target.name ?? String(localized: "esta nota", bundle: .module))?", bundle: .module), isPresented: $confirmDelete,
                      message: String(localized: "As versões são mantidas para você poder restaurá-la.", bundle: .module)) {
            [DialogAction(String(localized: "Excluir", bundle: .module), destructive: true) {
                Task { await change(content: nil) }
            }]
        }
    }

    @ViewBuilder private var actions: some View {
        if memory.busy {
            Spinner()
        } else if editing {
            IconButton(String(localized: "Salvar", bundle: .module), systemImage: "checkmark") { Task { await change(content: draft) } }
                .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || (target.name == nil && !validName))
        } else if let detail, let content = detail.content {
            HStack(spacing: 2) {
                IconButton(String(localized: "Editar", bundle: .module), systemImage: "pencil") {
                    draft = content
                    withAnimation(Motion.layout) { editing = true }
                }
                IconButton(String(localized: "Excluir", bundle: .module), systemImage: "trash") { withAnimation(Motion.layout) { confirmDelete = true } }
            }
        }
    }

    private var validName: Bool { name.range(of: #"^[a-z0-9-]+\.md$"#, options: .regularExpression) != nil }

    @ViewBuilder private var editor: some View {
        if target.name == nil {
            CardSection(footer: String(localized: "Letras minúsculas, números e hífens, terminando em .md.", bundle: .module)) {
                TextField("people.md", text: $name)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
        }
        CardSection(footer: String(localized: "A linha description: diz quando o bot deve lê-la.", bundle: .module)) {
            TextField("", text: $draft, axis: .vertical)
                .lineLimit(12...40)
                .font(.footnote.monospaced())
        }
        .textFieldStyle(.plain)
    }

    @ViewBuilder private func reader(_ detail: MemoryFileDetail) -> some View {
        CardSection(footer: detail.path) {
            if let content = detail.content {
                Text(content).font(.footnote.monospaced()).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Text(String(localized: "Excluída. Restaure uma versão abaixo.", bundle: .module)).foregroundStyle(Palette.secondary)
            }
        }
        CardSection(String(localized: "Versões", bundle: .module)) {
            ForEach(detail.versions) { version in
                HStack(spacing: 8) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(who(version))
                        Text(Date(milliseconds: version.at).formatted(.relative(presentation: .named)))
                            .font(.caption).foregroundStyle(Palette.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    if let content = version.content, content != detail.content {
                        IconButton(String(localized: "Restaurar esta versão", bundle: .module), systemImage: "arrow.uturn.backward") {
                            Task { await change(version: version.id) }
                        }
                        .disabled(memory.busy)
                    }
                }
                .padding(.vertical, 4)
            }
        }
    }

    private func who(_ version: MemoryFileVersion) -> String {
        let by = switch version.author {
        case "user": String(localized: "Você", bundle: .module)
        case "bot": host.bots[version.authorBot ?? ""]?.name ?? String(localized: "Um bot", bundle: .module)
        case "dream": String(localized: "Sonhando", bundle: .module)
        default: String(localized: "Este computador", bundle: .module)
        }
        return switch version.cause {
        case "delete": String(localized: "\(by) excluiu", bundle: .module)
        case "restore": String(localized: "\(by) restaurou uma versão", bundle: .module)
        case "undo": String(localized: "\(by) desfez uma alteração", bundle: .module)
        default: String(localized: "\(by) alterou", bundle: .module)
        }
    }

    /// Saves (`content`), deletes (nil) or restores `version`, over what was loaded.
    private func change(content: String? = nil, version: Int64? = nil) async {
        let file = target.name ?? name
        let base = target.name == nil ? nil : detail?.hash
        if await memory.changeFile(file, shared: target.shared, base: base, content: content, version: version) {
            if content == nil && version == nil { dismiss(); return }
            withAnimation(Motion.layout) { editing = false }
            if target.name == nil { dismiss(); return }
            detail = await memory.file(file, shared: target.shared)
        }
    }
}
