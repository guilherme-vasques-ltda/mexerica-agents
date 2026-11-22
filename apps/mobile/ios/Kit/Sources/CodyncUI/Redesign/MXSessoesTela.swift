import CodyncKit
import SwiftUI

/// Sessões do agente: topo com "‹ Conversas" e voz, a pelúcia com nome, função, estado e quantas sessões,
/// a busca, a árvore em "Agora" e "Paradas" (até 4 níveis, linhas de 44 pt), "Arquivadas · N" e a Nova sessão laranja flutuando embaixo.
///
/// Gestos (README, "Gestos"): tocar na sessão ou puxá-la para a esquerda (passou de 70 pt) abre; a seta abre e fecha
/// um nível por vez; arquivar uma parada mostra "Arquivada: …" com Desfazer. Cada gesto tem a ação acessível.
public struct MXSessoesTela: View {
    let fonte: any MXFonte
    let id: String
    let rotuloVoltar: String
    let voltar: () -> Void
    let irPara: (MXDestino) -> Void
    let cena: MXCena?
    @State private var busca = ""
    @FocusState private var buscaFocada: Bool
    @State private var abertas: Set<String>?
    /// A sessão arquivada agora há pouco (o aviso com Desfazer) e a espera que o tira.
    @State private var arquivada: (id: String, nome: String)?
    @State private var esperaDoAviso: Task<Void, Never>?
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.accessibilityReduceTransparency) private var reduzirTransparencia
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto
    @ScaledMetric(relativeTo: .body) private var iconeDaBusca = MX.Medida.buscaIcone
    @ScaledMetric(relativeTo: .body) private var iconeDoVoltar = MX.Sessoes.voltarIcone

    public init(fonte: any MXFonte, id: String, rotuloVoltar: String? = nil, cena: MXCena? = nil,
                voltar: @escaping () -> Void, irPara: @escaping (MXDestino) -> Void) {
        self.fonte = fonte
        self.id = id
        // O padrão é resolvido aqui: `Bundle.module` é interno e não pode ficar num argumento padrão público.
        self.rotuloVoltar = rotuloVoltar ?? String(localized: "Conversas", bundle: .module)
        self.cena = cena
        self.voltar = voltar
        self.irPara = irPara
    }

    private var s: MX.Sessoes.Type { MX.Sessoes.self }

    public var body: some View {
        Group {
            if let dados = fonte.sessoes(id) {
                conteudo(dados)
            } else {
                MXEmBreveTela(titulo: String(localized: "Sessões", bundle: .module), texto: String(localized: "Este bot não está mais aqui.", bundle: .module), voltar: voltar)
            }
        }
        .toolbarVisibility(.hidden, for: .navigationBar)
        // Só lê: ao abrir e ao reconectar (nada sai sozinho).
        .task(id: fonte.conexao.podeAgir) { await fonte.carregarSessoes(id) }
        .onDisappear {
            esperaDoAviso?.cancel()
            esperaDoAviso = nil
        }
    }

    private func conteudo(_ dados: MXSessoesDados) -> some View {
        let abertas = self.abertas ?? MXSessoesRegras.abertasIniciais(dados.sessoes, recolhida: cena == nil && MX.Sessoes.comecaRecolhida)
        let grupos = MXSessoesRegras.grupos(dados.sessoes, busca: busca, abertas: abertas, fonteArquiva: dados.podeArquivar)
        return ZStack(alignment: .bottom) {
            VStack(spacing: MX.Espaco.entreBlocos) {
                topo(dados)
                cabecalho(dados)
                campoDeBusca
                painel(dados, grupos: grupos, abertas: abertas)
            }
            .padding([.top, .horizontal], MX.Espaco.margemTela)
            .ignoresSafeArea(edges: .bottom)
            if let arquivada { aviso(arquivada, dados: dados) }
            novaSessao(dados)
        }
        .background { MXFundo() }
    }

    // MARK: Topo e cabeçalho

    private func topo(_ dados: MXSessoesDados) -> some View {
        HStack(spacing: MX.Espaco.dentroDeLinha) {
            Button(action: voltar) {
                HStack(spacing: 2) {
                    MXIconeView(.voltar, tamanho: iconeDoVoltar, traco: s.voltarTraco)
                    Text(rotuloVoltar).mxTipo(MX.Tipo.voltarComRotulo).lineLimit(1)
                }
                .foregroundStyle(MX.Claro.texto.color)
                .padding(.leading, s.voltarRespiroEsquerda)
                .padding(.trailing, s.voltarRespiroDireita)
                .frame(minHeight: s.topoAltura)
                .mxVidro(.botao, em: Capsule(), interativo: true)
                .contentShape(Capsule())
            }
            .buttonStyle(MXToque())
            .accessibilityLabel(String(localized: "Voltar para \(rotuloVoltar)", bundle: .module))
            Spacer(minLength: 0)
            Button { irPara(.emBreve(.aoVivo)) } label: {
                MXIconeView(.ligar, tamanho: s.vozIcone)
                    .foregroundStyle(MX.Claro.texto.color)
                    .frame(width: MX.Medida.botaoRedondo, height: MX.Medida.botaoRedondo)
                    .mxVidro(.botao, em: Circle(), interativo: true)
            }
            .buttonStyle(MXToque())
            // Sem ligação na fonte: à vista, desligada, "Em breve".
            .disabled(!dados.podeLigar)
            .opacity(dados.podeLigar ? 1 : MX.Conversa.desligadoOpacidade)
            .accessibilityLabel(String(localized: "Falar por voz", bundle: .module))
            .accessibilityValue(dados.podeLigar ? "" : MXIndisponivel.valorAcessivel)
            .accessibilityHint(dados.podeLigar ? "" : MXIndisponivel.emBreve)
        }
        .dynamicTypeSize(...MX.Conversa.capsulaTextoAte) // barra do topo: teto como a nativa
    }

    private func cabecalho(_ dados: MXSessoesDados) -> some View {
        HStack(spacing: s.cabecalhoEntre) {
            MXPelucia(rosto: dados.rosto, estado: dados.estado, tamanho: s.pelucia, comSelos: false)
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: MX.Espaco.dentroDeLinha) {
                    Text(dados.nome)
                        .mxTipo(MX.Tipo.tituloDoAgente)
                        .foregroundStyle(MX.Claro.texto.color)
                        .lineLimit(1)
                        .minimumScaleFactor(MXConversasRegras.escalaMinimaDoNome)
                        .accessibilityAddTraits(.isHeader)
                    if !dados.funcao.isEmpty {
                        Text(dados.funcao)
                            .mxTipo(MX.Tipo.rotulo)
                            .foregroundStyle(MX.Claro.textoSecundario.color)
                            .padding(MX.Medida.etiquetaRespiro)
                            .background(MX.Claro.etiqueta.color, in: Capsule())
                    }
                }
                Text(verbatim: "\(dados.estadoTexto) · \(MXSessoesRegras.contagem(MXSessoesRegras.conta(dados.sessoes)))")
                    .mxTipo(MX.Tipo.estadoDoAgente)
                    .foregroundStyle(MXEstadoRegras.cor(dados.estado).color)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, s.cabecalhoRecuo)
        .accessibilityElement(children: .combine)
    }

    private var campoDeBusca: some View {
        HStack(spacing: MX.Espaco.dentroDeLinha) {
            MXIconeView(.busca, tamanho: iconeDaBusca).foregroundStyle(MX.Claro.textoSecundario.color)
            TextField("", text: $busca, prompt: Text("Buscar nas sessões", bundle: .module).foregroundStyle(MX.Claro.textoSecundario.color))
                .mxTipo(MX.Tipo.campo)
                .foregroundStyle(MX.Claro.texto.color)
                .submitLabel(.search)
                .focused($buscaFocada)
                .accessibilityLabel(String(localized: "Buscar nas sessões", bundle: .module))
        }
        .padding(.horizontal, MX.Medida.buscaRespiro)
        .frame(minHeight: MX.Medida.buscaAltura)
        .mxVidro(.campo, em: RoundedRectangle(cornerRadius: MX.Raio.chip), interativo: true)
        .frame(minHeight: MXAlvoDeToque.lado(MX.Medida.buscaAltura))
        .contentShape(Rectangle())
        .simultaneousGesture(TapGesture().onEnded { buscaFocada = true })
    }

    // MARK: Árvore

    private func painel(_ dados: MXSessoesDados, grupos: [MXGrupoDeSessoes], abertas: Set<String>) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(grupos) { grupo in
                    Text(verbatim: "\(grupo.titulo) · \(grupo.quantidade)")
                        .mxTipo(MX.Tipo.grupoDeSessoes)
                        .foregroundStyle(MX.Claro.textoSecundario.color)
                        .padding(.top, s.grupoAcima)
                        .padding(.horizontal, s.grupoRecuo)
                        .accessibilityAddTraits(.isHeader)
                    ForEach(grupo.linhas) { linha in
                        // A sessão que não abre nesta fonte (as extras na fonte real) fica à vista, sem abrir: "Em breve".
                        let abre = fonte.conversaDaSessao(linha.id, em: id) != nil || fonte.capacidades.ligado(.sessaoExtra)
                        MXLinhaDaSessao(
                            linha: linha,
                            abrir: abre ? { abrirSessao(linha.id) } : nil,
                            alternar: { alternar(linha.id, abertas: abertas) },
                            arquivar: linha.arquivavel ? { arquivar(linha, dados: dados) } : nil)
                    }
                }
                if grupos.isEmpty, !busca.isEmpty {
                    Text(MXSessoesRegras.semResultado(busca))
                        .mxTipo(MX.Tipo.previa)
                        .foregroundStyle(MX.Claro.textoSecundario.color)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 24)
                        .padding(.horizontal, MX.Espaco.margemTela)
                }
                if let nota = dados.nota {
                    Text(nota)
                        .mxTipo(MX.Tipo.legenda)
                        .foregroundStyle(MX.Claro.textoSecundario.color)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, s.grupoRecuo)
                        .padding(.top, MX.Espaco.dentroDeLinha)
                }
                if let n = dados.arquivadas { linhaDeArquivadas(n, ligada: fonte.capacidades.ligado(.arquivadas)) }
            }
            .padding(.top, s.painelRespiroTopo)
            .padding(.horizontal, s.painelRespiroLado)
            .padding(.bottom, s.painelFolgaFinal)
        }
        .scrollIndicators(.hidden)
        .scrollDismissesKeyboard(.interactively)
        .mxVidro(.painel, em: UnevenRoundedRectangle(topLeadingRadius: MX.Raio.painel, topTrailingRadius: MX.Raio.painel))
    }

    private func linhaDeArquivadas(_ n: Int, ligada: Bool) -> some View {
        Button { irPara(.emBreve(.arquivadas)) } label: {
            HStack(spacing: MX.Espaco.dentroDeLinha) {
                MXIconeView(.arquivar, tamanho: s.arquivadasIcone)
                Text(verbatim: "\(String(localized: "Arquivadas", bundle: .module)) · \(n)").mxTipo(MX.Tipo.arquivadas).frame(maxWidth: .infinity, alignment: .leading)
                MXIconeView(.setaParaDireita, tamanho: MX.Ajustes.itemSeta)
            }
            .foregroundStyle(MX.Claro.textoSecundario.color)
            .padding(.horizontal, MX.Espaco.entreBlocos)
            .frame(minHeight: MX.Espaco.alvoMinimo)
            .contentShape(Rectangle())
        }
        .buttonStyle(MXToque())
        .mxIndisponivel(!ligada)
        .padding(.top, MX.Espaco.entreBlocos)
        .padding(.horizontal, s.recuoBase)
        .accessibilityLabel(String(localized: "Arquivadas, \(n)", bundle: .module))
    }

    private func alternar(_ sessao: String, abertas: Set<String>) {
        var novas = abertas
        if novas.contains(sessao) { novas.remove(sessao) } else { novas.insert(sessao) }
        withAnimation(MX.Movimento.troca(reduzir: reduzirMovimento).animacaoDaPosicao) { self.abertas = novas }
    }

    private func abrirSessao(_ sessao: String) {
        irPara(.sessao(agente: id, sessao: sessao))
    }

    // MARK: Arquivar e Nova sessão

    private func arquivar(_ linha: MXLinhaDeSessao, dados: MXSessoesDados) {
        guard dados.podeArquivar else { return }
        fonte.arquivarSessao(linha.id, em: id)
        withAnimation(MX.Movimento.troca(reduzir: reduzirMovimento).animacao) { arquivada = (linha.id, linha.nome) }
        AccessibilityNotification.Announcement(MXSessoesRegras.avisoDeArquivada(linha.nome)).post()
        esperaDoAviso?.cancel()
        esperaDoAviso = Task {
            try? await Task.sleep(for: .seconds(MX.Sessoes.avisoDuracao))
            guard !Task.isCancelled else { return }
            withAnimation(MX.Movimento.troca(reduzir: reduzirMovimento).animacao) { arquivada = nil }
        }
    }

    private func aviso(_ a: (id: String, nome: String), dados: MXSessoesDados) -> some View {
        HStack(spacing: 0) {
            Text(MXSessoesRegras.avisoDeArquivada(a.nome))
                .mxTipo(MX.Tipo.aviso)
                .foregroundStyle(MX.CorDasSessoes.avisoTexto.color)
                .lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button {
                fonte.desfazerArquivar(a.id, em: id)
                esperaDoAviso?.cancel()
                withAnimation(MX.Movimento.troca(reduzir: reduzirMovimento).animacao) { arquivada = nil }
            } label: {
                Text("Desfazer", bundle: .module)
                    .mxTipo(MX.Tipo.avisoAcao)
                    .foregroundStyle(MX.CorDasSessoes.avisoAcao.color)
                    .padding(.horizontal, MX.Espaco.entreBlocos)
                    .frame(minWidth: MX.Espaco.alvoMinimo, minHeight: MX.Espaco.alvoMinimo)
                    .contentShape(Rectangle())
            }
            .buttonStyle(MXToque())
            .accessibilityLabel(String(localized: "Desfazer: \(a.nome) volta para as sessões", bundle: .module))
        }
        .padding(.leading, s.avisoRespiroEsquerda)
        .padding(.trailing, s.avisoRespiroDireita)
        .frame(minHeight: s.avisoAltura)
        .background(MX.CorDasSessoes.avisoFundo.color, in: RoundedRectangle(cornerRadius: s.avisoRaio))
        .shadow(color: MX.CorDasSessoes.avisoSombra.color, radius: s.avisoSombraRaio, y: s.avisoSombraY)
        .padding(.horizontal, s.novaMargem)
        .padding(.bottom, s.avisoDaBorda)
        .transition(MX.Movimento.troca(reduzir: reduzirMovimento).transicaoDeConteudo)
        .accessibilityElement(children: .contain)
    }

    private func novaSessao(_ dados: MXSessoesDados) -> some View {
        Button {
            guard dados.podeCriar, let nova = fonte.novaSessao(em: id) else { return }
            irPara(.sessao(agente: id, sessao: nova))
        } label: {
            HStack(spacing: MX.Espaco.dentroDeLinha) {
                MXIconeView(.mais, tamanho: s.novaIcone, traco: MX.Conversa.enviarTraco)
                Text("Nova sessão", bundle: .module).mxTipo(MX.Tipo.botaoGrande)
                // A fonte não cria: à vista, desligada, "Em breve" (como a autonomia Total).
                if !dados.podeCriar { Text(MXIndisponivel.emBreve).mxTipo(MX.Tipo.legendaForte) }
            }
            // Texto marrom sobre o laranja (nunca branco).
            .foregroundStyle(MX.Claro.sobreMexerica.color)
            .frame(maxWidth: .infinity, minHeight: s.novaAltura)
            .background(MX.Claro.mexerica.color, in: Capsule())
            .shadow(color: MX.CorDasSessoes.novaSombra.color, radius: s.novaSombraRaio, y: s.novaSombraY)
            .contentShape(Capsule())
        }
        .buttonStyle(MXToque())
        .disabled(!dados.podeCriar)
        .opacity(dados.podeCriar ? 1 : MX.Conversa.desligadoOpacidade)
        .accessibilityLabel(String(localized: "Nova sessão", bundle: .module))
        .accessibilityValue(dados.podeCriar ? "" : MXIndisponivel.valorAcessivel)
        .accessibilityHint(dados.podeCriar ? "" : MXIndisponivel.emBreve)
        .padding(.horizontal, s.novaMargem)
        .padding(.bottom, s.novaDaBorda)
        .ignoresSafeArea(edges: .bottom)
    }
}

/// Uma linha da árvore: recuo por nível com o "L" de ligação, a seta (se tem filhas), o estado (gomo, ponto ou anel),
/// o nome, quantas há dentro (recolhida), a informação à direita e arquivar (paradas). Puxar para a esquerda mostra
/// "Abrir" em laranja e, passando de 70 pt, abre. Para o VoiceOver é um elemento só, com as ações.
struct MXLinhaDaSessao: View {
    let linha: MXLinhaDeSessao
    /// Nulo: a sessão não abre nesta fonte (à vista, com "Em breve"; sem toque nem puxão que abra).
    let abrir: (() -> Void)?
    let alternar: () -> Void
    let arquivar: (() -> Void)?
    @State private var arrasto: CGFloat = 0
    /// O toque virou puxão que abriu: o toque que o termina não abre de novo (zera no começo e no fim do gesto).
    @State private var gesto = MXGestoDoToque()
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto
    @ScaledMetric(relativeTo: .footnote) private var seta = MX.Sessoes.setaIcone
    @ScaledMetric(relativeTo: .body) private var iconeDeArquivar = MX.Sessoes.arquivarIcone
    @ScaledMetric(relativeTo: .body) private var gomo = MX.Sessoes.gomo

    init(linha: MXLinhaDeSessao, abrir: (() -> Void)?, alternar: @escaping () -> Void, arquivar: (() -> Void)?) {
        self.linha = linha
        self.abrir = abrir
        self.alternar = alternar
        self.arquivar = arquivar
    }

    private var s: MX.Sessoes.Type { MX.Sessoes.self }
    private var deVoce: Bool { linha.estado == .precisaDeVoce }
    private var acessivel: Bool { tamanhoDoTexto.isAccessibilitySize }

    var body: some View {
        ZStack(alignment: .trailing) {
            if arrasto > 0 { acaoDeAbrir }
            conteudo
                // Com Reduzir movimento, a linha não desliza: só o "Abrir" aparece por opacidade.
                .offset(x: reduzirMovimento ? 0 : -arrasto)
        }
        .clipShape(RoundedRectangle(cornerRadius: s.linhaRaio))
        .simultaneousGesture(puxar)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(linha.rotuloAcessivel)
        .accessibilityAddTraits(abrir != nil ? .isButton : [])
        .accessibilityValue(abrir != nil ? "" : MXIndisponivel.valorAcessivel)
        .accessibilityHint(abrir != nil ? String(localized: "Abre a sessão", bundle: .module) : MXIndisponivel.emBreve)
        .accessibilityAction { abrir?() }
        .accessibilityActions {
            if linha.temFilhas {
                Button(linha.aberta ? String(localized: "Recolher", bundle: .module) : String(localized: "Mostrar as sessões de dentro", bundle: .module), action: alternar)
            }
            if let arquivar { Button(String(localized: "Arquivar", bundle: .module), action: arquivar) }
        }
    }

    private var conteudo: some View {
        HStack(spacing: 0) {
            Color.clear.frame(width: s.recuoBase + CGFloat(linha.nivel) * s.recuoPorNivel, height: 1)
            if linha.nivel > 0 { ligacao }
            if linha.temFilhas {
                Button(action: alternar) {
                    MXSetaQueAbre(aberta: linha.aberta, tamanho: seta, traco: s.setaTraco)
                        .foregroundStyle(MX.Claro.textoSecundario.color)
                        .frame(width: s.setaLargura)
                        .frame(minHeight: MX.Espaco.alvoMinimo)
                        // Desenho de 20; toque de 44, crescendo para a esquerda (sobre o recuo).
                        .contentShape(MXFaixaDeToque(inicio: s.setaLargura - MXAlvoDeToque.lado(s.setaLargura), fim: s.setaLargura))
                }
                .buttonStyle(MXToque())
            } else {
                Color.clear.frame(width: s.setaLargura, height: 1)
            }
            Button {
                guard gesto.toqueAbre(), let abrir else { return }
                abrir()
            } label: {
                principal.contentShape(Rectangle())
            }
            .buttonStyle(MXToque())
            .disabled(abrir == nil)
            if let arquivar {
                Button(action: arquivar) {
                    MXIconeView(.arquivar, tamanho: iconeDeArquivar)
                        .foregroundStyle(MX.Claro.textoSecundario.color)
                        .frame(width: MXAlvoDeToque.lado(s.arquivarLargura))
                        .frame(minHeight: MX.Espaco.alvoMinimo)
                        .contentShape(Rectangle())
                }
                .buttonStyle(MXToque())
            } else {
                Color.clear.frame(width: 6, height: 1)
            }
        }
        .frame(minHeight: s.linhaAltura)
        .background {
            RoundedRectangle(cornerRadius: s.linhaRaio)
                .fill(fundo)
                .overlay {
                    if deVoce {
                        RoundedRectangle(cornerRadius: s.linhaRaio).strokeBorder(MX.Claro.mexerica.color, lineWidth: s.anelDeVoce)
                    }
                }
        }
    }

    /// Pede você: branca com anel laranja; puxada: creme claro; as outras, transparentes.
    private var fundo: Color {
        if deVoce { return MX.CorDasSessoes.linhaDeVoce.color }
        return arrasto > 0 ? MX.CorDasSessoes.linhaPuxada.color : .clear
    }

    /// Nome, quantidade e informação; no texto grande, a informação desce para baixo do nome.
    private var principal: some View {
        HStack(spacing: s.entreItens) {
            indicador
            if acessivel {
                VStack(alignment: .leading, spacing: 0) {
                    nome.lineLimit(2)
                    HStack(spacing: s.entreItens) { quantidade; info; emBreve }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            } else if linha.infoEhDoPai {
                // Busca: o nome do resultado tem a prioridade; "em <pai>" é que corta no fim.
                nome.lineLimit(1).frame(maxWidth: .infinity, alignment: .leading).layoutPriority(1)
                info.lineLimit(1).truncationMode(.tail)
                emBreve
            } else {
                nome.lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                quantidade
                info.fixedSize()
                emBreve
            }
        }
        .padding(.leading, 2)
        .padding(.trailing, 4)
        .frame(minHeight: s.linhaAltura)
    }

    private var nome: some View {
        Text(linha.nome)
            .mxTipo(linha.principal ? MX.Tipo.sessaoPrincipal : linha.nivel == 0 ? MX.Tipo.sessaoRaiz : MX.Tipo.sessaoFilha)
            .foregroundStyle(MX.Claro.texto.color)
            .truncationMode(.tail)
    }

    @ViewBuilder private var quantidade: some View {
        if let n = linha.quantidade {
            Text(verbatim: "\(n)")
                .mxTipo(MX.Tipo.rotulo)
                .foregroundStyle(MX.Claro.textoSecundario.color)
                .padding(.horizontal, 6)
                .padding(.vertical, 1)
                .background(MX.Claro.etiqueta.color, in: Capsule())
                .fixedSize()
        }
    }

    /// "Em breve" na sessão que não abre nesta fonte.
    @ViewBuilder private var emBreve: some View {
        if abrir == nil {
            Text(MXIndisponivel.emBreve).mxTipo(MX.Tipo.legenda).foregroundStyle(MX.Claro.textoSecundario.color)
                .lineLimit(1)
                .fixedSize()
        }
    }

    private var info: some View {
        Text(linha.info)
            .mxTipo(linha.estado == .livre ? MX.Tipo.sessaoInfo : MX.Tipo.sessaoInfoForte)
            .foregroundStyle(corDaInfo.color)
            .lineLimit(1)
    }

    private var corDaInfo: MXCor {
        switch linha.estado {
        case .precisaDeVoce: MX.Claro.mexericaTexto
        case .trabalhando: MX.Claro.sucessoTexto
        case .livre: MX.Claro.textoSecundario
        }
    }

    @ViewBuilder private var indicador: some View {
        switch linha.estado {
        case .trabalhando:
            MXGomo(tamanho: gomo).accessibilityHidden(true)
        case .precisaDeVoce:
            Circle().fill(MX.Claro.mexerica.color)
                .frame(width: s.ponto, height: s.ponto)
                .padding(.horizontal, s.pontoRecuo)
        case .livre:
            Circle().strokeBorder(MX.Claro.borda.color, lineWidth: s.pontoAnel)
                .frame(width: s.ponto, height: s.ponto)
                .padding(.horizontal, s.pontoRecuo)
        }
    }

    /// O "L" que liga a filha ao pai.
    private var ligacao: some View {
        MXLigacao(raio: s.ligacaoRaio)
            .stroke(MX.CorDasSessoes.ligacao.color, style: StrokeStyle(lineWidth: s.ligacaoTraco, lineCap: .round))
            .frame(width: s.ligacaoLargura, height: s.ligacaoAltura)
            .offset(y: -s.ligacaoAltura / 2)
            .padding(.trailing, 1)
            .accessibilityHidden(true)
    }

    /// O "Abrir" laranja (texto marrom) que aparece atrás da linha puxada.
    private var acaoDeAbrir: some View {
        Button { abrir?() } label: {
            HStack(spacing: 4) {
                Text("Abrir", bundle: .module).mxTipo(MX.Tipo.acaoDaLinha)
                MXIconeView(.setaParaDireita, tamanho: s.abrirIcone)
            }
            .foregroundStyle(MX.Claro.sobreMexerica.color)
            .frame(width: s.abrirLargura)
            .frame(maxHeight: .infinity)
            .background(MX.Claro.mexerica.color)
        }
        .buttonStyle(.plain)
        .opacity(min(1, arrasto / MX.Movimento.puxarSessaoLimiar))
        .accessibilityHidden(true)
    }

    /// Desde o dedo descer (zera a marca do toque); só o puxão horizontal para a esquerda mexe na linha.
    private var puxar: some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { v in
                gesto.movimento()
                // A sessão que não abre não desliza (o "Abrir" não aparece).
                guard abrir != nil else { return }
                let dx = v.translation.width
                let horizontal = dx < -MX.Espaco.dentroDeLinha && MXSessoesRegras.ehHorizontal(dx: dx, dy: v.translation.height)
                guard arrasto > 0 || horizontal else { return }
                arrasto = MXSessoesRegras.arrastoDaSessao(-dx)
            }
            .onEnded { _ in
                let abre = MXSessoesRegras.abreAoSoltar(arrasto)
                // Puxão curto (não abriu) não marca nada: o próximo toque abre normalmente.
                gesto.fim(valeu: abre)
                if reduzirMovimento {
                    arrasto = 0
                } else {
                    withAnimation(MX.Movimento.molaPuxar.animacao) { arrasto = 0 }
                }
                if abre { abrir?() }
            }
    }
}

/// O "L" da árvore (borda esquerda e de baixo, com o canto arredondado): `border-left` + `border-bottom` do desenho.
struct MXLigacao: Shape {
    let raio: CGFloat

    func path(in r: CGRect) -> Path {
        let raio = min(raio, r.width, r.height)
        var p = Path()
        p.move(to: CGPoint(x: r.minX, y: r.minY))
        p.addLine(to: CGPoint(x: r.minX, y: r.maxY - raio))
        p.addQuadCurve(to: CGPoint(x: r.minX + raio, y: r.maxY), control: CGPoint(x: r.minX, y: r.maxY))
        p.addLine(to: CGPoint(x: r.maxX, y: r.maxY))
        return p
    }
}
