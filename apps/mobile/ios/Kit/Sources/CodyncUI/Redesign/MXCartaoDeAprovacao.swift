import SwiftUI

/// "Esperando você" (`capturas/06-esperando-voce.webp`): o pedido de aprovação na conversa, com as opções do Hub.
/// Permitir se confirma segurando 0,9 s (como no Mac); recusar é um toque. Sem conexão, os botões ficam à vista,
/// desligados, com o motivo embaixo. Respondido, o cartão mostra o resultado.
struct MXCartaoDeAprovacao: View {
    let pedido: MXPedidoDeAprovacao
    let conectado: Bool
    let responder: (String) -> Void
    @State private var detalhesAbertos = false
    /// A página de alterações à vista em Detalhes.
    @State private var paginaDosDiffs = 0

    init(pedido: MXPedidoDeAprovacao, conectado: Bool, responder: @escaping (String) -> Void) {
        self.pedido = pedido
        self.conectado = conectado
        self.responder = responder
    }

    private struct Fato: Identifiable {
        let rotulo: String
        let valor: String
        var id: String { rotulo }
    }
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.accessibilityReduceTransparency) private var reduzirTransparencia
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto

    private var c: MX.Conversa.Type { MX.Conversa.self }

    var body: some View {
        VStack(alignment: .leading, spacing: c.cartaoEntre) {
            selo
            VStack(alignment: .leading, spacing: c.tituloEntre) {
                Text(pedido.titulo)
                    .mxTipo(MX.TipoDaConversa.tituloDoCartao)
                    .foregroundStyle(MX.Claro.texto.color)
                    .accessibilityAddTraits(.isHeader)
                if let detalhe = pedido.detalhe {
                    Text(detalhe.previa)
                        .font(.system(.footnote, design: .monospaced))
                        .foregroundStyle(MX.Claro.texto.color)
                        .lineLimit(detalhesAbertos ? nil : c.detalheLinhas)
                        .textSelection(.enabled)
                }
            }
            fatos
            if pedido.temDetalhes { detalhes }
            if pedido.pendente { botoes }
        }
        .padding(c.cartaoRespiro)
        .background {
            let forma = RoundedRectangle(cornerRadius: c.cartaoRaio)
            forma.fill(fundo.color)
                .overlay(forma.stroke(MX.CorDaConversa.contornoDoCartao.color, lineWidth: MX.Vidro.contornoOpacoEspessura))
                .shadow(color: MX.CorDaConversa.sombraDoCartao.color, radius: c.cartaoSombraRaio, y: c.cartaoSombraY)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "Pedido de autorização", bundle: .module))
        .onChange(of: pedido.diffs.count) { _, total in
            // O pedido mudou de tamanho: a página guardada volta para dentro do intervalo.
            paginaDosDiffs = MXConversaRegras.paginaNormalizada(total: total, pagina: paginaDosDiffs)
        }
    }

    private var fundo: MXCor {
        reduzirTransparencia ? MX.CorDaConversa.cartaoDeAprovacao.sobre(MX.Claro.fundo) : MX.CorDaConversa.cartaoDeAprovacao
    }

    // MARK: Selo

    @ViewBuilder private var selo: some View {
        switch pedido.resultado {
        case .pendente:
            seloCom(icone: .relogio, texto: String(localized: "Esperando você", bundle: .module), cor: MX.Claro.texto, fundo: MX.CorDaConversa.seloEsperando)
        case let .permitido(texto):
            seloCom(icone: .confirmar, texto: texto, cor: MX.Claro.sucessoTexto, fundo: MX.Claro.rodandoFundo)
        case let .recusado(texto), let .encerrado(texto):
            seloCom(icone: nil, texto: texto, cor: MX.Claro.textoSecundario, fundo: MX.CorDaConversa.neutro)
        }
    }

    private func seloCom(icone: MXIcone?, texto: String, cor: MXCor, fundo: MXCor) -> some View {
        HStack(spacing: MX.Medida.entreItens) {
            if let icone { MXIconeView(icone, tamanho: c.seloIcone) }
            Text(texto).mxTipo(MX.TipoDaConversa.seloDoCartao)
        }
        .foregroundStyle(cor.color)
        .padding(.horizontal, c.seloRespiro)
        .frame(minHeight: c.seloAltura)
        .background(fundo.color, in: Capsule())
    }

    // MARK: Fatos

    @ViewBuilder private var fatos: some View {
        let linhas = [Fato(rotulo: String(localized: "Ação", bundle: .module), valor: pedido.acao), Fato(rotulo: String(localized: "Onde", bundle: .module), valor: pedido.onde),
                      Fato(rotulo: String(localized: "Quem decide", bundle: .module), valor: String(localized: "Você", bundle: .module))]
        Group {
            if MXConversaRegras.umaColuna(tamanhoDoTexto) {
                VStack(alignment: .leading, spacing: c.fatosEntreLinhas) {
                    ForEach(linhas) { fato in
                        VStack(alignment: .leading, spacing: 0) {
                            Text(fato.rotulo).foregroundStyle(MX.Claro.textoSecundario.color)
                            Text(fato.valor).foregroundStyle(MX.Claro.texto.color)
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            } else {
                Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 0, verticalSpacing: c.fatosEntreLinhas) {
                    ForEach(linhas) { fato in
                        GridRow {
                            Text(fato.rotulo)
                                .foregroundStyle(MX.Claro.textoSecundario.color)
                                .frame(minWidth: c.fatosColuna, alignment: .leading)
                            Text(fato.valor).foregroundStyle(MX.Claro.texto.color)
                        }
                        // Sem modificador no GridRow: modificado, ele vira uma célula só e a tabela perde as colunas.
                    }
                }
            }
        }
        .mxTipo(MX.TipoDaConversa.fatos)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(c.fatosRespiro)
        .background(MX.CorDaConversa.fundoDosPassos.color, in: RoundedRectangle(cornerRadius: c.fatosRaio))
    }

    // MARK: Detalhes

    private var detalhes: some View {
        VStack(alignment: .leading, spacing: MX.Espaco.dentroDeLinha) {
            Button {
                withAnimation(MX.Movimento.troca(reduzir: reduzirMovimento).animacaoDaPosicao) { detalhesAbertos.toggle() }
            } label: {
                HStack {
                    Text("Detalhes", bundle: .module).mxTipo(MX.TipoDaConversa.detalhes)
                    Spacer(minLength: 0)
                    MXSetaQueAbre(aberta: detalhesAbertos, tamanho: c.detalhesSeta, traco: MX.Conversa.voltarTraco)
                }
                .foregroundStyle(MX.Claro.texto.color)
                .frame(minHeight: MX.Espaco.alvoMinimo)
                .contentShape(Rectangle())
            }
            .buttonStyle(MXToque())
            .accessibilityValue(detalhesAbertos ? String(localized: "Abertos", bundle: .module) : String(localized: "Fechados", bundle: .module))
            if detalhesAbertos {
                // O título inteiro, se a prévia do topo cortou; depois comando e saída. Identidade pela posição.
                if let detalhe = pedido.detalhe, detalhe.cortadoNaPrevia {
                    MXBlocoDeTextoLongo(texto: detalhe).transition(MX.Movimento.troca(reduzir: reduzirMovimento).transicaoDeConteudo)
                }
                ForEach(Array(pedido.detalhes.enumerated()), id: \.offset) { _, texto in
                    MXBlocoDeTextoLongo(texto: texto).transition(MX.Movimento.troca(reduzir: reduzirMovimento).transicaoDeConteudo)
                }
                // As alterações, uma página por vez (só as dela preparam a prévia); identidade pela posição; o patch
                // e o caminho inteiros sob demanda.
                let faixa = MXConversaRegras.paginaDeDiffs(total: pedido.diffs.count, pagina: paginaDosDiffs)
                ForEach(pedido.diffs[faixa]) { diff in
                    MXPreviaDeDiff(diff: diff).transition(MX.Movimento.troca(reduzir: reduzirMovimento).transicaoDeConteudo)
                }
                if MXConversaRegras.paginasDeDiffs(pedido.diffs.count) > 1 { paginador(faixa) }
            }
        }
    }

    /// "Anteriores · Arquivos 21–40 de 523 · Próximos". Cada botão é um alvo próprio de 44 × 44 pt no label; em
    /// tamanhos de acessibilidade, tudo empilha na vertical. A navegação parte da página exibida (normalizada).
    @ViewBuilder private func paginador(_ faixa: Range<Int>) -> some View {
        let total = pedido.diffs.count
        let rotulo = Text(MXConversaRegras.rotuloDaPagina(faixa, total: total))
            .foregroundStyle(MX.Claro.textoSecundario.color)
        let anteriores = botaoDePagina(String(localized: "Anteriores", bundle: .module), acessivel: String(localized: "Arquivos anteriores", bundle: .module), ligado: faixa.lowerBound > 0) {
            paginaDosDiffs = MXConversaRegras.paginaAnterior(total: total, pagina: paginaDosDiffs)
        }
        let proximos = botaoDePagina(String(localized: "Próximos", bundle: .module), acessivel: String(localized: "Próximos arquivos", bundle: .module), ligado: faixa.upperBound < total) {
            paginaDosDiffs = MXConversaRegras.proximaPagina(total: total, pagina: paginaDosDiffs)
        }
        Group {
            if MXConversaRegras.umaColuna(tamanhoDoTexto) {
                VStack(alignment: .leading, spacing: MX.Medida.entreItens) {
                    rotulo
                    anteriores
                    proximos
                }
            } else {
                HStack(spacing: MX.Medida.entreItens) {
                    anteriores
                    Spacer(minLength: 0)
                    rotulo
                    Spacer(minLength: 0)
                    proximos
                }
            }
        }
        .mxTipo(MX.TipoDaConversa.botaoPequeno)
    }

    private func botaoDePagina(_ titulo: String, acessivel: String, ligado: Bool, acao: @escaping () -> Void) -> some View {
        Button(action: acao) {
            Text(titulo)
                .frame(minWidth: MX.Espaco.alvoMinimo, minHeight: MX.Espaco.alvoMinimo)
                .contentShape(Rectangle())
        }
        .buttonStyle(MXToque())
        .foregroundStyle(MX.Claro.mexericaTexto.color)
        .disabled(!ligado)
        .opacity(ligado ? 1 : c.desligadoOpacidade)
        .accessibilityLabel(acessivel)
    }

    // MARK: Botões

    private var principal: MXOpcaoDeAprovacao? { pedido.opcoes.first(where: \.principal) }
    private var outras: [MXOpcaoDeAprovacao] { pedido.opcoes.filter { !$0.principal } }
    private var modo: MXModoDePermitir { MXSegurarRegras.modo(reduzirMovimento: reduzirMovimento) }
    private var desligado: Bool { !conectado || pedido.respondendo != nil }

    private var botoes: some View {
        VStack(alignment: .leading, spacing: c.cartaoEntre) {
            VStack(spacing: c.entreBotoes) {
                if let principal { botao(principal) }
                if MXConversaRegras.umaColuna(tamanhoDoTexto) {
                    ForEach(outras) { botao($0) }
                } else {
                    LazyVGrid(columns: [GridItem(.flexible(), spacing: c.entreBotoes), GridItem(.flexible(), spacing: c.entreBotoes)],
                              spacing: c.entreBotoes) {
                        ForEach(outras) { botao($0) }
                    }
                }
            }
            if !conectado {
                dica(icone: nil, texto: MXTextoDeQueda.semAprovar)
            } else if pedido.bloqueadoPorTamanho {
                dica(icone: .mao, texto: MXTextoDeQueda.grandeDemais)
            } else if pedido.opcoes.contains(where: \.permite) {
                dica(icone: .mao, texto: MXSegurarRegras.dica(modo))
            }
        }
    }

    @ViewBuilder private func botao(_ opcao: MXOpcaoDeAprovacao) -> some View {
        let respondendo = pedido.respondendo == opcao.id
        if opcao.permite {
            MXBotaoDeSegurar(principal: opcao.principal, desligado: desligado || pedido.bloqueadoPorTamanho, modo: modo,
                             rotulo: opcao.rotulo, versao: pedido.versao, confirmar: { responder(opcao.id) }) {
                HStack(spacing: MX.Medida.entreItens) {
                    if opcao.principal { MXIconeView(.confirmar, tamanho: c.permitirIcone) }
                    Text(opcao.rotulo).mxTipo(opcao.principal ? MX.TipoDaConversa.permitir : MX.TipoDaConversa.botaoDoCartao)
                    if respondendo { ProgressView().tint(MX.Claro.texto.color) }
                }
            }
        } else {
            Button { responder(opcao.id) } label: {
                HStack(spacing: MX.Medida.entreItens) {
                    Text(opcao.rotulo).mxTipo(MX.TipoDaConversa.botaoDoCartao)
                    if respondendo { ProgressView().tint(MX.Claro.texto.color) }
                }
                .foregroundStyle(MX.Claro.texto.color)
                .frame(maxWidth: .infinity, minHeight: c.botaoDoCartaoAltura)
                .overlay(Capsule().stroke(MX.Claro.borda.color, lineWidth: c.contornoRecusar))
                .contentShape(Capsule())
            }
            .buttonStyle(MXToque())
            .disabled(desligado)
            .opacity(desligado ? c.desligadoOpacidade : 1)
        }
    }

    private func dica(icone: MXIcone?, texto: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: MX.Medida.entreItens) {
            if let icone { MXIconeView(icone, tamanho: c.dicaIcone) } else { Image(systemName: "wifi.slash").accessibilityHidden(true) }
            Text(texto).mxTipo(MX.TipoDaConversa.dica)
        }
        .foregroundStyle(MX.Claro.textoSecundario.color)
        .accessibilityElement(children: .combine)
    }
}

/// Botão de permitir: segurar 0,9 s confirma, soltar antes cancela (o preenchimento volta). Com Reduzir movimento,
/// o mesmo segurar, sem animação: o rótulo vira "Segure…" enquanto segura. Com VoiceOver ou Controle por Botão,
/// ativar abre uma confirmação ("Permitir uma vez?" · Permitir / Cancelar); nunca aprova direto. Háptico ao confirmar.
struct MXBotaoDeSegurar<Rotulo: View>: View {
    let principal: Bool
    let desligado: Bool
    let modo: MXModoDePermitir
    let rotulo: String
    /// A versão do pedido na tela (`MXPedidoDeAprovacao.versao`).
    let versao: String
    let confirmar: () -> Void
    let conteudo: Rotulo
    @State private var segurando = false
    @State private var progresso: CGFloat = 0
    @State private var confirmacoes = 0
    /// A confirmação deliberada da tecnologia assistiva.
    @State private var confirmandoAcessivel = false
    /// A versão do pedido quando o segurar (ou a confirmação) começou.
    @State private var versaoInicial: String?

    init(principal: Bool, desligado: Bool, modo: MXModoDePermitir, rotulo: String, versao: String,
         confirmar: @escaping () -> Void, @ViewBuilder conteudo: () -> Rotulo) {
        self.principal = principal
        self.desligado = desligado
        self.modo = modo
        self.rotulo = rotulo
        self.versao = versao
        self.confirmar = confirmar
        self.conteudo = conteudo()
    }

    private var c: MX.Conversa.Type { MX.Conversa.self }

    var body: some View {
        let forma = Capsule()
        Group {
            if segurando && modo == .segurarParado {
                Text(MXSegurarRegras.rotuloSegurando).mxTipo(principal ? MX.TipoDaConversa.permitir : MX.TipoDaConversa.botaoDoCartao)
            } else {
                conteudo
            }
        }
            .foregroundStyle(MX.Claro.sobreMexerica.color)
            .frame(maxWidth: .infinity, minHeight: principal ? c.permitirAltura : c.botaoDoCartaoAltura)
            .background {
                if principal {
                    forma.fill(MX.Claro.mexerica.color)
                        .shadow(color: MX.CorDaConversa.sombraDoPermitir.color, radius: c.permitirSombraRaio, y: c.permitirSombraY)
                } else {
                    forma.fill(MX.CorDaConversa.neutro.color)
                }
            }
            .overlay(alignment: .leading) {
                // O preenchimento que mostra o tempo de segurar (com Reduzir movimento fica em zero: o rótulo avisa).
                GeometryReader { g in
                    Rectangle()
                        .fill(MX.CorDaConversa.segurando.color)
                        .frame(width: g.size.width * progresso)
                }
                .clipShape(forma)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
            }
            .contentShape(forma)
            .scaleEffect(segurando && modo == .segurarAnimado ? MX.Movimento.toqueEscala : 1)
            .opacity(desligado ? c.desligadoOpacidade : 1)
            .modifier(MXGestoDePermitir(modo: modo, desligado: desligado, segurando: $segurando, progresso: $progresso,
                                        comecou: { versaoInicial = versao }, confirmar: confirmarAgora))
            .sensoryFeedback(.success, trigger: confirmacoes)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(rotulo)
            .accessibilityHint(MXSegurarRegras.dicaAcessivel(modo))
            .accessibilityAddTraits(.isButton)
            .accessibilityValue(desligado ? String(localized: "Desligado", bundle: .module) : "")
            // Ativar pela tecnologia assistiva não aprova: pede a confirmação.
            .accessibilityAction {
                guard !desligado else { return }
                versaoInicial = versao
                confirmandoAcessivel = true
            }
            .confirmationDialog(MXSegurarRegras.pergunta(rotulo), isPresented: $confirmandoAcessivel, titleVisibility: .visible) {
                Button(rotulo) { confirmarAgora() }
                Button(String(localized: "Cancelar", bundle: .module), role: .cancel) {}
            }
            .onChange(of: desligado) { _, agora in
                // Desligar (sem conexão, outra resposta a caminho) cancela um segurar em curso; nunca completa.
                if agora { cancelar() }
            }
            .onChange(of: versao) { _, _ in
                // O pedido mudou (revisão nova, deixou de estar pendente, opções novas): fecha a confirmação e
                // cancela o segurar; a pessoa vê o pedido atualizado e começa de novo.
                cancelar()
            }
    }

    private func cancelar() {
        segurando = false
        progresso = 0
        confirmandoAcessivel = false
        versaoInicial = nil
    }

    private func confirmarAgora() {
        // Só o pedido que estava na tela quando o gesto (ou a confirmação) começou.
        guard !desligado, MXSegurarRegras.confirmaAinda(versaoInicial: versaoInicial, versaoAtual: versao) else {
            cancelar()
            return
        }
        versaoInicial = nil
        confirmacoes += 1
        confirmar()
    }
}

/// O gesto do botão de permitir: sempre segurar 0,9 s. O preenchimento anima só fora de Reduzir movimento.
struct MXGestoDePermitir: ViewModifier {
    let modo: MXModoDePermitir
    let desligado: Bool
    @Binding var segurando: Bool
    @Binding var progresso: CGFloat
    /// O dedo desceu: guarda qual pedido estava na tela.
    let comecou: () -> Void
    let confirmar: () -> Void

    func body(content: Content) -> some View {
        content.onLongPressGesture(minimumDuration: MX.Movimento.segurarParaPermitir,
                                   maximumDistance: MX.Movimento.segurarDistanciaMaxima) {
            guard !desligado else { return }
            segurando = false
            progresso = 0
            confirmar()
        } onPressingChanged: { pressionando in
            guard !desligado else { return }
            segurando = pressionando
            if pressionando { comecou() }
            guard modo == .segurarAnimado else { return }
            if pressionando {
                withAnimation(.linear(duration: MX.Movimento.segurarParaPermitir)) { progresso = 1 }
            } else {
                // Soltou antes: cancela e o preenchimento volta.
                withAnimation(MX.Movimento.molaPadrao.animacao) { progresso = 0 }
            }
        }
    }
}

/// Seta que abre: normal, gira de → para ↓ com a mola; Reduzir movimento, troca de glifo com cross-fade (nada gira).
struct MXSetaQueAbre: View {
    let aberta: Bool
    let tamanho: CGFloat
    let traco: CGFloat?
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento

    /// Fechada aponta para a direita; aberta, para baixo (passos, Detalhes e a seta da fila, que recolhida gira −90°).
    init(aberta: Bool, tamanho: CGFloat, traco: CGFloat? = nil) {
        self.aberta = aberta
        self.tamanho = tamanho
        self.traco = traco
    }

    var body: some View {
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        if troca.interpola {
            MXIconeView(.setaParaDireita, tamanho: tamanho, traco: traco)
                .rotationEffect(.degrees(aberta ? MX.Conversa.anguloDaSetaAberta : 0))
                .animation(troca.animacao, value: aberta)
        } else {
            ZStack {
                if aberta {
                    MXIconeView(.setaParaBaixo, tamanho: tamanho, traco: traco).transition(.opacity.animation(troca.animacao))
                } else {
                    MXIconeView(.setaParaDireita, tamanho: tamanho, traco: traco).transition(.opacity.animation(troca.animacao))
                }
            }
            .frame(width: tamanho, height: tamanho)
        }
    }
}

/// Um texto do pedido em Detalhes: a prévia e, se cortou, a nota e "Mostrar tudo" (o inteiro, ou começo e fim acima
/// do limite absoluto). O inteiro só é montado quando a pessoa pede.
struct MXBlocoDeTextoLongo: View {
    let texto: MXTextoLongo
    @State private var inteiro: Bool

    init(texto: MXTextoLongo, abertoDeInicio: Bool = false) {
        self.texto = texto
        _inteiro = State(initialValue: abertoDeInicio)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: MX.Medida.entreItens) {
            ScrollView([.horizontal, .vertical]) {
                Text(inteiro ? texto.paraInspecao() : texto.previa)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(MX.Claro.texto.color)
                    .textSelection(.enabled)
                    .padding(MX.Espaco.dentroDeLinha)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.hidden)
            .frame(maxHeight: inteiro ? MX.Conversa.inspecaoAltura : nil)
            .fixedSize(horizontal: false, vertical: !inteiro)
            .background(MX.CorDaConversa.fundoDosPassos.color, in: RoundedRectangle(cornerRadius: MX.Raio.chip))
            if let nota = texto.notaDoCorte {
                HStack(alignment: .firstTextBaseline, spacing: MX.Medida.entreItens) {
                    Text(nota)
                        .mxTipo(MX.TipoDaConversa.dica)
                        .foregroundStyle(MX.Claro.textoSecundario.color)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Button(inteiro ? String(localized: "Mostrar só o começo", bundle: .module) : String(localized: "Mostrar tudo", bundle: .module)) { inteiro.toggle() }
                        .mxTipo(MX.TipoDaConversa.botaoPequeno)
                        .foregroundStyle(MX.Claro.mexericaTexto.color)
                        .frame(minHeight: MX.Espaco.alvoMinimo)
                }
            }
        }
    }
}

/// Uma alteração em Detalhes: o caminho inteiro (com quebra de linha, selecionável, lido inteiro pelo VoiceOver),
/// +/−, as primeiras linhas (já limitadas no modelo) e, se cortou, "Mostrar tudo" com o patch inteiro (começo e fim
/// acima do limite absoluto).
struct MXPreviaDeDiff: View {
    let diff: MXDiffPrevia
    /// Preparada aqui, só quando a alteração aparece (Detalhes aberto, página à vista).
    let previa: MXLinhasDoDiff
    @State private var inteiro = false
    @State private var caminhoInteiro = false

    init(diff: MXDiffPrevia) {
        self.diff = diff
        previa = diff.linhasDaPrevia()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: MX.Medida.entreItens) {
            // O caminho sempre conferível: inteiro, quebrando linha (sem truncar), selecionável; o VoiceOver lê tudo.
            ScrollView(.vertical) {
                Text(verbatim: diff.caminhoNaTela(inteiro: caminhoInteiro))
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(MX.Claro.texto.color)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
            }
            .frame(maxHeight: caminhoInteiro ? MX.Conversa.inspecaoAltura : nil)
            .fixedSize(horizontal: false, vertical: !caminhoInteiro)
            if diff.caminhoPedeMostrarTudo {
                // Alvo próprio de 44 × 44 pt no label, como o paginador.
                Button { caminhoInteiro.toggle() } label: {
                    Text(caminhoInteiro ? String(localized: "Mostrar só o começo do caminho", bundle: .module) : String(localized: "Mostrar o caminho inteiro", bundle: .module))
                        .mxTipo(MX.TipoDaConversa.botaoPequeno)
                        .frame(minWidth: MX.Espaco.alvoMinimo, minHeight: MX.Espaco.alvoMinimo, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(MXToque())
                .foregroundStyle(MX.Claro.mexericaTexto.color)
            }
            HStack(spacing: MX.Medida.entreItens) {
                Text(verbatim: "+\(diff.adicionadas)").foregroundStyle(MX.Claro.sucessoTexto.color)
                Text(verbatim: "−\(diff.removidas)").foregroundStyle(MX.Claro.erroTexto.color)
            }
            .font(.system(.caption, design: .monospaced))
            .accessibilityElement(children: .combine)
            .accessibilityLabel([
                diff.adicionadas == 1 ? String(localized: "\(diff.adicionadas) linha adicionada", bundle: .module)
                    : String(localized: "\(diff.adicionadas) linhas adicionadas", bundle: .module),
                diff.removidas == 1 ? String(localized: "\(diff.removidas) removida", bundle: .module)
                    : String(localized: "\(diff.removidas) removidas", bundle: .module),
            ].joined(separator: ", "))
            if inteiro {
                MXBlocoDeTextoLongo(texto: diff.patch, abertoDeInicio: true)
            } else {
                ScrollView(.horizontal) {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(previa.linhas.enumerated()), id: \.offset) { _, linha in
                            Text(verbatim: linha.isEmpty ? " " : linha)
                                .foregroundStyle((linha.hasPrefix("+") ? MX.Claro.sucessoTexto
                                    : linha.hasPrefix("-") ? MX.Claro.erroTexto : MX.Claro.textoSecundario).color)
                        }
                    }
                    .font(.system(.caption2, design: .monospaced))
                    .padding(MX.Espaco.dentroDeLinha)
                }
                .scrollIndicators(.hidden)
                .background(MX.CorDaConversa.fundoDosPassos.color, in: RoundedRectangle(cornerRadius: MX.Raio.chip))
                if previa.cortado {
                    Button(String(localized: "Mostrar tudo", bundle: .module)) { inteiro = true }
                        .mxTipo(MX.TipoDaConversa.botaoPequeno)
                        .foregroundStyle(MX.Claro.mexericaTexto.color)
                        .frame(minHeight: MX.Espaco.alvoMinimo)
                }
            }
        }
    }
}
