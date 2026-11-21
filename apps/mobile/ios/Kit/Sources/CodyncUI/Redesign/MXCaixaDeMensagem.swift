import SwiftUI

/// O que a caixa faz (intenções; quem decide é a fonte).
struct MXAcoesDaCaixa {
    var enviar: () -> Void
    var salvarEdicao: () -> Void
    var cancelarEdicao: () -> Void
    var parar: () -> Void
    var aoVivo: () -> Void
    var anexar: () -> Void
    var mandarAgora: (MXItemDaFila) -> Void
    var editar: (MXItemDaFila) -> Void
    var remover: (MXItemDaFila) -> Void
    var limparAviso: () -> Void
}

/// A parte de baixo da Conversa (`capturas/05-conversa.webp`, `05a-fila-editando.webp`): a fila "Na fila · N" em
/// cima, um pouco mais estreita; "Editando a 2ª da fila · Cancelar"; o `+` redondo separado e a cápsula com o
/// texto, o ditado e o botão ao vivo (vira enviar com texto, ✓ editando, parar esperando a decisão); embaixo, o
/// modelo e o % de contexto.
struct MXCaixaDeMensagem: View {
    let dados: MXConversaDados
    let conexao: MXEstadoConexao
    /// O que a fonte tem de verdade: o `+` e o ao vivo ficam desligados ("Em breve") quando ela não tem.
    let capacidades: MXCapacidades
    let aviso: String?
    @Binding var texto: String
    let edicao: MXEdicaoDaFila
    /// Mensagens da fila com um salvar a caminho (a reserva é da fonte).
    let salvando: Set<String>
    @Binding var filaRecolhida: Bool
    let acoes: MXAcoesDaCaixa
    @State private var ditado = Dictation()
    /// A caixa quando o ditado começou: o que se ouve vai depois dela.
    @State private var antesDoDitado = ""
    /// Tocar em qualquer ponto da cápsula do texto (44 pt de altura) foca o campo.
    @FocusState private var campoFocado: Bool
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.accessibilityReduceTransparency) private var reduzirTransparencia

    /// Texto grande: o arranjo (linhas do campo) e a altura máxima da caixa (nula no padrão).
    let arranjo: MXArranjoDaCaixa
    let alturaMaxima: CGFloat?

    init(dados: MXConversaDados, conexao: MXEstadoConexao, capacidades: MXCapacidades, aviso: String?, texto: Binding<String>,
         edicao: MXEdicaoDaFila,
         salvando: Set<String>, filaRecolhida: Binding<Bool>, arranjo: MXArranjoDaCaixa = MXArranjoDaCaixa(acessibilidade: false),
         alturaMaxima: CGFloat? = nil, acoes: MXAcoesDaCaixa) {
        self.arranjo = arranjo
        self.alturaMaxima = alturaMaxima
        self.dados = dados
        self.conexao = conexao
        self.capacidades = capacidades
        self.aviso = aviso
        _texto = texto
        self.edicao = edicao
        self.salvando = salvando
        _filaRecolhida = filaRecolhida
        self.acoes = acoes
    }

    private var c: MX.Conversa.Type { MX.Conversa.self }

    private var caixa: MXCaixa {
        MXCaixa.estado(texto: texto, editando: edicao.ativa, esperandoDecisao: dados.esperandoDecisao,
                       trabalhando: dados.trabalhando, conexao: conexao, nome: dados.nome,
                       salvando: edicao.salvando != nil || (edicao.item.map { salvando.contains($0.id) } ?? false),
                       aoVivo: capacidades.ligado(.aoVivo), semEnvio: dados.semEnvio)
    }

    var body: some View {
        if let alturaMaxima {
            // Texto grande: a caixa para em metade da tela e rola por dentro, ancorada embaixo (campo à vista).
            ScrollView { conteudo }
                .scrollBounceBehavior(.basedOnSize)
                .defaultScrollAnchor(.bottom)
                .frame(maxHeight: alturaMaxima)
                .fixedSize(horizontal: false, vertical: true)
        } else {
            conteudo
        }
    }

    private var conteudo: some View {
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        return VStack(spacing: c.caixaEntreBlocos) {
            if let aviso { linhaDeAviso(aviso) }
            // Esta conversa não aceita mensagem agora (sessão extra sem o envio confirmado): o motivo fica à vista
            // enquanto valer, não só na dica do campo vazio.
            if let semEnvio = dados.semEnvio, semEnvio != aviso {
                Text(semEnvio)
                    .mxTipo(MX.TipoDaConversa.dica)
                    .foregroundStyle(MX.Claro.textoSecundario.color)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, c.editandoRespiroEsquerda)
            }
            if MXFilaRegras.visivel(dados.fila) {
                MXFilaDeMensagens(fila: dados.fila, editando: edicao.item?.id, conectado: conexao.podeAgir,
                                  salvando: salvando, recolhida: $filaRecolhida, acoes: acoes)
                    .padding(.horizontal, c.filaRecuo)
                    .transition(MX.Movimento.troca(reduzir: reduzirMovimento).transicaoDeConteudo)
            }
            if let item = edicao.item, let posicao = MXFilaRegras.posicao(item.id, em: dados.fila) {
                // Posição numerada só com a ordem do Hub; sem ela, não se promete "a 2ª".
                linhaEditando(dados.ordemDaFilaConfiavel ? posicao : nil)
                    .transition(MX.Movimento.troca(reduzir: reduzirMovimento).transicaoDeConteudo)
            }
            HStack(spacing: c.caixaEntreBotoes) {
                botaoMais
                capsula
            }
            rodape
        }
        .padding(.horizontal, c.caixaMargem)
        .animation(troca.animacaoDaPosicao, value: dados.fila)
        .animation(troca.animacaoDaPosicao, value: edicao)
        .onDisappear { ditado.stop() }
    }

    // MARK: Aviso

    private func linhaDeAviso(_ aviso: String) -> some View {
        HStack(spacing: MX.Espaco.dentroDeLinha) {
            Text(aviso)
                .mxTipo(MX.TipoDaConversa.dica)
                .foregroundStyle(MX.Claro.erroTexto.color)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button(String(localized: "OK", bundle: .module), action: acoes.limparAviso)
                .mxTipo(MX.TipoDaConversa.botaoPequeno)
                .foregroundStyle(MX.Claro.texto.color)
                .frame(minWidth: MX.Espaco.alvoMinimo, minHeight: MX.Espaco.alvoMinimo)
                .accessibilityLabel(String(localized: "Dispensar aviso", bundle: .module))
        }
        .padding(.horizontal, c.editandoRespiroEsquerda)
        .accessibilityElement(children: .contain)
    }

    // MARK: Editando

    private func linhaEditando(_ posicao: Int?) -> some View {
        HStack(spacing: MX.Medida.entreItens) {
            MXIconeView(.lapis, tamanho: c.editandoIcone)
            Text(MXFilaRegras.textoEditando(posicao: posicao)).mxTipo(MX.TipoDaConversa.editando)
            Spacer(minLength: 0)
            Button(action: acoes.cancelarEdicao) {
                Text("Cancelar", bundle: .module)
                    .mxTipo(MX.TipoDaConversa.botaoPequeno)
                    .foregroundStyle(MX.Claro.texto.color)
                    // Nunca quebra no meio da palavra (no maior tamanho vinha "Cancela / r"): quem quebra é o rótulo.
                    .fixedSize(horizontal: true, vertical: false)
                    .padding(.horizontal, c.cancelarRespiro)
                    .frame(minHeight: c.cancelarAltura)
                    .background(MX.CorDaConversa.neutro.color, in: Capsule())
                    // Cápsula desenhada de 28; área de toque de 44 em volta (a linha cresce, nada se sobrepõe).
                    .frame(minWidth: MXAlvoDeToque.minimo, minHeight: MXAlvoDeToque.lado(c.cancelarAltura))
                    .contentShape(Rectangle())
            }
            .buttonStyle(MXToque())
            .accessibilityLabel(String(localized: "Cancelar a edição", bundle: .module))
        }
        .foregroundStyle(MX.Claro.mexericaTexto.color)
        .padding(.leading, c.editandoRespiroEsquerda)
        .padding(.trailing, c.editandoRespiroDireita)
        .frame(minHeight: c.editandoAltura)
    }

    // MARK: Caixa

    private var botaoMais: some View {
        Button(action: acoes.anexar) {
            MXIconeView(.mais, tamanho: c.maisIcone, traco: c.maisTraco)
                .foregroundStyle(MX.Claro.texto.color)
                .frame(width: c.mais, height: c.mais)
                .mxVidro(.caixa, em: Circle(), interativo: true)
        }
        .buttonStyle(MXToque())
        .mxIndisponivel(!capacidades.ligado(.anexar))
        .accessibilityLabel(String(localized: "Anexar foto, arquivo ou colar", bundle: .module))
    }

    private var capsula: some View {
        HStack(spacing: c.capsulaDaCaixaEntre) {
            ZStack(alignment: .leading) {
                // Texto grande: a dica quebra em até 2 linhas (e encolhe um pouco) em vez de cortar.
                if arranjo.dicaPropria, texto.isEmpty {
                    Text(caixa.dica)
                        .mxTipo(MX.Tipo.campo)
                        .foregroundStyle(MX.Claro.textoSecundario.color)
                        .lineLimit(arranjo.linhasDaDica)
                        .minimumScaleFactor(arranjo.escalaMinimaDaDica)
                        .fixedSize(horizontal: false, vertical: true)
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
                TextField("", text: $texto,
                          prompt: arranjo.dicaPropria ? nil : Text(caixa.dica).foregroundStyle(MX.Claro.textoSecundario.color),
                          axis: .vertical)
                    .mxTipo(MX.Tipo.campo)
                    .foregroundStyle(MX.Claro.texto.color)
                    .lineLimit(1...arranjo.linhasDoCampo)
                    .focused($campoFocado)
                    // A dica desenhada fora do campo continua chegando ao VoiceOver.
                    .accessibilityHint(arranjo.dicaPropria && texto.isEmpty ? caixa.dica : "")
            }
            .padding(.horizontal, c.campoRespiro)
            .frame(minHeight: MXAlvoDeToque.lado(c.campoAltura))
            .background { Color.clear.contentShape(Rectangle()).onTapGesture { campoFocado = true } }
            .accessibilityLabel(String(localized: "Mensagem para \(dados.nome)", bundle: .module))
            botaoDitado
            botaoDaDireita
        }
        .padding(c.capsulaDaCaixaRespiro)
        .frame(minHeight: c.capsulaDaCaixaAltura)
        .mxVidro(.caixa, em: RoundedRectangle(cornerRadius: c.capsulaDaCaixaRaio))
    }

    private var botaoDitado: some View {
        let ouvindo = ditado.listening
        return Button {
            if ouvindo {
                ditado.stop()
            } else {
                antesDoDitado = texto
                Task { await ditado.start { ouvido in texto = Dictation.join(antesDoDitado, ouvido) } }
            }
        } label: {
            MXIconeView(.microfone, tamanho: c.ditadoIcone)
                .foregroundStyle((ouvindo ? MX.Claro.mexericaTexto : MX.Claro.texto).color)
                // Desenho de 40 de largura; área de toque de 44.
                .frame(width: MXAlvoDeToque.lado(c.ditadoLargura), height: MXAlvoDeToque.lado(c.campoAltura))
                .contentShape(Rectangle())
        }
        .buttonStyle(MXToque())
        .accessibilityLabel(ouvindo ? String(localized: "Parar o ditado", bundle: .module) : String(localized: "Ditar: voz para texto", bundle: .module))
    }

    /// Ao vivo ↔ enviar ↔ ✓ ↔ parar. Normal: o novo entra crescendo de 0.7 com a mola; Reduzir movimento: cross-fade.
    private var botaoDaDireita: some View {
        let estado = caixa
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        let entrada: AnyTransition = troca.interpola ? .scale(scale: c.trocaEscalaInicial).combined(with: .opacity) : .opacity
        return ZStack {
            switch estado.botao {
            case .aoVivo:
                Button(action: acoes.aoVivo) {
                    MXIconeView(.onda, tamanho: c.aoVivoIcone)
                        .foregroundStyle(MX.Claro.sobreMexerica.color)
                        .frame(width: c.aoVivoLargura, height: c.enviar)
                        .background(MX.Claro.mexerica.color, in: Capsule())
                        .shadow(color: MX.CorDaConversa.sombraLaranja.color, radius: c.aoVivoSombraRaio, y: c.aoVivoSombraY)
                }
                .accessibilityLabel(estado.rotulo)
                .transition(entrada)
            case .enviar, .salvarEdicao:
                Button(action: estado.botao == .enviar ? acoes.enviar : acoes.salvarEdicao) {
                    Group {
                        if estado.botao == .enviar {
                            MXIconeView(.enviar, tamanho: c.enviarIcone)
                        } else {
                            MXIconeView(.confirmar, tamanho: c.enviarIcone)
                        }
                    }
                    .foregroundStyle(MX.Claro.sobreMexerica.color)
                    .frame(width: c.enviar, height: c.enviar)
                    .background(MX.Claro.mexerica.color, in: Circle())
                }
                .accessibilityLabel(estado.rotulo)
                .transition(entrada)
            case .parar:
                Button(action: acoes.parar) {
                    MXIconeView(.parar, tamanho: c.pararIcone)
                        .foregroundStyle(MX.Claro.texto.color)
                        .frame(width: c.enviar, height: c.enviar)
                        .background(MX.CorDaConversa.parar.color, in: Circle())
                }
                .accessibilityLabel(estado.rotulo)
                .transition(entrada)
            }
        }
        .buttonStyle(MXToque())
        .disabled(!estado.ligado)
        .opacity(estado.ligado ? 1 : c.desligadoOpacidade)
        // O ao vivo que a fonte não tem: "Em breve" (valor acessível "Indisponível").
        .accessibilityValue(estado.emBreve ? MXIndisponivel.valorAcessivel : "")
        .accessibilityHint(estado.emBreve ? MXIndisponivel.emBreve : "")
        .animation(troca.animacao, value: estado.botao)
    }

    // MARK: Rodapé

    @ViewBuilder private var rodape: some View {
        if dados.modelo != nil || dados.contexto != nil {
            // Lado a lado, como no desenho; se as duas pílulas não cabem numa linha (texto grande), uma embaixo da
            // outra, para nenhuma palavra quebrar no meio ("context / o").
            ViewThatFits(in: .horizontal) {
                HStack {
                    pilulaDoModelo
                    Spacer(minLength: MX.Espaco.dentroDeLinha)
                    pilulaDoContexto
                }
                VStack(alignment: .leading, spacing: MX.Espaco.dentroDeLinha) {
                    pilulaDoModelo
                    pilulaDoContexto
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.horizontal, c.rodapeRespiro)
        }
    }

    @ViewBuilder private var pilulaDoModelo: some View {
        if let modelo = dados.modelo {
            pilula { Text(modelo) }
                .accessibilityLabel(String(localized: "Modelo: \(modelo)", bundle: .module))
        }
    }

    @ViewBuilder private var pilulaDoContexto: some View {
        if let contexto = dados.contexto {
            pilula {
                HStack(spacing: MX.Medida.entreItens) {
                    MXAnelDeContexto(fracao: Double(contexto) / 100)
                    Text(verbatim: String.localizedStringWithFormat(NSLocalizedString("%lld%% do contexto", bundle: .module, comment: "Percentual do contexto usado"), contexto))
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(String.localizedStringWithFormat(NSLocalizedString("%lld%% do contexto usado", bundle: .module, comment: "Percentual do contexto usado, lido pelo VoiceOver"), contexto))
        }
    }

    private func pilula<Conteudo: View>(@ViewBuilder _ conteudo: () -> Conteudo) -> some View {
        conteudo()
            .mxTipo(MX.TipoDaConversa.rodape)
            .foregroundStyle(MX.Claro.texto.color)
            .padding(.horizontal, c.rodapePilulaRespiro)
            .frame(minHeight: c.rodapeAltura)
            .background((reduzirTransparencia ? MX.CorDaConversa.rodape.sobre(MX.Claro.fundo) : MX.CorDaConversa.rodape).color,
                        in: Capsule())
    }
}

/// O anel do % de contexto: trilho claro e o arco usado, começando no alto.
struct MXAnelDeContexto: View {
    let fracao: Double
    @ScaledMetric(relativeTo: .caption) private var lado = MX.Conversa.anel

    init(fracao: Double) { self.fracao = fracao }

    var body: some View {
        ZStack {
            Circle().stroke(MX.CorDaConversa.trilhoDoContexto.color, lineWidth: MX.Conversa.anelTraco)
            Circle()
                .trim(from: 0, to: min(1, max(0, fracao)))
                .stroke(MX.Claro.texto.color, lineWidth: MX.Conversa.anelTraco)
                .rotationEffect(.degrees(MX.Conversa.anguloDoInicioDoAnel))
        }
        .frame(width: lado, height: lado)
        .accessibilityHidden(true)
    }
}

/// "Na fila · N" (`capturas/05a-fila-editando.webp`): passo de 44 pt em linhas, cabeçalho e ações (padrão Apple;
/// o compacto de 31/22 do desenho segue em `filaEspacamento`), texto 12; passou de 2, rola
/// e o fim esmaece; Mandar agora, Editar e Remover em cada linha; a seta recolhe numa linha só.
struct MXFilaDeMensagens: View {
    let fila: [MXItemDaFila]
    let editando: String?
    /// Mensagens com um salvar a caminho: Editar delas fica desligado até voltar.
    let salvando: Set<String>
    let conectado: Bool
    @Binding var recolhida: Bool
    let acoes: MXAcoesDaCaixa
    /// Altura desenhada da linha seguindo o Dynamic Type (cresce com o texto, sem teto).
    @ScaledMetric(relativeTo: .caption) private var linha = MX.Conversa.filaLinha
    /// Passo entre as linhas: a linha inteira é o alvo vertical (31 no desenho; 44 no padrão da Apple).
    private var passoDaLinha: CGFloat { MXAlvoDeToque.passoNaFila(linha) }
    /// Áreas de toque das três ações (44 / 44 / 44 no padrão da Apple, o escolhido; 36 / 28 / 36 no desenho), relativas
    /// à borda esquerda da primeira.
    private let areasDasAcoes = MXAlvoDeToque.areasDasAcoesDaFila()
    private var passoDaAcao: CGFloat { MXAlvoDeToque.passoNaFila(c.filaAcao) }
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    @Environment(\.accessibilityReduceTransparency) private var reduzirTransparencia
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto
    /// Texto grande: 2 linhas de texto e as ações numa linha embaixo.
    private var arranjo: MXArranjoDaFila { MXArranjoDaFila(acessibilidade: tamanhoDoTexto.isAccessibilitySize) }

    init(fila: [MXItemDaFila], editando: String?, conectado: Bool, salvando: Set<String>, recolhida: Binding<Bool>,
         acoes: MXAcoesDaCaixa) {
        self.fila = fila
        self.editando = editando
        self.conectado = conectado
        self.salvando = salvando
        _recolhida = recolhida
        self.acoes = acoes
    }

    private var c: MX.Conversa.Type { MX.Conversa.self }

    var body: some View {
        VStack(spacing: 0) {
            cabecalho
            if !recolhida {
                lista.transition(MX.Movimento.troca(reduzir: reduzirMovimento).transicaoDeConteudo)
            }
        }
        .padding(c.filaRespiro)
        .mxVidro(.fila, em: RoundedRectangle(cornerRadius: c.filaRaio))
        .shadow(color: MX.CorDaConversa.sombraDoBalao.color, radius: c.filaSombraRaio, y: c.filaSombraY)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "Mensagens na fila", bundle: .module))
    }

    private var cabecalho: some View {
        HStack(spacing: c.filaCabecalhoEntre) {
            Text(MXFilaRegras.titulo(fila))
                .mxTipo(MX.TipoDaConversa.filaCabecalho)
                .lineLimit(1)
                .layoutPriority(1)
            if recolhida, let primeira = MXFilaRegras.resumoRecolhida(fila) {
                Text(primeira).mxTipo(MX.TipoDaConversa.filaResumo).lineLimit(1)
            }
            Spacer(minLength: 0)
            Button {
                withAnimation(MX.Movimento.troca(reduzir: reduzirMovimento).animacaoDaPosicao) { recolhida.toggle() }
            } label: {
                MXSetaQueAbre(aberta: !recolhida, tamanho: c.filaSetaIcone, traco: c.filaSetaTraco)
                    .frame(width: MXAlvoDeToque.passoNaFila(c.filaSetaLargura), height: MXAlvoDeToque.passoNaFila(c.filaCabecalho))
                    // Desenho de 26; área de toque de 44 de largura, crescendo para a esquerda (espaço vazio do
                    // cabeçalho), sem mudar o desenho. Altura: a do cabeçalho.
                    .contentShape(MXFaixaDeToque(inicio: MXAlvoDeToque.passoNaFila(c.filaSetaLargura) - MXAlvoDeToque.lado(c.filaSetaLargura),
                                                 fim: MXAlvoDeToque.passoNaFila(c.filaSetaLargura)))
            }
            .buttonStyle(MXToque())
            .accessibilityLabel(recolhida ? String(localized: "Mostrar a fila", bundle: .module) : String(localized: "Recolher a fila", bundle: .module))
        }
        .foregroundStyle(MX.Claro.textoSecundario.color)
        .padding(.leading, c.filaCabecalhoRespiro)
        .frame(minHeight: MXAlvoDeToque.passoNaFila(c.filaCabecalho))
    }

    private var lista: some View {
        let rola = MXFilaRegras.rola(fila)
        return ScrollView {
            VStack(spacing: 0) {
                ForEach(fila) { linhaDaFila($0) }
            }
        }
        .scrollIndicators(rola ? .visible : .hidden)
        .scrollBounceBehavior(.basedOnSize)
        // Até 2 linhas, a lista abraça o conteúdo; passou disso, para em 2 e rola.
        .frame(maxHeight: arranjo.alturaDaLinha(linha: linha, passo: passoDaLinha, acao: passoDaAcao) * CGFloat(c.filaLinhasVisiveis))
        .fixedSize(horizontal: false, vertical: true)
        .mask {
            // Passou de 2: o fim esmaece nos últimos 14 pt.
            if rola {
                VStack(spacing: 0) {
                    Rectangle()
                    LinearGradient(colors: [.black, .clear], startPoint: .top, endPoint: .bottom).frame(height: c.filaEsmaecer)
                }
            } else {
                Rectangle()
            }
        }
    }

    private func linhaDaFila(_ item: MXItemDaFila) -> some View {
        let emEdicao = item.id == editando
        let texto = Text(item.texto)
            .mxTipo(MX.TipoDaConversa.filaLinha)
            .foregroundStyle(MX.Claro.texto.color)
            .lineLimit(arranjo.linhasDoTexto)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.trailing, c.filaTextoRespiro)
        return Group {
            if arranjo.acoesEmbaixo {
                // Texto grande: o texto ganha a largura toda (2 linhas) e as ações descem, à direita.
                VStack(alignment: .leading, spacing: 0) {
                    texto
                    HStack(spacing: 0) {
                        Spacer(minLength: 0)
                        acoesDaLinha(item)
                    }
                }
            } else {
                HStack(spacing: 0) {
                    texto
                    acoesDaLinha(item)
                }
            }
        }
        .padding(.leading, c.filaLinhaRespiro)
        .frame(minHeight: passoDaLinha)
        .background {
            // O realce da linha em edição tem a altura desenhada (31), centrado no passo da linha; com as ações
            // embaixo, cobre a mensagem inteira.
            RoundedRectangle(cornerRadius: c.filaLinhaRaio)
                .fill(emEdicao ? MX.CorDaConversa.filaEditando.color : .clear)
                .frame(height: arranjo.acoesEmbaixo ? nil : linha)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(item.texto + (emEdicao ? String(localized: ", em edição", bundle: .module) : ""))
    }

    /// As três ações de uma mensagem, encostadas (o afastamento vem do passo de cada uma).
    private func acoesDaLinha(_ item: MXItemDaFila) -> some View {
        HStack(spacing: 0) {
            acao(0, rotulo: String(localized: "Mandar agora", bundle: .module), acao: { acoes.mandarAgora(item) }) {
                MXIconeView(.enviar, tamanho: c.filaMandarAgoraIcone, traco: c.confirmarTraco)
                    .foregroundStyle(MX.Claro.mexericaTexto.color)
                    .frame(width: c.filaMandarAgoraDisco, height: c.filaMandarAgoraDisco)
                    .background(MX.CorDaConversa.mandarAgora.color, in: Circle())
            }
            acao(1, rotulo: String(localized: "Editar", bundle: .module), ligada: !salvando.contains(item.id), acao: { acoes.editar(item) }) {
                MXIconeView(.lapis, tamanho: c.filaIcone).foregroundStyle(MX.Claro.texto.color)
            }
            acao(2, rotulo: String(localized: "Remover da fila", bundle: .module), acao: { acoes.remover(item) }) {
                MXIconeView(.lixeira, tamanho: c.filaIcone).foregroundStyle(MX.Claro.erroTexto.color)
            }
        }
    }

    /// Uma ação da linha: o ícone do desenho, num quadro com o passo de `filaEspacamento` (44 no padrão da Apple, o
    /// escolhido: cada ação com alvo de 44 × 44 e 16 pt entre os desenhos; 28 no desenho compacto, com as áreas das
    /// pontas crescendo para fora). A área de toque é a de `areasDasAcoes`; nada se sobrepõe. Sem conexão, fica à
    /// vista e desligada.
    private func acao<Icone: View>(_ indice: Int, rotulo: String, ligada: Bool = true, acao: @escaping () -> Void,
                                   @ViewBuilder icone: () -> Icone) -> some View {
        let area = areasDasAcoes[indice]
        let deslocamento = CGFloat(indice) * passoDaAcao
        return Button(action: acao) {
            icone()
                .frame(width: passoDaAcao)
                .frame(minHeight: passoDaLinha)
                .contentShape(MXFaixaDeToque(inicio: area.lowerBound - deslocamento, fim: area.upperBound - deslocamento))
        }
        .buttonStyle(MXToque())
        .disabled(!conectado || !ligada)
        .opacity(conectado && ligada ? 1 : c.desligadoOpacidade)
        .accessibilityLabel(rotulo)
    }
}

/// Área de toque em x de `inicio` a `fim` (relativos ao próprio quadro, podem passar dele), com a altura do quadro.
/// Amplia o alvo sem mudar o desenho.
struct MXFaixaDeToque: Shape {
    let inicio: CGFloat
    let fim: CGFloat

    func path(in rect: CGRect) -> Path {
        Path(CGRect(x: rect.minX + inicio, y: rect.minY, width: max(0, fim - inicio), height: rect.height))
    }
}

public extension MXTroca {
    /// Transição de conteúdo que aparece e some (fila, linha "Editando", passos, Detalhes). Normal: opacidade junto
    /// com a mola da troca. Reduzir movimento: a troca não anima posição (o layout pula), e a opacidade anima sozinha
    /// com o cross-fade curto do token.
    var transicaoDeConteudo: AnyTransition { conteudoAnimaSozinho ? .opacity.animation(animacao) : .opacity }
}
