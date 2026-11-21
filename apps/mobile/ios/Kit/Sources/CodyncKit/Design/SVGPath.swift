import SwiftUI

/// Lê o atributo `d` de um `<path>` do SVG e devolve um `Path`.
///
/// A Mexeriquinha Simples do desktop (`render-simples.ts`) é uma lista de textos SVG. Em vez de
/// reescrever cada peça à mão, o iPhone usa os mesmos textos e esta leitura: assim o desenho não
/// se afasta do original. Aceita M, L, H, V, C, S, Q, T, A e Z, em maiúscula e minúscula.
///
/// Subconjunto aceito (o que os caminhos copiados do desktop usam): números decimais com sinal, separados por
/// espaço, vírgula ou só pelo sinal/ponto ("-4-3.5", ".5.5"). NÃO lê expoentes ("1e-3") nem os flags de arco
/// compactados ("a1 1 0 01.5 .5"): os dois flags precisam vir como números comuns. Ao incorporar um SVG novo,
/// confira que ele não usa essas formas, ou complete este leitor antes.
enum SVGPath {
    static func path(_ d: String) -> Path {
        var reader = Reader(Array(d.unicodeScalars))
        var path = Path()
        var current = CGPoint.zero
        var start = CGPoint.zero
        // Último ponto de controle, para S e T (reflexo).
        var lastCubic: CGPoint?
        var lastQuad: CGPoint?
        var command: Character = " "

        while true {
            reader.skipSeparators()
            if reader.atEnd { break }
            if let letter = reader.command() {
                command = letter
                if letter == "Z" || letter == "z" {
                    path.closeSubpath()
                    current = start
                    lastCubic = nil; lastQuad = nil
                    continue
                }
            } else if command == " " || command == "Z" || command == "z" {
                break  // número sem comando: texto inválido
            }
            let relative = command.isLowercase
            let dx = relative ? current.x : 0
            let dy = relative ? current.y : 0
            var nextCubic: CGPoint?
            var nextQuad: CGPoint?

            switch command {
            case "M", "m":
                guard let x = reader.number(), let y = reader.number() else { return path }
                current = CGPoint(x: x + dx, y: y + dy)
                start = current
                path.move(to: current)
                // Pares seguintes de um M são linhas.
                command = relative ? "l" : "L"
            case "L", "l":
                guard let x = reader.number(), let y = reader.number() else { return path }
                current = CGPoint(x: x + dx, y: y + dy)
                path.addLine(to: current)
            case "H", "h":
                guard let x = reader.number() else { return path }
                current = CGPoint(x: x + dx, y: current.y)
                path.addLine(to: current)
            case "V", "v":
                guard let y = reader.number() else { return path }
                current = CGPoint(x: current.x, y: y + dy)
                path.addLine(to: current)
            case "C", "c":
                guard let x1 = reader.number(), let y1 = reader.number(), let x2 = reader.number(),
                      let y2 = reader.number(), let x = reader.number(), let y = reader.number() else { return path }
                let c2 = CGPoint(x: x2 + dx, y: y2 + dy)
                let end = CGPoint(x: x + dx, y: y + dy)
                path.addCurve(to: end, control1: CGPoint(x: x1 + dx, y: y1 + dy), control2: c2)
                nextCubic = c2
                current = end
            case "S", "s":
                guard let x2 = reader.number(), let y2 = reader.number(), let x = reader.number(), let y = reader.number() else { return path }
                let c1 = lastCubic.map { CGPoint(x: 2 * current.x - $0.x, y: 2 * current.y - $0.y) } ?? current
                let c2 = CGPoint(x: x2 + dx, y: y2 + dy)
                let end = CGPoint(x: x + dx, y: y + dy)
                path.addCurve(to: end, control1: c1, control2: c2)
                nextCubic = c2
                current = end
            case "Q", "q":
                guard let x1 = reader.number(), let y1 = reader.number(), let x = reader.number(), let y = reader.number() else { return path }
                let control = CGPoint(x: x1 + dx, y: y1 + dy)
                let end = CGPoint(x: x + dx, y: y + dy)
                path.addQuadCurve(to: end, control: control)
                nextQuad = control
                current = end
            case "T", "t":
                guard let x = reader.number(), let y = reader.number() else { return path }
                let control = lastQuad.map { CGPoint(x: 2 * current.x - $0.x, y: 2 * current.y - $0.y) } ?? current
                let end = CGPoint(x: x + dx, y: y + dy)
                path.addQuadCurve(to: end, control: control)
                nextQuad = control
                current = end
            case "A", "a":
                guard let rx = reader.number(), let ry = reader.number(), let rotation = reader.number(),
                      let large = reader.number(), let sweep = reader.number(), let x = reader.number(), let y = reader.number() else { return path }
                let end = CGPoint(x: x + dx, y: y + dy)
                addArc(to: &path, from: current, rx: rx, ry: ry, rotation: rotation, large: large != 0, sweep: sweep != 0, end: end)
                current = end
            default:
                return path  // comando desconhecido
            }
            lastCubic = nextCubic
            lastQuad = nextQuad
        }
        return path
    }

    /// Arco elíptico do SVG (apêndice F.6 da especificação) como curvas de Bézier de até 90°.
    private static func addArc(to path: inout Path, from p0: CGPoint, rx rx0: Double, ry ry0: Double, rotation: Double,
                               large: Bool, sweep: Bool, end p1: CGPoint) {
        var rx = abs(rx0), ry = abs(ry0)
        if p0 == p1 { return }
        if rx == 0 || ry == 0 { path.addLine(to: p1); return }
        let phi = rotation * .pi / 180
        let cosPhi = cos(phi), sinPhi = sin(phi)
        let hx = (p0.x - p1.x) / 2, hy = (p0.y - p1.y) / 2
        let x1p = cosPhi * hx + sinPhi * hy
        let y1p = -sinPhi * hx + cosPhi * hy
        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry)
        if lambda > 1 { let s = lambda.squareRoot(); rx *= s; ry *= s }
        let numerator = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p
        let denominator = rx * rx * y1p * y1p + ry * ry * x1p * x1p
        let sign: Double = large == sweep ? -1 : 1
        let coefficient = denominator == 0 ? 0 : sign * max(0, numerator / denominator).squareRoot()
        let cxp = coefficient * rx * y1p / ry
        let cyp = -coefficient * ry * x1p / rx
        let cx = cosPhi * cxp - sinPhi * cyp + (p0.x + p1.x) / 2
        let cy = sinPhi * cxp + cosPhi * cyp + (p0.y + p1.y) / 2
        let ux = (x1p - cxp) / rx, uy = (y1p - cyp) / ry
        let vx = (-x1p - cxp) / rx, vy = (-y1p - cyp) / ry
        let theta1 = atan2(uy, ux)
        var delta = atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        if !sweep && delta > 0 { delta -= 2 * .pi }
        if sweep && delta < 0 { delta += 2 * .pi }

        let segments = max(1, Int((abs(delta) / (.pi / 2)).rounded(.up)))
        let step = delta / Double(segments)
        let k = 4.0 / 3.0 * tan(step / 4)
        func point(_ x: Double, _ y: Double) -> CGPoint {
            let ex = x * rx, ey = y * ry
            return CGPoint(x: cosPhi * ex - sinPhi * ey + cx, y: sinPhi * ex + cosPhi * ey + cy)
        }
        for i in 0 ..< segments {
            let a = theta1 + Double(i) * step
            let b = a + step
            let c1 = point(cos(a) - k * sin(a), sin(a) + k * cos(a))
            let c2 = point(cos(b) + k * sin(b), sin(b) - k * cos(b))
            // O último ponto é exatamente o destino, sem erro de arredondamento.
            let end = i == segments - 1 ? p1 : point(cos(b), sin(b))
            path.addCurve(to: end, control1: c1, control2: c2)
        }
    }

    private struct Reader {
        let scalars: [Unicode.Scalar]
        var index = 0

        init(_ scalars: [Unicode.Scalar]) { self.scalars = scalars }

        var atEnd: Bool { index >= scalars.count }

        mutating func skipSeparators() {
            while index < scalars.count, scalars[index] == " " || scalars[index] == "," || scalars[index] == "\n" || scalars[index] == "\t" {
                index += 1
            }
        }

        /// Uma letra de comando, se for a próxima.
        mutating func command() -> Character? {
            guard index < scalars.count else { return nil }
            let c = scalars[index]
            guard (c.value >= 65 && c.value <= 90) || (c.value >= 97 && c.value <= 122) else { return nil }
            index += 1
            return Character(c)
        }

        mutating func number() -> Double? {
            skipSeparators()
            let begin = index
            if index < scalars.count, scalars[index] == "-" || scalars[index] == "+" { index += 1 }
            var digits = 0
            while index < scalars.count, scalars[index].value >= 48, scalars[index].value <= 57 { index += 1; digits += 1 }
            if index < scalars.count, scalars[index] == "." {
                index += 1
                while index < scalars.count, scalars[index].value >= 48, scalars[index].value <= 57 { index += 1; digits += 1 }
            }
            guard digits > 0 else { index = begin; return nil }
            let text = String(String.UnicodeScalarView(scalars[begin ..< index]))
            return Double(text)
        }
    }
}
