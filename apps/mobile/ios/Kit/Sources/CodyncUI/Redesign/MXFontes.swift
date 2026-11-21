import CoreText
import SwiftUI
import UIKit

/// Nunito (títulos) e DM Sans (interface), fontes variáveis oficiais do Google Fonts (OFL), em
/// `Resources/Redesign/Fontes`. Os avisos de licença ficam na mesma pasta.
///
/// O peso vai no eixo `wght` e, na DM Sans, o tamanho óptico no eixo `opsz` acompanha o tamanho do texto
/// (como `font-optical-sizing: auto` no protótipo). O tamanho escala com o Dynamic Type pelo estilo do sistema.
public enum MXFontes {
    static let arquivos = ["Nunito-VariableFont_wght.ttf", "DMSans-VariableFont_opsz-wght.ttf"]
    /// Eixos OpenType como inteiros de 4 letras.
    static let eixoPeso = 0x7767_6874 // 'wght'
    static let eixoOptico = 0x6F70_737A // 'opsz'
    /// Faixa do `opsz` da DM Sans.
    static let opticoMinimo: CGFloat = 9
    static let opticoMaximo: CGFloat = 40

    /// Registra as duas fontes no processo, uma vez. Devolve se as duas estão disponíveis.
    @discardableResult
    public static func registrar() -> Bool { registradas }

    private static let registradas: Bool = arquivos.allSatisfy { nome in
        guard let url = recurso(nome) else { return false }
        var erro: Unmanaged<CFError>?
        if CTFontManagerRegisterFontsForURL(url as CFURL, .process, &erro) { return true }
        // Já registrada (outro teste, outra tela): conta como disponível.
        let codigo = erro.map { CFErrorGetCode($0.takeRetainedValue()) }
        return codigo == CTFontManagerError.alreadyRegistered.rawValue
    }

    /// Um arquivo de `Resources/Redesign` no pacote (fontes e licenças).
    static func recurso(_ nome: String) -> URL? { Bundle.module.url(forResource: nome, withExtension: nil) }

    /// O tamanho do estilo no tamanho de texto escolhido.
    static func tamanho(_ tipo: MXTipo, categoria: UIContentSizeCategory) -> CGFloat {
        let traits = UITraitCollection(preferredContentSizeCategory: categoria)
        return UIFontMetrics(forTextStyle: tipo.relativoA).scaledValue(for: tipo.tamanho, compatibleWith: traits)
    }

    /// A fonte do estilo já no tamanho final.
    static func ctFont(_ tipo: MXTipo, tamanho: CGFloat) -> CTFont {
        registrar()
        var variacoes: [Int: CGFloat] = [eixoPeso: tipo.peso]
        if tipo.familia == .dmSans { variacoes[eixoOptico] = min(opticoMaximo, max(opticoMinimo, tamanho)) }
        let atributos: [CFString: Any] = [
            kCTFontNameAttribute: tipo.familia.rawValue,
            kCTFontVariationAttribute: variacoes as CFDictionary,
        ]
        return CTFontCreateWithFontDescriptor(CTFontDescriptorCreateWithAttributes(atributos as CFDictionary), tamanho, nil)
    }
}

/// Aplica um `MXTipo` escalando com o Dynamic Type da tela.
struct MXTipoModifier: ViewModifier {
    let tipo: MXTipo
    @Environment(\.dynamicTypeSize) private var dynamicType

    func body(content: Content) -> some View {
        let tamanho = MXFontes.tamanho(tipo, categoria: UIContentSizeCategory(dynamicType))
        let entrelinha = tipo.entrelinha.map { AttributedString.LineHeight.exact(points: $0 * tamanho / tipo.tamanho) }
        content
            .font(Font(MXFontes.ctFont(tipo, tamanho: tamanho)))
            .lineHeight(entrelinha)
    }
}

public extension View {
    /// Texto no estilo do redesenho.
    func mxTipo(_ tipo: MXTipo) -> some View { modifier(MXTipoModifier(tipo: tipo)) }
}
