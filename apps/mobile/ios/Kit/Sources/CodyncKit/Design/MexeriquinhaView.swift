import SwiftUI
import UIKit

/// A Mexeriquinha de um bot, igual à do desktop. É um desenho parado: no iPhone ela não anima.
///
/// - `.simples`: a Mexeriquinha em vetor. Porta de `render-simples.ts` em `apps/desktop/src/renderer/lib/mexeriquinhas/`,
///   com os mesmos textos SVG (ver `SVGPath`).
/// - `.pelucia`: a Mexeriquinha de feltro, com um quadro por estado vindo de `Mexeriquinhas.xcassets`.
public struct MexeriquinhaView: View {
    let avatar: BotAvatar
    let state: MexeriquinhaState
    let size: CGFloat

    public init(avatar: BotAvatar, state: MexeriquinhaState = .livre, size: CGFloat = 40) {
        self.avatar = avatar
        self.state = state
        self.size = size
    }

    public var body: some View {
        Group {
            switch avatar {
            case let .simples(aparencia):
                MexeriquinhaSimplesView(aparencia: aparencia, state: state, size: size)
            case let .pelucia(conjunto, _):
                MexeriquinhaPeluciaView(conjunto: conjunto, state: state, size: size)
            }
        }
        .frame(width: size, height: size)
    }
}

/// O avatar de um bot no estado em que ele está agora.
public extension MexeriquinhaView {
    init(bot: Bot, size: CGFloat = 40, now: Date = Date()) {
        self.init(avatar: bot.avatar, state: bot.avatarState(now: now), size: size)
    }
}

// MARK: - Feltro

struct MexeriquinhaPeluciaView: View {
    let conjunto: String
    let state: MexeriquinhaState
    let size: CGFloat

    /// O quadro do conjunto para o estado, ou nulo se o catálogo não o tiver.
    static func quadro(conjunto: String, state: MexeriquinhaState) -> UIImage? {
        guard let asset = NSDataAsset(name: "mxq-\(conjunto)-\(state.quadro)", bundle: .module) else { return nil }
        return UIImage(data: asset.data)
    }

    var body: some View {
        if let image = Self.quadro(conjunto: conjunto, state: state) {
            Image(uiImage: image)
                .resizable()
                .scaledToFit()
                .frame(width: size, height: size)
                .overlay(alignment: .topTrailing) { selo }
        } else {
            // Sem o quadro, desenha a aparência do mesmo conjunto em vetor (nunca fica sem rosto).
            MexeriquinhaSimplesView(aparencia: MexeriquinhaData.conjuntos[conjunto] ?? MexeriquinhaData.padrao, state: state, size: size)
        }
    }

    @ViewBuilder private var selo: some View {
        if let selo = state.selo {
            Text(selo)
                .font(.system(size: size * 0.2, weight: .heavy, design: .rounded))
                .foregroundStyle(state == .precisaResposta ? Color.black : Color.white)
                .frame(width: size * 0.34, height: size * 0.34)
                .background(Self.fundo(state), in: Circle())
                .overlay(Circle().stroke(Color.black.opacity(0.7), lineWidth: max(1, size * 0.03)))
        }
    }

    private static func fundo(_ state: MexeriquinhaState) -> Color {
        switch state {
        case .dormindo: Painter.color("#3d4b8f")
        case .precisaResposta: Painter.color("#ffc93c")
        case .erro: Painter.color("#e5484d")
        case .livre, .trabalhando: .clear
        }
    }
}

// MARK: - Simples

struct MexeriquinhaSimplesView: View {
    let aparencia: MexeriquinhaAparencia
    let state: MexeriquinhaState
    let size: CGFloat

    var body: some View {
        Canvas { context, canvasSize in
            var painter = Painter(ctx: context)
            let side = min(canvasSize.width, canvasSize.height)
            painter.t = CGAffineTransform(scaleX: side / 128, y: side / 128)
            MexeriquinhaSimples.draw(&painter, aparencia: aparencia, state: state, compacto: side <= 40)
        }
        .frame(width: size, height: size)
    }
}

/// Um contexto de desenho com os atalhos do SVG: cor em hexadecimal, preenchimento e traço.
/// As transformações (escala da grade 128, `translate`, `rotate`) ficam em `t` e são aplicadas
/// a cada caminho, em vez de mudar o contexto: assim os degradês e as larguras seguem exatamente o SVG.
struct Painter {
    var ctx: GraphicsContext
    /// Do espaço do SVG (128 × 128) para pontos do Canvas.
    var t = CGAffineTransform.identity

    static func color(_ hex: String, _ opacity: Double = 1) -> Color {
        var digits = Array(hex.hasPrefix("#") ? hex.dropFirst() : Substring(hex))
        if digits.count == 3 { digits = digits.flatMap { [$0, $0] } }
        let value = UInt32(String(digits), radix: 16) ?? 0
        return Color(.sRGB, red: Double((value >> 16) & 255) / 255, green: Double((value >> 8) & 255) / 255,
                     blue: Double(value & 255) / 255, opacity: opacity)
    }

    /// A escala das transformações (as usadas são uniformes), para as larguras de traço.
    private var escala: Double { (t.a * t.a + t.b * t.b).squareRoot() }

    private func map(_ path: Path) -> Path { path.applying(t) }

    /// Um bloco com transformações e recortes que não vazam para o resto do desenho.
    func layer(_ body: (inout Painter) -> Void) {
        var copy = self
        body(&copy)
    }

    /// `<g transform="translate(x y)">`.
    mutating func translate(_ x: Double, _ y: Double) { t = t.translatedBy(x: x, y: y) }
    /// `rotate(graus)`, no sentido do SVG.
    mutating func rotate(_ degrees: Double) { t = t.rotated(by: degrees * .pi / 180) }
    /// `scale(s)`.
    mutating func scale(_ s: Double) { t = t.scaledBy(x: s, y: s) }
    /// `clip-path`.
    mutating func clip(_ path: Path) { ctx.clip(to: map(path)) }

    mutating func fill(_ d: String, _ hex: String, opacity: Double = 1, evenOdd: Bool = false) {
        fill(SVGPath.path(d), hex, opacity: opacity, evenOdd: evenOdd)
    }

    mutating func fill(_ path: Path, _ hex: String, opacity: Double = 1, evenOdd: Bool = false) {
        ctx.fill(map(path), with: .color(Self.color(hex, opacity)), style: FillStyle(eoFill: evenOdd))
    }

    /// `<path fill="none" stroke-linecap="round" stroke-linejoin="round">` (o `traco` do desktop).
    mutating func traco(_ d: String, _ hex: String, _ width: Double, opacity: Double = 1) {
        ctx.stroke(map(SVGPath.path(d)), with: .color(Self.color(hex, opacity)),
                   style: StrokeStyle(lineWidth: width * escala, lineCap: .round, lineJoin: .round))
    }

    /// Contorno com as pontas padrão do SVG (quina em ponta), ou arredondadas.
    mutating func contorno(_ path: Path, _ hex: String, _ width: Double, opacity: Double = 1, round: Bool = false) {
        ctx.stroke(map(path), with: .color(Self.color(hex, opacity)),
                   style: StrokeStyle(lineWidth: width * escala, lineCap: .butt, lineJoin: round ? .round : .miter, miterLimit: 4))
    }

    mutating func circle(_ cx: Double, _ cy: Double, _ r: Double, _ hex: String, opacity: Double = 1) {
        fill(Path(ellipseIn: CGRect(x: cx - r, y: cy - r, width: 2 * r, height: 2 * r)), hex, opacity: opacity)
    }

    mutating func ellipse(_ cx: Double, _ cy: Double, _ rx: Double, _ ry: Double, _ hex: String, opacity: Double = 1) {
        fill(Path(ellipseIn: CGRect(x: cx - rx, y: cy - ry, width: 2 * rx, height: 2 * ry)), hex, opacity: opacity)
    }

    mutating func rect(_ x: Double, _ y: Double, _ w: Double, _ h: Double, radius: Double, fill hex: String,
                       stroke: String, width: Double) {
        let path = Path(roundedRect: CGRect(x: x, y: y, width: w, height: h), cornerRadius: radius)
        fill(path, hex)
        contorno(path, stroke, width)
    }

    /// `<radialGradient cx=".36" cy=".3" r=".8">` (em `objectBoundingBox`) sobre a caixa `box` do elemento.
    /// O SVG deforma o degradê na caixa, então ele é elíptico (nos braços, 18 × 22: raios 14,4 e 17,6). Aqui o
    /// degradê é desenhado num contexto transformado pela caixa, num espaço unitário, mantendo o recorte do path.
    mutating func fillRadial(_ path: Path, box: CGRect, gradient: Gradient) {
        guard box.width > 0, box.height > 0 else { return }
        // Do espaço unitário da caixa para o SVG: escala (w, h), depois translação para o canto da caixa.
        let caixa = CGAffineTransform(a: box.width, b: 0, c: 0, d: box.height, tx: box.minX, ty: box.minY)
        var unitario = ctx  // cópia: a transformação não vaza, e o recorte já aplicado continua valendo
        unitario.concatenate(caixa.concatenating(t))
        unitario.fill(path.applying(caixa.inverted()),
                      with: .radialGradient(gradient, center: CGPoint(x: 0.36, y: 0.3), startRadius: 0, endRadius: 0.8))
    }

    /// Degradê vertical de `from` a `to` (no espaço do SVG).
    mutating func fillVertical(_ path: Path, from: CGPoint, to: CGPoint, gradient: Gradient) {
        ctx.fill(map(path), with: .linearGradient(gradient, startPoint: from.applying(t), endPoint: to.applying(t)))
    }
}

/// Porta de `apps/desktop/src/renderer/lib/mexeriquinhas/render-simples.ts`. Mesmas peças, mesma grade 128 × 128.
/// Diferenças: não anima, e a textura de pontinhos do corpo (5% de opacidade, só acima de 40 pt) não é desenhada.
enum MexeriquinhaSimples {
    static let tinta = "#2a1a12"
    static let corpo = "M64 24C91 24 108 44 108 72C108 96 94 109 64 109C34 109 20 96 20 72C20 44 37 24 64 24Z"
    static let gola = "M0 94Q64 84 128 94V128H0Z"
    /// Caixa do corpo e da gola (a caixa exata das curvas), para os degradês.
    static let caixaCorpo = CGRect(x: 20, y: 24, width: 88, height: 85)
    static let caixaGola = CGRect(x: 0, y: 89, width: 128, height: 39)

    struct Cor { let base: String; let clara: String; let escura: String }
    static let cores: [String: Cor] = [
        "laranja": Cor(base: "#f28a1e", clara: "#ffbf5c", escura: "#c45f0c"),
        "lima": Cor(base: "#94c630", clara: "#c8ec6e", escura: "#62921a"),
        "limao": Cor(base: "#f2c41e", clara: "#ffe57a", escura: "#c08f0a"),
        "toranja": Cor(base: "#ec5a5e", clara: "#ff9690", escura: "#b8343c"),
        "roxo": Cor(base: "#8d56d8", clara: "#bd92f4", escura: "#5f33a6"),
        "celeste": Cor(base: "#2f9cf0", clara: "#7cc8ff", escura: "#1a68bb"),
    ]
    static let roupaCor: [String: String] = [
        "blazer": "#2c3e6b", "colete": "#f3efe6", "moletom": "#d9b98b", "jaleco": "#f2f2ee", "polo": "#2e8b83",
    ]
    static let cobreFolha: Set<String> = ["coroa", "capacete", "boina", "bone", "cartola"]

    static func draw(_ p: inout Painter, aparencia a0: MexeriquinhaAparencia, state: MexeriquinhaState, compacto: Bool) {
        // Ids desconhecidos viram a opção base, como `normalizarConfig` no desktop.
        let a = a0.valida ? a0 : MexeriquinhaData.padrao
        let cor = cores[a.cor] ?? cores["laranja"]!
        let manga = roupaCor[a.roupa]
        let comOculos = a.olhos.hasPrefix("oculos")
        let tipoOlho: String = switch state {
        case .dormindo: "fechado"
        case .precisaResposta: "aberto"
        case .erro: "xx"
        case .livre, .trabalhando: a.olhos == "sonolentos" ? "sonolento" : a.olhos == "brilhantes" ? "brilhante" : a.olhos == "piscando" ? "piscando" : "ponto"
        }
        let degrade = Gradient(stops: [
            .init(color: Painter.color(cor.clara), location: 0),
            .init(color: Painter.color(cor.base), location: 0.55),
            .init(color: Painter.color(cor.escura), location: 1),
        ])
        let corpoPath = SVGPath.path(corpo)

        p.ellipse(64, 115, 34, 5, "#000", opacity: 0.28)

        // Braços: cor da roupa, ou o degradê do corpo.
        for (cx, angulo) in [(24.0, -28.0), (104.0, 28.0)] {
            var braco = p
            braco.translate(cx, 90)
            braco.rotate(angulo)
            braco.translate(-cx, -90)
            let caixa = CGRect(x: cx - 9, y: 79, width: 18, height: 22)
            if let manga {
                braco.fill(Path(ellipseIn: caixa), manga)
            } else {
                braco.fillRadial(Path(ellipseIn: caixa), box: caixa, gradient: degrade)
            }
        }

        p.fillRadial(corpoPath, box: caixaCorpo, gradient: degrade)
        p.contorno(corpoPath, cor.escura, compacto ? 2.5 : 1.5, opacity: 0.55)

        if a.roupa != "nenhuma" {
            p.layer { g in
                g.clip(corpoPath)
                g.translate(0, 4)
                roupa(&g, a.roupa, compacto)
                let sombra = Gradient(stops: [
                    .init(color: Color.white.opacity(0.2), location: 0),
                    .init(color: Color.white.opacity(0), location: 0.45),
                    .init(color: Color.black.opacity(0.22), location: 1),
                ])
                g.fillVertical(SVGPath.path(gola), from: CGPoint(x: 0, y: caixaGola.minY), to: CGPoint(x: 0, y: caixaGola.maxY), gradient: sombra)
            }
        }
        if !compacto && a.objeto == "gravata" {
            p.fill("M61 94H67L68 103L64 109L60 103Z", "#d23a3a")
            p.fill("M60.5 92.5H67.5L66 97H62Z", "#b02c2c")
        }
        if a.acessorio == "capuz" {
            p.fill(SVGPath.path("M64 14C100 14 118 44 118 76C118 100 108 116 64 116C20 116 10 100 10 76C10 44 28 14 64 14ZM64 46C86 46 98 60 98 76C98 94 84 104 64 104C44 104 30 94 30 76C30 60 42 46 64 46Z"),
                   "#2b2b31", evenOdd: true)
            p.contorno(SVGPath.path("M64 14C100 14 118 44 118 76C118 100 108 116 64 116C20 116 10 100 10 76C10 44 28 14 64 14ZM64 46C86 46 98 60 98 76C98 94 84 104 64 104C44 104 30 94 30 76C30 60 42 46 64 46Z"),
                       "#4a4a54", 2)
        }
        // Bochechas.
        p.fill("M31.5 82a6.5 4.5 0 1 0 13 0a6.5 4.5 0 1 0-13 0ZM83.5 82a6.5 4.5 0 1 0 13 0a6.5 4.5 0 1 0-13 0Z", "#ff7b8a", opacity: 0.6)

        olhos(&p, tipoOlho, compacto)
        oculos(&p, a.olhos, compacto)
        sobrancelhas(&p, state, comOculos: comOculos, compacto: compacto)
        boca(&p, state, compacto)

        if a.acessorio != "nenhum" && a.acessorio != "capuz" {
            p.layer { g in
                g.translate(0, -6)
                acessorio(&g, a.acessorio, compacto)
            }
        }
        if !cobreFolha.contains(a.acessorio) { folha(&p, compacto) }
        if !compacto { objeto(&p, a.objeto) }
        selo(&p, state, compacto)
    }

    // MARK: Peças

    static func folha(_ p: inout Painter, _ compacto: Bool) {
        p.traco("M63 26C62 20 62.5 16 64.5 12", "#5b7a2a", compacto ? 5 : 4)
        p.fill("M65 18C70 6 84 3 95 7C90 19 78 24 65 18Z", "#4fa03a")
        if !compacto { p.traco("M67 17C75 13 83 10 91 8", "#3b7f2a", 1.2) }
    }

    static func roupa(_ p: inout Painter, _ r: String, _ compacto: Bool) {
        if let base = roupaCor[r] { p.fill(gola, base) }
        switch r {
        case "blazer":
            p.fill("M53 89L64 106L75 89Q64 87 53 89Z", "#f4f1ea")
            if !compacto { p.traco("M53 89L61 103M75 89L67 103", "#1d2b4f", 2) }
        case "colete":
            p.fill("M0 94Q40 86 54 89L64 110V128H0ZM128 94Q88 86 74 89L64 110V128H128Z", "#4c7a3c")
        case "moletom":
            p.traco("M42 91Q64 104 86 91", "#bf9d6c", compacto ? 6 : 5)
            if !compacto { p.traco("M58 98V109M70 98V109", "#f6efe2", 2.2) }
        case "avental":
            p.fill("M47 91Q64 88 81 91V128H47Z", "#ecdcbc")
            if !compacto { p.traco("M48 92L38 87M80 92L90 87M55 104H73V112H55Z", "#c8b086", 2) }
        case "jaleco":
            p.fill("M54 89L64 104L74 89Z", "#2e4b74")
            if !compacto { p.traco("M54 89L62 106M74 89L66 106M80 102H91", "#c9c9c0", 2) }
        case "polo":
            p.fill("M52 89L58 98L64 91ZM76 89L70 98L64 91Z", "#44aaa0")
            if !compacto { p.traco("M64 91V104", "#22706a", 1.8) }
        default:
            break
        }
    }

    static func olhos(_ p: inout Painter, _ tipo: String, _ compacto: Bool) {
        let w = compacto ? 4.4 : 2.8
        let r = compacto ? 6.6 : 5.2
        func ponto(_ p: inout Painter, _ x: Double, _ rr: Double) {
            p.circle(x, 70, rr, tinta)
            if !compacto { p.circle(x + 1.8, 68.2, 1.7, "#fff") }
        }
        switch tipo {
        case "ponto":
            ponto(&p, 48, r); ponto(&p, 80, r)
        case "aberto":
            ponto(&p, 48, r + 0.8); ponto(&p, 80, r + 0.8)
        case "sonolento":
            p.fill("M42 69A6 6 0 0 0 54 69ZM74 69A6 6 0 0 0 86 69Z", tinta)
            p.traco("M41 69H55M73 69H87", tinta, w)
        case "brilhante":
            p.circle(48, 70, r + 1.6, tinta); p.circle(80, 70, r + 1.6, tinta)
            if !compacto {
                for x in [48.0, 80.0] {
                    p.fill("M\(x) 65.5L\(x + 1.2) 68.8L\(x + 4.5) 70L\(x + 1.2) 71.2L\(x) 74.5L\(x - 1.2) 71.2L\(x - 4.5) 70L\(x - 1.2) 68.8Z", "#fff")
                }
            }
        case "piscando":
            ponto(&p, 48, r)
            p.traco("M74 72Q80 64 86 72", tinta, w)
        case "fechado":
            p.traco("M42 69Q48 75 54 69M74 69Q80 75 86 69", tinta, w)
        case "xx":
            p.traco("M43 65L53 70L43 75M85 65L75 70L85 75", tinta, w)
        default:
            break
        }
    }

    static func oculos(_ p: inout Painter, _ o: String, _ compacto: Bool) {
        let w = compacto ? 4.0 : 3.0
        let hastes = "M37 68L27 65M91 68L101 65"
        if o == "oculos-redondos" {
            let lentes = SVGPath.path("M38 70a10 10 0 1 0 20 0a10 10 0 1 0-20 0ZM70 70a10 10 0 1 0 20 0a10 10 0 1 0-20 0Z")
            p.fill(lentes, "#fff", opacity: 0.14)
            p.contorno(lentes, "#1e1a18", w)
            p.traco("M58 69Q64 65 70 69", "#1e1a18", w)
            p.traco(hastes, "#1e1a18", w)
        } else if o == "oculos-escuros" || o == "oculos-verdes" {
            let lentes = SVGPath.path("M36 62H59V69A9 9 0 0 1 50 78H45A9 9 0 0 1 36 69ZM69 62H92V69A9 9 0 0 1 83 78H78A9 9 0 0 1 69 69Z")
            p.fill(lentes, o == "oculos-verdes" ? "#20b455" : "#141416")
            p.contorno(lentes, "#0d0d0f", 2, round: true)
            p.traco("M59 64H69", "#0d0d0f", w)
            p.traco(hastes, "#1e1a18", w)
            if !compacto { p.traco("M40 65.5H46M73 65.5H79", "#fff", 2, opacity: 0.45) }
        }
    }

    static func sobrancelhas(_ p: inout Painter, _ estado: MexeriquinhaState, comOculos: Bool, compacto: Bool) {
        let y = comOculos ? 54.0 : 59.0
        let w = compacto ? 4.2 : 2.8
        if estado == .trabalhando {
            p.traco("M41 \(y)L53 \(y + 3)M87 \(y)L75 \(y + 3)", tinta, w)
        } else if estado == .precisaResposta {
            p.traco("M42 \(y + 3)L53 \(y)M86 \(y + 3)L75 \(y)", tinta, w)
        }
    }

    static func boca(_ p: inout Painter, _ estado: MexeriquinhaState, _ compacto: Bool) {
        let w = compacto ? 4.4 : 2.8
        switch estado {
        case .livre: p.traco("M57 82Q64 89 71 82", tinta, w)
        case .trabalhando: p.traco("M58 84Q64 86 70 84", tinta, w)
        case .dormindo: p.traco("M60 84Q64 87 68 84", tinta, w)
        case .precisaResposta: p.ellipse(64, 85, compacto ? 4.4 : 3.2, compacto ? 5 : 3.8, tinta)
        case .erro: p.traco("M56 86q2 -3 4 0t4 0t4 0t4 0", tinta, w)
        }
    }

    static func acessorio(_ p: inout Painter, _ a: String, _ compacto: Bool) {
        switch a {
        case "coroa":
            let coroa = SVGPath.path("M40 42L36 20L51 31L64 14L77 31L92 20L88 42Z")
            p.fill(coroa, "#f4b731")
            p.contorno(coroa, "#c98b14", 1.5, round: true)
            if !compacto {
                p.fill("M33 20a3 3 0 1 0 6 0a3 3 0 1 0-6 0ZM61 14a3 3 0 1 0 6 0a3 3 0 1 0-6 0ZM89 20a3 3 0 1 0 6 0a3 3 0 1 0-6 0Z", "#ffd45e")
                p.circle(64, 35, 2.8, "#e04848")
            }
        case "fone":
            p.traco("M20 72C18 12 110 12 108 72", "#3a3a42", 6)
            for x in [12.0, 101.0] { p.rect(x, 60, 15, 26, radius: 7, fill: "#34343a", stroke: "#4e4e58", width: 1.5) }
            if !compacto {
                p.traco("M20 85Q24 98 43 96", "#34343a", 3)
                p.circle(46, 96, 3.6, "#34343a")
            }
        case "capacete":
            p.fill("M30 52C30 20 98 20 98 52Z", "#f6c12a")
            if !compacto { p.traco("M64 24V49", "#ffdc72", 5) }
            p.fill("M22 52Q64 44 106 52Q107 57 102 57Q64 50 26 57Q21 57 22 52Z", "#d9a015")
        case "boina":
            p.fill("M26 46C20 30 46 18 74 20C100 22 110 34 102 46C86 52 42 52 26 46Z", "#d63c3c")
            p.circle(72, 19, 3.2, "#b02c2c")
            if !compacto { p.traco("M30 46C46 50 84 50 100 45", "#a92a2a", 2) }
        case "bone":
            p.fill("M30 50C30 20 98 20 98 50Z", "#f3b13a")
            p.fill("M34 47Q18 48 10 56Q30 58 52 50Z", "#d8962a")
            if !compacto {
                p.traco("M64 23V50", "#d8962a", 1.5)
                p.circle(64, 23, 2.6, "#d8962a")
            }
        case "flor":
            p.layer { g in
                g.translate(90, 32)
                for (x, y) in [(0.0, -6.8), (6.5, -2.1), (4.0, 5.5), (-4.0, 5.5), (-6.5, -2.1)] { g.circle(x, y, 6.3, "#f58fb3") }
                g.circle(0, 0, 4.2, "#ffcf3a")
            }
        case "cartola":
            let aba = Path(ellipseIn: CGRect(x: 28, y: 35.5, width: 72, height: 13))
            p.fill(aba, "#1f1f24")
            p.contorno(aba, "#4a4a54", 1.5)
            let copa = SVGPath.path("M44 42L46 6H82L84 42Z")
            p.fill(copa, "#2a2a30")
            p.contorno(copa, "#4a4a54", 1.5, round: true)
            p.fill("M45.3 30H82.7L83.3 39H44.7Z", "#6c3fa6")
        default:
            break
        }
    }

    static func objeto(_ p: inout Painter, _ o: String) {
        switch o {
        case "moeda":
            let moeda = Path(ellipseIn: CGRect(x: 9, y: 75, width: 22, height: 22))
            p.fill(moeda, "#f4b731")
            p.contorno(moeda, "#c98b14", 2)
            p.traco("M24 81Q20 78 17 80Q14 83 18 85.5Q23 88 22 91Q19 94 15.5 91.5M19.7 77V95", "#9a6a0c", 2)
        case "chave":
            p.traco("M18 80V104M18 97H25M18 102H24", "#e3a823", 3.6)
            p.contorno(Path(ellipseIn: CGRect(x: 12, y: 68, width: 12, height: 12)), "#e3a823", 3.6)
        case "megafone":
            p.traco("M24 90L22 99", "#e04848", 4)
            let corneta = SVGPath.path("M30 80L9 70V100L30 90Z")
            p.fill(corneta, "#f2eee8")
            p.contorno(corneta, "#cfc6ba", 1.5, round: true)
            p.ellipse(9, 85, 3.4, 15, "#e04848")
        case "notebook":
            p.rect(84, 82, 30, 21, radius: 2.5, fill: "#2c2f37", stroke: "#a8aeb8", width: 2)
            p.traco("M95 88l-4 4l4 4M103 88l4 4l-4 4M101 87l-4 10", "#8fe3a0", 1.6)
            p.fill("M80 103H118L115 108H83Z", "#c3c8d0")
        case "lupa":
            p.traco("M106 82L116 94", "#6b4a2b", 5)
            let lente = Path(ellipseIn: CGRect(x: 89, y: 64, width: 20, height: 20))
            p.fill(lente, "#d7ecff", opacity: 0.35)
            p.contorno(lente, "#2c2c32", 3.5)
        case "pincel":
            p.traco("M98 106L113 80", "#8b5a2b", 4)
            p.traco("M113 80L116.5 74", "#b9bec6", 5)
            p.fill("M116 75Q122 66 118 58Q111 64 114 73Z", "#e44d8a")
        case "caixa":
            p.rect(5, 82, 24, 20, radius: 2, fill: "#c99a5b", stroke: "#9c7240", width: 1.5)
            p.traco("M17 82V90", "#ecd7ad", 3)
        case "carrinho":
            p.traco("M4 80H9L13 96H29L32 84H11", "#3a3a40", 2.6)
            p.fill("M12.4 101a2.6 2.6 0 1 0 5.2 0a2.6 2.6 0 1 0-5.2 0ZM24.4 101a2.6 2.6 0 1 0 5.2 0a2.6 2.6 0 1 0-5.2 0Z", "#3a3a40")
        case "coracao":
            let coracao = SVGPath.path("M20 97C7 89 9 77 15.5 77C18.5 77 20 80 20 80C20 80 21.5 77 24.5 77C31 77 33 89 20 97Z")
            p.fill(coracao, "#f05a7e")
            p.contorno(coracao, "#c93d61", 1.2)
        case "ferramenta":
            p.traco("M100 106L113 84", "#8e96a3", 5)
            p.fill(SVGPath.path("M108 80a7 7 0 1 0 14 0a7 7 0 1 0-14 0ZM113.2 73.6h3.6V80h-3.6Z"), "#8e96a3", evenOdd: true)
        default:
            break
        }
    }

    /// O selo do estado no canto: zzz, ? ou ×. Nunca some, nem no tamanho pequeno.
    static func selo(_ p: inout Painter, _ estado: MexeriquinhaState, _ compacto: Bool) {
        guard let tipo = estado.selo else { return }
        let (cx, cy, r) = compacto ? (99.0, 29.0, 22.0) : (103.0, 27.0, 14.0)
        let fundo = estado == .dormindo ? "#3d4b8f" : estado == .precisaResposta ? "#ffc93c" : "#e5484d"
        let tinta = estado == .precisaResposta ? Self.tinta : "#fff"
        let anel = Path(ellipseIn: CGRect(x: cx - r, y: cy - r, width: 2 * r, height: 2 * r))
        p.fill(anel, fundo)
        p.contorno(anel, "#17120e", compacto ? 4 : 3)
        p.layer { g in
            g.translate(cx, cy)
            g.scale(r / 14)
            switch tipo {
            case "zzz":
                if compacto { g.traco("M-5-5H5L-5 5H5", tinta, 3.2) } else { g.traco("M-6-3H1L-6 5H1M3-8H7L3-3.5H7", tinta, 2.2) }
            case "?":
                g.traco("M-4-3.5C-4-8.5 4-8.5 4-3.5C4-.5 0-.5 0 2.5", tinta, compacto ? 3.4 : 2.8)
                g.circle(0, 7, compacto ? 2.1 : 1.8, tinta)
            default:
                g.traco("M-4.5-4.5L4.5 4.5M4.5-4.5L-4.5 4.5", tinta, compacto ? 3.4 : 3)
            }
        }
    }
}
