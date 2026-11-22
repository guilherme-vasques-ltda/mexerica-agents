import CodyncKit
import SwiftUI
import UIKit

/// Selos que aparecem no canto da pelúcia.
public enum MXSeloPelucia: Equatable, Sendable {
    /// Gomo animado embaixo à direita: alguma sessão rodando.
    case gomo
    /// "z" em cima à direita: dormindo.
    case sono
}

/// Regras da pelúcia (sem desenho), para a view e para os testes.
public enum MXPeluciaRegras {
    /// O quadro do catálogo para o estado. `piscando` só vale para livre.
    public static func quadro(_ estado: MexeriquinhaState, piscando: Bool = false) -> String {
        switch estado {
        case .livre: piscando ? "piscar" : "livre-1"
        case .trabalhando: "trabalhando-1"
        case .precisaResposta: "precisa-resposta"
        case .dormindo: "dormindo-1"
        case .erro: "erro"
        }
    }

    /// Selos do desenho: gomo quando há sessão rodando (ou o próprio agente trabalha), "z" quando dorme.
    public static func selos(_ estado: MexeriquinhaState, sessoesRodando: Int) -> [MXSeloPelucia] {
        var selos: [MXSeloPelucia] = []
        if sessoesRodando > 0 || estado == .trabalhando { selos.append(.gomo) }
        if estado == .dormindo { selos.append(.sono) }
        return selos
    }

    /// Balança enquanto trabalha (no protótipo, sempre que há sessão rodando).
    public static func balanca(_ estado: MexeriquinhaState, sessoesRodando: Int) -> Bool {
        sessoesRodando > 0 || estado == .trabalhando
    }

    /// O balanço no instante `tempo` (s): de −2° e escala 1 × 1 até +2° e 1.03 × 0.97 no meio do ciclo, e volta.
    public static func balanco(tempo: Double) -> (graus: Double, escalaX: CGFloat, escalaY: CGFloat) {
        let m = MX.Movimento.self
        let fase = (1 - cos(2 * .pi * tempo / m.peluciaCiclo)) / 2 // 0 → 1 → 0, suave nas pontas
        return (-m.peluciaRotacao + 2 * m.peluciaRotacao * fase,
                1 + (m.peluciaEscalaX - 1) * fase,
                1 + (m.peluciaEscalaY - 1) * fase)
    }

    /// Se a pelúcia livre está piscando no instante `tempo`, com o deslocamento `atraso` do ciclo.
    public static func piscando(tempo: Double, atraso: Double = 0) -> Bool {
        let m = MX.Movimento.self
        let t = (tempo + atraso).truncatingRemainder(dividingBy: m.piscarCiclo)
        // As datas da agenda caem exatamente nas bordas; com o tempo absoluto (~8e8 s) o arredondamento joga a fase
        // para 0,92999… no início e 0,95999… no fim, invertendo a piscada. A folga de 1e-6 (5 µs) põe cada data
        // do lado certo da borda.
        let fase = (t < 0 ? t + m.piscarCiclo : t) / m.piscarCiclo + 1e-6
        return fase >= m.piscarInicio && fase < m.piscarFim
    }

    /// Atraso estável por nome, para as pelúcias não piscarem todas juntas.
    static func atraso(_ nome: String) -> Double {
        let soma = nome.unicodeScalars.reduce(UInt32(5381)) { ($0 &* 33) &+ $1.value }
        return Double(soma % 1000) / 1000 * MX.Movimento.piscarCiclo
    }
}

/// O rosto de um agente: um conjunto de feltro do catálogo, ou o desenho que o Hub manda para o bot (a Mexeriquinha
/// Simples em vetor, quando ele não tem pelúcia). Etapa 2: os bots reais nem sempre têm conjunto de feltro.
public enum MXRosto: Hashable, Sendable {
    case conjunto(String)
    case desenho(BotAvatar)

    /// O rosto de um bot real (`Bot.avatar`): feltro quando há conjunto, senão o desenho.
    public init(_ avatar: BotAvatar) {
        if case let .pelucia(conjunto, _) = avatar { self = .conjunto(conjunto) } else { self = .desenho(avatar) }
    }

    /// O conjunto de feltro, se houver.
    public var conjunto: String? {
        if case let .conjunto(c) = self { return c }
        return nil
    }

    /// O mesmo rosto como `BotAvatar` (para o desenho de reserva do app atual).
    var avatar: BotAvatar {
        switch self {
        case let .conjunto(c): .pelucia(conjunto: c, assetId: nil)
        case let .desenho(a): a
        }
    }

    /// Chave estável (atraso do piscar).
    var chave: String {
        switch self {
        case let .conjunto(c): c
        case let .desenho(a): String(describing: a)
        }
    }
}

/// A pelúcia (Mexeriquinha de feltro) de um agente, com a expressão do estado, o balanço de quem trabalha,
/// o piscar de quem está livre e os selos (gomo, "z"). Proporções de selo do protótipo na pelúcia de 48.
public struct MXPelucia: View {
    let rosto: MXRosto
    let estado: MexeriquinhaState
    let tamanho: CGFloat
    let sessoesRodando: Int
    let comSelos: Bool
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento

    public init(rosto: MXRosto, estado: MexeriquinhaState, tamanho: CGFloat, sessoesRodando: Int = 0, comSelos: Bool = true) {
        self.rosto = rosto
        self.estado = estado
        self.tamanho = tamanho
        self.sessoesRodando = sessoesRodando
        self.comSelos = comSelos
    }

    public init(conjunto: String, estado: MexeriquinhaState, tamanho: CGFloat, sessoesRodando: Int = 0, comSelos: Bool = true) {
        self.init(rosto: .conjunto(conjunto), estado: estado, tamanho: tamanho, sessoesRodando: sessoesRodando, comSelos: comSelos)
    }

    public var body: some View {
        figura
            .frame(width: tamanho, height: tamanho)
            .overlay(alignment: .bottomTrailing) {
                if comSelos, MXPeluciaRegras.selos(estado, sessoesRodando: sessoesRodando).contains(.gomo) { seloGomo }
            }
            .overlay(alignment: .topTrailing) {
                if comSelos, MXPeluciaRegras.selos(estado, sessoesRodando: sessoesRodando).contains(.sono) { seloSono }
            }
            .accessibilityHidden(true)
    }

    @ViewBuilder private var figura: some View {
        if MXPeluciaRegras.balanca(estado, sessoesRodando: sessoesRodando) {
            TimelineView(.animation(paused: reduzirMovimento)) { contexto in
                let b = reduzirMovimento ? (graus: 0, escalaX: CGFloat(1), escalaY: CGFloat(1))
                    : MXPeluciaRegras.balanco(tempo: contexto.date.timeIntervalSinceReferenceDate)
                quadro(piscando: false)
                    .scaleEffect(x: b.escalaX, y: b.escalaY, anchor: MX.Movimento.peluciaOrigem)
                    .rotationEffect(.degrees(b.graus), anchor: MX.Movimento.peluciaOrigem)
            }
        } else if estado == .livre && !reduzirMovimento && rosto.conjunto != nil {
            let atraso = MXPeluciaRegras.atraso(rosto.chave)
            TimelineView(MXAgendaDoPiscar(atraso: atraso)) { contexto in
                quadro(piscando: MXPeluciaRegras.piscando(tempo: contexto.date.timeIntervalSinceReferenceDate, atraso: atraso))
            }
        } else {
            quadro(piscando: false)
        }
    }

    @ViewBuilder private func quadro(piscando: Bool) -> some View {
        let nome = MXPeluciaRegras.quadro(estado, piscando: piscando)
        if let conjunto = rosto.conjunto,
           let imagem = MXQuadros.imagem(conjunto: conjunto, quadro: nome) ?? MXQuadros.imagem(conjunto: conjunto, quadro: MXPeluciaRegras.quadro(estado)) {
            Image(uiImage: imagem).resizable().interpolation(.high).scaledToFit()
        } else {
            // Conjunto sem quadro, ou bot sem pelúcia: o desenho em vetor existente, que nunca fica sem rosto.
            MexeriquinhaView(avatar: rosto.avatar, state: estado, size: tamanho)
        }
    }

    private var proporcao: CGFloat { tamanho / MX.Medida.pelucia }

    private var seloGomo: some View {
        let m = MX.Medida.self
        return Circle()
            .fill(MX.Claro.fundoSeloGomo.color)
            .shadow(color: .black.opacity(m.seloSombraOpacidade), radius: m.seloSombraRaio, y: m.seloSombraY)
            .frame(width: m.seloGomo * proporcao, height: m.seloGomo * proporcao)
            .overlay { MXGomo(tamanho: m.seloGomoDesenho * proporcao) }
            .offset(x: m.seloGomoForaDireita * proporcao, y: m.seloGomoForaAbaixo * proporcao)
    }

    private var seloSono: some View {
        let m = MX.Medida.self
        return Text("z")
            .mxTipo(MX.Tipo.seloSono)
            .foregroundStyle(MX.Claro.texto.color)
            .frame(width: m.seloSono * proporcao, height: m.seloSono * proporcao)
            .background(MX.Claro.gomo.color, in: Circle())
            .overlay(Circle().strokeBorder(MX.Claro.contornoSelo.color, lineWidth: m.seloSonoContorno))
            .offset(x: m.seloSonoForaDireita * proporcao, y: -m.seloSonoForaAcima * proporcao)
            .dynamicTypeSize(...DynamicTypeSize.large) // selo de tamanho fixo: o "z" não cresce para fora do disco
    }
}

/// Só acorda a tela no começo e no fim de cada piscada (não a 60 quadros por segundo).
struct MXAgendaDoPiscar: TimelineSchedule {
    let atraso: Double

    func entries(from startDate: Date, mode: TimelineScheduleMode) -> AnyIterator<Date> {
        let m = MX.Movimento.self
        let ciclo = m.piscarCiclo
        let marcas = [m.piscarInicio * ciclo, m.piscarFim * ciclo]
        let inicio = startDate.timeIntervalSinceReferenceDate + atraso
        var volta = (inicio / ciclo).rounded(.down)
        var proxima = 0
        var primeira = true
        return AnyIterator {
            if primeira { primeira = false; return startDate }
            while true {
                let t = volta * ciclo + marcas[proxima] - atraso
                proxima += 1
                if proxima == marcas.count { proxima = 0; volta += 1 }
                if t > startDate.timeIntervalSinceReferenceDate { return Date(timeIntervalSinceReferenceDate: t) }
            }
        }
    }
}

/// Quadros de feltro, com cache (a lista redesenha a cada piscada e balanço).
/// Os cinco quadros de estado vêm do catálogo do CodyncKit (`Mexeriquinhas.xcassets`, da entrega dos avatares);
/// o `piscar` é do catálogo do redesenho (`Resources/Redesign/Redesign.xcassets`, `mx-piscar-<conjunto>`).
enum MXQuadros {
    @MainActor private static var cache: [String: UIImage] = [:]

    @MainActor static func imagem(conjunto: String, quadro: String) -> UIImage? {
        let chave = "\(conjunto)/\(quadro)"
        if let pronta = cache[chave] { return pronta }
        let imagem: UIImage? = if quadro == "piscar" {
            NSDataAsset(name: "mx-piscar-\(conjunto)", bundle: .module).flatMap { UIImage(data: $0.data) }
        } else {
            RedesignPonte.quadroPelucia(conjunto: conjunto, quadro: quadro)
        }
        guard let imagem else { return nil }
        cache[chave] = imagem
        return imagem
    }
}
