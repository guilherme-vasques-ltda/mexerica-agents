import SwiftUI

/// Como a barra de abas se apresenta no tamanho de texto escolhido (regra pura, testada). Segue a barra nativa do
/// iOS: ícone e nome sempre visíveis, o nome com teto de tamanho e o ícone do tamanho do desenho; quem precisa de
/// maior segura a aba e vê o visualizador de conteúdo grande (ícone e nome). O contador cresce sem teto.
public enum MXApresentacaoDaBarra: Equatable, Sendable {
    /// Até "extra extra extra grande": as larguras do desenho (84 para Mission Board, 64 para as outras), nome com
    /// teto em "muito grande", contador no canto do ícone.
    case iconeERotulo
    /// Tamanhos de acessibilidade: abas de 68, para o contador caber inteiro embaixo do ícone; nome com teto no
    /// tamanho do desenho.
    case acessivel

    public static func para(_ tamanho: DynamicTypeSize) -> MXApresentacaoDaBarra {
        tamanho.isAccessibilitySize ? .acessivel : .iconeERotulo
    }

    /// O ícone não cresce (como na barra nativa); o grande fica no visualizador de conteúdo grande.
    public var icone: CGFloat { MX.Medida.abaIcone }

    /// O maior tamanho de texto que o nome da aba segue.
    public var tetoDoRotulo: DynamicTypeSize {
        self == .acessivel ? MX.Medida.abaRotuloAteAcessivel : MX.Medida.abaRotuloAte
    }

    /// Largura do item, nunca abaixo do alvo de toque.
    public func largura(_ aba: MXAba) -> CGFloat {
        let largura = switch self {
        case .iconeERotulo: aba.largura
        case .acessivel: MX.Medida.abaLarguraAcessivel
        }
        return max(largura, MX.Espaco.alvoMinimo)
    }

    /// Altura do item, nunca abaixo do alvo de toque.
    public var altura: CGFloat { max(MX.Medida.abaAltura, MX.Espaco.alvoMinimo) }

    /// Largura da barra inteira (itens, espaços entre eles e respiro), para conferir que cabe na tela.
    public var larguraTotal: CGFloat {
        let abas = MXAba.allCases
        let itens = abas.reduce(CGFloat(0)) { $0 + largura($1) }
        let entre = MX.Medida.barraAbasEntreItens * CGFloat(abas.count - 1)
        return itens + entre + 2 * MX.Medida.barraAbasRespiro
    }
}

/// Onde fica o contador da aba, conforme o tamanho do texto (regra pura, testada). O número segue o texto do
/// sistema em todos os tamanhos, sem teto e sem encolher; o que muda com o tamanho é o lugar dele e, nos
/// tamanhos de acessibilidade, o texto: acima de 9 vira "9+" (`MXConversasRegras.contadorDaAba`).
public enum MXPosicaoDoContador: Equatable, Sendable {
    /// Até "extra extra extra grande": no canto do ícone (5 acima, 9 à direita), como no desenho. Mesmo com
    /// "99+", o selo fica dentro da própria aba de 64.
    case noCantoDoIcone
    /// Tamanhos de acessibilidade: embaixo do ícone, dentro da própria aba (68 de largura), que fica mais alta
    /// (a barra também). Não cobre a aba vizinha: o maior texto, "9+", cabe na largura da aba (medido no teste).
    case abaixoDoIcone

    public static func para(_ tamanho: DynamicTypeSize) -> MXPosicaoDoContador {
        tamanho.isAccessibilitySize ? .abaixoDoIcone : .noCantoDoIcone
    }

    /// Largura disponível para o selo (com contorno) embaixo do ícone: a da aba menos uma folga de cada lado.
    public static func larguraMaxima(_ aba: MXAba, _ apresentacao: MXApresentacaoDaBarra) -> CGFloat {
        apresentacao.largura(aba) - 2 * MX.Medida.abaContadorFolgaLateral
    }
}

/// Barra de abas flutuante: cápsula de vidro centralizada, 26 pt acima da borda de baixo da tela, com a aba
/// ativa em laranja claro e o contador laranja de pendências (número em marrom) no canto de cada ícone.
///
/// É barra própria com vidro nativo, não a barra do `TabView`: a do sistema no iOS 26 não deixa pintar o realce
/// da aba ativa de laranja claro, nem pôr o contador laranja com número marrom, nem fixar a distância da borda.
///
/// Texto grande como a barra nativa (ver `MXApresentacaoDaBarra`): nome visível com teto, visualizador de
/// conteúdo grande em cada aba, contador sem teto.
/// Com Reduzir movimento, o realce não corre entre as abas: some numa e aparece na outra por opacidade.
public struct MXBarraDeAbas: View {
    @Binding var selecionada: MXAba
    let pendencias: [MXAba: Int]
    /// As abas que abrem nesta fonte; as outras ficam à vista e desligadas ("Em breve", valor "Indisponível").
    let ligadas: Set<MXAba>
    /// Só as abas ligadas aparecem (o desenho do Mission Board v1: "a barra mostra só as abas que existem", PLANO §1).
    let somenteLigadas: Bool
    #if DEBUG
    /// Barra no escuro (só o Mission Board segue o tema do sistema por enquanto): grafite chapado, sem vidro.
    let escuro: Bool
    #endif
    @Namespace private var realce
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto

    public init(selecionada: Binding<MXAba>, pendencias: [MXAba: Int], ligadas: Set<MXAba> = Set(MXAba.allCases),
                somenteLigadas: Bool = false) {
        _selecionada = selecionada
        self.pendencias = pendencias
        self.ligadas = ligadas
        self.somenteLigadas = somenteLigadas
        #if DEBUG
        escuro = false
        #endif
    }

    #if DEBUG
    init(selecionada: Binding<MXAba>, pendencias: [MXAba: Int], ligadas: Set<MXAba>, somenteLigadas: Bool, escuro: Bool) {
        _selecionada = selecionada
        self.pendencias = pendencias
        self.ligadas = ligadas
        self.somenteLigadas = somenteLigadas
        self.escuro = escuro
    }
    #endif

    /// As abas que a barra desenha, na ordem do desenho.
    var abas: [MXAba] { somenteLigadas ? MXAba.allCases.filter(ligadas.contains) : MXAba.allCases }

    public var body: some View {
        let apresentacao = MXApresentacaoDaBarra.para(tamanhoDoTexto)
        HStack(spacing: MX.Medida.barraAbasEntreItens) {
            ForEach(abas) { aba in item(aba, apresentacao) }
        }
        // Todas as abas com a altura da mais alta (com o contador embaixo do ícone, a barra cresce).
        .fixedSize(horizontal: false, vertical: true)
        .padding(MX.Medida.barraAbasRespiro)
        .modifier(MXFundoDaBarra(escuro: escuroDaBarra))
    }

    private var escuroDaBarra: Bool {
        #if DEBUG
        escuro
        #else
        false
        #endif
    }

    private func item(_ aba: MXAba, _ apresentacao: MXApresentacaoDaBarra) -> some View {
        let ativa = aba == selecionada
        let n = pendencias[aba] ?? 0
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        let posicao = MXPosicaoDoContador.para(tamanhoDoTexto)
        let texto = MXConversasRegras.contadorDaAba(n, tamanho: tamanhoDoTexto)
        let m = MX.Medida.self
        return Button {
            withAnimation(troca.animacao) { selecionada = aba }
        } label: {
            VStack(spacing: posicao == .abaixoDoIcone ? m.abaContadorAbaixoDoIcone : m.abaEntreIconeERotulo) {
                MXIconeView(aba.icone, tamanho: apresentacao.icone)
                    .overlay(alignment: .topTrailing) {
                        if posicao == .noCantoDoIcone, let texto {
                            contador(texto)
                                .offset(x: m.abaContadorDireita, y: -m.abaContadorAcima)
                        }
                    }
                if posicao == .abaixoDoIcone {
                    // Aba sem contador guarda o lugar dele (invisível): ícones e nomes ficam na mesma linha em
                    // todas as abas, com ou sem contador.
                    contador(texto ?? "9")
                        .opacity(texto == nil ? 0 : 1)
                        .accessibilityHidden(true)
                }
                Text(aba.rotulo)
                    .mxTipo(MX.Tipo.rotuloAba)
                    .lineLimit(1)
                    .minimumScaleFactor(m.abaRotuloEscalaMinima)
                    // O nome para de crescer no teto (como a barra nativa); o contador não tem teto.
                    .dynamicTypeSize(...apresentacao.tetoDoRotulo)
            }
            .foregroundStyle(corDoItem(ativa: ativa))
            .frame(width: apresentacao.largura(aba))
            .frame(minHeight: apresentacao.altura,
                   maxHeight: posicao == .abaixoDoIcone ? .infinity : apresentacao.altura,
                   alignment: posicao == .abaixoDoIcone ? .top : .center)
            .background { realceDaAba(ativa: ativa, interpola: troca.interpola) }
        }
        .buttonStyle(MXToque())
        // Aba sem tela nesta fonte: à vista, desligada, "Em breve".
        .mxIndisponivel(!ligadas.contains(aba))
        // O nome e o número inteiro (mesmo quando aparece "9+").
        .accessibilityLabel(aba.rotuloAcessivel(n))
        // Quem fala "Organograma" ou "Bots" (Controle por Voz) acerta a mesma aba.
        .accessibilityInputLabels([aba.titulo, aba.rotulo])
        .accessibilityAddTraits(ativa ? .isSelected : [])
        // Segurar a aba mostra o ícone e o nome grandes (o visualizador de conteúdo grande da barra nativa).
        .accessibilityShowsLargeContentViewer {
            MXIconeView(aba.icone, tamanho: apresentacao.icone)
            Text(aba.titulo)
        }
    }

    /// Realce da aba ativa. Normal: corre de uma aba para a outra com a mola. Reduzir movimento: sem
    /// `matchedGeometryEffect`; sai de uma e entra na outra por opacidade, no lugar.
    @ViewBuilder private func realceDaAba(ativa: Bool, interpola: Bool) -> some View {
        if ativa {
            if interpola {
                Capsule()
                    .fill(corDoRealce)
                    .matchedGeometryEffect(id: "aba-ativa", in: realce)
            } else {
                Capsule()
                    .fill(corDoRealce)
                    .transition(.opacity)
            }
        }
    }

    /// Selo laranja com o número em marrom: cápsula que cresce com o texto do sistema (largura e altura), sem
    /// teto e sem encolher a fonte. Quem garante que cabe é a posição e o texto do contador (ver acima).
    private func contador(_ texto: String) -> some View {
        let m = MX.Medida.self
        return Text(texto)
            .mxTipo(MX.Tipo.contadorAba)
            .lineLimit(1)
            .foregroundStyle(MX.Claro.sobreMexerica.color)
            .padding(.horizontal, m.abaContadorRespiro + m.abaContadorContorno)
            .frame(minWidth: m.abaContador, minHeight: m.abaContador)
            .background(MX.Claro.mexerica.color, in: Capsule())
            .overlay(Capsule().strokeBorder((escuroDaBarra ? MX.Escuro.superficie : MX.Claro.contornoSelo).color, lineWidth: m.abaContadorContorno))
            .fixedSize()
    }

    private func corDoItem(ativa: Bool) -> Color {
        if escuroDaBarra { return (ativa ? MX.Escuro.mexericaTexto : MX.Escuro.textoSecundario).color }
        return (ativa ? MX.Claro.mexericaTexto : MX.Claro.textoSecundario).color
    }

    /// O realce da aba ativa: no escuro, o tom de etiqueta que já existe (o realce laranja a 18% é token proposto, #37).
    private var corDoRealce: Color { (escuroDaBarra ? MX.Escuro.etiqueta : MX.Claro.abaAtiva).color }
}

/// O fundo da barra: vidro no claro; no escuro, grafite chapado com contorno fino (sem vidro nem papel de parede).
private struct MXFundoDaBarra: ViewModifier {
    let escuro: Bool

    func body(content: Content) -> some View {
        if escuro {
            content
                .background(MX.Escuro.superficie.color, in: Capsule())
                .overlay(Capsule().stroke(MX.Escuro.linha.color, lineWidth: MX.Vidro.contornoOpacoEspessura))
        } else {
            content.mxVidro(.barra, em: Capsule())
        }
    }
}
