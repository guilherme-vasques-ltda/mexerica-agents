import CodyncKit
import SwiftUI

/// Aba Conversas (`capturas/04-conversas.webp`): topo com conta, "Pessoal ⌄" e `+`; cartão do Berga; busca;
/// cartão com Canais (recolhidos) e a lista de agentes. O conteúdo rola por baixo do topo.
/// Etapa 2: tocar no agente ou no Berga abre a Conversa; a busca filtra; Canais e os botões do topo levam a
/// "Em breve". Puxar para Sessões e segurar para Ajustes são da etapa 3 (`MXAcoesDaLista.sessoes`/`ajustes`).
public struct MXConversasTela: View {
    let berga: MXBergaResumo?
    let space: String
    let canais: [MXCanalResumo]
    let agentes: [MXAgenteResumo]
    let acoes: MXAcoesDaLista
    /// O que a fonte tem de verdade: o que não tem (topo, canais) fica à vista e desligado, "Em breve".
    let capacidades: MXCapacidades
    @State private var busca = ""
    /// Tocar em qualquer ponto do campo (44 pt de área de toque) foca a busca.
    @FocusState private var buscaFocada: Bool
    @State private var canaisAbertos = false
    /// Reduzir movimento, ao recolher Canais: as linhas esmaecem no lugar antes de sair do layout.
    @State private var canaisEsmaecendo = false
    /// Conta os esmaeceres: só o último pode tirar as linhas (toques rápidos não se atropelam).
    @State private var esmaecer = 0
    /// Altura visível da rolagem (sem o topo): o painel da lista desce até a borda de baixo, como no desenho.
    @State private var alturaVisivel: CGFloat = 0
    /// O toque no cartão do Berga virou gesto (puxar ou segurar): não abre a conversa.
    @State private var gestoNoBerga = false
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto
    // Ícones pequenos crescem com o texto vizinho (no tamanho padrão, exatamente o desenho).
    @ScaledMetric(relativeTo: .body) private var iconeDoTopo = MX.Medida.botaoRedondoIcone
    @ScaledMetric(relativeTo: .body) private var setaDoSpace = MX.Medida.capsulaSeta
    @ScaledMetric(relativeTo: .body) private var iconeDaBusca = MX.Medida.buscaIcone
    @ScaledMetric(relativeTo: .body) private var seta = MX.Medida.seta
    @ScaledMetric(relativeTo: .footnote) private var iconeDosAgentes = MX.Medida.cabecalhoIcone

    /// O arranjo da lista no tamanho de texto (empilha e ganha linhas nos tamanhos de acessibilidade).
    private var arranjo: MXArranjoDaLista { MXArranjoDaLista(acessibilidade: tamanhoDoTexto.isAccessibilitySize) }

    public init(berga: MXBergaResumo?, space: String, canais: [MXCanalResumo], agentes: [MXAgenteResumo],
                acoes: MXAcoesDaLista = MXAcoesDaLista(), capacidades: MXCapacidades = .exemplo) {
        self.berga = berga
        self.space = space
        self.canais = canais
        self.agentes = agentes
        self.acoes = acoes
        self.capacidades = capacidades
    }

    public init(dados: MXConversasDados, acoes: MXAcoesDaLista, capacidades: MXCapacidades) {
        self.init(berga: dados.berga, space: dados.space, canais: dados.canais, agentes: dados.agentes, acoes: acoes,
                  capacidades: capacidades)
    }

    /// Agentes que a busca deixa.
    private var agentesVisiveis: [MXAgenteResumo] { MXConversasRegras.filtrar(agentes, busca: busca) }
    private var buscando: Bool { !MXConversasRegras.normalizar(busca).isEmpty }

    public var body: some View {
        ScrollView {
            VStack(spacing: MX.Espaco.entreBlocos) {
                if let berga { cartaoDoBerga(berga) }
                campoDeBusca
                painel
                    .frame(maxHeight: .infinity, alignment: .top)
            }
            .frame(minHeight: alturaVisivel, alignment: .top)
            .padding(.horizontal, MX.Espaco.margemTela)
        }
        .onScrollGeometryChange(for: CGFloat.self) { g in
            g.containerSize.height - g.contentInsets.top - g.contentInsets.bottom
        } action: { _, altura in
            alturaVisivel = max(0, altura)
        }
        .scrollIndicators(.hidden)
        .ignoresSafeArea(edges: .bottom)
        .safeAreaBar(edge: .top, spacing: MX.Espaco.entreBlocos) { topo }
        .scrollEdgeEffectStyle(.soft, for: .top)
        .background { MXFundo() }
    }

    // MARK: Topo

    private var topo: some View {
        GlassEffectContainer {
            HStack {
                botaoRedondo(.conta, rotulo: String(localized: "Ajustes e conta", bundle: .module), controle: .conta) { acoes.emBreve(.conta) }
                Spacer()
                Button { acoes.emBreve(.space) } label: {
                    HStack(spacing: MX.Medida.entreItens) {
                        Text(space).mxTipo(MX.Tipo.seletor)
                        MXIconeView(.setaParaBaixo, tamanho: setaDoSpace)
                    }
                    .foregroundStyle(MX.Claro.texto.color)
                    .padding(.leading, MX.Medida.capsulaRespiroEsquerda)
                    .padding(.trailing, MX.Medida.capsulaRespiroDireita)
                    .frame(minHeight: MX.Medida.capsulaAltura)
                    .mxVidro(.botao, em: Capsule(), interativo: true)
                    // Cápsula desenhada de 36; área de toque de 44 (a linha do topo já tem 44).
                    .frame(minHeight: MXAlvoDeToque.lado(MX.Medida.capsulaAltura))
                    .contentShape(Rectangle())
                }
                .buttonStyle(MXToque())
                .mxIndisponivel(!capacidades.ligado(.space), dica: String(localized: "Trocar de Space", bundle: .module))
                .accessibilityLabel(String(localized: "Space: \(space)", bundle: .module))
                Spacer()
                botaoRedondo(.mais, rotulo: String(localized: "Novo bot ou canal", bundle: .module), controle: .novo) { acoes.emBreve(.novo) }
            }
            .padding(.horizontal, MX.Espaco.margemTela + MX.Medida.topoRespiroLateral)
            .padding(.top, MX.Medida.topoAbaixoDaAreaSegura)
        }
    }

    private func botaoRedondo(_ icone: MXIcone, rotulo: String, controle: MXControle, acao: @escaping () -> Void) -> some View {
        // Cresce com o texto até o teto; o círculo acompanha e nunca fica abaixo de 44.
        let tamanho = MXConversasRegras.iconeDoBotaoRedondo(escalado: iconeDoTopo)
        let lado = MXConversasRegras.ladoDoBotaoRedondo(icone: tamanho)
        return Button(action: acao) {
            MXIconeView(icone, tamanho: tamanho)
                .foregroundStyle(MX.Claro.texto.color)
                .frame(width: lado, height: lado)
                .mxVidro(.botao, em: Circle(), interativo: true)
        }
        .buttonStyle(MXToque())
        // Não existe nesta fonte: à vista, desligado, "Em breve".
        .mxIndisponivel(!capacidades.ligado(controle))
        .accessibilityLabel(rotulo)
    }

    // MARK: Berga

    private func cartaoDoBerga(_ berga: MXBergaResumo) -> some View {
        Button {
            guard !gestoNoBerga else { gestoNoBerga = false; return }
            acoes.abrir(berga.id)
        } label: {
            HStack(spacing: MX.Medida.bergaEntreFotoETexto) {
                MXPelucia(rosto: berga.rosto, estado: berga.estado, tamanho: MX.Medida.berga)
                VStack(alignment: .leading, spacing: 0) {
                    // Nome e Space não disputam a linha no texto grande: empilham. O nome nunca quebra no meio da
                    // palavra: encolhe um pouco e, se ainda não couber, corta no fim.
                    let nome = Text(berga.nome).mxTipo(MX.Tipo.nomeNoCartao).foregroundStyle(MX.Claro.texto.color)
                        .lineLimit(1).minimumScaleFactor(MXConversasRegras.escalaMinimaDoNome).truncationMode(.tail)
                    let rotulo = Text(berga.space).mxTipo(MX.Tipo.legenda).foregroundStyle(MX.Claro.textoSecundario.color)
                    if arranjo.nomeESpaceEmpilhados {
                        nome
                        rotulo
                    } else {
                        HStack(alignment: .firstTextBaseline, spacing: MX.Medida.entreItens) {
                            nome
                            rotulo
                        }
                    }
                    HStack(spacing: MX.Medida.entreItens) {
                        Circle().fill(corDoPonto(berga.estado).color)
                            .frame(width: MX.Medida.pontoDeEstado, height: MX.Medida.pontoDeEstado)
                        Text(berga.estadoTexto).mxTipo(MX.Tipo.legendaEstado)
                    }
                    .foregroundStyle(MX.Claro.textoSecundario.color)
                    Text(berga.mensagem)
                        .mxTipo(MX.Tipo.previa)
                        .foregroundStyle(MX.Claro.texto.color)
                        .lineLimit(arranjo.linhasDaMensagemDoBerga)
                }
                Spacer(minLength: 0)
            }
            .padding(MX.Medida.bergaRespiro)
            .mxVidro(.painel, em: RoundedRectangle(cornerRadius: MX.Raio.painel), interativo: true)
        }
        .buttonStyle(MXToque())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(verbatim: "\(berga.nome), \(berga.space), \(berga.estadoTexto), \(berga.mensagem)")) // textos já traduzidos
        .accessibilityHint(String(localized: "Abre a conversa", bundle: .module))
        // Etapa 3: puxar para as Sessões, segurar para os ajustes.
        .mxGestosDoAgente(sessoes: acoes.sessoes.map { f in { f(berga.id) } },
                          ajustes: acoes.ajustes.map { f in { f(berga.id) } }, gestoUsado: $gestoNoBerga)
    }

    private func corDoPonto(_ estado: MexeriquinhaState) -> MXCor {
        switch estado {
        case .livre, .trabalhando: MX.Claro.sucesso
        case .precisaResposta: MX.Claro.mexerica
        case .erro: MX.Claro.erro
        case .dormindo: MX.Claro.borda
        }
    }

    // MARK: Busca

    private var campoDeBusca: some View {
        HStack(spacing: MX.Espaco.dentroDeLinha) {
            MXIconeView(.busca, tamanho: iconeDaBusca)
                .foregroundStyle(MX.Claro.textoSecundario.color)
            TextField("", text: $busca, prompt: Text("Buscar", bundle: .module).foregroundStyle(MX.Claro.textoSecundario.color))
                .mxTipo(MX.Tipo.campo)
                .foregroundStyle(MX.Claro.texto.color)
                .submitLabel(.search)
                .focused($buscaFocada)
                .accessibilityLabel(String(localized: "Buscar", bundle: .module))
        }
        .padding(.horizontal, MX.Medida.buscaRespiro)
        .frame(minHeight: MX.Medida.buscaAltura)
        .mxVidro(.campo, em: RoundedRectangle(cornerRadius: MX.Raio.chip), interativo: true)
        // Campo desenhado de 40; área de toque de 44 (2 pt de cada lado, dentro dos 10 entre blocos).
        .frame(minHeight: MXAlvoDeToque.lado(MX.Medida.buscaAltura))
        .contentShape(Rectangle())
        // Simultâneo: não tira o toque do próprio campo (cursor, seleção).
        .simultaneousGesture(TapGesture().onEnded { buscaFocada = true })
    }

    // MARK: Painel

    private var painel: some View {
        VStack(spacing: 0) {
            cabecalhoDosCanais
            if buscando {
                ForEach(MXConversasRegras.canaisVisiveis(canais, abertos: canaisAbertos, busca: busca)) { linhaDeCanal($0) }
            } else if canaisAbertos {
                ForEach(canais) { linhaDeCanal($0).opacity(canaisEsmaecendo ? 0 : 1) }
            }
            Rectangle()
                .fill(MX.Claro.linha.color)
                .frame(height: MX.Medida.divisoria)
                .padding(.horizontal, MX.Medida.respiroDeLinha)
                .padding(.top, MX.Medida.divisoriaAcima)
                .padding(.bottom, MX.Medida.divisoriaAbaixo)
            HStack(spacing: MX.Espaco.dentroDeLinha) {
                MXIconeView(.robo, tamanho: iconeDosAgentes)
                Text("Bots", bundle: .module).mxTipo(MX.Tipo.legendaForte)
                Spacer(minLength: 0)
            }
            .foregroundStyle(MX.Claro.textoSecundario.color)
            .padding(.horizontal, MX.Medida.respiroDeLinha)
            .frame(minHeight: MX.Medida.cabecalhoAltura)
            .accessibilityAddTraits(.isHeader)
            ForEach(agentesVisiveis) { agente in
                MXLinhaDeAgente(agente: agente, sessoes: acoes.sessoes.map { f in { f(agente.id) } },
                                ajustes: acoes.ajustes.map { f in { f(agente.id) } }) { acoes.abrir(agente.id) }
            }
            if buscando, agentesVisiveis.isEmpty, MXConversasRegras.canaisVisiveis(canais, abertos: false, busca: busca).isEmpty {
                Text(MXConversasRegras.semResultado(busca))
                    .mxTipo(MX.Tipo.legenda)
                    .foregroundStyle(MX.Claro.textoSecundario.color)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, MX.Medida.respiroDeLinha)
                    .frame(minHeight: MX.Medida.canalAltura)
            }
        }
        .padding([.top, .horizontal], MX.Medida.painelRespiro)
        .padding(.bottom, MX.Medida.painelFolgaFinal)
        .frame(maxHeight: .infinity, alignment: .top)
        .mxVidro(.painel, em: UnevenRoundedRectangle(topLeadingRadius: MX.Raio.painel, topTrailingRadius: MX.Raio.painel))
    }

    private var cabecalhoDosCanais: some View {
        Button { alternarCanais() } label: {
            HStack(spacing: MX.Espaco.dentroDeLinha) {
                Text("#").mxTipo(MX.Tipo.cerquilhaCabecalho)
                    .frame(minWidth: MX.Medida.cerquilhaLargura) // cresce com o texto, sem cortar o "#"
                // Texto grande: a contagem vai para a linha de baixo (não quebra "3 · 3 novas" ao lado de Canais).
                let resumo = Text(canaisMostrados ? "" : MXConversasRegras.resumoDosCanais(canais))
                    .mxTipo(MX.Tipo.legenda)
                    .foregroundStyle(MX.Claro.textoSecundario.color)
                if arranjo.contagemDosCanaisEmbaixo {
                    VStack(alignment: .leading, spacing: 0) {
                        Text("Canais", bundle: .module).mxTipo(MX.Tipo.corpoForte)
                        if !canaisMostrados { resumo }
                    }
                } else {
                    Text("Canais", bundle: .module).mxTipo(MX.Tipo.corpoForte)
                    resumo
                }
                Spacer(minLength: 0)
                setaDosCanais
                    .foregroundStyle(MX.Claro.textoSecundario.color)
            }
            .foregroundStyle(MX.Claro.texto.color)
            .padding(.horizontal, MX.Medida.respiroDeLinha)
            .frame(minHeight: MX.Medida.canaisAltura)
        }
        .buttonStyle(MXToque())
        .accessibilityLabel(String(localized: "Canais, \(MXConversasRegras.resumoDosCanais(canais))", bundle: .module))
        .accessibilityValue(canaisMostrados ? String(localized: "Abertos", bundle: .module) : String(localized: "Recolhidos", bundle: .module))
    }

    /// Canais abertos para quem olha (durante o esmaecer de saída, já contam como recolhidos).
    private var canaisMostrados: Bool { canaisAbertos && !canaisEsmaecendo }

    /// Abre ou recolhe Canais. Normal: a mola anima tudo (a lista desliza). Reduzir movimento: nada muda de
    /// posição animado; ao abrir, a lista pula e as linhas aparecem por cross-fade curto; ao recolher, as linhas
    /// esmaecem no lugar (a lista ainda não se moveu, então nada fica por cima) e só depois saem, sem animar.
    private func alternarCanais() {
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        if canaisMostrados, troca.saidaEmDuasFases {
            esmaecer += 1
            let este = esmaecer
            withAnimation(troca.animacao) {
                canaisEsmaecendo = true
            } completion: {
                // Se tocaram de novo durante o esmaecer, a reabertura já cancelou esta saída.
                guard canaisEsmaecendo, este == esmaecer else { return }
                canaisAbertos = false
                canaisEsmaecendo = false
            }
        } else if canaisEsmaecendo {
            // Tocou de novo durante o esmaecer: as linhas ainda estão no layout e voltam com o mesmo cross-fade.
            esmaecer += 1
            withAnimation(troca.animacao) { canaisEsmaecendo = false }
        } else {
            withAnimation(troca.animacaoDaPosicao) { canaisAbertos.toggle() }
        }
    }

    /// Seta de Canais. Normal: gira 0° → 90° com a mola. Reduzir movimento: não gira; a seta para a direita é
    /// substituída pela seta para baixo (o mesmo desenho a 90°) com cross-fade curto.
    @ViewBuilder private var setaDosCanais: some View {
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        if troca.interpola {
            MXIconeView(.setaParaDireita, tamanho: seta)
                .rotationEffect(.degrees(canaisMostrados ? 90 : 0))
        } else {
            // A troca vem sem animação de posição; o cross-fade vai preso à transição.
            ZStack {
                if canaisMostrados {
                    MXIconeView(.setaParaBaixo, tamanho: seta)
                        .transition(.opacity.animation(troca.animacao))
                } else {
                    MXIconeView(.setaParaDireita, tamanho: seta)
                        .transition(.opacity.animation(troca.animacao))
                }
            }
            .frame(width: seta, height: seta)
        }
    }

    /// Linhas de canal. Normal: entram e saem por opacidade enquanto a lista desliza com a mola.
    /// Reduzir movimento: a linha que entra aparece por cross-fade curto; a que sai já esmaeceu no lugar
    /// (`alternarCanais`), então sai do layout sem transição.
    private var transicaoDaLinha: AnyTransition {
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        return troca.interpola ? .opacity : .asymmetric(insertion: .opacity.animation(troca.animacao), removal: .identity)
    }

    private func linhaDeCanal(_ canal: MXCanalResumo) -> some View {
        let abre = capacidades.ligado(.canal)
        return Button { acoes.abrirCanal(canal.id) } label: {
            HStack(spacing: MX.Medida.entreItens) {
                Text("#").mxTipo(MX.Tipo.cerquilha).foregroundStyle(MX.Claro.textoSecundario.color)
                Text(canal.nome)
                    .mxTipo(canal.naoLidas > 0 ? MX.Tipo.corpoForte : MX.Tipo.corpo)
                    .foregroundStyle(MX.Claro.texto.color)
                // O canal (10) ainda não abre nesta fonte: a linha fica à vista, desligada, "Em breve".
                if !abre {
                    Text(MXIndisponivel.emBreve).mxTipo(MX.Tipo.legenda).foregroundStyle(MX.Claro.textoSecundario.color)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                MXContadores(rodando: canal.sessoesRodando, esperando: canal.sessoesEsperando, naoLidas: canal.naoLidas)
            }
            .padding(.leading, MX.Medida.canalRecuo)
            .padding(.trailing, MX.Medida.respiroDeLinha)
            .frame(minHeight: MXAlvoDeToque.lado(MX.Medida.canalAltura)) // linha de 40 no desenho; toque de 44
        }
        .buttonStyle(MXToque())
        .mxIndisponivel(!abre)
        .transition(transicaoDaLinha)
    }
}

/// Uma linha de agente: pelúcia (com gomo ou "z"), nome, função, hora, prévia e contadores.
struct MXLinhaDeAgente: View {
    let agente: MXAgenteResumo
    /// Etapa 3: puxar para as Sessões e segurar para os ajustes (nulos: o gesto não existe).
    var sessoes: (() -> Void)?
    var ajustes: (() -> Void)?
    var abrir: () -> Void = {}
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto
    /// O toque virou gesto (puxar ou segurar): não abre a conversa.
    @State private var gestoUsado = false

    init(agente: MXAgenteResumo, sessoes: (() -> Void)? = nil, ajustes: (() -> Void)? = nil, abrir: @escaping () -> Void = {}) {
        self.agente = agente
        self.sessoes = sessoes
        self.ajustes = ajustes
        self.abrir = abrir
    }

    private var arranjo: MXArranjoDaLista { MXArranjoDaLista(acessibilidade: tamanhoDoTexto.isAccessibilitySize) }

    /// A etiqueta da função ("Dev").
    private var etiqueta: some View {
        Text(agente.funcao)
            .mxTipo(MX.Tipo.rotulo)
            .foregroundStyle(MX.Claro.textoSecundario.color)
            .lineLimit(arranjo.funcaoEmLinhaPropria ? nil : 1)
            .padding(MX.Medida.etiquetaRespiro)
            .background(MX.Claro.etiqueta.color, in: Capsule())
    }

    var body: some View {
        Button {
            guard !gestoUsado else { gestoUsado = false; return }
            abrir()
        } label: {
            HStack(spacing: MX.Espaco.dentroDeLinha) {
                MXPelucia(rosto: agente.rosto, estado: agente.estado, tamanho: MX.Medida.pelucia,
                          sessoesRodando: agente.sessoesRodando)
                VStack(alignment: .leading, spacing: 0) {
                    HStack(spacing: MX.Medida.entreItens) {
                        Text(agente.nome).mxTipo(MX.Tipo.corpoForte).foregroundStyle(MX.Claro.texto.color)
                            .lineLimit(1).layoutPriority(1)
                        if !agente.funcao.isEmpty, !arranjo.funcaoEmLinhaPropria { etiqueta }
                        Spacer(minLength: 0)
                        let destaque = MXConversasRegras.horaDestacada(agente)
                        Text(agente.hora)
                            .mxTipo(destaque ? MX.Tipo.legendaForte : MX.Tipo.legenda)
                            .foregroundStyle((destaque ? MX.Claro.mexericaTexto : MX.Claro.textoSecundario).color)
                            .lineLimit(1)
                            .layoutPriority(1)
                    }
                    // Texto grande: a função ganha linha própria, embaixo do nome (não é cortada).
                    if !agente.funcao.isEmpty, arranjo.funcaoEmLinhaPropria { etiqueta }
                    HStack(spacing: MX.Medida.entreItens) {
                        Text(agente.previa)
                            .mxTipo(agente.tom == .alerta ? MX.Tipo.previaForte : MX.Tipo.previa)
                            .foregroundStyle(corDaPrevia.color)
                            .lineLimit(arranjo.linhasDaPrevia)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        MXContadores(rodando: agente.sessoesRodando, esperando: agente.sessoesEsperando, naoLidas: agente.naoLidas)
                    }
                }
            }
            .padding(.horizontal, MX.Medida.agenteRespiro)
            .frame(minHeight: MX.Medida.agenteAltura)
            .contentShape(RoundedRectangle(cornerRadius: MX.Raio.linha))
        }
        .buttonStyle(MXToque())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(MXConversasRegras.descricao(agente))
        .accessibilityHint(String(localized: "Abre a conversa", bundle: .module))
        .accessibilityAddTraits(.isButton)
        .mxGestosDoAgente(sessoes: sessoes, ajustes: ajustes, gestoUsado: $gestoUsado)
    }

    private var corDaPrevia: MXCor {
        switch agente.tom {
        case .alerta: MX.Claro.mexericaTexto
        case .ativo: MX.Claro.sucessoTexto
        case .normal: MX.Claro.textoSecundario
        }
    }
}

/// Pílulas de sessões (rodando em verde, esperando em amarelo) e o contador laranja de não lidas.
struct MXContadores: View {
    let rodando: Int
    let esperando: Int
    let naoLidas: Int

    var body: some View {
        HStack(spacing: MX.Medida.entreItens) {
            if let n = MXConversasRegras.contador(rodando) {
                pilula(n, fundo: MX.Claro.rodandoFundo, texto: MX.Claro.rodandoTexto, ponto: MX.Claro.sucesso)
            }
            if let n = MXConversasRegras.contador(esperando) {
                pilula(n, fundo: MX.Claro.esperaFundo, texto: MX.Claro.esperaTexto, ponto: MX.Claro.esperaPonto)
            }
            if let n = MXConversasRegras.contador(naoLidas) {
                Text(n)
                    .mxTipo(MX.Tipo.contador)
                    .foregroundStyle(MX.Claro.sobreMexerica.color)
                    .padding(.horizontal, MX.Medida.contadorRespiro)
                    .frame(minWidth: MX.Medida.contador, minHeight: MX.Medida.contador)
                    .background(MX.Claro.mexerica.color, in: Capsule())
            }
        }
        .fixedSize()
    }

    private func pilula(_ n: String, fundo: MXCor, texto: MXCor, ponto: MXCor) -> some View {
        HStack(spacing: MX.Medida.pilulaEntre) {
            Circle().fill(ponto.color).frame(width: MX.Medida.pilulaPonto, height: MX.Medida.pilulaPonto)
            Text(n).mxTipo(MX.Tipo.pilula)
        }
        .foregroundStyle(texto.color)
        .padding(MX.Medida.pilulaRespiro)
        .frame(minHeight: MX.Medida.pilulaAltura)
        .background(fundo.color, in: Capsule())
    }
}

public extension MXConversasRegras {
    /// Texto visível do contador da barra de abas. Fora dos tamanhos de acessibilidade, igual aos outros
    /// contadores ("99+" acima de 99). Nos de acessibilidade, "9+" acima de 9: assim o selo cabe na aba com a
    /// fonte do tamanho pedido, sem encolher. O VoiceOver lê o número inteiro (`descricaoDoContador`).
    static func contadorDaAba(_ n: Int, tamanho: DynamicTypeSize) -> String? {
        guard tamanho.isAccessibilitySize, n > 9 else { return contador(n) }
        return "9+"
    }
}
