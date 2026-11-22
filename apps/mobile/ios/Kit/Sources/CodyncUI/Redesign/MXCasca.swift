import SwiftUI

/// Casca do redesenho iOS v1: fundo, as 5 abas e a barra flutuante. Conversas está montada (etapas 1 e 2, com a
/// Conversa); as outras mostram fundo e título. Abre em Debug sem login (ver `MXCasca.abrirSemLogin`), com os dados
/// de exemplo, ou com os dados reais do computador (`-MexericaRedesignReal`, ver `MXCascaDoApp`).
public struct MXCasca: View {
    @State private var aba: MXAba = .conversas
    @State private var caminho: [MXDestino]
    /// A pilha do Mission Board (a Conversa que o "Abrir" empurra); a da aba Conversas é `caminho`.
    @State private var caminhoDoQuadro: [MXDestino] = []
    @State private var fonte: any MXFonte
    private let cena: MXCena?
    #if DEBUG
    /// Ajustes, conta e computadores (só Debug): sem ela o botão da conta segue "Em breve".
    private var conta: (any MXContaFonte)?
    /// O tema do sistema, lido aqui, por fora do claro fixo da casca: só os Ajustes o seguem (ver `body`).
    @Environment(\.colorScheme) private var esquemaDoSistema
    #endif

    /// Com os dados de exemplo (e a cena de captura pedida ao processo, se houver).
    public init() {
        let cena = MXCena.doProcesso
        self.init(fonte: MXFonteExemplo(cena: cena), cena: cena)
        #if DEBUG
        // Os Ajustes com dados de exemplo; `-MexericaCena conta-varios|conta-um|conta-sem` já abre neles.
        let cenaDaConta = MXCenaDaConta.doProcesso
        conta = MXContaExemplo(cena: cenaDaConta ?? .varios)
        if cenaDaConta != nil { _caminho = State(initialValue: [.emBreve(.conta)]) }
        #endif
    }

    /// Com uma fonte dada (exemplo ou real).
    public init(fonte: any MXFonte, cena: MXCena? = nil) {
        MXFontes.registrar()
        _fonte = State(initialValue: fonte)
        _caminho = State(initialValue: MXNavegacaoRegras.caminhoInicial(cena))
        self.cena = cena
    }

    #if DEBUG
    /// Com os dados reais de um computador e a conta do app (só Debug). `abrirNosAjustes`: a casca foi reconstruída por
    /// uma troca de computador e volta aos Ajustes.
    public init(fonte: any MXFonte, conta: any MXContaFonte, abrirNosAjustes: Bool = false) {
        self.init(fonte: fonte, cena: nil)
        self.conta = conta
        if abrirNosAjustes { _caminho = State(initialValue: [.emBreve(.conta)]) }
    }
    #endif

    /// Prévia de Debug: o app abre direto nesta casca com o argumento `-MexericaRedesign` ou a variável
    /// `MEXERICA_REDESIGN=1` (em `simctl launch`, `SIMCTL_CHILD_MEXERICA_REDESIGN=1`). Com `-MexericaRedesignReal`
    /// ou `MEXERICA_REDESIGN=real`, usa os dados reais do primeiro computador da conta.
    /// Fora de Debug é sempre falso: as flags não existem em Release (o gancho do app também é `#if DEBUG`).
    public static var abrirSemLogin: Bool {
        #if DEBUG
        let processo = ProcessInfo.processInfo
        return processo.arguments.contains("-MexericaRedesign") || dadosReais || processo.environment["MEXERICA_REDESIGN"] == "1"
        #else
        return false
        #endif
    }

    /// A casca com os dados reais (o login e o computador vêm do app de hoje, por baixo). Só em Debug.
    public static var dadosReais: Bool {
        #if DEBUG
        let processo = ProcessInfo.processInfo
        return processo.arguments.contains("-MexericaRedesignReal") || processo.environment["MEXERICA_REDESIGN"] == "real"
        #else
        return false
        #endif
    }

    /// Pendências da barra: as da fonte (exemplo ou real). Em Debug, `-MexericaRedesignContadorAlto` troca pelos
    /// contadores altos de `MXExemplo.pendenciasAltas`, para conferir na tela "9+" (acessibilidade) e "99+" sem encolher.
    static func pendencias(_ daFonte: [MXAba: Int]) -> [MXAba: Int] {
        #if DEBUG
        if ProcessInfo.processInfo.arguments.contains("-MexericaRedesignContadorAlto") { return MXExemplo.pendenciasAltas }
        #endif
        return daFonte
    }

    public var body: some View {
        TabView(selection: $aba) {
            ForEach(MXAba.allCases) { aba in
                Tab(value: aba) {
                    conteudo(aba)
                        .toolbarVisibility(.hidden, for: .tabBar)
                }
            }
        }
        .overlay(alignment: .bottom) {
            if MXNavegacaoRegras.mostraBarra(aba: aba, caminho: caminhoAtual) {
                barra
                    .padding(.bottom, MX.Medida.barraAbasDaBorda)
                    .ignoresSafeArea(edges: .bottom)
                    .transition(.opacity)
            }
        }
        .animation(MX.Movimento.crossFade, value: MXNavegacaoRegras.mostraBarra(aba: aba, caminho: caminhoAtual))
        .tint(MX.Claro.mexerica.color)
        #if DEBUG
        .environment(\.mxConta, conta)
        .environment(\.mxEsquemaDoSistema, esquemaDoSistema)
        #endif
        // O tema escuro do redesenho é a etapa 7; até lá, a casca fica no claro do desenho (só os Ajustes seguem o sistema).
        .environment(\.colorScheme, .light)
    }

    /// A pilha da aba que está aberta.
    private var caminhoAtual: [MXDestino] { aba == .missionBoard ? caminhoDoQuadro : caminho }

    /// A barra: na fonte real só as abas que existem; o Mission Board segue o tema do sistema (só Debug), as outras abas não.
    @ViewBuilder private var barra: some View {
        let ligadas = Set(MXAba.allCases.filter { fonte.capacidades.ligado(.aba($0)) })
        let somenteLigadas = !fonte.capacidades.mostraEmBreve
        #if DEBUG
        MXBarraDeAbas(selecionada: $aba, pendencias: Self.pendencias(fonte.pendencias), ligadas: ligadas, somenteLigadas: somenteLigadas,
                      escuro: aba == .missionBoard && esquemaDoSistema == .dark)
        #else
        MXBarraDeAbas(selecionada: $aba, pendencias: Self.pendencias(fonte.pendencias), ligadas: ligadas, somenteLigadas: somenteLigadas)
        #endif
    }

    @ViewBuilder private func conteudo(_ aba: MXAba) -> some View {
        switch aba {
        case .conversas:
            MXConversasNavegacao(fonte: fonte, caminho: $caminho, cena: cena)
        #if DEBUG
        case .missionBoard where fonte.capacidades.ligado(.aba(.missionBoard)) && fonte is any MXQuadroFonte:
            // Mission Board v1 (derivado, só Debug): a raiz é o quadro; "Abrir" empurra a Conversa nesta pilha.
            MXConversasNavegacao(fonte: fonte, caminho: $caminhoDoQuadro, cena: nil, quadro: true)
        #endif
        default:
            MXAbaVazia(titulo: aba.titulo, contaLigada: fonte.capacidades.ligado(.conta))
        }
    }
}

/// O que o gancho de Debug abre: a casca com os dados de exemplo ou, com `-MexericaRedesignReal` ou com o interruptor
/// "Novo visual (teste)" dos Ajustes (`MXNovoVisual`), com o `BotStore` do primeiro computador da conta (o app de hoje
/// continua por baixo, fazendo login e conexão). Em Release nunca mostra nada.
public struct MXCascaDoApp: View {
    let contas: AccountStore
    #if DEBUG
    /// Lida no `body`: ligar ou desligar o interruptor troca o que o app mostra na hora.
    @AppStorage(MXNovoVisual.chave) private var interruptor = false
    /// "Voltar ao visual antigo" nesta abertura: vence os argumentos de lançamento até o interruptor ser ligado de novo.
    @State private var voltouAoAntigo = false
    /// O que só o app tem (login, pareamento, telas de hoje); nulo nos testes e na prévia sem app.
    private let ponte: MXPonteDoApp?
    #endif

    public init(contas: AccountStore) {
        self.contas = contas
        #if DEBUG
        ponte = nil
        #endif
    }

    #if DEBUG
    /// Com os Ajustes, a conta e a troca de computador (o app entrega a ponte; ver `MXContaReal`).
    public init(contas: AccountStore, ponte: MXPonteDoApp) {
        self.contas = contas
        self.ponte = ponte
    }
    #endif

    public var body: some View {
        #if DEBUG
        let modo = MXNovoVisual.abre(interruptor: interruptor, semLogin: MXCasca.abrirSemLogin, real: MXCasca.dadosReais,
                                     voltouAoAntigo: voltouAoAntigo)
        Group {
            switch modo {
            case .antigo:
                EmptyView()
            case .exemplo:
                MXCasca()
            case .real:
                real.safeAreaInset(edge: .top, spacing: 0) {
                    if interruptor { MXFaixaDoNovoVisual { voltar() } }
                }
            }
        }
        .onChange(of: interruptor) { _, ligou in if ligou { voltouAoAntigo = false } }
        #else
        EmptyView()
        #endif
    }

    #if DEBUG
    private func voltar() {
        interruptor = false
        voltouAoAntigo = true
    }

    @ViewBuilder private var real: some View {
        if let ponte {
            // Um `MXContaReal` por conta do app: trocar de conta recria (o `AccountStore` é outro).
            MXCascaComConta(contas: contas, ponte: ponte).id(ObjectIdentifier(contas))
        } else if let store = contas.computers.first.flatMap({ contas.store(for: $0.id) }) {
            MXCascaReal(store: store).id(ObjectIdentifier(store))
        } else {
            MXSemComputador(peloInterruptor: interruptor)
        }
    }
    #endif
}

#if DEBUG
/// A casca real com a conta do app: o computador em uso é o que o dono escolheu nos Ajustes (ou o último do app de hoje);
/// trocar reconstrói a casca com a fonte do outro computador e reabre nos Ajustes. Sem computador nenhum, a raiz são os
/// próprios Ajustes, com o cartão "Conectar um computador" (nunca um beco sem saída).
struct MXCascaComConta: View {
    @State private var conta: MXContaReal
    @Environment(\.colorScheme) private var esquema

    init(contas: AccountStore, ponte: MXPonteDoApp) {
        _conta = State(initialValue: MXContaReal(contas: contas, ponte: ponte))
    }

    var body: some View {
        if let store = conta.storeEmUso {
            MXCascaReal(store: store, conta: conta, abrirNosAjustes: conta.reabrirNosAjustes).id(ObjectIdentifier(store))
        } else {
            MXAjustesDaContaTela(conta: conta, esquema: esquema, voltar: nil)
        }
    }
}
#endif

/// Guarda uma fonte real por `BotStore` (a view pode ser recriada; a fonte não).
struct MXCascaReal: View {
    @State private var fonte: MXFonteReal
    #if DEBUG
    private let conta: (any MXContaFonte)?
    private let abrirNosAjustes: Bool
    #endif

    init(store: BotStore) {
        _fonte = State(initialValue: MXFonteReal(store: store))
        #if DEBUG
        conta = nil
        abrirNosAjustes = false
        #endif
    }

    #if DEBUG
    init(store: BotStore, conta: any MXContaFonte, abrirNosAjustes: Bool) {
        _fonte = State(initialValue: MXFonteReal(store: store))
        self.conta = conta
        self.abrirNosAjustes = abrirNosAjustes
    }
    #endif

    var body: some View {
        #if DEBUG
        if let conta {
            MXCasca(fonte: fonte, conta: conta, abrirNosAjustes: abrirNosAjustes)
                // Já reabriu nos Ajustes: a próxima troca é que pede de novo.
                .onAppear { (conta as? MXContaReal)?.reabrirNosAjustes = false }
        } else {
            MXCasca(fonte: fonte)
        }
        #else
        MXCasca(fonte: fonte)
        #endif
    }
}

/// Dados reais pedidos, mas nenhum computador ligado ainda: entre e conecte pelo app de hoje e abra de novo.
struct MXSemComputador: View {
    #if DEBUG
    /// Aberto pelo interruptor dos Ajustes (e não pelo argumento): o caminho de volta é a faixa do topo.
    var peloInterruptor = false
    #endif

    var body: some View {
        VStack(alignment: .leading, spacing: MX.Espaco.entreBlocos) {
            Text("Conversas", bundle: .module).mxTipo(MX.Tipo.tituloDeTela).foregroundStyle(MX.Claro.texto.color)
            Group {
                #if DEBUG
                if peloInterruptor {
                    Text(verbatim: MXTextosDebug.semComputador())
                } else {
                    Text("Nenhum computador conectado. Entre e conecte pelo app sem o argumento -MexericaRedesignReal e abra de novo.", bundle: .module)
                }
                #else
                Text("Nenhum computador conectado. Entre e conecte pelo app sem o argumento -MexericaRedesignReal e abra de novo.", bundle: .module)
                #endif
            }
                .mxTipo(MX.Tipo.corpo)
                .foregroundStyle(MX.Claro.textoSecundario.color)
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, MX.Espaco.margemTela)
        .padding(.top, MX.Medida.topoAbaixoDaAreaSegura)
        .background { MXFundo() }
        .onAppear { MXFontes.registrar() }
    }
}

/// Aba ainda não montada: fundo e título, como o topo do Mission Board.
struct MXAbaVazia: View {
    let titulo: String
    /// "Ajustes e conta" ainda não abre nada: na fonte real (que nem chega aqui, a aba fica desligada) fica desligado.
    var contaLigada = true

    var body: some View {
        VStack {
            HStack(spacing: MX.Espaco.dentroDeLinha) {
                Button {} label: {
                    MXIconeView(.conta, tamanho: MX.Medida.botaoRedondoIcone)
                        .foregroundStyle(MX.Claro.texto.color)
                        .frame(width: MX.Medida.botaoRedondo, height: MX.Medida.botaoRedondo)
                        .mxVidro(.botao, em: Circle(), interativo: true)
                }
                .buttonStyle(MXToque())
                .mxIndisponivel(!contaLigada)
                .accessibilityLabel(String(localized: "Ajustes e conta", bundle: .module))
                Text(titulo)
                    .mxTipo(MX.Tipo.tituloDeTela)
                    .foregroundStyle(MX.Claro.texto.color)
                    .accessibilityAddTraits(.isHeader)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, MX.Espaco.margemTela + MX.Medida.topoRespiroLateral)
            .padding(.top, MX.Medida.topoAbaixoDaAreaSegura)
            Spacer()
        }
        .background { MXFundo() }
    }
}
