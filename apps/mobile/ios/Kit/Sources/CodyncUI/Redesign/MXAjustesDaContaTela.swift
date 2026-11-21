#if DEBUG
import CodyncKit
import SwiftUI

// Ajustes, conta e computadores (PLANO §4.1; mockups `mock-ios-ajustes-*`). Só Debug, atrás do interruptor
// "Novo visual (em teste)". O que já existe no app de hoje (login, pareamento, voz, uso, ajustes de hoje) abre pela ponte
// numa folha; o que não existe aparece como "Em breve", desligado.

// MARK: - Ícones desta tela

/// Os ícones de traço do mockup (`mockups/fonte/icones.svg.html`) que a casca ainda não tinha. Caixa de 24 × 24.
enum MXIconeDaConta {
    case mac, voz, memoria, uso, aviso, idioma, fora, ok, vazio, alerta, ajustes
    /// Mission Board: o "i" da cobertura parcial.
    case info

    var partes: [MXIcone.Parte] {
        switch self {
        case .mac: [.retangulo(x: 4, y: 5, w: 16, h: 11, r: 2), .caminho("M2.5 19.5h19")]
        case .voz: [.caminho("M5 10v4M9 7v10M13 4v16M17 8v8M21 11v2")]
        case .memoria:
            [.caminho("M12 4a4.5 4.5 0 0 0-4.5 4.5c-2 .5-3 2-3 3.8 0 1.4.7 2.5 1.8 3.1.1 2 1.6 3.6 3.6 3.6h4.2c2 0 3.5-1.6 3.6-3.6 1.1-.6 1.8-1.7 1.8-3.1 0-1.8-1-3.3-3-3.8A4.5 4.5 0 0 0 12 4z"),
             .caminho("M12 4v16")]
        case .uso: [.caminho("M4 17a8 8 0 1 1 16 0"), .caminho("M12 17l4-5")]
        case .aviso: [.caminho("M6 17V11a6 6 0 0 1 12 0v6l1.5 2h-15z"), .caminho("M10 21h4")]
        case .idioma:
            [.circulo(cx: 12, cy: 12, r: 8.5),
             .caminho("M3.5 12h17M12 3.5c2.5 2.4 3.5 5.3 3.5 8.5s-1 6.1-3.5 8.5c-2.5-2.4-3.5-5.3-3.5-8.5s1-6.1 3.5-8.5z")]
        case .fora: [.caminho("M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5")]
        case .ok: [.circulo(cx: 12, cy: 12, r: 9), .caminho("M8 12.5l2.7 2.7L16 9.5")]
        case .vazio: [.circulo(cx: 12, cy: 12, r: 9)]
        case .alerta: [.circulo(cx: 12, cy: 12, r: 9), .caminho("M12 7.5v5.5M12 16.2v.1")]
        case .info: [.circulo(cx: 12, cy: 12, r: 9), .caminho("M12 11v5.5M12 7.8v.1")]
        case .ajustes: [.caminho("M4 7h10M18 7h2M4 17h4M12 17h8"), .circulo(cx: 16, cy: 7, r: 2), .circulo(cx: 10, cy: 17, r: 2)]
        }
    }

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

struct MXIconeDaContaView: View {
    let icone: MXIconeDaConta
    let tamanho: CGFloat

    var body: some View {
        let escala = tamanho / 24
        icone.caminho
            .applying(CGAffineTransform(scaleX: escala, y: escala))
            .stroke(style: StrokeStyle(lineWidth: MX.Conta.traco * escala, lineCap: .round, lineJoin: .round))
            .frame(width: tamanho, height: tamanho)
            .accessibilityHidden(true)
    }
}

// MARK: - Paleta (claro e escuro)

/// As cores da tela nos dois temas, dos tokens que já existem. No escuro não há papel de parede nem vidro: grafite chapado.
struct MXPaletaDaConta {
    let escuro: Bool

    init(_ esquema: ColorScheme) { escuro = esquema == .dark }

    var texto: Color { (escuro ? MX.Escuro.texto : MX.Claro.texto).color }
    var secundario: Color { (escuro ? MX.Escuro.textoSecundario : MX.Claro.textoSecundario).color }
    var fundo: Color { (escuro ? MX.Escuro.fundo : MX.Claro.fundo).color }
    var painel: Color { MX.Escuro.superficie.color }
    var cartao: Color { (escuro ? MX.Escuro.campo : MX.Claro.balaoDoAgente).color }
    var linha: Color { (escuro ? MX.Escuro.linha : MX.Claro.linha).color }
    var tile: Color { (escuro ? MX.Escuro.etiqueta : MX.Claro.etiqueta).color }
    var destaque: Color { MX.Claro.mexerica.color }
    var sobreDestaque: Color { MX.Claro.sobreMexerica.color }
    var destaqueTexto: Color { (escuro ? MX.Escuro.mexericaTexto : MX.Claro.mexericaTexto).color }
    var tintaDoDestaque: Color { MX.Claro.abaAtiva.color }
    var ok: Color { (escuro ? MX.Escuro.sucessoTexto : MX.Claro.sucessoTexto).color }
    /// Contorno do círculo vazio (computador que não está em uso).
    var borda: Color { (escuro ? MX.Escuro.textoSecundario : MX.Claro.borda).color }
    /// Texto de erro e de "Sair". No escuro não há vermelho aprovado que passe de 4,5:1 sobre o cartão: usa o texto
    /// principal (o `#FF8F85` do PLANO §5 espera o dono); o significado fica na confirmação e na posição, não na cor.
    var erro: Color { escuro ? MX.Escuro.texto.color : MX.Claro.erroTexto.color }
}

// MARK: - Tela

struct MXAjustesDaContaTela: View {
    let conta: any MXContaFonte
    let esquema: ColorScheme
    /// Nulo quando a tela é a raiz (sem computador não há Conversas para voltar).
    let voltar: (() -> Void)?
    /// O idioma da tela (o do app; os testes de imagem fixam o pt-BR).
    var textos = MXTextosDaConta()
    /// Sem `ScrollView`: só para as imagens de teste, porque o `ImageRenderer` não desenha o que fica dentro de uma.
    var semRolagem = false

    @State private var destino: MXDestinoDaConta?
    @State private var confirmandoSair = false
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto
    @Environment(\.accessibilityReduceTransparency) private var reduzirTransparencia
    @Environment(\.scenePhase) private var fase
    @Environment(\.openURL) private var abrirURL

    private var acessivel: Bool { tamanhoDoTexto.isAccessibilitySize }
    private var paleta: MXPaletaDaConta { MXPaletaDaConta(esquema) }
    private var tile: CGFloat { acessivel ? MX.Conta.tileAcessivel : MX.Conta.tile }
    private var alturaDaLinha: CGFloat { acessivel ? MX.Conta.linhaAlturaMinimaAcessivel : MX.Conta.linhaAlturaMinima }

    var body: some View {
        let dados = conta.dados
        VStack(spacing: 0) {
            topo
            if let erro = conta.erro { aviso(erro) }
            painel(dados)
        }
        .background { fundo }
        .environment(\.colorScheme, esquema)
        .accessibilityAction(.escape) { voltar?() }
        .task { await conta.aoAbrir() }
        .onChange(of: fase) { _, nova in
            if nova == .active { Task { await conta.aoVoltarAoApp() } }
        }
        .onChange(of: conta.erro) { _, novo in
            if let novo { AccessibilityNotification.Announcement(frase(novo)).post() }
        }
        .codyncSheet(item: $destino) { d in conta.vista(d) }
        .confirmationDialog(textos.sairPergunta(conta.dados.email), isPresented: $confirmandoSair, titleVisibility: .visible) {
            Button(textos.sair, role: .destructive) { Task { await conta.sair() } }
            Button(textos.cancelar, role: .cancel) {}
        } message: {
            Text(textos.sairAviso)
        }
    }

    @ViewBuilder private var fundo: some View {
        if paleta.escuro { paleta.fundo.ignoresSafeArea() } else { MXFundo() }
    }

    // MARK: Topo

    private var topo: some View {
        HStack(spacing: MX.Espaco.dentroDeLinha + 2) {
            if let voltar {
                Button(action: voltar) {
                    MXIconeView(.voltar, tamanho: MX.Medida.botaoRedondoIcone)
                        .foregroundStyle(paleta.texto)
                        .frame(width: MX.Medida.botaoRedondo, height: MX.Medida.botaoRedondo)
                        .modifier(MXBotaoRedondoDaConta(paleta: paleta))
                }
                .buttonStyle(MXToque())
                .accessibilityLabel(textos.voltar)
            }
            Text(textos.ajustes)
                .mxTipo(MX.Tipo.tituloDeTela)
                .foregroundStyle(paleta.texto)
                .accessibilityAddTraits(.isHeader)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, MX.Espaco.margemTela + MX.Medida.topoRespiroLateral)
        .padding(.top, MX.Medida.topoAbaixoDaAreaSegura)
        .padding(.bottom, MX.Espaco.dentroDeLinha + 2)
    }

    private func frase(_ erro: MXErroDaConta) -> String {
        switch erro {
        case .atualizar: textos.erroAoAtualizar
        case .sair: textos.erroAoSair
        }
    }

    private func aviso(_ erro: MXErroDaConta) -> some View {
        let layout = acessivel ? AnyLayout(VStackLayout(alignment: .leading, spacing: 4)) : AnyLayout(HStackLayout(spacing: MX.Espaco.dentroDeLinha))
        return layout {
            Text(frase(erro))
                .mxTipo(MX.Tipo.aviso)
                .foregroundStyle(paleta.erro)
                .frame(maxWidth: .infinity, alignment: .leading)
            if erro == .atualizar {
                botaoDoAviso(textos.tentarDeNovo) { Task { await conta.atualizar() } }
            }
            botaoDoAviso(textos.ok, rotulo: textos.dispensarAviso) { conta.limparErro() }
        }
        .padding(.horizontal, MX.Espaco.margemTela + MX.Medida.topoRespiroLateral)
        .padding(.bottom, MX.Espaco.dentroDeLinha)
        .accessibilityElement(children: .contain)
    }

    private func botaoDoAviso(_ titulo: String, rotulo: String? = nil, acao: @escaping () -> Void) -> some View {
        Button(action: acao) {
            Text(titulo)
                .mxTipo(MX.Tipo.avisoAcao)
                .foregroundStyle(paleta.texto)
                .frame(minWidth: MX.Espaco.alvoMinimo, minHeight: MX.Espaco.alvoMinimo)
                .contentShape(Rectangle())
        }
        .buttonStyle(MXToque())
        .accessibilityLabel(rotulo ?? titulo)
    }

    // MARK: Painel

    private func painel(_ dados: MXContaDados) -> some View {
        let conteudo = VStack(alignment: .leading, spacing: 0) {
            if dados.semComputador { cartaoSemComputador(dados) }
            cartaoDaConta(dados)
            if !dados.semComputador { computadores(dados) }
            pessoal(dados)
            esteIphone(dados)
            Text(textos.nota)
                .mxTipo(MX.Tipo.legenda)
                .foregroundStyle(paleta.secundario)
                .padding(.top, MX.Conta.notaTopo)
                .padding(.horizontal, MX.Conta.secaoLado)
            maisAjustes
            if !dados.versao.isEmpty {
                Text(textos.versao(dados.versao))
                    .mxTipo(MX.Tipo.legenda)
                    .foregroundStyle(paleta.secundario)
                    .padding(.top, MX.Conta.notaTopo)
                    .padding(.horizontal, MX.Conta.secaoLado)
            }
            if dados.logada { botaoSair }
        }
        .padding(MX.Conta.painelRespiro)
        return Group {
            if semRolagem {
                conteudo.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            } else {
                ScrollView { conteudo }
                    .scrollIndicators(.hidden)
                    .refreshable { await conta.atualizar() }
            }
        }
        .background { fundoDoPainel }
        .clipShape(UnevenRoundedRectangle(topLeadingRadius: MX.Conta.painelRaio, topTrailingRadius: MX.Conta.painelRaio))
        .padding(.horizontal, MX.Espaco.margemTela)
        .ignoresSafeArea(edges: .bottom)
    }

    @ViewBuilder private var fundoDoPainel: some View {
        let forma = UnevenRoundedRectangle(topLeadingRadius: MX.Conta.painelRaio, topTrailingRadius: MX.Conta.painelRaio)
        if paleta.escuro {
            forma.fill(paleta.painel)
                .overlay(forma.stroke(paleta.linha, lineWidth: MX.Vidro.contornoOpacoEspessura))
        } else {
            Color.clear.mxVidro(.painel, em: forma)
        }
    }

    // MARK: Conta

    private func cartaoDaConta(_ dados: MXContaDados) -> some View {
        // Com conta no build e sem ninguém dentro, a entrada é do app de hoje até a etapa 1b: o cartão só informa.
        let semLogin = dados.configurada && !dados.logada
        let subtitulo = semLogin ? textos.semLogin : (dados.email ?? (dados.configurada ? nil : textos.pareamentoLocal))
        let avatar = ZStack {
            Circle().fill(paleta.destaque)
            MXIconeView(.conta, tamanho: MX.Conta.avatarIcone, traco: MX.Conta.traco).foregroundStyle(paleta.sobreDestaque)
        }
        .frame(width: MX.Conta.avatar, height: MX.Conta.avatar)
        let nome = Text(textos.suaConta).mxTipo(MX.Conta.nomeDaConta).foregroundStyle(paleta.texto)
        let email = Text(MXContaRegras.quebravel(subtitulo ?? "")).mxTipo(MX.Tipo.previa).foregroundStyle(paleta.secundario)
        let seta = MXIconeView(.setaParaDireita, tamanho: 18, traco: MX.Conta.traco).foregroundStyle(paleta.secundario)
        let conteudo = Group {
            if acessivel {
                VStack(alignment: .leading, spacing: MX.Conta.contaRespiro) {
                    HStack { avatar; Spacer(minLength: 0); if !semLogin { seta } }
                    VStack(alignment: .leading, spacing: 0) { nome; if subtitulo != nil { email } }
                }
            } else {
                HStack(spacing: MX.Conta.linhaEntre) {
                    avatar
                    VStack(alignment: .leading, spacing: 0) { nome; if subtitulo != nil { email } }
                    Spacer(minLength: 0)
                    if !semLogin { seta }
                }
            }
        }
        .padding(MX.Conta.contaRespiro)
        .frame(maxWidth: .infinity, minHeight: MX.Conta.contaAlturaMinima, alignment: .leading)
        .modifier(MXCartaoDaConta(paleta: paleta))
        return Group {
            if semLogin {
                conteudo
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(textos.suaConta)
                    .accessibilityValue(subtitulo ?? "")
            } else {
                Button { conta.abrirConta() } label: { conteudo }
                    .buttonStyle(MXToque())
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(textos.suaConta)
                    .accessibilityValue(subtitulo ?? "")
                    .accessibilityHint(textos.abreAConta)
            }
        }
    }

    private func cartaoSemComputador(_ dados: MXContaDados) -> some View {
        let parear = dados.conectar == .parear
        return VStack(alignment: .leading, spacing: MX.Conta.linhaEntre) {
            Text(textos.semComputadorTitulo)
                .mxTipo(MX.Tipo.nomeNoCartao)
                .foregroundStyle(paleta.texto)
                .accessibilityAddTraits(.isHeader)
            Text(parear ? textos.semComputadorTexto : textos.semComputadorNaContaTexto)
                .mxTipo(MX.Tipo.corpo)
                .foregroundStyle(paleta.secundario)
            Button { conectar(dados) } label: {
                Text(dados.atualizando ? textos.procurando : (parear ? textos.conectarUm : textos.procurar))
                    .mxTipo(MX.Tipo.corpoForte)
                    .foregroundStyle(paleta.sobreDestaque)
                    .padding(.horizontal, 20)
                    .frame(minHeight: MX.Espaco.alvoMinimo)
                    .frame(maxWidth: acessivel ? .infinity : nil)
                    .background(paleta.destaque, in: Capsule())
                    .contentShape(Capsule())
            }
            .buttonStyle(MXToque())
            .disabled(dados.atualizando)
            .accessibilityHint(parear ? textos.abreOPareamento : textos.atualizaALista)
        }
        .padding(MX.Conta.contaRespiro)
        .frame(maxWidth: .infinity, alignment: .leading)
        .modifier(MXCartaoDaConta(paleta: paleta))
        .padding(.bottom, MX.Espaco.entreBlocos)
    }

    private func conectar(_ dados: MXContaDados) {
        switch dados.conectar {
        case .parear: destino = .conectar
        case .atualizar: Task { await conta.atualizar() }
        }
    }

    // MARK: Computadores

    private func secao(_ titulo: String) -> some View {
        Text(titulo)
            .mxTipo(MX.Tipo.estadoDoAgente)
            .foregroundStyle(paleta.secundario)
            .padding(.top, MX.Conta.secaoTopo)
            .padding(.horizontal, MX.Conta.secaoLado)
            .padding(.bottom, MX.Conta.secaoBaixo)
            .accessibilityAddTraits(.isHeader)
    }

    private func computadores(_ dados: MXContaDados) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            secao(textos.computadores)
            grupo {
                ForEach(dados.computadores) { linha in
                    linhaDoComputador(linha)
                    separador
                }
                linhaDeConectar(dados)
            }
            if dados.conectar == .atualizar {
                Text(textos.computadorDaContaNota)
                    .mxTipo(MX.Tipo.legenda)
                    .foregroundStyle(paleta.secundario)
                    .padding(.top, MX.Espaco.dentroDeLinha)
                    .padding(.horizontal, MX.Conta.secaoLado)
            }
        }
    }

    @ViewBuilder private func linhaDoComputador(_ linha: MXComputadorLinha) -> some View {
        let agora = Date.now
        let leitura = MXContaRegras.rotuloAcessivel(linha, textos: textos, agora: agora)
        let estado = textos.estado(linha.estado, emUso: linha.emUso, agora: agora)
        let corDoEstado = linha.estado.marca == .ok ? paleta.ok : paleta.secundario
        let acaoEmTexto: String? = switch linha.acao {
        case .pedirAcesso: textos.pedirAcesso
        case .verCodigo: textos.verCodigo
        case .usar, .nenhuma: nil
        }
        let conteudo = HStack(alignment: acessivel ? .top : .center, spacing: MX.Conta.linhaEntre) {
            tileView(.mac)
            VStack(alignment: .leading, spacing: 0) {
                Text(linha.nome).mxTipo(MX.Tipo.corpoForte).foregroundStyle(paleta.texto)
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    MXIconeDaContaView(icone: marca(linha.estado.marca), tamanho: MX.Conta.estadoIcone)
                    Text(estado).mxTipo(MX.Tipo.legendaEstado)
                }
                .foregroundStyle(corDoEstado)
                if acessivel, let acaoEmTexto {
                    Text(acaoEmTexto).mxTipo(MX.Tipo.acaoDaLinha).foregroundStyle(paleta.destaqueTexto).padding(.top, 4)
                }
            }
            Spacer(minLength: 0)
            if !acessivel, let acaoEmTexto {
                Text(acaoEmTexto).mxTipo(MX.Tipo.acaoDaLinha).foregroundStyle(paleta.destaqueTexto)
            } else if linha.acao == .usar || linha.acao == .nenhuma, linha.estado != .copiaAntiga {
                MXIconeDaContaView(icone: linha.emUso ? .ok : .vazio, tamanho: MX.Conta.marca)
                    .foregroundStyle(linha.emUso ? paleta.destaqueTexto : paleta.borda)
                    .padding(.top, acessivel ? 2 : 0)
            }
        }
        .padding(.horizontal, MX.Conta.linhaRespiroH)
        .padding(.vertical, MX.Conta.linhaRespiroV)
        .frame(maxWidth: .infinity, minHeight: alturaDaLinha, alignment: .leading)
        switch linha.acao {
        case .nenhuma:
            conteudo
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(leitura.rotulo)
                .accessibilityValue(leitura.valor)
                .accessibilityAddTraits(linha.emUso ? .isSelected : [])
        case .usar:
            Button {
                conta.usar(linha.id)
                AccessibilityNotification.Announcement(textos.agoraUsando(linha.nome)).post()
            } label: { conteudo.contentShape(Rectangle()) }
                .buttonStyle(MXToque())
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(leitura.rotulo)
                .accessibilityValue(leitura.valor)
                .accessibilityHint(textos.usarEste)
        case .pedirAcesso, .verCodigo:
            Button { destino = .acesso(linha.id) } label: { conteudo.contentShape(Rectangle()) }
                .buttonStyle(MXToque())
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(leitura.rotulo)
                .accessibilityValue(leitura.valor)
                .accessibilityHint(textos.abreOPedido)
        }
    }

    private func marca(_ marca: MXMarcaDoEstado) -> MXIconeDaConta {
        switch marca {
        case .ok: .ok
        case .vazio: .vazio
        case .alerta: .alerta
        }
    }

    private func linhaDeConectar(_ dados: MXContaDados) -> some View {
        let titulo = dados.atualizando ? textos.procurando : textos.conectarOutro
        return Button { conectar(dados) } label: {
            HStack(alignment: acessivel ? .top : .center, spacing: MX.Conta.linhaEntre) {
                tileViewIcone(.mais, tinta: true)
                Text(titulo)
                    .mxTipo(MX.Tipo.corpoForte)
                    .foregroundStyle(paleta.destaqueTexto)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.horizontal, MX.Conta.linhaRespiroH)
            .padding(.vertical, MX.Conta.linhaRespiroV)
            .frame(maxWidth: .infinity, minHeight: alturaDaLinha, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(MXToque())
        .disabled(dados.atualizando)
        .accessibilityLabel(textos.conectarOutro)
        .accessibilityValue(dados.atualizando ? textos.procurando : "")
        .accessibilityHint(dados.conectar == .parear ? textos.abreOPareamento : textos.atualizaALista)
    }

    // MARK: Pessoal e Este iPhone

    private func pessoal(_ dados: MXContaDados) -> some View {
        let vozValor: String = switch dados.voz {
        case .noAparelho: textos.vozNoAparelho
        case let .provedor(nome): nome
        case .indisponivel: textos.precisaDeComputador
        }
        let usoValor: String = switch dados.uso {
        case let .percentual(n): textos.usoValor(n)
        case .semDados: textos.semDados
        case .indisponivel: textos.precisaDeComputador
        }
        return VStack(alignment: .leading, spacing: 0) {
            secao(textos.pessoal)
            grupo {
                linhaDeValor(icone: .voz, titulo: textos.voz, valor: vozValor, destino: .voz, ligada: dados.voz != .indisponivel)
                separador
                // Memória "Sobre você" ainda não tem tela própria (hoje só existe dentro de cada bot): à vista, desligada.
                linhaEstatica(icone: .memoria, titulo: textos.memoria, valor: textos.emBreve)
                separador
                linhaDeValor(icone: .uso, titulo: textos.uso, valor: usoValor, destino: .uso, ligada: dados.uso != .indisponivel)
            }
        }
    }

    private func esteIphone(_ dados: MXContaDados) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            secao(textos.esteIphone)
            grupo {
                if let avisos = dados.avisos {
                    linhaDeAvisos(avisos)
                    separador
                }
                linhaDeValor(icone: .idioma, titulo: textos.idioma, valor: dados.idioma, destino: nil, ligada: true,
                             externa: true, dica: textos.abreOsAjustesDoIphone) { abrirAjustesDoIphone() }
            }
        }
    }

    private func abrirAjustesDoIphone() {
        if let url = URL(string: UIApplication.openSettingsURLString) { abrirURL(url) }
    }

    private var maisAjustes: some View {
        grupo {
            linhaDeValor(icone: .ajustes, titulo: textos.maisAjustes, valor: textos.maisAjustesValor, destino: .maisAjustes,
                         ligada: true, dica: textos.maisAjustesDica)
        }
        .padding(.top, MX.Conta.secaoTopo)
    }

    /// Uma linha com nome, valor e seta (ou o ícone de "abre fora"). Sem `destino` nem `acao`, não faz nada e fica desligada.
    private func linhaDeValor(icone: MXIconeDaConta, titulo: String, valor: String, destino alvo: MXDestinoDaConta?, ligada: Bool,
                              externa: Bool = false, dica: String = "", acao: (() -> Void)? = nil) -> some View {
        let indicador = Group {
            if externa {
                MXIconeDaContaView(icone: .fora, tamanho: MX.Conta.marca - 10).foregroundStyle(paleta.secundario)
            } else {
                MXIconeView(.setaParaDireita, tamanho: 18, traco: MX.Conta.traco).foregroundStyle(paleta.secundario)
            }
        }
        let nome = Text(titulo).mxTipo(MX.Tipo.corpoForte).foregroundStyle(paleta.texto)
        let valorTexto = Text(valor).mxTipo(MX.Tipo.previa).foregroundStyle(paleta.secundario)
        let conteudo = HStack(alignment: acessivel ? .top : .center, spacing: MX.Conta.linhaEntre) {
            tileView(icone)
            if acessivel {
                VStack(alignment: .leading, spacing: 2) { nome; valorTexto }
                Spacer(minLength: 0)
            } else {
                nome
                Spacer(minLength: MX.Espaco.dentroDeLinha)
                valorTexto.multilineTextAlignment(.trailing)
            }
            indicador.padding(.top, acessivel ? 6 : 0)
        }
        .padding(.horizontal, MX.Conta.linhaRespiroH)
        .padding(.vertical, MX.Conta.linhaRespiroV)
        .frame(maxWidth: .infinity, minHeight: alturaDaLinha, alignment: .leading)
        .contentShape(Rectangle())
        return Button {
            if let acao { acao() } else if let alvo { destino = alvo }
        } label: { conteudo }
            .buttonStyle(MXToque())
            .disabled(!ligada || (acao == nil && alvo == nil))
            .opacity(ligada ? 1 : MX.Conversa.desligadoOpacidade)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(titulo)
            .accessibilityValue(valor)
            .accessibilityHint(ligada ? dica : "")
    }

    /// Uma linha só de informação (sem botão, sem seta, sem trait de botão): o que ainda não existe aparece como texto honesto.
    private func linhaEstatica(icone: MXIconeDaConta, titulo: String, valor: String) -> some View {
        let nome = Text(titulo).mxTipo(MX.Tipo.corpoForte).foregroundStyle(paleta.secundario)
        let valorTexto = Text(valor).mxTipo(MX.Tipo.previa).foregroundStyle(paleta.secundario)
        return HStack(alignment: acessivel ? .top : .center, spacing: MX.Conta.linhaEntre) {
            tileView(icone)
            if acessivel {
                VStack(alignment: .leading, spacing: 2) { nome; valorTexto }
                Spacer(minLength: 0)
            } else {
                nome
                Spacer(minLength: MX.Espaco.dentroDeLinha)
                valorTexto.multilineTextAlignment(.trailing)
            }
        }
        .opacity(MX.Conversa.desligadoOpacidade + 0.35)
        .padding(.horizontal, MX.Conta.linhaRespiroH)
        .padding(.vertical, MX.Conta.linhaRespiroV)
        .frame(maxWidth: .infinity, minHeight: alturaDaLinha, alignment: .leading)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(titulo)
        .accessibilityValue(valor)
    }

    private func linhaDeAvisos(_ avisos: MXAvisosDoIphone) -> some View {
        Group {
            if avisos == .negadoNoIphone {
                linhaDeValor(icone: .aviso, titulo: textos.avisos, valor: textos.avisosNegados, destino: nil, ligada: true,
                             externa: true, dica: textos.abreOsAjustesDoIphone) { abrirAjustesDoIphone() }
            } else {
                Toggle(isOn: Binding(get: { avisos == .ligados }, set: { _ in Task { await conta.alternarAvisos() } })) {
                    HStack(alignment: acessivel ? .top : .center, spacing: MX.Conta.linhaEntre) {
                        tileView(.aviso)
                        if acessivel {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(textos.avisos).mxTipo(MX.Tipo.corpoForte).foregroundStyle(paleta.texto)
                                Text(avisos == .ligados ? textos.avisosLigados : textos.avisosDesligados)
                                    .mxTipo(MX.Tipo.previa).foregroundStyle(paleta.secundario)
                            }
                        } else {
                            Text(textos.avisos).mxTipo(MX.Tipo.corpoForte).foregroundStyle(paleta.texto)
                            Spacer(minLength: MX.Espaco.dentroDeLinha)
                            Text(avisos == .ligados ? textos.avisosLigados : textos.avisosDesligados)
                                .mxTipo(MX.Tipo.previa).foregroundStyle(paleta.secundario)
                        }
                    }
                    // O interruptor já diz ligado ou desligado ao VoiceOver; o texto de valor é só visual.
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(textos.avisos)
                }
                .toggleStyle(.switch)
                .tint(paleta.destaque)
                .accessibilityValue(avisos == .ligados ? textos.avisosLigados : textos.avisosDesligados)
                .padding(.horizontal, MX.Conta.linhaRespiroH)
                .padding(.vertical, MX.Conta.linhaRespiroV)
                .frame(maxWidth: .infinity, minHeight: alturaDaLinha, alignment: .leading)
            }
        }
    }

    // MARK: Sair

    private var botaoSair: some View {
        Button { confirmandoSair = true } label: {
            Text(textos.sair)
                .mxTipo(MX.Tipo.corpoForte)
                .foregroundStyle(paleta.erro)
                .frame(maxWidth: .infinity, minHeight: alturaDaLinha)
                .modifier(MXCartaoDaConta(paleta: paleta))
        }
        .buttonStyle(MXToque())
        .padding(.top, MX.Conta.notaTopo)
        .accessibilityHint(textos.sairPergunta(conta.dados.email))
    }

    // MARK: Peças

    private func grupo<Conteudo: View>(@ViewBuilder _ conteudo: () -> Conteudo) -> some View {
        VStack(spacing: 0, content: conteudo)
            .modifier(MXCartaoDaConta(paleta: paleta))
    }

    private var separador: some View {
        Rectangle().fill(paleta.linha).frame(height: 1)
            .padding(.leading, MX.Conta.linhaRespiroH * 2 + tile)
    }

    private func tileView(_ icone: MXIconeDaConta, tinta: Bool = false) -> some View {
        MXIconeDaContaView(icone: icone, tamanho: MX.Conta.tileIcone)
            .foregroundStyle(tinta ? paleta.destaqueTexto : paleta.texto)
            .frame(width: tile, height: tile)
            .background(tinta ? paleta.tintaDoDestaque : paleta.tile, in: RoundedRectangle(cornerRadius: MX.Conta.tileRaio))
    }

    private func tileViewIcone(_ icone: MXIcone, tinta: Bool = false) -> some View {
        MXIconeView(icone, tamanho: MX.Conta.tileIcone, traco: MX.Conta.traco)
            .foregroundStyle(tinta ? paleta.destaqueTexto : paleta.texto)
            .frame(width: tile, height: tile)
            .background(tinta ? paleta.tintaDoDestaque : paleta.tile, in: RoundedRectangle(cornerRadius: MX.Conta.tileRaio))
    }
}

/// Cartão branco (claro) ou grafite (escuro) com contorno fino, como o `.grupo` do mockup.
struct MXCartaoDaConta: ViewModifier {
    let paleta: MXPaletaDaConta

    func body(content: Content) -> some View {
        let forma = RoundedRectangle(cornerRadius: MX.Conta.cartaoRaio)
        content
            .background(paleta.cartao, in: forma)
            .overlay(forma.stroke(paleta.linha, lineWidth: MX.Vidro.contornoOpacoEspessura))
            .clipShape(forma)
    }
}

/// O botão redondo de voltar: vidro no claro, grafite chapado no escuro (sem vidro, sem papel de parede).
struct MXBotaoRedondoDaConta: ViewModifier {
    let paleta: MXPaletaDaConta

    func body(content: Content) -> some View {
        if paleta.escuro {
            content
                .background(paleta.tile, in: Circle())
                .overlay(Circle().stroke(paleta.linha, lineWidth: MX.Vidro.contornoOpacoEspessura))
        } else {
            content.mxVidro(.botao, em: Circle(), interativo: true)
        }
    }
}

extension MXContaRegras {
    /// O e-mail com uma quebra permitida depois do `@`: em texto grande ele não corta no meio do endereço.
    static func quebravel(_ email: String) -> String {
        email.replacingOccurrences(of: "@", with: "@\u{200B}")
    }
}
#endif
