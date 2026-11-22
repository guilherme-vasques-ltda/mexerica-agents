// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// Approval card in Grok Bot's choice-card style: what the agent wants, where
/// it runs, and the answers as a list of rows.
struct PermissionCard: View {
    let entry: Entry
    let hostName: String
    /// The option whose answer is on its way to the computer, if any.
    var answering: String?
    /// During a short drop the answers stay in view but can't be tapped; `semAprovar` says why.
    var bloqueado = false
    var semAprovar: String?
    let respond: (String?) -> Void
    @State private var expanded = false

    private var d: EntryData { entry.data }
    private var pending: Bool { d.status == "pending" }

    private var headline: String {
        switch d.toolKind {
        case "execute": String(localized: "Quer executar um comando", bundle: .module)
        case "edit", "delete", "move": String(localized: "Quer alterar arquivos", bundle: .module)
        case "fetch": String(localized: "Quer acessar a web", bundle: .module)
        case "read", "search": String(localized: "Quer ler arquivos", bundle: .module)
        default: String(localized: "Quer usar uma ferramenta", bundle: .module)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text(headline).font(.headline).foregroundStyle(Palette.text)
                if pending {
                    Circle().fill(Palette.warning).frame(width: 7, height: 7)
                }
            }
            Text(d.title ?? "")
                .font(.subheadline.monospaced())
                .foregroundStyle(Palette.secondary)
                .lineLimit(expanded ? nil : 3)
            Label(d.cwd.map { String(localized: "Executa em \(hostName) · \(($0 as NSString).lastPathComponent)", bundle: .module) }
                  ?? String(localized: "Executa em \(hostName)", bundle: .module), systemImage: "desktopcomputer")
                .font(.caption)
                .foregroundStyle(Palette.tertiary)

            if hasDetail {
                Disclosure(isExpanded: $expanded) {
                    VStack(alignment: .leading, spacing: 8) {
                        if let command = d.command, !command.isEmpty {
                            CodeBox(text: command)
                        }
                        if let detail = d.detail, !detail.isEmpty {
                            CodeBox(text: String(detail.prefix(2000)))
                        }
                        ForEach(d.diffs ?? [], id: \.self) { DiffView(diff: $0) }
                    }
                    .padding(.top, 6)
                } label: {
                    Text(String(localized: "Detalhes", bundle: .module)).font(.caption.weight(.medium)).foregroundStyle(Palette.secondary)
                }
            }

            if pending {
                buttons
                if bloqueado, let semAprovar {
                    Text(semAprovar)
                        .font(.footnote)
                        .foregroundStyle(Palette.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } else {
                Text(outcome)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(Palette.secondary)
            }
        }
        .padding(16)
        .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
        .padding(.trailing, 40)
    }

    private var hasDetail: Bool {
        !(d.command ?? "").isEmpty || !(d.detail ?? "").isEmpty || !(d.diffs ?? []).isEmpty
    }

    /// Ordered like Grok Bot: Allow once, Always allow, then Deny.
    private var ordered: [PermissionOption] {
        let rank = ["allow_once": 0, "allow_always": 1, "reject_once": 2, "reject_always": 3]
        return (d.options ?? []).sorted { (rank[$0.kind] ?? 9) < (rank[$1.kind] ?? 9) }
    }

    private func label(_ o: PermissionOption) -> String {
        if sessionOption(o) { return "Permitir nesta sessão" }
        return switch o.kind {
        case "allow_once": String(localized: "Permitir uma vez", bundle: .module)
        case "allow_always": String(localized: "Permitir sempre", bundle: .module)
        case "reject_once": String(localized: "Negar", bundle: .module)
        case "reject_always": String(localized: "Nunca", bundle: .module)
        default: o.name
        }
    }

    private func sessionOption(_ o: PermissionOption) -> Bool {
        o.kind == "allow_always" && ["allow_for_session", "allow_session", "allow_permissions_session"].contains(o.optionId)
    }

    private var buttons: some View {
        let options = ordered
        return VStack(spacing: 0) {
            ForEach(Array(options.enumerated()), id: \.element.id) { i, o in
                if i > 0 { Rectangle().fill(Palette.border).frame(height: 0.5) }
                Button { respond(o.optionId) } label: {
                    HStack(spacing: 8) {
                        Text(label(o))
                            .font(.body.weight(o.kind == "allow_once" ? .semibold : .regular))
                            .foregroundStyle(o.kind.hasPrefix("allow") ? Palette.text : Palette.danger)
                        Spacer(minLength: 0)
                        if answering == o.optionId { Spinner(size: 14) }
                    }
                    .frame(maxWidth: .infinity, minHeight: 46, alignment: .leading)
                    .padding(.horizontal, 14)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(answering != nil || bloqueado)
                .opacity(bloqueado ? 0.5 : (answering == nil || answering == o.optionId ? 1 : 0.4))
            }
        }
        .background(Palette.background, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .padding(.top, 4)
    }

    private var outcome: String {
        switch d.status {
        case "answered":
            let chosen = d.options?.first { $0.optionId == d.selected }
            if let chosen, sessionOption(chosen) { return "Permitido nesta sessão" }
            return switch chosen?.kind {
            case "allow_once": String(localized: "Permitido uma vez", bundle: .module)
            case "allow_always": String(localized: "Permitido sempre", bundle: .module)
            case "reject_once": String(localized: "Negado", bundle: .module)
            case "reject_always": String(localized: "Nunca permitido", bundle: .module)
            default: chosen?.name ?? String(localized: "Respondido", bundle: .module)
            }
        case "cancelled": return String(localized: "Cancelado", bundle: .module)
        default: return String(localized: "Expirou — o bot continuou", bundle: .module)
        }
    }
}

struct CodeBox: View {
    let text: String

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Text(text)
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(Palette.text)
                .textSelection(.enabled)
                .padding(8)
        }
        .frame(maxHeight: 240)
        .background(Palette.codeBackground, in: RoundedRectangle(cornerRadius: 8))
    }
}

struct DiffView: View {
    let diff: FileDiff

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Image(systemName: diff.isNew ? "doc.badge.plus" : "doc.text")
                Text((diff.path as NSString).lastPathComponent).lineLimit(1)
                Spacer()
                Text("+\(diff.added)").foregroundStyle(Palette.added)
                Text("−\(diff.removed)").foregroundStyle(Palette.removed)
            }
            .font(.caption.monospaced())
            .foregroundStyle(Palette.secondary)
            ScrollView(.horizontal, showsIndicators: false) {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(diff.patch.split(separator: "\n", omittingEmptySubsequences: false).prefix(80).enumerated()), id: \.offset) { _, line in
                        Text(line.isEmpty ? " " : String(line))
                            .foregroundStyle(line.hasPrefix("+") ? Palette.added : line.hasPrefix("-") ? Palette.removed : Palette.secondary)
                    }
                }
                .font(.system(.caption2, design: .monospaced))
                .padding(8)
            }
            .frame(maxHeight: 260)
            .background(Palette.codeBackground, in: RoundedRectangle(cornerRadius: 8))
        }
    }
}
