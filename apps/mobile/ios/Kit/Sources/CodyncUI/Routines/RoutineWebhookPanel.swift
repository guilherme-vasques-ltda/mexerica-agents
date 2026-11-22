// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// A saved webhook routine's address and key: copy either, reveal or replace the key.
struct RoutineWebhookPanel: View {
    let botId: String
    let routineId: String
    @Environment(BotStore.self) private var model
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var webhook: RoutineWebhook?
    @State private var error: String?
    @State private var showKey = false
    @State private var copied: String?
    @State private var confirmRotate = false
    @State private var rotating = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if let webhook {
                Field(webhook.url == nil ? String(localized: "URL local", bundle: .module) : "URL") {
                    value(webhook.url ?? webhook.localUrl, copy: "url")
                }
                Field(String(localized: "Chave", bundle: .module)) {
                    value(showKey ? webhook.key : String(repeating: "•", count: 16), copy: "key") {
                        IconButton(showKey ? String(localized: "Ocultar chave", bundle: .module) : String(localized: "Mostrar chave", bundle: .module), systemImage: showKey ? "eye.slash" : "eye") {
                            withAnimation(Motion.reduced(Motion.hover, reduceMotion)) { showKey.toggle() }
                        }
                        IconButton(String(localized: "Substituir chave", bundle: .module), systemImage: "arrow.triangle.2.circlepath") { confirmRotate = true }
                            .disabled(rotating || model.isOffline)
                    }
                }
                Text(note(webhook))
                    .font(.caption).foregroundStyle(Palette.secondary).padding(.leading, 4)
                    .fixedSize(horizontal: false, vertical: true)
            } else if let error {
                Text(error).font(.caption).foregroundStyle(Palette.danger).padding(.leading, 4)
            } else {
                Text(String(localized: "Carregando webhook…", bundle: .module)).font(.caption).foregroundStyle(Palette.secondary).padding(.leading, 4)
            }
        }
        .task(id: routineId) { await load(rotate: false) }
        .codyncDialog(String(localized: "Substituir a chave?", bundle: .module), isPresented: $confirmRotate,
                      message: String(localized: "Quem usa a chave atual deixará de enviar até receber a nova chave.", bundle: .module)) {
            [DialogAction(String(localized: "Substituir chave", bundle: .module), destructive: true) { Task { await load(rotate: true) } }]
        }
    }

    private func note(_ webhook: RoutineWebhook) -> String {
        guard webhook.url != nil else {
            return String(localized: "A nuvem da Mexerica está desativada, então apenas este computador pode enviar para ela.", bundle: .module)
        }
        return String(localized: "Envie um POST com Authorization: Bearer <key>. Para o GitHub, use o tipo de conteúdo application/json e a chave como segredo. As entregas aguardam até 72 horas enquanto este computador estiver desligado. Elas passam pela nuvem da Mexerica, que pode lê-las.", bundle: .module)
    }

    /// A selectable monospaced value with a copy button, plus any extra buttons.
    private func value(_ text: String, copy: String, @ViewBuilder extra: () -> some View = { EmptyView() }) -> some View {
        HStack(alignment: .center, spacing: 8) {
            Text(text)
                .font(.callout.monospaced())
                .lineLimit(2)
                .truncationMode(.middle)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
            extra()
            IconButton(copied == copy ? String(localized: "Copiado", bundle: .module) : String(localized: "Copiar", bundle: .module), systemImage: copied == copy ? "checkmark" : "doc.on.doc") {
                guard let webhook else { return }
                Pasteboard.copy(copy == "key" ? webhook.key : webhook.url ?? webhook.localUrl)
                withAnimation(Motion.reduced(Motion.hover, reduceMotion)) { copied = copy }
            }
        }
        .padding(.vertical, 8)
        .padding(.leading, 12)
        .padding(.trailing, 6)
        .background(Palette.bubbleUser, in: RoundedRectangle(cornerRadius: 12))
    }

    private func load(rotate: Bool) async {
        guard let client = model.client else { error = String(localized: "Conecte-se a este computador para ver o webhook.", bundle: .module); return }
        rotating = rotate
        defer { rotating = false }
        do {
            let value = try await client.routineWebhook(botId: botId, id: routineId, rotate: rotate)
            withAnimation(Motion.reduced(Motion.hover, reduceMotion)) {
                webhook = value
                if rotate { copied = nil; showKey = true }
            }
            error = nil
        } catch { self.error = error.localizedDescription }
    }
}
