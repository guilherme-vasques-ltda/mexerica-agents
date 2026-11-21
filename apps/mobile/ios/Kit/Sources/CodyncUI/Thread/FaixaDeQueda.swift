import CodyncKit
import SwiftUI

/// A short drop shows this quiet strip on top of the conversation and the bot list instead of
/// switching screens. It appears once a computer has been reconnecting past its grace (`faixaDeQueda`);
/// "Try now" brings the reconnect forward and reads "Trying…" for about three seconds.
public struct FaixaDeQueda: View {
    let stores: [BotStore]
    let textos: QuedaTextos
    @State private var tentando = false

    public init(stores: [BotStore], textos: QuedaTextos = .para()) {
        self.stores = stores
        self.textos = textos
    }

    private var emQueda: [BotStore] { stores.filter { $0.faixaDeQueda } }

    public var body: some View {
        if !emQueda.isEmpty {
            HStack(spacing: 10) {
                HStack(spacing: 8) {
                    Image(systemName: "wifi.slash")
                    Text(textos.titulo)
                        .lineLimit(2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(textos.titulo)
                .accessibilityHint(textos.detalhe)
                .accessibilityAddTraits(.updatesFrequently)
                Spacer(minLength: 8)
                if emQueda.contains(where: { $0.podeTentarAgora }) {
                    Button(tentando ? textos.tentando : textos.tentar, action: tentarAgora)
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(Palette.text)
                        .disabled(tentando)
                        .accessibilityHint(textos.detalhe)
                }
            }
            .font(.footnote.weight(.medium))
            .foregroundStyle(Palette.secondary)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(Palette.surface, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).stroke(Palette.border, lineWidth: 0.5))
            .padding(.horizontal, 16)
            .padding(.vertical, 4)
            .transition(.move(edge: .top).combined(with: .opacity))
        }
    }

    private func tentarAgora() {
        for store in emQueda { store.tentarAgora() }
        tentando = true
        Task {
            try? await Task.sleep(for: .seconds(3))
            tentando = false
        }
    }
}
