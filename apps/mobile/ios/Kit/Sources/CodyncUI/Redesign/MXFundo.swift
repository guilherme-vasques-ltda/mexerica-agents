import SwiftUI
import UIKit

/// Fundo do app: creme `#F6EEDC` com o "Jardim cítrico" desfocado por cima, cobrindo a tela toda.
/// Imagem: `brand/apps/ios-v1-design/fundos/jardim-citrico-desfocado.webp` do repositório `mexerica`.
public struct MXFundo: View {
    public init() {}

    static let imagem: UIImage? = NSDataAsset(name: "mx-jardim-citrico-desfocado", bundle: .module).flatMap { UIImage(data: $0.data) }

    public var body: some View {
        MX.Claro.fundo.color
            .overlay {
                if let imagem = Self.imagem {
                    Image(uiImage: imagem)
                        .resizable()
                        .scaledToFill()
                }
            }
            .clipped()
            .ignoresSafeArea()
            .accessibilityHidden(true)
    }
}
