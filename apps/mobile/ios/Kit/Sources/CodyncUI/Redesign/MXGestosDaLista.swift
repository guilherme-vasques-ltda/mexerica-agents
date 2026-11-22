import SwiftUI

/// Gestos do agente (e do Berga) na lista de Conversas (README, "Gestos"; `movimento.puxarParaSessoes` e
/// `segurarParaAjustes` do JSON):
/// - puxar da direita para a esquerda: aparece "Sessões" em laranja (texto marrom) e, passando de 80 pt, abre as
///   sessões; até 150 pt de puxão; ao soltar volta com a mola de puxar (com Reduzir movimento a linha não desliza:
///   só o "Sessões" aparece por opacidade);
/// - segurar 0,55 s: abre os ajustes do agente (folha), com háptico médio.
/// O toque que termina um desses gestos não abre a conversa (`gestoUsado`). Os dois têm ação acessível.
struct MXGestosDoAgente: ViewModifier {
    let sessoes: (() -> Void)?
    let ajustes: (() -> Void)?
    /// O gesto foi usado neste toque: a linha não abre a conversa ao soltar.
    @Binding var gestoUsado: Bool
    @State private var arrasto: CGFloat = 0
    @State private var tocando = false
    @State private var seguradas = 0
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento

    init(sessoes: (() -> Void)?, ajustes: (() -> Void)?, gestoUsado: Binding<Bool>) {
        self.sessoes = sessoes
        self.ajustes = ajustes
        _gestoUsado = gestoUsado
    }

    func body(content: Content) -> some View {
        content
            .offset(x: reduzirMovimento ? 0 : -arrasto)
            .background(alignment: .trailing) {
                if arrasto > 0 {
                    Text("Sessões", bundle: .module)
                        .mxTipo(MX.Tipo.acaoDaLinha)
                        .foregroundStyle(MX.Claro.sobreMexerica.color)
                        .lineLimit(1)
                        .fixedSize()
                        .frame(width: max(arrasto, MX.Espaco.alvoMinimo), alignment: .center)
                        .frame(maxHeight: .infinity)
                        .background(MX.Claro.mexerica.color, in: RoundedRectangle(cornerRadius: MX.Raio.linha))
                        .opacity(min(1, arrasto / MX.Movimento.puxarParaSessoesLimiar))
                        .accessibilityHidden(true)
                }
            }
            .simultaneousGesture(toque, including: .all)
            .simultaneousGesture(segurar, including: ajustes == nil ? .none : .all)
            .sensoryFeedback(.impact(weight: .medium), trigger: seguradas)
            .accessibilityActions {
                if let sessoes { Button(String(localized: "Sessões", bundle: .module), action: sessoes) }
                if let ajustes { Button(String(localized: "Ajustes do bot", bundle: .module), action: ajustes) }
            }
    }

    /// Começo do toque (zera `gestoUsado`) e o puxão horizontal para as Sessões; o vertical fica com a rolagem.
    private var toque: some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { v in
                if !tocando {
                    tocando = true
                    gestoUsado = false
                }
                guard sessoes != nil else { return }
                let dx = v.translation.width
                let horizontal = dx < -MX.Espaco.dentroDeLinha && MXSessoesRegras.ehHorizontal(dx: dx, dy: v.translation.height)
                guard arrasto > 0 || horizontal else { return }
                arrasto = MXSessoesRegras.arrastoParaSessoes(-dx)
            }
            .onEnded { _ in
                tocando = false
                guard arrasto > 0 else { return }
                gestoUsado = true
                let abre = MXSessoesRegras.abreSessoesAoSoltar(arrasto)
                if reduzirMovimento {
                    arrasto = 0
                } else {
                    withAnimation(MX.Movimento.molaPuxar.animacao) { arrasto = 0 }
                }
                if abre { sessoes?() }
            }
    }

    private var segurar: some Gesture {
        LongPressGesture(minimumDuration: MX.Movimento.segurarParaAjustes)
            .onEnded { _ in
                guard arrasto == 0, let ajustes else { return }
                gestoUsado = true
                seguradas += 1
                ajustes()
            }
    }
}

extension View {
    /// Puxar para as Sessões e segurar para os ajustes (ver `MXGestosDoAgente`).
    func mxGestosDoAgente(sessoes: (() -> Void)?, ajustes: (() -> Void)?, gestoUsado: Binding<Bool>) -> some View {
        modifier(MXGestosDoAgente(sessoes: sessoes, ajustes: ajustes, gestoUsado: gestoUsado))
    }
}
