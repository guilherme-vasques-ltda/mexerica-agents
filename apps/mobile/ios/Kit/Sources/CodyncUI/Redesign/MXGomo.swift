import SwiftUI

/// O Gomo: sinal animado de "trabalhando" (`brand/apps/ios-v1-design/gomo.svg`).
/// Os 8 gomos acendem em roda (0,15 s de atraso cada, ciclo 1,2 s) e a roda gira em 8 s.
/// Com Reduzir movimento, fica parado e aceso (o quadro inicial do ciclo).
public struct MXGomo: View {
    let tamanho: CGFloat
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento

    public init(tamanho: CGFloat) {
        self.tamanho = tamanho
    }

    public var body: some View {
        TimelineView(.animation(paused: reduzirMovimento)) { contexto in
            let tempo = reduzirMovimento ? 0 : contexto.date.timeIntervalSinceReferenceDate
            Canvas { ctx, size in
                Self.desenhar(&ctx, lado: min(size.width, size.height), tempo: tempo)
            }
        }
        .frame(width: tamanho, height: tamanho)
        .accessibilityLabel(String(localized: "Trabalhando", bundle: .module))
    }

    // MARK: Geometria do SVG (caixa 24 × 24)

    nonisolated static let caixa: CGFloat = 24
    nonisolated static let aro: CGFloat = 11.4
    nonisolated static let miolo: CGFloat = 9.6
    nonisolated static let raioInterno: CGFloat = 1.8
    nonisolated static let raioExterno: CGFloat = 8.4
    /// O primeiro gomo vai de −87,75° a −47,27° (y para baixo); cada um dos seguintes gira 45°.
    nonisolated static let anguloInicial: Double = -87.75
    nonisolated static let anguloFinal: Double = -47.27

    /// O gomo `indice` na caixa de 24: borda interna reta, borda externa em arco.
    nonisolated static func caminho(indice: Int) -> Path {
        let centro = CGPoint(x: caixa / 2, y: caixa / 2)
        let giro = Double(indice) * 360 / Double(MX.Movimento.gomoQuantidade)
        let a0 = Angle(degrees: anguloInicial + giro)
        let a1 = Angle(degrees: anguloFinal + giro)
        let ponto = { (r: CGFloat, a: Angle) in CGPoint(x: centro.x + r * cos(a.radians), y: centro.y + r * sin(a.radians)) }
        var p = Path()
        p.move(to: ponto(raioInterno, a0))
        p.addLine(to: ponto(raioExterno, a0))
        p.addArc(center: centro, radius: raioExterno, startAngle: a0, endAngle: a1, clockwise: false)
        p.addLine(to: ponto(raioInterno, a1))
        p.closeSubpath()
        return p
    }

    /// A cor do gomo `indice` no instante `tempo` (s): laranja → meio (40%) → apagado (100%), linear.
    nonisolated static func cor(indice: Int, tempo: Double) -> MXCor {
        let ciclo = MX.Movimento.gomoCiclo
        let t = (tempo - Double(indice) * MX.Movimento.gomoAtraso).truncatingRemainder(dividingBy: ciclo)
        let bruta = (t < 0 ? t + ciclo : t) / ciclo
        let fase = bruta >= 1 - 1e-9 ? 0 : bruta // erro de ponto flutuante na virada do ciclo
        let meio = MX.Movimento.gomoMeioDoCiclo
        if fase <= meio { return misturar(MX.Claro.mexerica, MX.Claro.gomoMeio, fase / meio) }
        return misturar(MX.Claro.gomoMeio, MX.Claro.gomoApagado, (fase - meio) / (1 - meio))
    }

    /// Ângulo da roda no instante `tempo` (s), em graus.
    nonisolated static func giro(tempo: Double) -> Double {
        let volta = MX.Movimento.gomoVolta
        let t = tempo.truncatingRemainder(dividingBy: volta)
        return (t < 0 ? t + volta : t) / volta * 360
    }

    nonisolated static func misturar(_ a: MXCor, _ b: MXCor, _ f: Double) -> MXCor {
        let c = { (x: Double, y: Double) in Int(((x + (y - x) * f) * 255).rounded()) }
        return MXCor(r: c(a.r, b.r), g: c(a.g, b.g), b: c(a.b, b.b), a: 1)
    }

    nonisolated static func desenhar(_ ctx: inout GraphicsContext, lado: CGFloat, tempo: Double) {
        let escala = lado / caixa
        ctx.scaleBy(x: escala, y: escala)
        let centro = CGPoint(x: caixa / 2, y: caixa / 2)
        let disco = { (r: CGFloat) in Path(ellipseIn: CGRect(x: centro.x - r, y: centro.y - r, width: r * 2, height: r * 2)) }
        ctx.fill(disco(aro), with: .color(MX.Claro.mexerica.color))
        ctx.fill(disco(miolo), with: .color(MX.Claro.gomoMiolo.color))
        ctx.translateBy(x: centro.x, y: centro.y)
        ctx.rotate(by: .degrees(giro(tempo: tempo)))
        ctx.translateBy(x: -centro.x, y: -centro.y)
        for i in 0..<MX.Movimento.gomoQuantidade {
            ctx.fill(caminho(indice: i), with: .color(cor(indice: i, tempo: tempo).color))
        }
    }
}
