import SwiftUI

/// Superfícies de vidro do redesenho: Liquid Glass nativo do iOS 26 (`glassEffect`).
/// Com "Reduzir transparência", vira superfície opaca no tom do desenho, com contorno fino.
public enum MXSuperficie: Sendable {
    /// Botão redondo e cápsula do topo.
    case botao
    /// Cartões grandes (Berga, lista).
    case painel
    /// Campo de busca.
    case campo
    /// Barra de abas.
    case barra
    /// Caixa de mensagem e o `+` (etapa 2).
    case caixa
    /// Menu do agente (etapa 2).
    case menu
    /// Fila de mensagens (etapa 2).
    case fila

    var opaco: MXCor {
        switch self {
        case .botao: MX.Vidro.opacoBotao
        case .painel: MX.Vidro.opacoPainel
        case .campo: MX.Vidro.opacoCampo
        case .barra: MX.Vidro.opacoBarra
        case .caixa: MX.Vidro.opacoCaixa
        case .menu: MX.Vidro.opacoMenu
        case .fila: MX.Vidro.opacoFila
        }
    }
}

struct MXVidroModifier<Forma: Shape>: ViewModifier {
    let superficie: MXSuperficie
    let forma: Forma
    let interativo: Bool
    @Environment(\.accessibilityReduceTransparency) private var reduzirTransparencia

    func body(content: Content) -> some View {
        if reduzirTransparencia {
            content
                .background(superficie.opaco.color, in: forma)
                .overlay(forma.stroke(MX.Vidro.contornoOpaco.color, lineWidth: MX.Vidro.contornoOpacoEspessura))
        } else {
            content.glassEffect(.regular.tint(MX.Vidro.tinturaClara.color).interactive(interativo), in: forma)
        }
    }
}

public extension View {
    /// Vidro nativo na forma dada; opaco com Reduzir transparência.
    func mxVidro(_ superficie: MXSuperficie, em forma: some Shape, interativo: Bool = false) -> some View {
        modifier(MXVidroModifier(superficie: superficie, forma: forma, interativo: interativo))
    }
}

/// Toque do redesenho: encolhe para 0.97 no toque para baixo e volta com a mola padrão. Interrompível
/// (é uma mola do SwiftUI). Com Reduzir movimento, não encolhe.
public struct MXToque: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento

    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        let escala = configuration.isPressed && !reduzirMovimento ? MX.Movimento.toqueEscala : 1
        configuration.label
            .contentShape(Rectangle())
            .scaleEffect(escala)
            .animation(configuration.isPressed ? .easeOut(duration: MX.Movimento.toqueDuracao) : MX.Movimento.molaPadrao.animacao,
                       value: configuration.isPressed)
    }
}
