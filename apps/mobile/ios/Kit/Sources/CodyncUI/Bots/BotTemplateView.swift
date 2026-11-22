// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// A portable settings snapshot, with no conversation identity or runtime state.
struct BotTemplateView: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var copied = false
    private let template: BotDraft
    private let encoded: String?

    init(draft: BotDraft) {
        var template = draft
        template.id = nil
        template.pinned = nil
        template.hidden = nil
        self.template = template
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        encoded = (try? encoder.encode(template)).flatMap { String(data: $0, encoding: .utf8) }
    }

    var body: some View {
        VStack(spacing: 0) {
        ModalHeader(String(localized: "Criar modelo de bot", bundle: .module))
        VStack(alignment: .leading, spacing: 20) {
            Text(template.name).font(.subheadline).foregroundStyle(Palette.secondary)
            Text(String(localized: "Copie as configurações deste bot para reutilizar como modelo. O histórico e o ID da conversa ficam de fora.", bundle: .module))
                .font(.callout)
                .foregroundStyle(Palette.secondary)
            ScrollView {
                Text(encoded ?? String(localized: "Não foi possível criar este modelo.", bundle: .module))
                    .font(.system(size: 12, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(16)
            }
            .background(Palette.surface, in: RoundedRectangle(cornerRadius: 12))
            HStack {
                Text(String(localized: "Inclui preferências do espaço de trabalho e configuração do bot.", bundle: .module))
                    .font(.caption).foregroundStyle(Palette.secondary)
                Spacer()
                Button {
                    Pasteboard.copy(encoded)
                    withAnimation(Motion.reduced(Motion.morph, reduceMotion)) { copied = true }
                } label: {
                    Label(copied ? String(localized: "Copiado", bundle: .module) : String(localized: "Copiar modelo", bundle: .module), systemImage: copied ? "checkmark" : "square.on.square")
                        .font(.system(size: 13, weight: .semibold))
                        .padding(.horizontal, 14).padding(.vertical, 9)
                        .foregroundStyle(Palette.onAccent)
                        .background(Palette.accentFill, in: RoundedRectangle(cornerRadius: 9))
                }
                .buttonStyle(.plain)
                .disabled(encoded == nil)
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding([.horizontal, .bottom], 24)
        }
        .background(Palette.background)
    }
}
