import CodyncKit
import SwiftUI

/// A Conversa de um agente (`capturas/05-conversa.webp`): topo flutuante de vidro (voltar, cápsula do agente,
/// WhatsApp), mensagens que rolam por baixo dele com a borda desfocada, passos recolhidos, o gomo na bolha de quem
/// trabalha, o cartão "Esperando você" (06) e a caixa com a fila (05a). Tocar na cápsula abre o menu (05b).
///
/// Esta view só lê a fonte; o estado da tela (texto da caixa, edição, menu) mora em `MXConversaCorpo`, para cada
/// tecla não remontar a conversa inteira.
public struct MXConversaTela: View {
    let fonte: any MXFonte
    let id: String
    let voltar: () -> Void
    let irPara: (MXDestino) -> Void
    let cena: MXCena?
    /// Para onde o voltar leva ("Conversas"; "Sessões" na sessão aberta, 07b).
    let rotuloVoltar: String

    public init(fonte: any MXFonte, id: String, cena: MXCena? = nil, rotuloVoltar: String? = nil,
                voltar: @escaping () -> Void, irPara: @escaping (MXDestino) -> Void) {
        self.fonte = fonte
        self.id = id
        self.cena = cena
        // O padrão é resolvido aqui: `Bundle.module` é interno e não pode ficar num argumento padrão público.
        self.rotuloVoltar = rotuloVoltar ?? String(localized: "Conversas", bundle: .module)
        self.voltar = voltar
        self.irPara = irPara
    }

    public var body: some View {
        Group {
            if let dados = fonte.conversa(id) {
                MXConversaCorpo(fonte: fonte, id: id, dados: dados, cena: cena, rotuloVoltar: rotuloVoltar, voltar: voltar,
                                irPara: irPara)
            } else {
                MXEmBreveTela(titulo: String(localized: "Conversa", bundle: .module), texto: String(localized: "Este bot não está mais aqui.", bundle: .module), voltar: voltar)
            }
        }
        .toolbarVisibility(.hidden, for: .navigationBar)
        // Ao abrir e ao reconectar: numa sessão extra, a fonte lista as sessões e traz o histórico dela (só lê).
        .task(id: fonte.conexao.podeAgir) { await fonte.abrindo(id) }
    }
}

/// O corpo da Conversa, com o estado da tela.
struct MXConversaCorpo: View {
    let fonte: any MXFonte
    let id: String
    let dados: MXConversaDados
    let voltar: () -> Void
    let irPara: (MXDestino) -> Void
    let cena: MXCena?

    @State private var texto = ""
    @State private var carregou = false
    @State private var edicao = MXEdicaoDaFila()
    @State private var filaRecolhida = false
    /// A pessoa está no fim da conversa (abre no fim); a caixa que muda de altura a mantém lá (`MXFimDaRolagem`).
    @State private var noFim = true
    @State private var rolando = false
    @State private var menuAberto = false
    /// Passos que o usuário abriu ou fechou (o balão de quem trabalha começa aberto; o resto, recolhido).
    @State private var passosTrocados: Set<String> = []
    @State private var capsula: CGRect = .zero
    @State private var tela: CGSize = .zero
    @State private var carregandoAnteriores = false
    @State private var topoVisivel = false
    @State private var falhouAnteriores = false
    /// "Tentar agora" da faixa de queda: lê "Tentando…" por uns 3 s.
    @State private var tentandoReconectar = false
    /// A espera do "Tentando…": guardada para cancelar ao sair da tela (e ao tocar de novo).
    @State private var esperaDoTentar: Task<Void, Never>?
    /// Registro de leitura desta tela (`MXFonte.lendo`).
    @State private var leitura = UUID()
    @State private var visivel = false
    @Environment(\.scenePhase) private var fase
    /// Balões grandes que a pessoa abriu inteiros ("Mostrar tudo").
    @State private var inteiros: Set<String> = []
    @ScaledMetric(relativeTo: .body) private var larguraDoMenu = MX.Conversa.menuLargura
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.accessibilityReduceTransparency) private var reduzirTransparencia
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto

    nonisolated static let espaco = "mx.conversa"

    /// Com Reduzir transparência, as superfícies translúcidas (balão do agente, chip de hora) ficam opacas.
    private func superficie(_ cor: MXCor) -> MXCor { reduzirTransparencia ? cor.sobre(MX.Claro.fundo) : cor }

    let rotuloVoltar: String

    init(fonte: any MXFonte, id: String, dados: MXConversaDados, cena: MXCena?, rotuloVoltar: String? = nil,
         voltar: @escaping () -> Void, irPara: @escaping (MXDestino) -> Void) {
        self.fonte = fonte
        self.id = id
        // O padrão é resolvido aqui: `Bundle.module` é interno e não pode ficar num argumento padrão público.
        self.rotuloVoltar = rotuloVoltar ?? String(localized: "Conversas", bundle: .module)
        self.dados = dados
        self.cena = cena
        self.voltar = voltar
        self.irPara = irPara
    }

    private var c: MX.Conversa.Type { MX.Conversa.self }

    var body: some View { conteudo(dados) }

    private func conteudo(_ dados: MXConversaDados) -> some View {
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        return ZStack(alignment: .topLeading) {
            ScrollViewReader { leitor in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: c.entreMensagens) {
                        // No topo: chegar aqui carrega a página anterior. A rolagem fica ancorada embaixo também quando
                        // o conteúdo cresce (`defaultScrollAnchor(.bottom, for: .sizeChanges)`), então o que se lia não
                        // pula.
                        if fonte.podeCarregarAnteriores(id) {
                            Group {
                                if falhouAnteriores && !carregandoAnteriores {
                                    Button(String(localized: "Tentar de novo", bundle: .module)) { carregarAnteriores() }
                                        .mxTipo(MX.TipoDaConversa.botaoPequeno)
                                        .foregroundStyle(MX.Claro.mexericaTexto.color)
                                        .accessibilityLabel(String(localized: "Carregar mensagens anteriores de novo", bundle: .module))
                                } else {
                                    ProgressView().accessibilityLabel(String(localized: "Carregando mensagens anteriores", bundle: .module))
                                }
                            }
                            .frame(maxWidth: .infinity, minHeight: MX.Espaco.alvoMinimo)
                            .onAppear { topoVisivel = true; carregarAnteriores() }
                            .onDisappear { topoVisivel = false }
                        }
                        ForEach(dados.itens) { linha($0, dados) }
                    }
                    .padding(.horizontal, c.margem)
                    .padding(.bottom, c.respiroFinal)
                }
                .defaultScrollAnchor(.bottom)
                .defaultScrollAnchor(.bottom, for: .sizeChanges)
                .onScrollPhaseChange { _, fase, contexto in
                    rolando = fase != .idle
                    guard fase == .idle else { return }
                    let g = contexto.geometry
                    noFim = MXFimDaRolagem.noFim(falta: MXFimDaRolagem.falta(
                        conteudo: g.contentSize.height, deslocamento: g.contentOffset.y,
                        janela: g.containerSize.height, margemDeBaixo: g.contentInsets.bottom))
                }
                .scrollDismissesKeyboard(.interactively)
                // As duas bordas desfocam suave, como no desenho; a de baixo, sem isto, vinha com a faixa dura do
                // sistema atrás da caixa.
                .scrollEdgeEffectStyle(.soft, for: [.top, .bottom])
                .safeAreaBar(edge: .top, spacing: MX.Espaco.entreBlocos) { topo(dados) }
                .safeAreaBar(edge: .bottom) {
                    caixa(dados).onGeometryChange(for: CGFloat.self) { $0.size.height } action: { _ in
                        // A caixa mudou de altura: quem estava no fim continua no fim. Rola depois do layout (a margem
                        // nova já aplicada) e até a última mensagem: um `ScrollPosition` que já diz "fim" não rola de
                        // novo.
                        guard noFim, !rolando, let ultima = dados.itens.last?.id else { return }
                        Task { @MainActor in leitor.scrollTo(ultima, anchor: .bottom) }
                    }
                }
                .background { MXFundo() }
            }

            if menuAberto {
                Button { fecharMenu() } label: {
                    MX.CorDaConversa.veuDoMenu.color.ignoresSafeArea()
                }
                .buttonStyle(.plain)
                .accessibilityLabel(String(localized: "Fechar menu", bundle: .module))
                .transition(.opacity)
                MXMenuDoAgente(dados: dados, conexao: fonte.conexao, capacidades: fonte.capacidades, acoes: MXAcoesDoMenu(
                    emBreve: { destino in fecharMenu(); irPara(.emBreve(destino)) },
                    // Etapa 3: Ajustes do agente e Autonomia abrem a folha de ajustes (08).
                    ajustes: { fecharMenu(); irPara(.ajustes(dados.agenteId)) },
                    // Nova sessão: cria e abre (só ligada quando a fonte cria; senão, desligada com "Em breve").
                    novaSessao: {
                        guard fonte.capacidades.acao(.novaSessao) == .real else {
                            fecharMenu()
                            irPara(.emBreve(.novaSessao))
                            return
                        }
                        guard let nova = fonte.novaSessao(em: dados.agenteId) else { return }
                        fecharMenu()
                        irPara(.sessao(agente: dados.agenteId, sessao: nova))
                    },
                    // Só com autoridade (o item fica desligado sem ela): fecha o menu e fixa.
                    alternarFixar: {
                        guard fonte.conexao.podeAgir else { return }
                        fecharMenu()
                        fonte.alternarFixar(dados.agenteId)
                    }))
                    .frame(width: max(0, min(MX.Conversa.menuLarguraExterna(conteudo: larguraDoMenu), tela.width - capsula.minX - c.topoMargem)))
                    .offset(x: capsula.minX, y: capsula.maxY + c.menuAbaixoDaCapsula)
                    .transition(transicaoDoMenu)
                    .accessibilityAction(.escape) { fecharMenu() }
            }
        }
        .coordinateSpace(.named(Self.espaco))
        // Links das mensagens: só web e e-mail abrem (`MXLinks`); o resto é descartado.
        .environment(\.openURL, OpenURLAction { url in MXLinks.permitido(url) ? .systemAction : .discarded })
        .onGeometryChange(for: CGSize.self) { $0.size } action: { tela = $0 }
        .animation(troca.animacao, value: menuAberto)
        .onAppear { aoAbrir(dados); visivel = true; atualizarLeitura() }
        .onDisappear {
            visivel = false
            atualizarLeitura()
            // Saiu da tela: a espera do "Tentando…" não fica pendurada.
            esperaDoTentar?.cancel()
            esperaDoTentar = nil
            tentandoReconectar = false
        }
        .onChange(of: fase) { _, _ in atualizarLeitura() }
        .onChange(of: fonte.conexao.podeAgir) { _, _ in
            atualizarLeitura()
            // Reconectou com o topo à vista: o indicador já apareceu, então a carga é reagendada aqui.
            if MXConversaRegras.recarregaAoReconectar(topoVisivel: topoVisivel, conexao: fonte.conexao,
                                                      pode: fonte.podeCarregarAnteriores(id), carregando: carregandoAnteriores) {
                carregarAnteriores()
            }
        }
        .onChange(of: dados.fila) { _, fila in
            // A mensagem em edição saiu da fila (foi mandada, removida em outro aparelho): a edição acaba aqui.
            if let volta = edicao.filaMudou(fila) { texto = volta }
        }
        .onChange(of: texto) { _, novo in
            // O rascunho fica guardado (sobrevive à queda e a sair da tela); a edição da fila não é rascunho.
            if carregou && !edicao.ativa { fonte.guardarRascunho(novo, em: id) }
        }
    }

    /// Abrir o menu: cresce de 0.9 a partir da cápsula; Reduzir movimento, só aparece (nada desliza nem cresce).
    private var transicaoDoMenu: AnyTransition {
        if MX.Movimento.troca(reduzir: reduzirMovimento).interpola {
            return .scale(scale: c.menuEscalaInicial, anchor: .topLeading)
                .combined(with: .offset(y: c.menuDeslocamentoInicial))
                .combined(with: .opacity)
        }
        return .opacity
    }

    private func fecharMenu() { menuAberto = false }

    private func atualizarLeitura() {
        fonte.lendo(id, token: leitura, ativo: MXConversaRegras.lendo(visivel: visivel, cenaAtiva: fase == .active))
    }

    private func carregarAnteriores() {
        guard MXConversaRegras.carregaAnteriores(pode: fonte.podeCarregarAnteriores(id), carregando: carregandoAnteriores,
                                                 conexao: fonte.conexao) else { return }
        carregandoAnteriores = true
        Task {
            let ok = await fonte.carregarAnteriores(id)
            falhouAnteriores = !ok
            carregandoAnteriores = false
        }
    }

    private func aoAbrir(_ dados: MXConversaDados) {
        guard !carregou else { return }
        texto = fonte.rascunho(id)
        carregou = true
        switch cena {
        case .menu: menuAberto = true
        case .filaEditando:
            if dados.fila.count > 1 { texto = edicao.comecar(dados.fila[1], caixa: texto) }
        default: break
        }
    }

    // MARK: Topo

    private func topo(_ dados: MXConversaDados) -> some View {
        VStack(spacing: MX.Espaco.dentroDeLinha) {
            GlassEffectContainer {
                HStack(spacing: c.topoEntreItens) {
                    Button(action: voltar) {
                        MXIconeView(.voltar, tamanho: MX.Medida.botaoRedondoIcone)
                            .foregroundStyle(MX.Claro.texto.color)
                            .frame(width: MX.Medida.botaoRedondo, height: MX.Medida.botaoRedondo)
                            .mxVidro(.botao, em: Circle(), interativo: true)
                    }
                    .buttonStyle(MXToque())
                    .accessibilityLabel(String(localized: "Voltar para \(rotuloVoltar)", bundle: .module))
                    capsulaDoAgente(dados)
                    Spacer(minLength: 0)
                    if dados.mostraWhatsApp { botaoWhatsApp(dados) }
                }
            }
            if let titulo = MXTextoDeQueda.tituloDaFaixa(fonte.conexao, faixa: fonte.faixaDeQueda) { faixaDeQueda(titulo) }
        }
        .padding(.horizontal, c.topoMargem)
        .padding(.top, MX.Medida.topoAbaixoDaAreaSegura)
    }

    private func botaoWhatsApp(_ dados: MXConversaDados) -> some View {
        let ligado = fonte.capacidades.ligado(.whatsapp)
        return Button { irPara(.emBreve(.whatsapp)) } label: {
            MXIconeView(.whatsapp, tamanho: c.whatsappIcone)
                .foregroundStyle(MX.Claro.sucessoTexto.color)
                .frame(width: c.whatsappLargura, height: MX.Espaco.alvoMinimo)
                .padding(.horizontal, c.whatsappRespiro)
                .frame(minHeight: c.capsulaAltura)
                .mxVidro(.botao, em: Capsule(), interativo: true)
        }
        .buttonStyle(MXToque())
        // Não existe nesta fonte: à vista, desligado, "Em breve".
        .mxIndisponivel(!ligado)
        .accessibilityLabel(String(localized: "WhatsApp de \(dados.nome)", bundle: .module))
    }

    private func capsulaDoAgente(_ dados: MXConversaDados) -> some View {
        let estado = MXEstadoRegras.texto(dados.estado)
        return Button { menuAberto.toggle() } label: {
            HStack(spacing: c.topoEntreItens) {
                MXPelucia(rosto: dados.rosto, estado: dados.estado, tamanho: c.capsulaPelucia, comSelos: false)
                VStack(alignment: .leading, spacing: 0) {
                    Text(dados.nome)
                        .mxTipo(MX.TipoDaConversa.nomeNaCapsula)
                        .foregroundStyle(MX.Claro.texto.color)
                    Text(estado)
                        .mxTipo(MX.TipoDaConversa.estadoNaCapsula)
                        .foregroundStyle(MXEstadoRegras.cor(dados.estado).color)
                }
                .lineLimit(1)
                // Como o título da barra nativa: o texto para de crescer no teto (a cápsula fica do tamanho certo e
                // não invade a conversa); o grande fica no visualizador de conteúdo grande.
                .dynamicTypeSize(...MX.Conversa.capsulaTextoAte)
                MXIconeView(.setaParaBaixo, tamanho: c.capsulaSeta, traco: c.capsulaSetaTraco)
                    .foregroundStyle(MX.Claro.textoSecundario.color)
                    .padding(.leading, c.capsulaSetaRecuo)
            }
            .padding(.leading, c.capsulaRespiroEsquerda)
            .padding(.trailing, c.capsulaRespiroDireita)
            .frame(minHeight: c.capsulaAltura)
            .mxVidro(.botao, em: Capsule(), interativo: true)
        }
        .buttonStyle(MXToque())
        .onGeometryChange(for: CGRect.self) { $0.frame(in: .named(Self.espaco)) } action: { capsula = $0 }
        .layoutPriority(1)
        .accessibilityLabel(String(localized: "\(MXConversaRegras.rotuloDaCapsula(nome: dados.nome, funcao: dados.funcao, estado: estado)). Opções", bundle: .module))
        // Segurar a cápsula mostra o nome e o estado grandes.
        .accessibilityShowsLargeContentViewer {
            Text(MXConversaRegras.rotuloDaCapsula(nome: dados.nome, funcao: "", estado: estado))
        }
        .accessibilityValue(menuAberto ? String(localized: "Menu aberto", bundle: .module) : "")
        .accessibilityHint(String(localized: "Abre o menu do bot", bundle: .module))
    }

    /// Queda curta: discreta, no topo da conversa (texto e detalhe do desktop), depois da carência do store
    /// (`faixaDeQueda`), com "Tentar agora" como a `FaixaDeQueda` do app (lê "Tentando…" por uns 3 s).
    private func faixaDeQueda(_ titulo: String) -> some View {
        HStack(spacing: MX.Medida.entreItens) {
            HStack(spacing: MX.Medida.entreItens) {
                Image(systemName: "wifi.slash")
                Text(titulo).mxTipo(MX.Tipo.legendaForte)
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(titulo)
            .accessibilityHint(MXTextoDeQueda.detalhe)
            .accessibilityAddTraits(.updatesFrequently)
            if fonte.conexao != .desconectado, fonte.podeTentarAgora {
                Button {
                    fonte.tentarAgora()
                    tentandoReconectar = true
                    esperaDoTentar?.cancel()
                    esperaDoTentar = Task {
                        try? await Task.sleep(for: .seconds(3))
                        guard !Task.isCancelled else { return }
                        tentandoReconectar = false
                    }
                } label: {
                    Text(tentandoReconectar ? MXTextoDeQueda.tentando : MXTextoDeQueda.tentar)
                        .mxTipo(MX.Tipo.legendaForte)
                        .foregroundStyle(MX.Claro.mexericaTexto.color)
                        .frame(minWidth: MX.Espaco.alvoMinimo, minHeight: MX.Espaco.alvoMinimo)
                        .contentShape(Rectangle())
                }
                .buttonStyle(MXToque())
                .disabled(tentandoReconectar)
                .accessibilityHint(MXTextoDeQueda.detalhe)
            }
        }
        .foregroundStyle(MX.Claro.texto.color)
        .padding(.horizontal, c.chipRespiro)
        .frame(minHeight: c.passosAltura)
        .mxVidro(.botao, em: Capsule())
    }

    // MARK: Mensagens

    @ViewBuilder private func linha(_ item: MXItemDaConversa, _ dados: MXConversaDados) -> some View {
        switch item.conteudo {
        case let .separador(texto):
            Text(texto)
                .mxTipo(MX.TipoDaConversa.chipDeHora)
                .foregroundStyle(MX.Claro.texto.color)
                .padding(.horizontal, c.chipRespiro)
                .frame(minHeight: c.chipAltura)
                .background(superficie(MX.CorDaConversa.chipDeHora).color, in: Capsule())
                .overlay(Capsule().stroke(MX.CorDaConversa.contorno.color, lineWidth: MX.Vidro.contornoOpacoEspessura))
                .frame(maxWidth: .infinity)
                .accessibilityAddTraits(.isHeader)
        case let .usuario(m):
            balaoDoUsuario(m, id: item.id)
        case let .agente(m):
            comPelucia(dados) { balaoDoAgente(m, id: item.id) }
        case let .trabalhando(t):
            comPelucia(dados) { balaoTrabalhando(t, id: item.id) }
        case let .aprovacao(p):
            MXCartaoDeAprovacao(pedido: p, conectado: fonte.conexao.podeAgir) { opcao in
                fonte.responder(p, opcao: opcao, em: id)
            }
        case let .aviso(texto, erro):
            Text(texto)
                .mxTipo(MX.TipoDaConversa.atividade)
                .foregroundStyle((erro ? MX.Claro.erroTexto : MX.Claro.textoSecundario).color)
                .multilineTextAlignment(.center)
                .frame(maxWidth: .infinity)
        }
    }

    private func balaoDoUsuario(_ m: MXMensagemDoUsuario, id itemId: String) -> some View {
        let corte = MXConversaRegras.cortar(m.texto)
        let inteiro = inteiros.contains(itemId)
        let texto = Text(inteiro ? m.texto : corte.texto)
            .mxTipo(MX.TipoDaConversa.balaoDoUsuario)
            .foregroundStyle(MX.Claro.sobreMexerica.color)
            .textSelection(.enabled)
        return VStack(alignment: .trailing, spacing: c.notaAcima) {
            // Como o `fit-content` do desenho: o que cabe numa linha abraça o texto; o que quebra ocupa a largura
            // máxima do balão (o SwiftUI, sozinho, encolheria o balão até a linha mais longa).
            ViewThatFits(in: .horizontal) {
                texto.fixedSize(horizontal: true, vertical: false)
                texto.frame(maxWidth: .infinity, alignment: .leading)
            }
                .padding(c.balaoRespiro)
                .background {
                    UnevenRoundedRectangle(topLeadingRadius: c.raioDoBalao, bottomLeadingRadius: c.raioDoBalao,
                                           bottomTrailingRadius: c.raioDoCantoDoBalao, topTrailingRadius: c.raioDoBalao)
                        .fill(MX.Claro.balaoDoUsuario.color)
                        .shadow(color: MX.CorDaConversa.sombraLaranja.color, radius: c.sombraDoUsuarioRaio, y: c.sombraDoUsuarioY)
                }
            if corte.cortado && !inteiro { botaoMostrarTudo(itemId) }
            if m.nota != nil || m.podeReenviar {
                HStack(spacing: MX.Espaco.dentroDeLinha) {
                    if let nota = m.nota {
                        Text(nota)
                            .mxTipo(MX.TipoDaConversa.nota)
                            .foregroundStyle((m.podeReenviar ? MX.Claro.erroTexto : MX.Claro.textoSecundario).color)
                    }
                    if m.podeReenviar {
                        Button(String(localized: "Reenviar", bundle: .module)) { fonte.reenviar(itemId, em: id) }
                            .mxTipo(MX.TipoDaConversa.botaoPequeno)
                            .foregroundStyle(MX.Claro.mexericaTexto.color)
                            .frame(minHeight: MX.Espaco.alvoMinimo)
                            .disabled(!fonte.conexao.podeAgir)
                    }
                }
                .padding(.trailing, c.notaRecuo)
            }
        }
        .frame(maxWidth: MXConversaRegras.larguraMaxima(c.balaoDoUsuarioLarguraExterna, tamanhoDoTexto), alignment: .trailing)
        .frame(maxWidth: .infinity, alignment: .trailing)
        .accessibilityElement(children: .contain)
    }

    /// Foto do agente (30 pt) ao lado do balão, como no desenho.
    private func comPelucia<Balao: View>(_ dados: MXConversaDados, @ViewBuilder _ balao: () -> Balao) -> some View {
        HStack(alignment: .top, spacing: c.entreFotoEBalao) {
            MXPelucia(rosto: dados.rosto, estado: dados.estado, tamanho: c.peluciaDoBalao, comSelos: false)
                .padding(.top, c.peluciaDoBalaoAcima)
            balao()
                .frame(maxWidth: MXConversaRegras.larguraMaxima(c.balaoDoAgenteLarguraExterna, tamanhoDoTexto), alignment: .leading)
            Spacer(minLength: 0)
        }
    }

    /// Forma do balão do agente: o canto de cima à esquerda é o "de quem fala".
    private var formaDoAgente: UnevenRoundedRectangle {
        UnevenRoundedRectangle(topLeadingRadius: c.raioDoCantoDoBalao, bottomLeadingRadius: c.raioDoBalao,
                               bottomTrailingRadius: c.raioDoBalao, topTrailingRadius: c.raioDoBalao)
    }

    private func fundoDoAgente<Conteudo: View>(_ conteudo: Conteudo, respiro: EdgeInsets) -> some View {
        conteudo
            .padding(respiro)
            .background {
                formaDoAgente.fill(superficie(MX.Claro.balaoDoAgente).color)
                    .overlay(formaDoAgente.stroke(MX.CorDaConversa.contorno.color, lineWidth: MX.Vidro.contornoOpacoEspessura))
                    .shadow(color: MX.CorDaConversa.sombraDoBalao.color, radius: c.sombraDoBalaoRaio, y: c.sombraDoBalaoY)
            }
    }

    private func balaoDoAgente(_ m: MXMensagemDoAgente, id itemId: String) -> some View {
        fundoDoAgente(
            VStack(alignment: .leading, spacing: c.dentroDoBalao) {
                if m.cabecalho {
                    HStack(alignment: .firstTextBaseline, spacing: MX.Medida.entreItens) {
                        Text(m.autor).mxTipo(MX.TipoDaConversa.autor).foregroundStyle(MX.Claro.texto.color)
                        if let hora = m.hora {
                            Text(hora).mxTipo(MX.TipoDaConversa.horaDoBalao).foregroundStyle(MX.Claro.textoSecundario.color)
                        }
                    }
                }
                if let passos = m.passos { blocoDePassos(passos, id: itemId, abertoDeInicio: false, emCurso: false) }
                textoDoAgente(m, id: itemId)
            },
            respiro: c.balaoDoAgenteRespiro)
        .accessibilityElement(children: .contain)
    }

    /// O texto do agente: markdown do cache (por id e revisão); grande, entra cortado com "Mostrar tudo".
    @ViewBuilder private func textoDoAgente(_ m: MXMensagemDoAgente, id itemId: String) -> some View {
        let corte = MXConversaRegras.cortar(m.texto)
        let inteiro = inteiros.contains(itemId) || !corte.cortado
        let chave = m.chave.isEmpty ? "" : m.chave + (inteiro ? "#inteiro" : "#corte")
        Text(MXCacheDeTexto.rico(inteiro ? m.texto : corte.texto, chave: chave))
            .mxTipo(MX.TipoDaConversa.balaoDoAgente)
            .foregroundStyle(MX.Claro.texto.color)
            .textSelection(.enabled)
        if !inteiro { botaoMostrarTudo(itemId) }
    }

    private func botaoMostrarTudo(_ itemId: String) -> some View {
        Button(String(localized: "Mostrar tudo", bundle: .module)) { inteiros.insert(itemId) }
            .mxTipo(MX.TipoDaConversa.botaoPequeno)
            .foregroundStyle(MX.Claro.mexericaTexto.color)
            .frame(minHeight: MX.Espaco.alvoMinimo)
    }

    private func balaoTrabalhando(_ t: MXTrabalhoEmCurso, id itemId: String) -> some View {
        let conteudo = VStack(alignment: .leading, spacing: t.passos == nil ? c.dentroDoBalaoTrabalhando : c.dentroDoBalao) {
            Text(t.autor).mxTipo(MX.TipoDaConversa.autor).foregroundStyle(MX.Claro.texto.color)
            if let passos = t.passos { blocoDePassos(passos, id: itemId, abertoDeInicio: true, emCurso: true) }
            if t.esperandoDecisao {
                Text("Esperando a sua decisão acima", bundle: .module)
                    .mxTipo(MX.TipoDaConversa.status)
                    .foregroundStyle(MX.Claro.textoSecundario.color)
                    .accessibilityAddTraits(.updatesFrequently)
            } else if let decisao = t.decisao {
                Text(decisao)
                    .mxTipo(MX.TipoDaConversa.status)
                    .foregroundStyle((decisao.hasPrefix("✓") ? MX.Claro.sucessoTexto : MX.Claro.textoSecundario).color)
            } else {
                HStack(spacing: MX.Espaco.dentroDeLinha) {
                    MXGomo(tamanho: c.gomoNoBalao)
                    Text(t.atividade)
                        .mxTipo(MX.TipoDaConversa.atividade)
                        .foregroundStyle(MX.Claro.textoSecundario.color)
                }
                .accessibilityElement(children: .combine)
            }
        }
        // `fit-content` do desenho (06): o balão abraça o conteúdo e o passo ocupa a largura dele; o que não cabe
        // numa linha usa a largura máxima.
        return fundoDoAgente(
            ViewThatFits(in: .horizontal) {
                conteudo.fixedSize(horizontal: true, vertical: false)
                conteudo
            },
            respiro: c.balaoRespiro)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "\(t.autor) está trabalhando", bundle: .module))
    }

    /// "3 passos · Leitura, Edição, Comando · 2 min" que abre a lista dos passos.
    private func blocoDePassos(_ passos: MXPassos, id itemId: String, abertoDeInicio: Bool, emCurso: Bool) -> some View {
        let aberto = abertoDeInicio != passosTrocados.contains(itemId)
        return VStack(alignment: .leading, spacing: c.dentroDoBalao) {
            Button {
                withAnimation(MX.Movimento.troca(reduzir: reduzirMovimento).animacaoDaPosicao) {
                    if passosTrocados.contains(itemId) { passosTrocados.remove(itemId) } else { passosTrocados.insert(itemId) }
                }
            } label: {
                HStack(spacing: MX.Medida.entreItens) {
                    MXSetaQueAbre(aberta: aberto, tamanho: c.passosSeta, traco: MX.Medida.iconeTracoSeta)
                    Text(passos.resumo).mxTipo(MX.TipoDaConversa.passos)
                }
                .foregroundStyle(MX.Claro.texto.color)
                .padding(.leading, c.passosRespiroEsquerda)
                .padding(.trailing, c.passosRespiroDireita)
                .frame(minHeight: c.passosAltura)
                .overlay(Capsule().stroke(MX.CorDaConversa.bordaDosPassos.color, lineWidth: MX.Vidro.contornoOpacoEspessura))
                // Pílula desenhada de 30; área de toque de 44 em volta, transparente.
                .frame(minHeight: MXAlvoDeToque.lado(c.passosAltura))
                .contentShape(Rectangle())
            }
            .buttonStyle(MXToque())
            .accessibilityLabel(passos.resumo)
            .accessibilityValue(aberto ? String(localized: "Abertos", bundle: .module) : String(localized: "Recolhidos", bundle: .module))
            if aberto {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(passos.itens) { passo in
                        HStack(spacing: MX.Espaco.dentroDeLinha) {
                            if passo.concluido {
                                MXIconeView(.confirmar, tamanho: c.passoCheck, traco: c.passoCheckTraco)
                                    .foregroundStyle((emCurso ? MX.Claro.textoSecundario : MX.Claro.sucessoTexto).color)
                            } else {
                                MXGomo(tamanho: c.passoCheck)
                            }
                            Text(passo.texto)
                                .mxTipo(emCurso ? MX.TipoDaConversa.passoEmCurso : MX.TipoDaConversa.passo)
                                .foregroundStyle(MX.Claro.texto.color)
                        }
                        .frame(minHeight: emCurso ? c.passoEmCursoAltura : c.passoAltura)
                        .accessibilityElement(children: .combine)
                        .accessibilityValue(passo.concluido ? String(localized: "Feito", bundle: .module) : String(localized: "Em andamento", bundle: .module))
                    }
                }
                // Como no desenho, a lista de passos ocupa a largura do balão (bloco, não abraça o texto); no balão de
                // quem trabalha (ChatAprovar) cada passo tem 34 de altura e 12 de respiro nos lados, sem respiro em cima.
                .padding(emCurso ? c.passoEmCursoRespiro : c.passosListaRespiro)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(MX.CorDaConversa.fundoDosPassos.color, in: RoundedRectangle(cornerRadius: MX.Raio.chip))
                .transition(MX.Movimento.troca(reduzir: reduzirMovimento).transicaoDeConteudo)
            }
        }
    }

    // MARK: Caixa

    private var arranjoDaCaixa: MXArranjoDaCaixa { MXArranjoDaCaixa(acessibilidade: tamanhoDoTexto.isAccessibilitySize) }

    private func caixa(_ dados: MXConversaDados) -> some View {
        MXCaixaDeMensagem(
            dados: dados, conexao: fonte.conexao, capacidades: fonte.capacidades, aviso: fonte.aviso, texto: $texto,
            edicao: edicao,
            // A reserva do salvar é da fonte (sobrevive a sair e reabrir): aqui só as da fila à vista.
            salvando: Set(dados.fila.map(\.id).filter { fonte.salvando($0, em: id) }),
            filaRecolhida: $filaRecolhida,
            arranjo: arranjoDaCaixa,
            // Texto grande: a caixa para em metade da tela (o texto em edição não sai por baixo).
            alturaMaxima: arranjoDaCaixa.alturaMaxima(tela: tela.height),
            acoes: MXAcoesDaCaixa(
                enviar: enviar,
                salvarEdicao: { salvarEdicao(dados) },
                cancelarEdicao: { texto = edicao.encerrar() },
                parar: { fonte.parar(id) },
                aoVivo: { irPara(.emBreve(.aoVivo)) },
                anexar: { irPara(.emBreve(.anexar)) },
                // Mandar agora e Remover não encerram a edição: ela acaba quando o espelho confirmar que a mensagem
                // saiu da fila (`filaMudou`); se o Hub recusar, a edição fica como estava.
                mandarAgora: { item in fonte.mandarAgora(item, em: id) },
                // Enquanto o salvar desta mensagem não voltar, ela não reabre para edição.
                editar: { item in
                    guard !fonte.salvando(item.id, em: id) else { return }
                    texto = edicao.comecar(item, caixa: texto)
                },
                remover: { item in fonte.remover(item, em: id) },
                limparAviso: { fonte.limparAviso() }))
    }

    private func enviar() {
        // Uma sessão extra que ainda não pode receber (sessões não listadas, Hub sem envio por sessão): o texto fica.
        guard fonte.conexao.podeAgir, fonte.conversa(id)?.semEnvio == nil else { return }
        let t = texto
        guard !t.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        texto = ""
        fonte.enviar(t, para: id)
    }

    private func salvarEdicao(_ dados: MXConversaDados) {
        guard fonte.conexao.podeAgir, let original = edicao.item else { return }
        let novo = texto.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !novo.isEmpty, !fonte.salvando(original.id, em: id), let salvamento = edicao.iniciarSalvar(novo) else { return }
        let revisao = MXFilaRegras.revisaoParaSalvar(original, fila: dados.fila)
        // No toque, sem esperar: a fonte confere a ligação, guarda a geração do link e reserva a mensagem.
        guard let tarefa = fonte.editar(original, texto: novo, revisao: revisao, em: id) else {
            _ = edicao.concluirSalvar(salvamento, ok: false, caixa: "")
            return
        }
        Task {
            let ok = await tarefa.value
            let caixa = texto.trimmingCharacters(in: .whitespacesAndNewlines)
            if let volta = edicao.concluirSalvar(salvamento, ok: ok, caixa: caixa) { texto = volta }
        }
    }
}

/// Destino que ainda não existe nesta etapa: fundo, voltar, o nome e "Em breve".
public struct MXEmBreveTela: View {
    let titulo: String
    let texto: String
    let voltar: () -> Void

    public init(titulo: String, texto: String? = nil, voltar: @escaping () -> Void) {
        self.titulo = titulo
        // O padrão é resolvido aqui: `Bundle.module` é interno e não pode ficar num argumento padrão público.
        self.texto = texto ?? String(localized: "Esta parte do app chega numa próxima versão.", bundle: .module)
        self.voltar = voltar
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: MX.Espaco.entreBlocos) {
            Button(action: voltar) {
                MXIconeView(.voltar, tamanho: MX.Medida.botaoRedondoIcone)
                    .foregroundStyle(MX.Claro.texto.color)
                    .frame(width: MX.Medida.botaoRedondo, height: MX.Medida.botaoRedondo)
                    .mxVidro(.botao, em: Circle(), interativo: true)
            }
            .buttonStyle(MXToque())
            .accessibilityLabel(String(localized: "Voltar", bundle: .module))
            Text(titulo)
                .mxTipo(MX.Tipo.tituloDeTela)
                .foregroundStyle(MX.Claro.texto.color)
                .accessibilityAddTraits(.isHeader)
            Text("Em breve", bundle: .module).mxTipo(MX.Tipo.corpoForte).foregroundStyle(MX.Claro.mexericaTexto.color)
            Text(texto).mxTipo(MX.Tipo.corpo).foregroundStyle(MX.Claro.textoSecundario.color)
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, MX.Espaco.margemTela + MX.Medida.topoRespiroLateral)
        .padding(.top, MX.Medida.topoAbaixoDaAreaSegura)
        .background { MXFundo() }
        .toolbarVisibility(.hidden, for: .navigationBar)
    }
}

extension View {
    /// O controle não existe nesta fonte (nota (a) da revisão da etapa 3): fica à vista e desligado, esmaecido, com
    /// valor acessível "Indisponível" e a dica "Em breve". Ligado, a dica é `dica`.
    func mxIndisponivel(_ desligado: Bool, dica: String = "") -> some View {
        self
            .disabled(desligado)
            .opacity(desligado ? MX.Conversa.desligadoOpacidade : 1)
            .accessibilityValue(desligado ? MXIndisponivel.valorAcessivel : "")
            .accessibilityHint(desligado ? MXIndisponivel.emBreve : dica)
    }
}
