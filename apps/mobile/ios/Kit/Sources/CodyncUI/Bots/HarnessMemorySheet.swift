// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

/// The memory screen's info button beside the harness's own memory switch: what turning it on means. The same
/// text as the desktop app's; the terminal UI says it short in a confirmation before turning it on.
struct HarnessMemorySheet: View {
    /// The harness's name ("Claude Code").
    let harness: String
    /// Whether Codync can show the harness's notes (Claude Code's are a file on the computer).
    let readable: Bool

    private var details: [(icon: String, title: String, detail: String)] {
        [("brain", String(localized: "Uma memória própria", bundle: .module), String(localized: "\(harness) mantém notas separadas da memória da Mexerica. Essa opção começa desativada, assim o bot tem uma memória e você pode vê-la e alterá-la aqui.", bundle: .module)),
         ("square.on.square", String(localized: "Lembrado duas vezes", bundle: .module), String(localized: "Com as duas ativadas, o bot pode salvar a mesma coisa duas vezes, e as memórias podem divergir após alguma mudança.", bundle: .module)),
         ("trash", String(localized: "Ao esquecer", bundle: .module), String(localized: "Esquecer algo aqui não o remove das notas de \(harness). A Mexerica não pode alterá-las.", bundle: .module)),
         ("eye", String(localized: "Onde ver", bundle: .module), readable ? String(localized: "As notas aparecem somente para leitura na aba \(harness).", bundle: .module) : String(localized: "A Mexerica não pode mostrá-las; consulte o próprio \(harness).", bundle: .module))]
    }

    var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Memória própria de \(harness)", bundle: .module))
            CardForm {
                CardSection {
                    ForEach(details, id: \.title) { item in
                        HStack(alignment: .firstTextBaseline, spacing: 12) {
                            Image(systemName: item.icon)
                                .foregroundStyle(Palette.secondary)
                                .frame(width: 22)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(item.title)
                                Text(item.detail)
                                    .font(.footnote)
                                    .foregroundStyle(Palette.secondary)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }
                    }
                }
            }
        }
        .background(Palette.background)
    }
}
