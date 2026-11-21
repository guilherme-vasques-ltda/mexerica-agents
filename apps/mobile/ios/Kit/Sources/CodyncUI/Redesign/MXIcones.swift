import CodyncKit
import SwiftUI

/// Ícones de traço do desenho (caixa 24 × 24, pontas e cantos arredondados), copiados dos SVG de
/// `telas/Conversas.dc.html` e, na etapa 2, de `telas/Chat.dc.html` e `telas/ChatAprovar.dc.html`.
public enum MXIcone: Sendable {
    case conta, setaParaBaixo, mais, busca, setaParaDireita, robo
    case missionBoard, conversas, equipes, rotinas, estado
    // Etapa 2 (Conversa, fila, menu do agente, aprovação):
    case voltar, whatsapp, microfone, onda, enviar, confirmar, lapis, lixeira, relogio, mao
    case ajustes, escudo, alfinete, parar
    // Etapa 3 (`telas/Sessoes.dc.html`, `ConfigBot.dc.html`):
    case arquivar, sessoes, ligar, sincronizado

    enum Parte: Sendable {
        case caminho(String)
        case circulo(cx: CGFloat, cy: CGFloat, r: CGFloat)
        case retangulo(x: CGFloat, y: CGFloat, w: CGFloat, h: CGFloat, r: CGFloat)
    }

    var partes: [Parte] {
        switch self {
        case .conta: [.circulo(cx: 12, cy: 8.5, r: 3.5), .caminho("M5 20c0-3.6 3.1-6 7-6s7 2.4 7 6")]
        case .setaParaBaixo: [.caminho("M6 9l6 6 6-6")]
        case .mais: [.caminho("M12 5v14M5 12h14")]
        case .busca: [.circulo(cx: 11, cy: 11, r: 6.5), .caminho("M16 16l4 4")]
        case .setaParaDireita: [.caminho("M9 6l6 6-6 6")]
        case .robo: [.retangulo(x: 5, y: 8, w: 14, h: 11, r: 3), .caminho("M12 4v4M9 13h.01M15 13h.01")]
        case .missionBoard: [.caminho("M8 21l2-12h4l2 12M7 9h10M9 9V6h6v3M12 3v3M10.5 15h3")]
        case .conversas: [.caminho("M4 6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-7l-5 4v-4H6a2 2 0 0 1-2-2z")]
        case .equipes: [.circulo(cx: 9, cy: 8, r: 3.2), .caminho("M3 20c0-3.3 2.7-5.5 6-5.5s6 2.2 6 5.5"),
                        .circulo(cx: 17.5, cy: 9, r: 2.5), .caminho("M17 14.6c2.4.2 4 2 4 4.4")]
        case .rotinas: [.retangulo(x: 4, y: 5, w: 16, h: 15, r: 3), .caminho("M8 3v4M16 3v4M4 10h16")]
        case .estado: [.retangulo(x: 5, y: 9, w: 14, h: 11, r: 3), .caminho("M7 6h10M9 3h6")]
        case .voltar: [.caminho("M15 6l-6 6 6 6")]
        case .whatsapp: [.caminho("M4 20l1.4-4A8 8 0 1 1 8 19z"), .caminho("M9 9.5c.3 1.9 1.7 3.3 3.6 3.7l1-.9 1.6.8")]
        case .microfone: [.retangulo(x: 9, y: 3, w: 6, h: 11, r: 3), .caminho("M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21")]
        case .onda: [.caminho("M4 10v4M8 7v10M12 4v16M16 8v8M20 10.5v3")]
        case .enviar: [.caminho("M12 19V5M6 11l6-6 6 6")]
        case .confirmar: [.caminho("M5 12.5l4.5 4.5L19 7.5")]
        case .lapis: [.caminho("M4 20h4L19 9l-4-4L4 16z"), .caminho("M13.5 6.5l4 4")]
        case .lixeira: [.caminho("M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3")]
        case .relogio: [.circulo(cx: 12, cy: 12, r: 9), .caminho("M12 7v5l3 2")]
        case .mao: [.caminho("M8 11V5.5a1.5 1.5 0 0 1 3 0V10m0-1.5V4a1.5 1.5 0 0 1 3 0v6m0-3.5a1.5 1.5 0 0 1 3 0V13a7 7 0 0 1-7 7h-.5A6.5 6.5 0 0 1 4 15.5L3.5 12a1.5 1.5 0 0 1 3-.5L8 13")]
        case .ajustes: [.caminho("M4 7h10M18 7h2M4 17h4M12 17h8"), .circulo(cx: 16, cy: 7, r: 2), .circulo(cx: 10, cy: 17, r: 2)]
        case .escudo: [.caminho("M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6z")]
        case .alfinete: [.caminho("M9 3h6l-1 6 4 4H6l4-4z"), .caminho("M12 13v8")]
        case .parar: [.retangulo(x: 6, y: 6, w: 12, h: 12, r: 2.5)]
        case .arquivar: [.retangulo(x: 3, y: 4, w: 18, h: 5, r: 1.5), .caminho("M5 9v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9M10 13h4")]
        case .sessoes: [.retangulo(x: 4, y: 9, w: 16, h: 11, r: 3), .caminho("M6.5 6h11M9 3h6")]
        case .ligar: [.caminho("M4 10v4M8 7v10M12 4v16M16 7v10M20 10v4")]
        case .sincronizado: [.caminho("M20 11a8 8 0 0 0-14.5-4.5L4 8M4 4v4h4M4 13a8 8 0 0 0 14.5 4.5L20 16M20 20v-4h-4")]
        }
    }

    /// Espessura do traço no desenho (na caixa de 24).
    var traco: CGFloat {
        switch self {
        case .conta: MX.Medida.iconeTracoFino
        case .setaParaBaixo, .setaParaDireita: MX.Medida.iconeTracoSeta
        case .mais: MX.Medida.iconeTracoMais
        case .busca, .robo: MX.Medida.iconeTraco
        case .missionBoard, .conversas, .equipes, .rotinas, .estado: MX.Medida.abaIconeTraco
        case .voltar, .relogio, .parar: MX.Conversa.voltarTraco
        case .enviar: MX.Conversa.enviarTraco
        case .confirmar: MX.Conversa.confirmarTraco
        case .onda: MX.Conversa.voltarTraco
        case .whatsapp, .microfone, .lapis, .lixeira, .mao, .ajustes, .escudo, .alfinete: MX.Conversa.iconeTraco
        case .arquivar, .sessoes, .ligar: MX.Sessoes.arquivarTraco
        case .sincronizado: MX.Medida.iconeTracoSeta
        }
    }

    /// O ícone inteiro na caixa de 24.
    var caminho: Path {
        var p = Path()
        for parte in partes {
            switch parte {
            case let .caminho(d): p.addPath(RedesignPonte.caminhoSVG(d))
            case let .circulo(cx, cy, r): p.addEllipse(in: CGRect(x: cx - r, y: cy - r, width: r * 2, height: r * 2))
            case let .retangulo(x, y, w, h, r): p.addRoundedRect(in: CGRect(x: x, y: y, width: w, height: h), cornerSize: CGSize(width: r, height: r))
            }
        }
        return p
    }
}

/// Um ícone de traço no tamanho dado, na cor do primeiro plano.
public struct MXIconeView: View {
    let icone: MXIcone
    let tamanho: CGFloat
    /// Traço na caixa de 24 quando o desenho usa outro neste lugar (ex.: o `+` do menu tem 1.9, o do topo 2.2).
    let traco: CGFloat?

    public init(_ icone: MXIcone, tamanho: CGFloat, traco: CGFloat? = nil) {
        self.icone = icone
        self.tamanho = tamanho
        self.traco = traco
    }

    public var body: some View {
        let escala = tamanho / 24
        icone.caminho
            .applying(CGAffineTransform(scaleX: escala, y: escala))
            .stroke(style: StrokeStyle(lineWidth: (traco ?? icone.traco) * escala, lineCap: .round, lineJoin: .round))
            .frame(width: tamanho, height: tamanho)
            .accessibilityHidden(true)
    }
}
