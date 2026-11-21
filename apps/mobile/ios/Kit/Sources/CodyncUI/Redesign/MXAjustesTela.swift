import CodyncKit
import SwiftUI

/// O que a folha de ajustes leva para fora dela.
struct MXAcoesDosAjustes {
    var fechar: () -> Void
    /// Conversar, Sessões, Ligar e os destinos "Em breve": fecha a folha e abre.
    var irPara: (MXDestino) -> Void
    /// O agente saiu (excluído): fecha a folha e volta para a lista.
    var excluido: () -> Void
}

/// Ajustes do agente (`capturas/08-ajustes-do-agente.webp`, `telas/ConfigBot.dc.html`): folha creme com "Configurações"
/// e Pronto, a pelúcia com nome, função e "Sincronizado com o …", os atalhos Conversar (laranja), Sessões e Ligar,
/// Autonomia (Pedir, Liberar, Total em breve), a lista (Identidade, Motor, Comportamento, Ferramentas, WhatsApp,
/// Memória, Voz, Rotinas), Avisos deste agente, Exportar e Excluir, e a nota de que vale no computador na hora.
///
/// O que muda aqui (autonomia, avisos, excluir) só sai com autoridade e preso à geração do toque (a fonte confere).
/// Excluir pede confirmação.
struct MXAjustesTela: View {
    let fonte: any MXFonte
    let id: String
    let acoes: MXAcoesDosAjustes
    @State private var confirmandoExcluir = false
    /// O pedido de excluir saiu daqui: quando o agente some (o computador confirmou), a folha fecha e volta à lista.
    @State private var pediuExcluir = false
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto
    @ScaledMetric(relativeTo: .footnote) private var iconeDoAtalho = MX.Ajustes.atalhoIcone
    @ScaledMetric(relativeTo: .footnote) private var iconeDaSincronia = MX.Ajustes.sincroniaIcone
    @ScaledMetric(relativeTo: .body) private var letra = MX.Ajustes.letra
    @ScaledMetric(relativeTo: .body) private var seta = MX.Ajustes.itemSeta

    init(fonte: any MXFonte, id: String, acoes: MXAcoesDosAjustes) {
        self.fonte = fonte
        self.id = id
        self.acoes = acoes
    }

    private var a: MX.Ajustes.Type { MX.Ajustes.self }
    private var acessivel: Bool { tamanhoDoTexto.isAccessibilitySize }

    var body: some View {
        VStack(spacing: 0) {
            titulo
            // O que não valeu (o Hub recusou, o link caiu): a mesma linha de aviso da Conversa. O controle já voltou
            // ao valor de antes (a fonte desfaz).
            if let aviso = fonte.aviso { linhaDeAviso(aviso) }
            if let dados = fonte.ajustes(id) {
                ScrollView {
                    VStack(alignment: .leading, spacing: a.entreBlocos) {
                        cabecalho(dados)
                        atalhos(dados)
                        autonomia(dados)
                        itens(dados)
                        rodape(dados)
                        Text("As mesmas configurações do app do computador. O que você muda aqui vale lá na hora.", bundle: .module)
                            .mxTipo(MX.Tipo.sincronia)
                            .foregroundStyle(MX.Claro.textoSecundario.color)
                            .padding(.horizontal, 4)
                    }
                    .padding(.horizontal, a.respiroLado)
                    .padding(.top, a.respiroTopo)
                    .padding(.bottom, a.respiroFim)
                }
                .scrollIndicators(.hidden)
                .confirmationDialog(MXAjustesRegras.perguntaDeExcluir(dados.nome), isPresented: $confirmandoExcluir,
                                    titleVisibility: .visible) {
                    Button(String(localized: "Excluir", bundle: .module), role: .destructive) {
                        guard fonte.excluirAgente(id) else { return }
                        pediuExcluir = true
                        // Na fonte de exemplo sai na hora; na real, espera o ok do computador ("Excluindo…").
                        if fonte.ajustes(id) == nil { acoes.excluido() }
                    }
                    Button(String(localized: "Cancelar", bundle: .module), role: .cancel) {}
                } message: {
                    Text(MXAjustesRegras.avisoDeExcluir)
                }
            } else {
                Text("Este bot não está mais aqui.", bundle: .module)
                    .mxTipo(MX.Tipo.corpo)
                    .foregroundStyle(MX.Claro.textoSecundario.color)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(MX.CorDasSessoes.folha.color)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "Configurações", bundle: .module))
        .accessibilityAction(.escape) { acoes.fechar() }
        .onChange(of: fonte.ajustes(id) == nil) { _, sumiu in
            if sumiu && pediuExcluir { acoes.excluido() }
        }
        .onChange(of: fonte.aviso) { _, novo in
            // O VoiceOver lê o aviso quando ele chega.
            if let novo { AccessibilityNotification.Announcement(novo).post() }
        }
    }

    private func linhaDeAviso(_ aviso: String) -> some View {
        HStack(spacing: MX.Espaco.dentroDeLinha) {
            Text(aviso)
                .mxTipo(MX.TipoDaConversa.dica)
                .foregroundStyle(MX.Claro.erroTexto.color)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button(String(localized: "OK", bundle: .module)) { fonte.limparAviso() }
                .mxTipo(MX.TipoDaConversa.botaoPequeno)
                .foregroundStyle(MX.Claro.texto.color)
                .frame(minWidth: MX.Espaco.alvoMinimo, minHeight: MX.Espaco.alvoMinimo)
                .accessibilityLabel(String(localized: "Dispensar aviso", bundle: .module))
        }
        .padding(.horizontal, a.respiroLado)
        .transition(MX.Movimento.troca(reduzir: reduzirMovimento).transicaoDeConteudo)
        .accessibilityElement(children: .contain)
    }

    // MARK: Título

    private var titulo: some View {
        HStack {
            Text("Configurações", bundle: .module)
                .mxTipo(MX.Tipo.tituloDaFolha)
                .foregroundStyle(MX.Claro.texto.color)
                .accessibilityAddTraits(.isHeader)
            Spacer(minLength: MX.Espaco.dentroDeLinha)
            Button(action: acoes.fechar) {
                Text("Pronto", bundle: .module)
                    .mxTipo(MX.Tipo.voltarComRotulo)
                    .foregroundStyle(MX.Claro.texto.color)
                    .padding(.horizontal, a.prontoRespiro)
                    .frame(minHeight: a.prontoAltura)
                    .background(MX.Claro.etiqueta.color, in: Capsule())
                    // Cápsula de 36; toque de 44.
                    .frame(minHeight: MXAlvoDeToque.lado(a.prontoAltura))
                    .contentShape(Rectangle())
            }
            .buttonStyle(MXToque())
        }
        .padding(.leading, a.tituloRespiroEsquerda)
        .padding(.trailing, a.tituloRespiroDireita)
        .padding(.top, 6)
        .dynamicTypeSize(...MX.Conversa.capsulaTextoAte) // barra da folha: teto como a nativa
    }

    // MARK: Cabeçalho e atalhos

    private func cabecalho(_ d: MXAjustesDoAgente) -> some View {
        HStack(spacing: a.cabecalhoEntre) {
            MXPelucia(rosto: d.rosto, estado: d.estado, tamanho: a.pelucia, comSelos: false)
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: MX.Espaco.dentroDeLinha) {
                    Text(d.nome)
                        .mxTipo(MX.Tipo.tituloDoAgente)
                        .foregroundStyle(MX.Claro.texto.color)
                        .lineLimit(1)
                        .minimumScaleFactor(MXConversasRegras.escalaMinimaDoNome)
                    if !d.funcao.isEmpty {
                        Text(d.funcao)
                            .mxTipo(MX.Tipo.rotulo)
                            .foregroundStyle(MX.Claro.textoSecundario.color)
                            .padding(MX.Medida.etiquetaRespiro)
                            .background(MX.Claro.etiqueta.color, in: Capsule())
                    }
                }
                HStack(alignment: .firstTextBaseline, spacing: 5) {
                    if d.sincronizado { MXIconeView(.sincronizado, tamanho: iconeDaSincronia) }
                    Text(d.sincronia).mxTipo(MX.Tipo.sincronia)
                }
                .foregroundStyle((d.sincronizado ? MX.Claro.sucessoTexto : MX.Claro.textoSecundario).color)
            }
            Spacer(minLength: 0)
        }
        .accessibilityElement(children: .combine)
    }

    private struct Atalho: Identifiable {
        let icone: MXIcone
        let nome: String
        let destino: MXDestino
        let principal: Bool
        var disponivel = true
        var id: String { nome }
    }

    private func atalhos(_ d: MXAjustesDoAgente) -> some View {
        let botoes = [
            Atalho(icone: .conversas, nome: String(localized: "Conversar", bundle: .module), destino: .conversa(id), principal: true),
            Atalho(icone: .sessoes, nome: String(localized: "Sessões", bundle: .module), destino: .sessoes(id), principal: false),
            // Sem ligação na fonte: à vista, desligado, "Em breve".
            Atalho(icone: .ligar, nome: String(localized: "Ligar", bundle: .module), destino: .emBreve(.aoVivo), principal: false, disponivel: d.podeLigar),
        ]
        let layout = acessivel ? AnyLayout(VStackLayout(spacing: a.atalhoEntre))
            : AnyLayout(HStackLayout(spacing: a.atalhoEntre))
        return layout {
            ForEach(botoes) { b in
                Button { acoes.irPara(b.destino) } label: {
                    VStack(spacing: 3) {
                        MXIconeView(b.icone, tamanho: iconeDoAtalho, traco: MX.Sessoes.arquivarTraco)
                        Text(b.nome).mxTipo(MX.Tipo.atalho)
                        if !b.disponivel {
                            Text(MXIndisponivel.emBreve).mxTipo(MX.Tipo.legenda).foregroundStyle(MX.Claro.textoSecundario.color)
                        }
                    }
                    // Texto marrom sobre o laranja (nunca branco).
                    .foregroundStyle(MX.Claro.texto.color)
                    .frame(maxWidth: .infinity, minHeight: a.atalhoAltura)
                    .background {
                        let forma = RoundedRectangle(cornerRadius: a.atalhoRaio)
                        if b.principal {
                            forma.fill(MX.Claro.mexerica.color)
                        } else {
                            forma.fill(MX.CorDasSessoes.cartao.color)
                                .overlay(forma.strokeBorder(MX.CorDasSessoes.anelDoCartao.color, lineWidth: MX.Vidro.contornoOpacoEspessura))
                        }
                    }
                    .contentShape(RoundedRectangle(cornerRadius: a.atalhoRaio))
                }
                .buttonStyle(MXToque())
                .disabled(!b.disponivel)
                .opacity(b.disponivel ? 1 : MX.Conversa.desligadoOpacidade)
                .accessibilityLabel(b.nome)
                .accessibilityValue(b.disponivel ? "" : MXIndisponivel.valorAcessivel)
                .accessibilityHint(b.disponivel ? "" : MXIndisponivel.emBreve)
            }
        }
    }

    // MARK: Autonomia

    private func autonomia(_ d: MXAjustesDoAgente) -> some View {
        let conectado = fonte.conexao.podeAgir
        return VStack(alignment: .leading, spacing: a.autonomiaEntre) {
            // Título e dica lado a lado; no texto grande, a dica desce (sem quebrar "Autonomia" no meio da palavra).
            let titulo = Text("Autonomia", bundle: .module).mxTipo(MX.Tipo.corpoForte).foregroundStyle(MX.Claro.texto.color)
            let dica = d.autonomia.map { Text($0.dica).mxTipo(MX.Tipo.legenda).foregroundStyle(MX.Claro.textoSecundario.color) }
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline) {
                    titulo.fixedSize()
                    Spacer(minLength: MX.Espaco.dentroDeLinha)
                    dica?.fixedSize()
                }
                VStack(alignment: .leading, spacing: 0) {
                    titulo.fixedSize(horizontal: false, vertical: true)
                    dica
                }
            }
            let layout = acessivel ? AnyLayout(VStackLayout(spacing: 0)) : AnyLayout(HStackLayout(spacing: 0))
            layout {
                ForEach(MXAutonomia.allCases) { opcao in
                    let escolhida = d.autonomia == opcao
                    let ligada = opcao.disponivel && conectado
                    Button {
                        guard !escolhida else { return }
                        withAnimation(MX.Movimento.troca(reduzir: reduzirMovimento).animacao) {
                            fonte.mudarAutonomia(opcao, em: id)
                        }
                    } label: {
                        Text(opcao.nome)
                            .mxTipo(MX.Tipo.segmento)
                            .foregroundStyle(MX.Claro.texto.color)
                            // 44 pt no próprio segmento que se toca (o trilho cresce junto; o desenho tem 34).
                            .frame(maxWidth: .infinity, minHeight: MXAjustesRegras.alturaDoSegmento)
                            .background {
                                if escolhida {
                                    RoundedRectangle(cornerRadius: a.segmentoRaio)
                                        .fill(MX.CorDasSessoes.cartao.color)
                                        .shadow(color: MX.CorDasSessoes.sombraDoSegmento.color, radius: a.segmentoSombraRaio, y: a.segmentoSombraY)
                                }
                            }
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(MXToque())
                    .disabled(!ligada)
                    .opacity(ligada || escolhida ? 1 : MX.Conversa.desligadoOpacidade)
                    .accessibilityAddTraits(escolhida ? .isSelected : [])
                    .accessibilityValue(opcao.disponivel ? (conectado ? opcao.dica : String(localized: "Indisponível sem conexão", bundle: .module)) : MXIndisponivel.emBreve)
                }
            }
            .padding(a.segmentosRespiro)
            .background(MX.CorDasSessoes.trilho.color, in: RoundedRectangle(cornerRadius: a.segmentosRaio))
            .accessibilityElement(children: .contain)
            .accessibilityLabel(String(localized: "Autonomia", bundle: .module))
        }
        .padding(.vertical, a.autonomiaRespiroV)
        .padding(.horizontal, a.autonomiaRespiroH)
        .modifier(MXCartaoDosAjustes())
    }

    // MARK: Lista

    private func itens(_ d: MXAjustesDoAgente) -> some View {
        VStack(spacing: 0) {
            ForEach(Array(d.itens.enumerated()), id: \.element.id) { i, item in
                Button { acoes.irPara(.emBreve(item.id)) } label: {
                    HStack(spacing: a.itemEntre) {
                        Text(item.letra)
                            .mxTipo(MX.Tipo.letraDeAjuste)
                            .foregroundStyle(MX.Claro.mexericaTexto.color)
                            .frame(width: letra, height: letra)
                            .background(MX.CorDasSessoes.letraFundo.color, in: RoundedRectangle(cornerRadius: a.letraRaio))
                            .accessibilityHidden(true)
                        if acessivel {
                            VStack(alignment: .leading, spacing: 0) {
                                nomeDoItem(item)
                                if let valor = item.valor { valorDoItem(valor) }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        } else {
                            nomeDoItem(item).frame(maxWidth: .infinity, alignment: .leading)
                            if let valor = item.valor { valorDoItem(valor).frame(maxWidth: a.valorLargura, alignment: .trailing) }
                        }
                        if item.disponivel {
                            MXIconeView(.setaParaDireita, tamanho: seta).foregroundStyle(MX.Claro.borda.color)
                        } else {
                            // A tela de dentro ainda não existe: à vista, desligado, "Em breve".
                            Text(MXIndisponivel.emBreve).mxTipo(MX.Tipo.legenda).foregroundStyle(MX.Claro.textoSecundario.color)
                                .fixedSize()
                        }
                    }
                    .padding(.leading, a.itemRespiroEsquerda)
                    .padding(.trailing, a.itemRespiroDireita)
                    .frame(minHeight: a.itemAltura)
                    .contentShape(Rectangle())
                }
                .buttonStyle(MXToque())
                .disabled(!item.disponivel)
                .overlay(alignment: .top) { if i > 0 { separador } }
                .accessibilityLabel(item.valor.map { "\(item.nome): \($0)" } ?? item.nome)
                .accessibilityValue(item.disponivel ? "" : MXIndisponivel.valorAcessivel)
                .accessibilityHint(item.disponivel ? "" : MXIndisponivel.emBreve)
            }
        }
        .modifier(MXCartaoDosAjustes())
    }

    private func nomeDoItem(_ item: MXItemDeAjuste) -> some View {
        Text(item.nome).mxTipo(MX.Tipo.itemDeAjuste).foregroundStyle(MX.Claro.texto.color)
    }

    private func valorDoItem(_ valor: String) -> some View {
        Text(valor)
            .mxTipo(MX.Tipo.valorDeAjuste)
            .foregroundStyle(MX.Claro.textoSecundario.color)
            .lineLimit(acessivel ? 2 : 1)
            .truncationMode(.tail)
    }

    private var separador: some View {
        Rectangle().fill(MX.CorDasSessoes.separador.color).frame(height: MX.Medida.divisoria)
    }

    // MARK: Avisos, Exportar, Excluir

    @ViewBuilder private func rodape(_ d: MXAjustesDoAgente) -> some View {
        let conectado = fonte.conexao.podeAgir
        VStack(spacing: 0) {
            if let avisos = d.avisos {
                Toggle(isOn: Binding(get: { avisos }, set: { _ in fonte.alternarAvisos(em: id) })) {
                    VStack(alignment: .leading, spacing: 0) {
                        Text("Avisos deste bot", bundle: .module).mxTipo(MX.Tipo.itemDeAjuste).foregroundStyle(MX.Claro.texto.color)
                        Text("Só quando precisar de você", bundle: .module).mxTipo(MX.Tipo.legenda).foregroundStyle(MX.Claro.textoSecundario.color)
                    }
                }
                .toggleStyle(MXInterruptor())
                .disabled(!conectado)
                .padding(.leading, a.itemRespiroEsquerda)
                .padding(.trailing, a.itemRespiroDireita)
                .frame(minHeight: a.avisosAltura)
                .accessibilityHint(conectado ? "" : String(localized: "Indisponível sem conexão", bundle: .module))
            }
            if d.podeExportar {
                linhaSimples(String(localized: "Exportar bot", bundle: .module), cor: MX.Claro.texto, tipo: MX.Tipo.itemDeAjuste, temSeparador: d.avisos != nil) {
                    acoes.irPara(.emBreve(.exportar))
                }
                .mxIndisponivel(!fonte.capacidades.ligado(.ajuste(.exportar)))
            }
            if d.podeExcluir {
                // Excluindo: o pedido saiu e o computador ainda não respondeu (desligado, "Excluindo…").
                let liga = conectado && d.motivoSemExcluir == nil && !d.excluindo
                linhaSimples(String(localized: "Excluir bot", bundle: .module), cor: MX.Claro.erroTexto, tipo: MX.Tipo.perigo,
                             temSeparador: d.avisos != nil || d.podeExportar,
                             nota: d.motivoSemExcluir ?? (d.excluindo ? MXAjustesRegras.excluindo : nil)) {
                    confirmandoExcluir = true
                }
                .disabled(!liga)
                .opacity(liga || d.excluindo ? 1 : MX.Conversa.desligadoOpacidade)
                .accessibilityValue(d.motivoSemExcluir != nil ? MXAjustesRegras.excluirIndisponivelAcessivel
                                    : d.excluindo ? MXAjustesRegras.excluindo
                                    : conectado ? "" : String(localized: "Indisponível sem conexão", bundle: .module))
                .accessibilityHint(d.motivoSemExcluir ?? (conectado && !d.excluindo ? String(localized: "Pede confirmação", bundle: .module) : ""))
            }
        }
        .modifier(MXCartaoDosAjustes())
    }

    private func linhaSimples(_ titulo: String, cor: MXCor, tipo: MXTipo, temSeparador: Bool, nota: String? = nil,
                              acao: @escaping () -> Void) -> some View {
        Button(action: acao) {
            HStack(spacing: MX.Espaco.dentroDeLinha) {
                Text(titulo).mxTipo(tipo).foregroundStyle(cor.color)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let nota {
                    Text(nota).mxTipo(MX.Tipo.legenda).foregroundStyle(MX.Claro.textoSecundario.color)
                        .multilineTextAlignment(.trailing)
                }
            }
            .frame(minHeight: a.itemAltura)
            .padding(.horizontal, a.itemRespiroEsquerda)
            .contentShape(Rectangle())
        }
        .buttonStyle(MXToque())
        .overlay(alignment: .top) { if temSeparador { separador } }
    }
}

/// Cartão branco com anel fino e raio 16 dos ajustes.
struct MXCartaoDosAjustes: ViewModifier {
    func body(content: Content) -> some View {
        let forma = RoundedRectangle(cornerRadius: MX.Ajustes.cartaoRaio)
        content
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(MX.CorDasSessoes.cartao.color, in: forma)
            .overlay(forma.strokeBorder(MX.CorDasSessoes.anelDoCartao.color, lineWidth: MX.Vidro.contornoOpacoEspessura))
            .clipShape(forma)
    }
}

/// O interruptor laranja do desenho (52 × 32, bola branca de 26): trilho laranja ligado, marrom claro desligado. É
/// um `Toggle` (o VoiceOver lê "interruptor, ligado").
struct MXInterruptor: ToggleStyle {
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.isEnabled) private var ligadoNaTela

    func makeBody(configuration: Configuration) -> some View {
        let a = MX.Ajustes.self
        return HStack(spacing: MX.Espaco.dentroDeLinha) {
            configuration.label.frame(maxWidth: .infinity, alignment: .leading)
            Capsule()
                .fill((configuration.isOn ? MX.Claro.mexerica : MX.CorDasSessoes.interruptorDesligado).color)
                .frame(width: a.interruptorLargura, height: a.interruptorAltura)
                .overlay(alignment: configuration.isOn ? .trailing : .leading) {
                    Circle()
                        .fill(Color.white)
                        .frame(width: a.interruptorBola, height: a.interruptorBola)
                        .shadow(color: .black.opacity(0.2), radius: 1.5, y: 1)
                        .padding(a.interruptorRespiro)
                }
                .opacity(ligadoNaTela ? 1 : MX.Conversa.desligadoOpacidade)
                .animation(MX.Movimento.troca(reduzir: reduzirMovimento).animacao, value: configuration.isOn)
        }
        .contentShape(Rectangle())
        .onTapGesture { if ligadoNaTela { configuration.isOn.toggle() } }
    }
}
