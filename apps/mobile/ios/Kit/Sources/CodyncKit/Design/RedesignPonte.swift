import SwiftUI
import UIKit

/// Ponte do redesenho iOS v1 (`CodyncUI/Redesign`) para o que vive aqui dentro: os quadros de feltro
/// de `Mexeriquinhas.xcassets` e o leitor de SVG. Só expõe; não muda o desenho existente.
public enum RedesignPonte {
    /// O quadro `quadro` (ex.: `livre-1`, `piscar`) do conjunto, ou nulo se o catálogo não o tiver.
    public static func quadroPelucia(conjunto: String, quadro: String) -> UIImage? {
        guard let asset = NSDataAsset(name: "mxq-\(conjunto)-\(quadro)", bundle: .module) else { return nil }
        return UIImage(data: asset.data)
    }

    /// O atributo `d` de um `<path>` SVG como `Path` (mesmo leitor da Mexeriquinha Simples).
    public static func caminhoSVG(_ d: String) -> Path {
        SVGPath.path(d)
    }
}
