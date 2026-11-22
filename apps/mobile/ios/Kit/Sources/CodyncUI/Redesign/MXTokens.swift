import SwiftUI
import UIKit

/// Tokens do redesenho iOS v1 da Mexerica, um lugar só.
///
/// Fonte: `brand/apps/ios-v1-design/tokens-ios.json` do repositório `mexerica` (aprovado pelo dono em 11/11/2026),
/// e, para medidas que o JSON não traz, `telas/Conversas.dc.html` do mesmo pacote. Views do redesenho não levam
/// número solto: tudo que é cor, raio, espaço, tipo ou movimento sai daqui.
public enum MX {}

// MARK: - Cor

/// Uma cor do token, guardada como números para poder ser testada.
public struct MXCor: Equatable, Sendable {
    public let r: Double
    public let g: Double
    public let b: Double
    public let a: Double

    /// `#RRGGBB`, com opacidade opcional (os `rgba(...)` do JSON).
    public init(_ hex: String, opacidade: Double = 1) {
        let s = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        precondition(s.count == 6 && UInt32(s, radix: 16) != nil, "Cor de token inválida: \(hex)")
        let v = UInt32(s, radix: 16) ?? 0
        r = Double((v >> 16) & 0xFF) / 255
        g = Double((v >> 8) & 0xFF) / 255
        b = Double(v & 0xFF) / 255
        a = opacidade
    }

    /// `rgba(r,g,b,a)` do JSON.
    public init(r: Int, g: Int, b: Int, a: Double) {
        self.r = Double(r) / 255
        self.g = Double(g) / 255
        self.b = Double(b) / 255
        self.a = a
    }

    public var color: Color { Color(.sRGB, red: r, green: g, blue: b, opacity: a) }

    /// `#RRGGBB` (sem a opacidade), para testes e logs.
    public var hex: String {
        String(format: "#%02X%02X%02X", Int((r * 255).rounded()), Int((g * 255).rounded()), Int((b * 255).rounded()))
    }

    /// Esta cor pousada (opaca) sobre `fundo`. Base das superfícies de "Reduzir transparência".
    public func sobre(_ fundo: MXCor) -> MXCor {
        let mix = { (c: Double, f: Double) in c * a + f * (1 - a) }
        return MXCor(r: Int((mix(r, fundo.r) * 255).rounded()), g: Int((mix(g, fundo.g) * 255).rounded()),
                     b: Int((mix(b, fundo.b) * 255).rounded()), a: 1)
    }
}

public extension MX {
    /// `cor.claro` do JSON. O app usa o claro nesta etapa; o escuro fica declarado para a etapa 7.
    enum Claro {
        public static let fundo = MXCor("#F6EEDC")
        public static let texto = MXCor("#2A1C14")
        public static let textoSecundario = MXCor("#6E5A4C")
        public static let linha = MXCor(r: 42, g: 28, b: 20, a: 0.08)
        public static let borda = MXCor("#94785F")
        public static let mexerica = MXCor("#F58A1F")
        public static let mexericaForte = MXCor("#DD6F10")
        public static let mexericaTexto = MXCor("#B4530A")
        /// Texto sobre laranja: marrom, nunca branco.
        public static let sobreMexerica = MXCor("#2A1C14")
        public static let sucesso = MXCor("#5C9B3A")
        public static let sucessoTexto = MXCor("#3F7A26")
        public static let erro = MXCor("#D92D20")
        public static let erroTexto = MXCor("#B42318")
        public static let esperaFundo = MXCor(r: 245, g: 197, b: 24, a: 0.26)
        public static let esperaTexto = MXCor("#6B4E00")
        public static let rodandoFundo = MXCor(r: 92, g: 155, b: 58, a: 0.16)
        public static let rodandoTexto = MXCor("#3F7A26")
        public static let gomo = MXCor("#FFC170")
        public static let balaoDoUsuario = MXCor("#F58A1F")
        public static let balaoDoAgente = MXCor(r: 255, g: 255, b: 255, a: 0.88)

        // Do Conversas.dc.html (não estão no JSON):
        /// Ponto da pílula "esperando você".
        public static let esperaPonto = MXCor("#D9A400")
        /// Fundo da etiqueta de função ("Dev", "Roteiros").
        public static let etiqueta = MXCor(r: 42, g: 28, b: 20, a: 0.07)
        /// Realce da aba ativa: laranja claro.
        public static let abaAtiva = MXCor(r: 245, g: 138, b: 31, a: 0.16)
        /// Contorno branco dos selos (contador da aba, "z").
        public static let contornoSelo = MXCor("#FFFFFF")
        /// Fundo do selo do gomo na pelúcia.
        public static let fundoSeloGomo = MXCor("#FFFFFF")
        /// Disco claro dentro do gomo e cor apagada dos gomos.
        public static let gomoMiolo = MXCor("#FFF1DC")
        public static let gomoApagado = MXCor("#FFE2BC")
        public static let gomoMeio = MXCor("#FFB45E")
    }

    /// `cor.escuro` do JSON: grafite neutro, sem papel de parede. Declarado; aplicado na etapa 7.
    /// Os cinzas ainda não estão no design system (decisão em aberto no README).
    enum Escuro {
        public static let fundo = MXCor("#1E1E21")
        public static let superficie = MXCor("#2D2D32")
        public static let campo = MXCor("#2A2A2F")
        public static let linha = MXCor("#3C3C42")
        public static let etiqueta = MXCor("#3A3A40")
        public static let texto = MXCor("#F5F5F7")
        public static let textoSecundario = MXCor("#AEAEB5")
        public static let mexerica = MXCor("#F58A1F")
        public static let mexericaTexto = MXCor("#FFA24D")
        public static let sucessoTexto = MXCor("#8CC86A")
        public static let esperaTexto = MXCor("#F5C518")
    }
}

// MARK: - Vidro

public extension MX {
    /// Vidro. No iOS 26 é o Liquid Glass nativo; os `rgba` do JSON eram a aproximação do protótipo web e
    /// aqui só servem de base para "Reduzir transparência" (vidro vira superfície opaca).
    enum Vidro {
        /// Botão redondo e cápsula: `rgba(255,255,255,0.72)` pousado no creme.
        public static let opacoBotao = MXCor(r: 255, g: 255, b: 255, a: 0.72).sobre(Claro.fundo)
        /// Painel (cartão do Berga, lista): `rgba(255,255,255,0.70–0.80)`; usamos 0.72 pousado no creme.
        public static let opacoPainel = MXCor(r: 255, g: 255, b: 255, a: 0.72).sobre(Claro.fundo)
        /// Busca: `rgba(255,255,255,0.55)` pousado no creme.
        public static let opacoCampo = MXCor(r: 255, g: 255, b: 255, a: 0.55).sobre(Claro.fundo)
        /// Barra de abas: `rgba(255,255,255,0.78)` pousado no creme.
        public static let opacoBarra = MXCor(r: 255, g: 255, b: 255, a: 0.78).sobre(Claro.fundo)
        /// Contorno fino das superfícies opacas (o `0 0 0 1px rgba(42,28,20,0.06–0.08)` do protótipo).
        public static let contornoOpaco = MXCor(r: 42, g: 28, b: 20, a: 0.08)
        /// Tinta branca leve sobre o vidro nativo, para ele puxar para o branco leitoso do desenho.
        public static let tinturaClara = MXCor(r: 255, g: 255, b: 255, a: 0.45)
        /// Espessura do contorno das superfícies opacas (o `1px` do mesmo `box-shadow`).
        public static let contornoOpacoEspessura: CGFloat = 1
    }
}

// MARK: - Raio, espaço

public extension MX {
    /// `raio` do JSON.
    enum Raio {
        public static let chip: CGFloat = 12
        public static let linha: CGFloat = 14
        public static let cartao: CGFloat = 18
        public static let painel: CGFloat = 22
        public static let folha: CGFloat = 28
        /// `999` no JSON: cápsula. No SwiftUI, use `Capsule()`; o número fica para teste.
        public static let capsula: CGFloat = 999
        /// Linha de agente arrastável (Conversas.dc.html).
        public static let linhaAgente: CGFloat = 16
    }

    /// `espaco` do JSON.
    enum Espaco {
        public static let margemTela: CGFloat = 12
        public static let entreBlocos: CGFloat = 10
        public static let dentroDeLinha: CGFloat = 8
        public static let alvoMinimo: CGFloat = 44
    }
}

// MARK: - Movimento

/// Mola com amortecimento e resposta, como no JSON.
public struct MXMola: Equatable, Sendable {
    public let amortecimento: Double
    public let resposta: Double
    public var animacao: Animation { .spring(response: resposta, dampingFraction: amortecimento) }
}

/// Como uma troca de estado anda na tela (abrir Canais, mudar de aba).
public enum MXTroca: Equatable, Sendable {
    /// Movimento normal: interpola ângulo e posição com a mola dos tokens.
    case mola(MXMola)
    /// Reduzir movimento: um estado substitui o outro com cross-fade curto; nada gira nem desliza.
    case crossFade(duracao: Double)

    /// Se a troca pode girar ou deslocar coisas (seta que gira, realce que corre entre as abas).
    /// No cross-fade, não: a view troca o glifo ou o realce e só a opacidade anima.
    public var interpola: Bool {
        if case .mola = self { return true }
        return false
    }

    public var animacao: Animation {
        switch self {
        case let .mola(mola):
            return mola.animacao
        case let .crossFade(duracao):
            return Animation.easeInOut(duration: duracao)
        }
    }

    /// A animação da troca de estado em si, que também move o layout (linhas abaixo descem ou sobem).
    /// No cross-fade é `nil`: nada muda de posição animado; o layout pula para o final e só as linhas que entram
    /// aparecem por opacidade (com `animacao`, presa à transição delas).
    public var animacaoDaPosicao: Animation? {
        interpola ? animacao : nil
    }

    /// No cross-fade, o que sai (linhas de Canais ao recolher) primeiro esmaece no lugar e só depois deixa o
    /// layout, sem animar posição: assim não fica por cima do que sobe. Com a mola, sai junto com o movimento.
    public var saidaEmDuasFases: Bool { !interpola }
}

public extension MX {
    /// `movimento` do JSON.
    enum Movimento {
        public static let molaPadrao = MXMola(amortecimento: 1.0, resposta: 0.35)
        public static let molaFolha = MXMola(amortecimento: 0.86, resposta: 0.32)
        /// Ao soltar um puxão; deve herdar a velocidade do dedo.
        public static let molaPuxar = MXMola(amortecimento: 0.8, resposta: 0.3)

        /// Toque: escala no toque para baixo (não ao soltar).
        public static let toqueEscala: CGFloat = 0.97
        public static let toqueDuracao: Double = 0.1

        public static let puxarParaSessoesLimiar: CGFloat = 80
        public static let puxarParaSessoesMaximo: CGFloat = 150
        public static let puxarSessaoLimiar: CGFloat = 70
        public static let puxarSessaoMaximo: CGFloat = 140
        public static let voltarArrastandoLimiar: CGFloat = 90
        public static let segurarParaAjustes: Double = 0.55

        /// Gomo: 8 gomos, ciclo 1,2 s, 0,15 s entre gomos, volta inteira em 8 s.
        public static let gomoQuantidade = 8
        public static let gomoCiclo: Double = 1.2
        public static let gomoAtraso: Double = 0.15
        public static let gomoVolta: Double = 8.0
        /// Pontos-chave da cor de cada gomo no ciclo (do `gomo.svg`): 0% laranja, 40% meio, 100% apagado.
        public static let gomoMeioDoCiclo: Double = 0.4

        /// Pelúcia trabalhando: balança ±2°, escala 1.03 × 0.97, ciclo 1,8 s, a partir da base.
        public static let peluciaCiclo: Double = 1.8
        public static let peluciaRotacao: Double = 2
        public static let peluciaEscalaX: CGFloat = 1.03
        public static let peluciaEscalaY: CGFloat = 0.97
        /// "A partir da base": `transform-origin: 50% 92%` no protótipo.
        public static let peluciaOrigem = UnitPoint(x: 0.5, y: 0.92)

        /// Piscar da pelúcia livre (do desktop, `mexeriquinhas.css`): a cada 5 s, de 93% a 96% do ciclo.
        public static let piscarCiclo: Double = 5
        public static let piscarInicio: Double = 0.93
        public static let piscarFim: Double = 0.96

        /// Botão ao vivo: ciclo 1,6 s, opacidade 1 → 0.35.
        public static let aoVivoCiclo: Double = 1.6
        public static let aoVivoOpacidadeMinima: Double = 0.35

        /// Reduzir movimento: "trocas viram cross-fade curto". O JSON não dá número; 0,2 s é nosso.
        public static let crossFadeCurto: Double = 0.2
        public static var crossFade: Animation { MXTroca.crossFade(duracao: crossFadeCurto).animacao }

        /// A troca do redesenho: a mola padrão, ou o cross-fade curto com Reduzir movimento.
        public static func troca(reduzir: Bool) -> MXTroca {
            reduzir ? .crossFade(duracao: crossFadeCurto) : .mola(molaPadrao)
        }

        /// A animação da troca (mola padrão ou cross-fade curto).
        public static func padrao(reduzir: Bool) -> Animation {
            troca(reduzir: reduzir).animacao
        }
    }
}

// MARK: - Tipo

/// Uma família do desenho.
public enum MXFamilia: String, Sendable {
    /// Títulos: Nunito ExtraBold/Bold.
    case nunito = "Nunito-Regular"
    /// Interface: DM Sans.
    case dmSans = "DMSans-9ptRegular"
}

/// Um estilo de texto: família, tamanho e peso do desenho, e o estilo do sistema que guia o Dynamic Type.
public struct MXTipo: Equatable, Sendable {
    public let familia: MXFamilia
    public let tamanho: CGFloat
    /// Peso no eixo `wght` (400 regular, 500 medium, 600 semibold, 700 bold, 800 extrabold).
    public let peso: CGFloat
    public let relativoA: UIFont.TextStyle
    /// Altura da linha do desenho (`line-height`), quando ele define; escala junto com o tamanho.
    public let entrelinha: CGFloat?

    public init(_ familia: MXFamilia, _ tamanho: CGFloat, _ peso: CGFloat, entrelinha: CGFloat? = nil, relativoA: UIFont.TextStyle) {
        self.familia = familia
        self.tamanho = tamanho
        self.peso = peso
        self.entrelinha = entrelinha
        self.relativoA = relativoA
    }
}

public extension MX {
    /// `tipo` do JSON e medidas do Conversas.dc.html. Tudo escala com o Dynamic Type.
    enum Tipo {
        public static let display = MXTipo(.nunito, 30, 800, relativoA: .largeTitle)
        public static let tituloDeTela = MXTipo(.nunito, 24, 800, entrelinha: 28, relativoA: .title1)
        public static let nomeNoCartao = MXTipo(.nunito, 22, 800, entrelinha: 28, relativoA: .title2)
        public static let corpo = MXTipo(.dmSans, 15, 400, entrelinha: 22, relativoA: .body)
        public static let corpoForte = MXTipo(.dmSans, 15, 600, entrelinha: 22, relativoA: .body)
        public static let campo = MXTipo(.dmSans, 16, 400, relativoA: .body)
        public static let previa = MXTipo(.dmSans, 14, 400, entrelinha: 20, relativoA: .subheadline)
        public static let previaForte = MXTipo(.dmSans, 14, 600, entrelinha: 20, relativoA: .subheadline)
        public static let legenda = MXTipo(.dmSans, 12.5, 500, entrelinha: 16, relativoA: .footnote)
        public static let legendaForte = MXTipo(.dmSans, 12.5, 600, entrelinha: 16, relativoA: .footnote)
        /// Linha de estado no cartão do Berga ("Livre"): 12.5/18.
        public static let legendaEstado = MXTipo(.dmSans, 12.5, 500, entrelinha: 18, relativoA: .footnote)
        public static let contador = MXTipo(.dmSans, 12, 600, relativoA: .caption1)
        public static let rotulo = MXTipo(.dmSans, 11, 600, entrelinha: 14, relativoA: .caption2)
        public static let rotuloAba = MXTipo(.dmSans, 10.5, 600, entrelinha: 14, relativoA: .caption2)
        public static let contadorAba = MXTipo(.dmSans, 10.5, 700, entrelinha: 14, relativoA: .caption2)
        public static let pilula = MXTipo(.dmSans, 9.5, 700, entrelinha: 12, relativoA: .caption2)
        public static let seloSono = MXTipo(.dmSans, 10, 600, relativoA: .caption2)
        public static let cerquilhaCabecalho = MXTipo(.dmSans, 17, 600, relativoA: .body)
        public static let cerquilha = MXTipo(.dmSans, 16, 400, relativoA: .body)
        /// "Pessoal ⌄" no topo: 15 semibold.
        public static let seletor = MXTipo(.dmSans, 15, 600, relativoA: .body)
    }
}

// MARK: - Medidas de componentes (Conversas.dc.html)

public extension MX {
    enum Medida {
        /// Distância da barra de abas até a borda de baixo da tela.
        public static let barraAbasDaBorda: CGFloat = 26
        public static let barraAbasRespiro: CGFloat = 5
        public static let barraAbasEntreItens: CGFloat = 2
        public static let abaAltura: CGFloat = 52
        public static let abaLargura: CGFloat = 64
        /// "Mission Board" é mais largo.
        public static let abaLarguraLonga: CGFloat = 84
        public static let abaIcone: CGFloat = 22
        public static let abaIconeTraco: CGFloat = 1.9
        /// Rótulo da aba: pode encolher até 80% antes de cortar (cabe "Mission Board" em 84 até o texto "muito grande").
        public static let abaRotuloEscalaMinima: CGFloat = 0.8
        /// Teto do nome da aba, como a barra nativa do iOS: o nome fica sempre visível e para de crescer em "muito
        /// grande" (em "extra extra extra grande", "Mission Board" e "Conversas" não caberiam nem a 80%). O nome e o
        /// ícone grandes ficam no visualizador de conteúdo grande (segurar a aba).
        public static var abaRotuloAte: DynamicTypeSize { .xLarge }
        /// Teto do nome nos tamanhos de acessibilidade, com abas de 68: o tamanho do desenho.
        public static var abaRotuloAteAcessivel: DynamicTypeSize { .large }
        /// Nos tamanhos de acessibilidade, o contador desce para baixo do ícone, dentro da própria aba (a barra
        /// fica mais alta): no canto ele cobriria a aba vizinha. Distância entre o ícone e o contador.
        public static let abaContadorAbaixoDoIcone: CGFloat = 2
        /// Folga de cada lado do contador dentro da aba, nos tamanhos de acessibilidade.
        public static let abaContadorFolgaLateral: CGFloat = 2
        /// Largura de cada aba nos tamanhos de acessibilidade: o maior texto do contador ("9+") cabe inteiro em
        /// "extra extra extra grande", na fonte pedida, sem encolher (medido no teste). A barra fica com 358 pt.
        public static let abaLarguraAcessivel: CGFloat = 68
        public static let abaEntreIconeERotulo: CGFloat = 1
        public static let abaContador: CGFloat = 18
        public static let abaContadorContorno: CGFloat = 2
        public static let abaContadorRespiro: CGFloat = 5
        /// Deslocamento do contador da aba: 5 acima e 9 à direita do canto do ícone.
        public static let abaContadorAcima: CGFloat = 5
        public static let abaContadorDireita: CGFloat = 9

        /// Topo da tela: distância do topo da área segura até a linha dos botões (54 − 47 da barra de status).
        public static let topoAbaixoDaAreaSegura: CGFloat = 7
        /// Traço dos ícones de 24 (Conversas.dc.html): conta 1.8; busca e robô 2; mais 2.2; setas 2.4.
        public static let iconeTracoFino: CGFloat = 1.8
        public static let iconeTraco: CGFloat = 2
        public static let iconeTracoMais: CGFloat = 2.2
        public static let iconeTracoSeta: CGFloat = 2.4
        public static let botaoRedondo: CGFloat = 44
        public static let botaoRedondoIcone: CGFloat = 22
        /// Teto do ícone dos botões do topo com texto grande (o círculo vai a 50).
        public static let botaoRedondoIconeTeto: CGFloat = 28
        public static let capsulaAltura: CGFloat = 36
        public static let capsulaRespiroEsquerda: CGFloat = 14
        public static let capsulaRespiroDireita: CGFloat = 12
        public static let capsulaSeta: CGFloat = 14
        public static let topoRespiroLateral: CGFloat = 2

        public static let berga: CGFloat = 76
        public static let bergaRespiro = EdgeInsets(top: 8, leading: 6, bottom: 10, trailing: 14)
        public static let bergaEntreFotoETexto: CGFloat = 6
        public static let pontoDeEstado: CGFloat = 8

        public static let buscaAltura: CGFloat = 40
        public static let buscaIcone: CGFloat = 18
        public static let buscaRespiro: CGFloat = 12

        public static let painelRespiro: CGFloat = 6
        /// Espaço no fim da lista para ela passar inteira acima da barra de abas.
        public static let painelFolgaFinal: CGFloat = 120
        public static let canaisAltura: CGFloat = 44
        public static let canalAltura: CGFloat = 40
        public static let canalRecuo: CGFloat = 36
        public static let cerquilhaLargura: CGFloat = 18
        public static let seta: CGFloat = 16
        public static let respiroDeLinha: CGFloat = 10
        public static let divisoria: CGFloat = 1
        public static let divisoriaAcima: CGFloat = 4
        public static let divisoriaAbaixo: CGFloat = 6
        public static let cabecalhoAltura: CGFloat = 32
        public static let cabecalhoIcone: CGFloat = 16

        public static let agenteAltura: CGFloat = 62
        public static let agenteRespiro: CGFloat = 4
        public static let pelucia: CGFloat = 48
        public static let entreItens: CGFloat = 6
        public static let etiquetaRespiro = EdgeInsets(top: 2, leading: 7, bottom: 2, trailing: 7)

        public static let pilulaAltura: CGFloat = 14
        public static let pilulaPonto: CGFloat = 4
        public static let pilulaEntre: CGFloat = 2
        public static let pilulaRespiro = EdgeInsets(top: 0, leading: 3, bottom: 0, trailing: 4)
        public static let contador: CGFloat = 20
        public static let contadorRespiro: CGFloat = 6

        /// Selo do gomo na pelúcia de 48: disco branco de 20 com o gomo de 16, 3 para fora à direita, 1 embaixo.
        public static let seloGomo: CGFloat = 20
        public static let seloGomoDesenho: CGFloat = 16
        public static let seloGomoForaDireita: CGFloat = 3
        public static let seloGomoForaAbaixo: CGFloat = 1
        /// Selo "z": 18, com contorno branco de 2; 1 acima e 2 à direita.
        public static let seloSono: CGFloat = 18
        public static let seloSonoContorno: CGFloat = 2
        public static let seloSonoForaAcima: CGFloat = 1
        public static let seloSonoForaDireita: CGFloat = 2
        /// Sombra do selo do gomo: `0 1px 4px rgba(0,0,0,0.2)`.
        public static let seloSombraRaio: CGFloat = 2
        public static let seloSombraY: CGFloat = 1
        public static let seloSombraOpacidade: Double = 0.2
    }
}

// MARK: - Etapa 2: Conversa, fila, menu do agente e aprovação

public extension MX {
    /// Cores da Conversa que não estão no JSON (`telas/Chat.dc.html` e `telas/ChatAprovar.dc.html`).
    enum CorDaConversa {
        /// Contorno fino de balões, cartões e chips: `0 0 0 1px rgba(42,28,20,0.06)`.
        public static let contorno = MXCor(r: 42, g: 28, b: 20, a: 0.06)
        /// Sombra leve do balão do agente: `0 6px 18px rgba(42,28,20,0.06)`.
        public static let sombraDoBalao = MXCor(r: 42, g: 28, b: 20, a: 0.06)
        /// Sombra laranja do balão do usuário e do botão ao vivo: `rgba(221,111,16,0.25)`.
        public static let sombraLaranja = MXCor(r: 221, g: 111, b: 16, a: 0.25)
        /// Sombra do botão Permitir uma vez: `rgba(221,111,16,0.3)`.
        public static let sombraDoPermitir = MXCor(r: 221, g: 111, b: 16, a: 0.3)
        /// Chip "Hoje 19:22": `rgba(255,255,255,0.78)`.
        public static let chipDeHora = MXCor(r: 255, g: 255, b: 255, a: 0.78)
        /// Contorno da pílula de passos: `rgba(42,28,20,0.14)`.
        public static let bordaDosPassos = MXCor(r: 42, g: 28, b: 20, a: 0.14)
        /// Fundo da lista de passos e da tabela do cartão de aprovação: `rgba(42,28,20,0.05)`.
        public static let fundoDosPassos = MXCor(r: 42, g: 28, b: 20, a: 0.05)
        /// Botão neutro (Cancelar, Sempre permitir): `rgba(42,28,20,0.07)`.
        public static let neutro = MXCor(r: 42, g: 28, b: 20, a: 0.07)
        /// Botão Parar: `rgba(42,28,20,0.08)`.
        public static let parar = MXCor(r: 42, g: 28, b: 20, a: 0.08)
        /// Linha da fila em edição: `rgba(245,138,31,0.14)`.
        public static let filaEditando = MXCor(r: 245, g: 138, b: 31, a: 0.14)
        /// Disco do "Mandar agora": `rgba(245,138,31,0.2)`.
        public static let mandarAgora = MXCor(r: 245, g: 138, b: 31, a: 0.2)
        /// Selo "Esperando você": `rgba(245,138,31,0.18)`.
        public static let seloEsperando = MXCor(r: 245, g: 138, b: 31, a: 0.18)
        /// Pílulas de modelo e contexto: `rgba(255,255,255,0.7)`.
        public static let rodape = MXCor(r: 255, g: 255, b: 255, a: 0.7)
        /// Trilho do anel de contexto: `rgba(42,28,20,0.2)`.
        public static let trilhoDoContexto = MXCor(r: 42, g: 28, b: 20, a: 0.2)
        /// Cartão de aprovação: `rgba(255,255,255,0.94)`.
        public static let cartaoDeAprovacao = MXCor(r: 255, g: 255, b: 255, a: 0.94)
        /// Contorno do cartão de aprovação: `rgba(42,28,20,0.08)`.
        public static let contornoDoCartao = MXCor(r: 42, g: 28, b: 20, a: 0.08)
        /// Sombra do cartão de aprovação: `0 10px 30px rgba(42,28,20,0.12)`.
        public static let sombraDoCartao = MXCor(r: 42, g: 28, b: 20, a: 0.12)
        /// Véu atrás do menu do agente: `rgba(42,28,20,0.1)`.
        public static let veuDoMenu = MXCor(r: 42, g: 28, b: 20, a: 0.1)
        /// Sombra do menu: `0 18px 44px rgba(42,28,20,0.2)`.
        public static let sombraDoMenu = MXCor(r: 42, g: 28, b: 20, a: 0.2)
        /// Preenchimento que cresce enquanto se segura Permitir (nosso: o laranja forte do JSON).
        public static let segurando = MXCor(r: 221, g: 111, b: 16, a: 0.55)
    }

    /// Tipos da Conversa (Chat.dc.html, ChatAprovar.dc.html). Escalam com o Dynamic Type, sem teto.
    enum TipoDaConversa {
        public static let nomeNaCapsula = MXTipo(.dmSans, 15, 600, entrelinha: 19, relativoA: .body)
        public static let estadoNaCapsula = MXTipo(.dmSans, 12, 600, entrelinha: 15, relativoA: .caption1)
        public static let balaoDoAgente = MXTipo(.dmSans, 15, 400, entrelinha: 22, relativoA: .body)
        public static let balaoDoUsuario = MXTipo(.dmSans, 15, 400, entrelinha: 21, relativoA: .body)
        public static let autor = MXTipo(.dmSans, 13, 600, relativoA: .footnote)
        public static let horaDoBalao = MXTipo(.dmSans, 12, 400, relativoA: .caption1)
        public static let chipDeHora = MXTipo(.dmSans, 12, 500, relativoA: .caption1)
        public static let passos = MXTipo(.dmSans, 12.5, 500, relativoA: .footnote)
        public static let passo = MXTipo(.dmSans, 13, 400, relativoA: .footnote)
        /// Passo dentro do balão de quem trabalha (ChatAprovar): 14.
        public static let passoEmCurso = MXTipo(.dmSans, 14, 400, relativoA: .subheadline)
        public static let atividade = MXTipo(.dmSans, 13, 400, entrelinha: 18, relativoA: .footnote)
        public static let status = MXTipo(.dmSans, 13, 500, entrelinha: 18, relativoA: .footnote)
        public static let nota = MXTipo(.dmSans, 11.5, 400, relativoA: .caption2)
        public static let filaCabecalho = MXTipo(.dmSans, 10, 600, relativoA: .caption2)
        public static let filaResumo = MXTipo(.dmSans, 10, 400, relativoA: .caption2)
        public static let filaLinha = MXTipo(.dmSans, 12, 400, entrelinha: 16, relativoA: .caption1)
        public static let editando = MXTipo(.dmSans, 12.5, 600, relativoA: .footnote)
        public static let botaoPequeno = MXTipo(.dmSans, 13, 600, relativoA: .footnote)
        public static let rodape = MXTipo(.dmSans, 12, 500, relativoA: .caption1)
        public static let menuCabecalho = MXTipo(.dmSans, 13, 400, entrelinha: 17, relativoA: .footnote)
        public static let menuCabecalhoForte = MXTipo(.dmSans, 13, 600, entrelinha: 17, relativoA: .footnote)
        public static let menuItem = MXTipo(.dmSans, 16, 500, relativoA: .body)
        public static let menuValor = MXTipo(.dmSans, 14, 400, relativoA: .subheadline)
        public static let seloDoCartao = MXTipo(.dmSans, 12.5, 600, relativoA: .footnote)
        public static let tituloDoCartao = MXTipo(.dmSans, 17, 600, entrelinha: 24, relativoA: .headline)
        public static let fatos = MXTipo(.dmSans, 14, 400, entrelinha: 20, relativoA: .subheadline)
        public static let detalhes = MXTipo(.dmSans, 14, 500, relativoA: .subheadline)
        public static let permitir = MXTipo(.dmSans, 16, 600, relativoA: .body)
        public static let botaoDoCartao = MXTipo(.dmSans, 15, 600, relativoA: .body)
        public static let dica = MXTipo(.dmSans, 12.5, 400, entrelinha: 16, relativoA: .footnote)
        /// Código (`Edit src/pace.js`, código no balão): monoespaçada do sistema, 13 no tamanho padrão; a fonte usada
        /// é relativa ao `footnote` (`MXConversaRegras.fonteDoCodigo`) e escala com o Dynamic Type.
        public static let codigoTamanho: CGFloat = 13
    }

    /// Medidas da Conversa, da fila, do menu e do cartão de aprovação (Chat.dc.html, ChatAprovar.dc.html).
    enum Conversa {
        // Topo
        public static let topoMargem: CGFloat = 10
        public static let topoEntreItens: CGFloat = 8
        public static let voltarTraco: CGFloat = 2.2
        public static let capsulaAltura: CGFloat = 48
        public static let capsulaRespiroEsquerda: CGFloat = 6
        public static let capsulaRespiroDireita: CGFloat = 12
        public static let capsulaPelucia: CGFloat = 36
        /// Teto do nome e do estado na cápsula do topo, como o título da barra de navegação nativa: param de crescer
        /// em "extra extra extra grande"; o grande fica no visualizador de conteúdo grande (segurar a cápsula).
        public static var capsulaTextoAte: DynamicTypeSize { .xxxLarge }
        public static let capsulaSeta: CGFloat = 12
        public static let capsulaSetaTraco: CGFloat = 2.8
        public static let capsulaSetaRecuo: CGFloat = 2
        public static let whatsappRespiro: CGFloat = 4
        public static let whatsappLargura: CGFloat = 36
        public static let whatsappIcone: CGFloat = 21

        // Mensagens
        public static let margem: CGFloat = 12
        public static let entreMensagens: CGFloat = 10
        public static let respiroFinal: CGFloat = 8
        public static let peluciaDoBalao: CGFloat = 30
        public static let peluciaDoBalaoAcima: CGFloat = 2
        public static let entreFotoEBalao: CGFloat = 6
        /// Larguras máximas do CONTEÚDO do balão, como o `max-width` do HTML (que não conta o padding).
        public static let balaoDoAgenteLargura: CGFloat = 316
        public static let balaoDoUsuarioLargura: CGFloat = 300
        public static let raioDoBalao: CGFloat = 18
        /// O canto "de quem fala" do balão: em cima à esquerda (agente), embaixo à direita (usuário).
        public static let raioDoCantoDoBalao: CGFloat = 6
        public static let balaoDoAgenteRespiro = EdgeInsets(top: 10, leading: 14, bottom: 12, trailing: 14)
        public static let balaoRespiro = EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14)
        public static let dentroDoBalao: CGFloat = 8
        public static let dentroDoBalaoTrabalhando: CGFloat = 2
        public static let sombraDoBalaoRaio: CGFloat = 9
        public static let sombraDoBalaoY: CGFloat = 6
        public static let sombraDoUsuarioRaio: CGFloat = 7
        public static let sombraDoUsuarioY: CGFloat = 4
        public static let notaAcima: CGFloat = 3
        public static let notaRecuo: CGFloat = 6
        public static let chipAltura: CGFloat = 24
        public static let chipRespiro: CGFloat = 10
        public static let passosAltura: CGFloat = 30
        public static let passosRespiroEsquerda: CGFloat = 8
        public static let passosRespiroDireita: CGFloat = 12
        public static let passosSeta: CGFloat = 14
        public static let passoAltura: CGFloat = 30
        /// Passo dentro do balão de quem trabalha (ChatAprovar): 34.
        public static let passoEmCursoAltura: CGFloat = 34
        public static let passosListaRespiro = EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 10)
        /// Passo no balão de quem trabalha (ChatAprovar: `padding: 0 12px`).
        public static let passoEmCursoRespiro = EdgeInsets(top: 0, leading: 12, bottom: 0, trailing: 12)
        public static let passoCheck: CGFloat = 14
        public static let passoCheckTraco: CGFloat = 2.6
        public static let gomoNoBalao: CGFloat = 20
        public static let codigoRespiro: CGFloat = 5

        // Caixa
        public static let caixaMargem: CGFloat = 10
        public static let caixaEntreBlocos: CGFloat = 6
        public static let caixaEntreBotoes: CGFloat = 8
        public static let mais: CGFloat = 52
        public static let maisIcone: CGFloat = 24
        public static let maisTraco: CGFloat = 2
        public static let capsulaDaCaixaAltura: CGFloat = 52
        public static let capsulaDaCaixaRaio: CGFloat = 26
        public static let capsulaDaCaixaRespiro = EdgeInsets(top: 4, leading: 14, bottom: 4, trailing: 4)
        public static let capsulaDaCaixaEntre: CGFloat = 2
        public static let campoAltura: CGFloat = 44
        public static let campoRespiro: CGFloat = 4
        /// Linhas do campo antes de rolar dentro dele.
        public static let campoLinhasMaximas = 5
        /// Texto grande: linhas do campo antes de rolar por dentro (5 linhas enormes passariam da caixa).
        public static let campoLinhasMaximasAcessivel = 3
        /// Texto grande: a caixa inteira (fila, linha "Editando", campo, rodapé) vai no máximo até esta fração da
        /// altura da tela; o resto rola por dentro.
        public static let caixaFracaoMaximaDaTela: CGFloat = 0.5
        public static let ditadoLargura: CGFloat = 40
        public static let ditadoIcone: CGFloat = 21
        public static let enviar: CGFloat = 44
        public static let enviarIcone: CGFloat = 20
        public static let enviarTraco: CGFloat = 2.4
        public static let confirmarTraco: CGFloat = 2.6
        public static let aoVivoLargura: CGFloat = 58
        public static let aoVivoIcone: CGFloat = 22
        public static let aoVivoSombraRaio: CGFloat = 6
        public static let aoVivoSombraY: CGFloat = 4
        public static let pararIcone: CGFloat = 16
        /// Botão que entra na troca ao vivo ↔ enviar: começa em 0.7 (`trocaBotao`).
        public static let trocaEscalaInicial: CGFloat = 0.7
        /// Botão desligado (sem conexão ou sem texto para salvar).
        public static let desligadoOpacidade: Double = 0.4

        /// Seta que abre: aponta para baixo (gira 90° a partir da direita).
        public static let anguloDaSetaAberta: Double = 90
        /// O anel de contexto começa no alto (−90° a partir da direita).
        public static let anguloDoInicioDoAnel: Double = -90

        // Rodapé
        public static let rodapeRespiro: CGFloat = 8
        public static let rodapeAltura: CGFloat = 24
        public static let rodapePilulaRespiro: CGFloat = 10
        public static let anel: CGFloat = 12
        /// `stroke-width: 4` na caixa de 24 → 2 no anel de 12.
        public static let anelTraco: CGFloat = 2

        // Fila
        /// "Um pouco mais estreita" que a caixa: 22 de cada lado.
        public static let filaRecuo: CGFloat = 22
        public static let filaRaio: CGFloat = 15
        public static let filaRespiro = EdgeInsets(top: 0, leading: 2, bottom: 2, trailing: 2)
        public static let filaCabecalho: CGFloat = 22
        public static let filaCabecalhoRespiro: CGFloat = 9
        public static let filaCabecalhoEntre: CGFloat = 5
        public static let filaSetaLargura: CGFloat = 26
        public static let filaSetaIcone: CGFloat = 12
        public static let filaSetaTraco: CGFloat = 2.6
        public static let filaLinha: CGFloat = 31
        /// Linhas visíveis antes de rolar.
        public static let filaLinhasVisiveis = 2
        /// O espaçamento da fila, num lugar só: `.desenho` = compacta como no desenho e no Mac (linhas de 31, cabeçalho
        /// 22, ações de 28 lado a lado; alvos de toque de 28–36 de largura e 31 de altura); `.apple` = passo de 44 pt
        /// em linhas, cabeçalho e ações (a fila fica ~132 pt de altura em vez de ~84). O padrão segue a Apple; `.desenho` mantém a opção compacta. Ver `MXAlvoDeToque.passoNaFila`.
        public static let filaEspacamento: MXEspacamentoDaFila = .apple
        /// Altura do esmaecer no fim da lista quando rola: `calc(100% - 14px)`.
        public static let filaEsmaecer: CGFloat = 14
        public static let filaLinhaRaio: CGFloat = 10
        public static let filaLinhaRespiro: CGFloat = 9
        public static let filaTextoRespiro: CGFloat = 3
        public static let filaAcao: CGFloat = 28
        /// Ações por linha da fila: Mandar agora, Editar, Remover.
        public static let filaAcoes = 3
        public static let filaMandarAgoraDisco: CGFloat = 21
        public static let filaMandarAgoraIcone: CGFloat = 11
        public static let filaIcone: CGFloat = 13
        public static let filaSombraRaio: CGFloat = 9
        public static let filaSombraY: CGFloat = 6

        // "Editando a 2ª da fila"
        public static let editandoAltura: CGFloat = 30
        public static let editandoRespiroEsquerda: CGFloat = 14
        public static let editandoRespiroDireita: CGFloat = 4
        public static let editandoIcone: CGFloat = 13
        public static let cancelarAltura: CGFloat = 28
        public static let cancelarRespiro: CGFloat = 12

        // Menu do agente
        /// Distância entre a cápsula e o menu (108 − 54 − 48).
        public static let menuAbaixoDaCapsula: CGFloat = 6
        /// Largura do conteúdo do menu, como o `width` do HTML (o padding de 6 de cada lado fica por fora).
        public static let menuLargura: CGFloat = 244
        public static let menuRaio: CGFloat = 26
        public static let menuRespiro: CGFloat = 6
        public static let menuCabecalhoRespiro = EdgeInsets(top: 8, leading: 14, bottom: 6, trailing: 14)
        public static let menuItemAltura: CGFloat = 48
        public static let menuItemRespiro: CGFloat = 14
        public static let menuItemRaio: CGFloat = 14
        public static let menuItemIcone: CGFloat = 20
        public static let menuItemEntre: CGFloat = 12
        public static let menuDivisoriaRespiro = EdgeInsets(top: 4, leading: 14, bottom: 4, trailing: 14)
        public static let menuSombraRaio: CGFloat = 22
        public static let menuSombraY: CGFloat = 18
        /// Abrir o menu: começa em 0.9 e 6 pt acima (`abrirMenuBot`); com Reduzir movimento, só opacidade.
        public static let menuEscalaInicial: CGFloat = 0.9
        public static let menuDeslocamentoInicial: CGFloat = -6
        /// Traço dos ícones de 24 no menu, na caixa e na fila (1.9).
        public static let iconeTraco: CGFloat = 1.9

        // Cartão de aprovação
        public static let cartaoRaio: CGFloat = 22
        public static let cartaoRespiro: CGFloat = 16
        public static let cartaoEntre: CGFloat = 12
        public static let cartaoSombraRaio: CGFloat = 15
        public static let cartaoSombraY: CGFloat = 10
        public static let seloAltura: CGFloat = 26
        public static let seloRespiro: CGFloat = 10
        public static let seloIcone: CGFloat = 14
        public static let tituloEntre: CGFloat = 2
        public static let fatosRaio: CGFloat = 12
        public static let fatosRespiro = EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12)
        public static let fatosColuna: CGFloat = 96
        public static let fatosEntreLinhas: CGFloat = 6
        public static let detalhesSeta: CGFloat = 16
        public static let permitirAltura: CGFloat = 50
        public static let permitirIcone: CGFloat = 18
        public static let permitirSombraRaio: CGFloat = 8
        public static let permitirSombraY: CGFloat = 6
        public static let botaoDoCartaoAltura: CGFloat = 44
        public static let entreBotoes: CGFloat = 8
        public static let contornoRecusar: CGFloat = 1
        public static let dicaIcone: CGFloat = 14
        /// Prévia dos detalhes do pedido (comando, saída, título): até 2000 bytes UTF-8 (2000 caracteres em texto
        /// simples, como no app atual). Os limites são em bytes: um caractere pode ter tamanho sem limite.
        public static let detalhesMaximo = 2000
        /// Linhas do comando antes de abrir Detalhes.
        public static let detalheLinhas = 3
        /// Limite absoluto de inspeção no iPhone, em bytes UTF-8 (comando, saída, título, caminho, alteração): acima dele,
        /// "Mostrar tudo" mostra o começo e o fim e diz quanto ficou de fora, e permitir fica bloqueado (aprovar no
        /// computador).
        public static let inspecaoMaxima = 200_000
        /// Altura da caixa que mostra um texto inteiro em Detalhes (rola por dentro).
        public static let inspecaoAltura: CGFloat = 320
        /// Bytes UTF-8 de um balão antes de "Mostrar tudo" (o resto só é desenhado se pedido).
        public static let textoLimite = 4000
        /// Arquivos alterados por página em Detalhes (uma página por vez: "Anteriores" / "Próximos").
        public static let diffsMaximo = 20
        /// Prévia de uma alteração: só os primeiros 8000 bytes do patch são lidos; deles, até 80 linhas (como o
        /// `DiffView` do app atual), cada uma com até 300 bytes (linha minificada enorme não passa inteira); o caminho
        public static let diffPreviaBytes = 8000
        public static let diffLinhas = 80
        public static let diffLinhaMaxima = 300
        /// O caminho do arquivo em Detalhes aparece inteiro, com quebra de linha, até 4096 bytes (o maior caminho do
        /// Linux, `PATH_MAX`). Maior que isso, o começo e "Mostrar o caminho inteiro" logo embaixo, sem depender do
        /// patch; acima do limite absoluto, bloqueia permitir.
        public static let diffCaminhoBytes = 4096
    }
}

public extension MX.Movimento {
    /// Segurar para permitir: 0,9 s, o mesmo do Mac (`HOLD_MS` em `HoldButton.tsx` do desktop). O desenho do iPhone
    /// não dá número. Soltar antes cancela; com Reduzir movimento um toque confirma; VoiceOver confirma ao ativar.
    static let segurarParaPermitir: Double = 0.9
    /// Distância que o dedo pode andar segurando Permitir antes de cancelar.
    static let segurarDistanciaMaxima: CGFloat = 24
}

public extension MX.Vidro {
    /// Caixa de mensagem e `+`: `rgba(255,255,255,0.86)` pousado no creme (topo do gradiente do protótipo).
    static let opacoCaixa = MXCor(r: 255, g: 255, b: 255, a: 0.86).sobre(MX.Claro.fundo)
    /// Menu do agente: `rgba(252,249,243,0.88)` pousado no creme.
    static let opacoMenu = MXCor(r: 252, g: 249, b: 243, a: 0.88).sobre(MX.Claro.fundo)
    /// Fila de mensagens: `rgba(252,249,243,0.9)` pousado no creme.
    static let opacoFila = MXCor(r: 252, g: 249, b: 243, a: 0.9).sobre(MX.Claro.fundo)
}

public extension MX.Conversa {
    /// Larguras por FORA (conteúdo do desenho + padding), que é o que o SwiftUI limita: balão do usuário 300 + 28,
    /// do agente 316 + 28, menu 244 + 12.
    static var balaoDoUsuarioLarguraExterna: CGFloat { balaoDoUsuarioLargura + balaoRespiro.leading + balaoRespiro.trailing }
    static var balaoDoAgenteLarguraExterna: CGFloat {
        balaoDoAgenteLargura + balaoDoAgenteRespiro.leading + balaoDoAgenteRespiro.trailing
    }
    static func menuLarguraExterna(conteudo: CGFloat = menuLargura) -> CGFloat { conteudo + 2 * menuRespiro }
}

// MARK: - Sessões (07, 07b) e Ajustes do agente (08) — telas/Sessoes.dc.html, ChatSessao.dc.html, ConfigBot.dc.html

public extension MX {
    /// Medidas da tela Sessões.
    enum Sessoes {
        /// A árvore vai até 4 níveis (README, tela 07).
        public static let niveisMaximos = 4
        /// Tudo começa recolhido (README, "Gestos"); a captura 07 mostra as ativas abertas. Valor único: ver o doc.
        public static let comecaRecolhida = true
        /// Topo: "‹ Conversas" em cápsula de 44 (8 à esquerda, 14 à direita), voz redonda de 44.
        public static let topoAltura: CGFloat = 44
        public static let voltarRespiroEsquerda: CGFloat = 8
        public static let voltarRespiroDireita: CGFloat = 14
        public static let voltarIcone: CGFloat = 22
        public static let voltarTraco: CGFloat = 2.2
        public static let vozIcone: CGFloat = 20
        /// Cabeçalho: pelúcia de 52, nome 24/30, etiqueta, estado e contagem.
        public static let pelucia: CGFloat = 52
        public static let cabecalhoEntre: CGFloat = 10
        public static let cabecalhoRecuo: CGFloat = 4
        /// Painel da árvore: raio 22 em cima, 4/6 de respiro, 120 no fim (o botão Nova sessão flutua por cima).
        public static let painelRespiroTopo: CGFloat = 4
        public static let painelRespiroLado: CGFloat = 6
        public static let painelFolgaFinal: CGFloat = 120
        /// "Agora · 2": 8 acima, 10 dos lados.
        public static let grupoAcima: CGFloat = 8
        public static let grupoRecuo: CGFloat = 10
        /// Linha: 44 pt (o desenho mostra 36 pt), raio 12, recuo 4 + 11 por nível.
        public static let linhaAltura: CGFloat = 44
        public static let linhaRaio: CGFloat = 12
        public static let recuoBase: CGFloat = 4
        public static let recuoPorNivel: CGFloat = 11
        /// O "L" que liga a filha ao pai: 7 × 16, traço 1,5.
        public static let ligacaoLargura: CGFloat = 7
        public static let ligacaoAltura: CGFloat = 16
        public static let ligacaoTraco: CGFloat = 1.5
        public static let ligacaoRaio: CGFloat = 6
        /// Seta que abre: caixa de 20 de largura (toque de 44), ícone 13, traço 2,8.
        public static let setaLargura: CGFloat = 20
        public static let setaIcone: CGFloat = 13
        public static let setaTraco: CGFloat = 2.8
        /// Ponto de estado 8 (anel 1,5 na livre), gomo 14.
        public static let ponto: CGFloat = 8
        public static let pontoAnel: CGFloat = 1.5
        public static let pontoRecuo: CGFloat = 3
        public static let gomo: CGFloat = 14
        public static let entreItens: CGFloat = 8
        /// Arquivar: caixa de 30 (toque de 44), ícone 17, traço 1,9.
        public static let arquivarLargura: CGFloat = 30
        public static let arquivarIcone: CGFloat = 17
        public static let arquivarTraco: CGFloat = 1.9
        /// Pede você: fundo branco com anel laranja de 2.
        public static let anelDeVoce: CGFloat = 2
        /// O "Abrir" laranja que aparece ao puxar: 88 de largura.
        public static let abrirLargura: CGFloat = 88
        public static let abrirIcone: CGFloat = 14
        /// Nova sessão: cápsula laranja de 54, 16 dos lados, 34 da borda de baixo.
        public static let novaAltura: CGFloat = 54
        public static let novaMargem: CGFloat = 16
        public static let novaDaBorda: CGFloat = 34
        public static let novaIcone: CGFloat = 20
        public static let novaSombraRaio: CGFloat = 11
        public static let novaSombraY: CGFloat = 10
        /// Aviso "Arquivada: …" com Desfazer: 48 de altura, raio 14, 104 da borda de baixo.
        public static let avisoAltura: CGFloat = 48
        public static let avisoRaio: CGFloat = 14
        public static let avisoDaBorda: CGFloat = 104
        public static let avisoRespiroEsquerda: CGFloat = 16
        public static let avisoRespiroDireita: CGFloat = 6
        public static let avisoSombraRaio: CGFloat = 12
        public static let avisoSombraY: CGFloat = 10
        /// Tempo que o aviso de arquivada fica (o desenho não dá número; o padrão do iOS para desfazer).
        public static let avisoDuracao: Double = 5
        /// "Arquivadas · N": linha de 44, ícone 18.
        public static let arquivadasIcone: CGFloat = 18
    }

    /// Cores da tela Sessões e da folha de ajustes.
    enum CorDasSessoes {
        /// Linha que pede você: branca.
        public static let linhaDeVoce = MXCor("#FFFFFF")
        /// Linha puxada: `#FFFDF8`.
        public static let linhaPuxada = MXCor("#FFFDF8")
        /// O "L" de ligação: `rgba(42,28,20,0.18)`.
        public static let ligacao = MXCor(r: 42, g: 28, b: 20, a: 0.18)
        /// Aviso escuro: fundo marrom, texto creme, Desfazer laranja claro.
        public static let avisoFundo = MXCor("#2A1C14")
        public static let avisoTexto = MXCor("#FFF8EE")
        public static let avisoAcao = MXCor("#FFC170")
        public static let avisoSombra = MXCor(r: 42, g: 28, b: 20, a: 0.25)
        /// Sombra da Nova sessão: `rgba(221,111,16,0.3)`.
        public static let novaSombra = MXCor(r: 221, g: 111, b: 16, a: 0.3)
        /// Folha de ajustes: fundo `#FFF8EE`, véu `rgba(42,28,20,0.32)`, cartões brancos com anel 1.
        public static let folha = MXCor("#FFF8EE")
        public static let veuDaFolha = MXCor(r: 42, g: 28, b: 20, a: 0.32)
        public static let cartao = MXCor("#FFFFFF")
        public static let anelDoCartao = MXCor(r: 42, g: 28, b: 20, a: 0.08)
        public static let separador = MXCor(r: 42, g: 28, b: 20, a: 0.06)
        /// Segmentos de autonomia: trilho `rgba(42,28,20,0.06)`, escolhido branco com sombra.
        public static let trilho = MXCor(r: 42, g: 28, b: 20, a: 0.06)
        public static let sombraDoSegmento = MXCor(r: 42, g: 28, b: 20, a: 0.14)
        /// Letra do item: fundo `rgba(245,138,31,0.14)`, letra `#B4530A`.
        public static let letraFundo = MXCor(r: 245, g: 138, b: 31, a: 0.14)
        /// Interruptor desligado: `rgba(42,28,20,0.18)`.
        public static let interruptorDesligado = MXCor(r: 42, g: 28, b: 20, a: 0.18)
    }

    /// Medidas da folha de ajustes do agente.
    enum Ajustes {
        /// "Total" ainda não existe no Hub: fica à vista, desligado, "Em breve" (desenho).
        public static let autonomiaTotalDisponivel = false
        public static let raio: CGFloat = 30
        public static let topo: CGFloat = 52
        public static let respiroLado: CGFloat = 16
        public static let respiroTopo: CGFloat = 8
        public static let respiroFim: CGFloat = 40
        public static let entreBlocos: CGFloat = 14
        public static let tituloRespiroEsquerda: CGFloat = 18
        public static let tituloRespiroDireita: CGFloat = 12
        /// "Pronto": cápsula de 36 (toque de 44), 14 dos lados.
        public static let prontoAltura: CGFloat = 36
        public static let prontoRespiro: CGFloat = 14
        public static let pelucia: CGFloat = 64
        public static let cabecalhoEntre: CGFloat = 12
        public static let sincroniaIcone: CGFloat = 13
        /// Conversar, Sessões, Ligar: 3 colunas de 58, raio 16, 8 entre.
        public static let atalhoAltura: CGFloat = 58
        public static let atalhoRaio: CGFloat = 16
        public static let atalhoEntre: CGFloat = 8
        public static let atalhoIcone: CGFloat = 20
        /// Cartões: raio 16, anel 1; Autonomia com 12/14 de respiro e 10 entre.
        public static let cartaoRaio: CGFloat = 16
        public static let autonomiaRespiroV: CGFloat = 12
        public static let autonomiaRespiroH: CGFloat = 14
        public static let autonomiaEntre: CGFloat = 10
        /// Segmentos: trilho de raio 12 com 3 de respiro; cada um com 34 (toque de 44 pela linha) e raio 9.
        public static let segmentosRaio: CGFloat = 12
        public static let segmentosRespiro: CGFloat = 3
        public static let segmentoAltura: CGFloat = 34
        public static let segmentoRaio: CGFloat = 9
        public static let segmentoSombraRaio: CGFloat = 1.5
        public static let segmentoSombraY: CGFloat = 1
        /// Itens: 50 de altura mínima, letra em quadro de 30 (raio 9), valor até 150 de largura.
        public static let itemAltura: CGFloat = 50
        public static let itemRespiroEsquerda: CGFloat = 14
        public static let itemRespiroDireita: CGFloat = 12
        public static let itemEntre: CGFloat = 12
        public static let letra: CGFloat = 30
        public static let letraRaio: CGFloat = 9
        public static let valorLargura: CGFloat = 150
        public static let itemSeta: CGFloat = 14
        /// Avisos: 52 de altura; interruptor 52 × 32 com bola de 26.
        public static let avisosAltura: CGFloat = 52
        public static let interruptorLargura: CGFloat = 52
        public static let interruptorAltura: CGFloat = 32
        public static let interruptorBola: CGFloat = 26
        public static let interruptorRespiro: CGFloat = 3
    }
}

public extension MX.Tipo {
    /// Sessões e ajustes (`telas/Sessoes.dc.html`, `ConfigBot.dc.html`).
    static let tituloDoAgente = MXTipo(.nunito, 24, 800, entrelinha: 30, relativoA: .title1)
    static let estadoDoAgente = MXTipo(.dmSans, 13, 600, entrelinha: 18, relativoA: .footnote)
    static let voltarComRotulo = MXTipo(.dmSans, 15, 600, relativoA: .body)
    static let grupoDeSessoes = MXTipo(.dmSans, 12, 600, entrelinha: 16, relativoA: .caption1)
    static let sessaoRaiz = MXTipo(.dmSans, 15, 500, entrelinha: 20, relativoA: .body)
    static let sessaoPrincipal = MXTipo(.dmSans, 15, 600, entrelinha: 20, relativoA: .body)
    static let sessaoFilha = MXTipo(.dmSans, 14, 400, entrelinha: 20, relativoA: .subheadline)
    static let sessaoInfo = MXTipo(.dmSans, 12, 500, entrelinha: 16, relativoA: .caption1)
    static let sessaoInfoForte = MXTipo(.dmSans, 12, 600, entrelinha: 16, relativoA: .caption1)
    static let acaoDaLinha = MXTipo(.dmSans, 13, 600, relativoA: .footnote)
    static let botaoGrande = MXTipo(.dmSans, 17, 600, relativoA: .body)
    static let arquivadas = MXTipo(.dmSans, 14, 600, relativoA: .subheadline)
    static let aviso = MXTipo(.dmSans, 14, 400, relativoA: .subheadline)
    static let avisoAcao = MXTipo(.dmSans, 14, 600, relativoA: .subheadline)
    static let tituloDaFolha = MXTipo(.dmSans, 17, 600, entrelinha: 22, relativoA: .headline)
    static let atalho = MXTipo(.dmSans, 13, 600, relativoA: .footnote)
    static let sincronia = MXTipo(.dmSans, 12.5, 500, entrelinha: 17, relativoA: .footnote)
    static let itemDeAjuste = MXTipo(.dmSans, 15, 400, entrelinha: 20, relativoA: .body)
    static let valorDeAjuste = MXTipo(.dmSans, 13.5, 400, entrelinha: 18, relativoA: .footnote)
    static let letraDeAjuste = MXTipo(.dmSans, 14, 700, relativoA: .subheadline)
    static let segmento = MXTipo(.dmSans, 14, 600, relativoA: .subheadline)
    static let perigo = MXTipo(.dmSans, 15, 600, relativoA: .body)
}

#if DEBUG
public extension MX {
    /// Ajustes, conta e computadores, só em Debug. As cores usam os tokens `Claro` e `Escuro`.
    enum Conta {
        public static let cartaoRaio: CGFloat = MX.Raio.cartao
        public static let painelRaio: CGFloat = MX.Raio.painel
        /// Respiro do painel (mockup: 12) e entre cartões.
        public static let painelRespiro: CGFloat = 12
        public static let avatar: CGFloat = 48
        public static let avatarIcone: CGFloat = 26
        public static let contaAlturaMinima: CGFloat = 76
        public static let contaRespiro: CGFloat = 12
        public static let linhaAlturaMinima: CGFloat = 56
        public static let linhaAlturaMinimaAcessivel: CGFloat = 64
        public static let linhaRespiroV: CGFloat = 8
        public static let linhaRespiroH: CGFloat = 12
        public static let linhaEntre: CGFloat = 12
        public static let tile: CGFloat = 40
        public static let tileAcessivel: CGFloat = 48
        public static let tileRaio: CGFloat = 12
        public static let tileIcone: CGFloat = 22
        public static let marca: CGFloat = 28
        public static let estadoIcone: CGFloat = 14
        public static let secaoTopo: CGFloat = 18
        public static let secaoLado: CGFloat = 8
        public static let secaoBaixo: CGFloat = 8
        public static let notaTopo: CGFloat = 14
        /// Espessura do traço dos ícones desta tela (`.pt` do mockup).
        public static let traco: CGFloat = 1.8
        /// Nome na linha "Sua conta" (mockup: Nunito 19, 700).
        public static let nomeDaConta = MXTipo(.nunito, 19, 700, entrelinha: 24, relativoA: .title3)
    }
}
#endif
