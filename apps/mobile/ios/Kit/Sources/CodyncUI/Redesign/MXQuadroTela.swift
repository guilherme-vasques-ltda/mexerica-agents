#if DEBUG
import CodyncKit
import SwiftUI

// Mission Board v1 (PLANO §4.2; mockups `mock-ios-mission-board-*`). Só Debug, atrás do interruptor "Novo visual (em teste)".
// A tela só lê `MXQuadroDados` e chama `abrir`: nenhuma regra de negócio mora aqui.

/// O que o quadro faz quando tocam nele. Abrir a conversa é a única ação sobre um bot: nada responde, aprova ou envia daqui.
struct MXQuadroAcoes {
    var abrir: (String) -> Void = { _ in }
    var abrirAjustes: () -> Void = {}
}

public extension MX {
    /// Medidas do Mission Board. Cores e tipos: só os tokens que já existem (os três tokens propostos esperam a issue #37).
    enum Quadro {
        public static let blocoAltura: CGFloat = 72
        public static let blocoEntre: CGFloat = 8
        public static let blocoRaio: CGFloat = 20
        public static let cartaoContorno: CGFloat = 1.5
        public static let cartaoPelucia: CGFloat = 52
        public static let linhaPelucia: CGFloat = 44
        public static let linhaAlturaMinima: CGFloat = 64
        public static let botaoAltura: CGFloat = 44
        public static let secaoTopo: CGFloat = 16
        public static let secaoBaixo: CGFloat = 8
        public static let gomo: CGFloat = 24
        public static let livresAvatar: CGFloat = 36
        public static let livresSobreposicao: CGFloat = 12
        public static let mascaraDaBarra: CGFloat = 110
        /// A partir daqui (um passo antes dos tamanhos de acessibilidade) os quatro blocos viram "Filtrar: …": o rótulo
        /// "Trabalhando" não cabe num quarto da largura sem encolher abaixo do piso.
        public static var filtroCompactoDe: DynamicTypeSize { .xxLarge }
    }
}

extension MXQuadroRegras {
    /// Os quatro blocos viram um botão de filtro com o texto grande.
    static func filtroCompacto(_ tamanho: DynamicTypeSize) -> Bool { tamanho >= MX.Quadro.filtroCompactoDe }
}

struct MXQuadroTela: View {
    let fonte: any MXQuadroFonte
    let esquema: ColorScheme
    let acoes: MXQuadroAcoes
    /// Os Ajustes e a conta existem nesta casca (senão o botão da conta fica desligado).
    var contaLigada = true
    /// O idioma da tela (o do app; os testes de imagem fixam o pt-BR).
    var textos = MXTextosDoQuadro()
    /// Sem `ScrollView` nem máscara: só para as imagens de teste, porque o `ImageRenderer` não desenha o que fica dentro de uma.
    var semRolagem = false
    /// O relógio da tela (os testes de imagem fixam).
    var relogio: Date?
    @State var filtro: MXQuadroFiltro = .todos
    @State var livresAbertos = false
    @Environment(\.dynamicTypeSize) private var tamanhoDoTexto
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento

    private var acessivel: Bool { tamanhoDoTexto.isAccessibilitySize }
    private var paleta: MXPaletaDaConta { MXPaletaDaConta(esquema) }

    var body: some View {
        if let relogio {
            // Relógio fixo (testes e imagens): sem `TimelineView`, dentro do qual o `ImageRenderer` não desenha o vidro.
            conteudo(agora: relogio)
        } else {
            // "há 3 min" se refaz sozinho a cada 30 s.
            TimelineView(.periodic(from: .now, by: 30)) { contexto in conteudo(agora: contexto.date) }
        }
    }

    private func conteudo(agora: Date) -> some View {
        let dados = fonte.quadro
        return VStack(spacing: 0) {
            topo(dados)
            if dados.mostraBots {
                if let faixa = faixaDeQueda(dados, agora: agora) { faixa }
                controleDeFiltro(dados)
            }
            painel(dados, agora: agora)
        }
        .background { fundo }
        .environment(\.colorScheme, esquema)
    }

    @ViewBuilder private var fundo: some View {
        if paleta.escuro { paleta.fundo.ignoresSafeArea() } else { MXFundo() }
    }

    // MARK: Topo

    private func topo(_ dados: MXQuadroDados) -> some View {
        let botao = Button { acoes.abrirAjustes() } label: {
            MXIconeView(.conta, tamanho: MX.Medida.botaoRedondoIcone)
                .foregroundStyle(paleta.texto)
                .frame(width: MX.Medida.botaoRedondo, height: MX.Medida.botaoRedondo)
                .modifier(MXBotaoRedondoDaConta(paleta: paleta))
        }
        .buttonStyle(MXToque())
        .mxIndisponivel(!contaLigada, dica: textos.abreOsAjustes)
        .accessibilityLabel(textos.ajustesEConta)
        let titulo = Text(textos.titulo)
            .mxTipo(MX.Tipo.tituloDeTela)
            .foregroundStyle(paleta.texto)
            .accessibilityAddTraits(.isHeader)
        let layout = acessivel ? AnyLayout(VStackLayout(alignment: .leading, spacing: MX.Espaco.dentroDeLinha))
                               : AnyLayout(HStackLayout(spacing: MX.Espaco.dentroDeLinha + 2))
        return layout {
            botao
            VStack(alignment: .leading, spacing: 0) {
                titulo
                if let sub = subtitulo(dados) { sub }
            }
            if !acessivel { Spacer(minLength: 0) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, MX.Espaco.margemTela + MX.Medida.topoRespiroLateral)
        .padding(.top, MX.Medida.topoAbaixoDaAreaSegura)
        .padding(.bottom, MX.Espaco.dentroDeLinha + 2)
    }

    /// "● Ao vivo · MacBook de exemplo": o estado vai em texto, não só na cor do ponto.
    private func subtitulo(_ dados: MXQuadroDados) -> AnyView? {
        guard dados.estado != .semComputador else { return nil }
        let (texto, vivo): (String, Bool) = switch dados.ligacao {
        case .aoVivo: (dados.atualizando ? textos.atualizando(dados.computador) : textos.aoVivo(dados.computador), !dados.atualizando)
        case .reconectando: (textos.reconectando(dados.computador), false)
        case .parado: (textos.semConexao(dados.computador), false)
        }
        return AnyView(HStack(spacing: 6) {
            Circle().fill(vivo ? paleta.ok : paleta.secundario).frame(width: MX.Medida.pontoDeEstado, height: MX.Medida.pontoDeEstado)
                .accessibilityHidden(true)
            Text(texto).mxTipo(MX.Tipo.legendaEstado).foregroundStyle(paleta.secundario)
        }
        .accessibilityElement(children: .combine))
    }

    // MARK: Queda: dados parados, com aviso

    private func faixaDeQueda(_ dados: MXQuadroDados, agora: Date) -> AnyView? {
        let visto: String?
        switch dados.ligacao {
        case .aoVivo: return nil
        case let .reconectando(desde): visto = textos.visto(desde: desde, agora: agora)
        case .parado: visto = textos.paradosSemHora
        }
        let corpo = VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                MXIconeDaContaView(icone: .alerta, tamanho: MX.Conta.estadoIcone + 2).foregroundStyle(paleta.erro)
                Text(textos.paradosTitulo).mxTipo(MX.Tipo.corpoForte).foregroundStyle(paleta.texto)
            }
            Text(textos.paradosTexto(visto: visto)).mxTipo(MX.Tipo.previa).foregroundStyle(paleta.secundario)
            if fonte.podeTentarAgora {
                Button { fonte.tentarAgora() } label: {
                    Text(textos.tentarAgora)
                        .mxTipo(MX.Tipo.acaoDaLinha)
                        .foregroundStyle(paleta.destaqueTexto)
                        .frame(minWidth: MX.Espaco.alvoMinimo, minHeight: MX.Espaco.alvoMinimo, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(MXToque())
            }
        }
        .padding(MX.Conta.contaRespiro)
        .frame(maxWidth: .infinity, alignment: .leading)
        .modifier(MXCartaoDaConta(paleta: paleta))
        .padding(.horizontal, MX.Espaco.margemTela)
        .padding(.bottom, MX.Espaco.entreBlocos)
        .accessibilityElement(children: .contain)
        return AnyView(corpo)
    }

    // MARK: Blocos e filtro

    private func controleDeFiltro(_ dados: MXQuadroDados) -> some View {
        Group {
            if MXQuadroRegras.filtroCompacto(tamanhoDoTexto) {
                filtroEmMenu(dados)
            } else {
                HStack(spacing: MX.Quadro.blocoEntre) {
                    bloco(.voce, textos.voce, dados.voce.count, cor: paleta.destaqueTexto)
                    bloco(.trabalhando, textos.trabalhando, dados.trabalhando.count, cor: paleta.ok)
                    bloco(.problema, textos.problema, dados.problema.count, cor: paleta.erro)
                    bloco(.livres, textos.livres, dados.livres.count, cor: paleta.secundario)
                }
            }
        }
        .padding(.horizontal, MX.Espaco.margemTela)
        .padding(.bottom, MX.Espaco.entreBlocos)
    }

    private func bloco(_ alvo: MXQuadroFiltro, _ nome: String, _ n: Int, cor: Color) -> some View {
        let ativo = filtro == alvo
        let forma = RoundedRectangle(cornerRadius: MX.Quadro.blocoRaio)
        return Button { filtro = MXQuadroRegras.alternar(filtro, alvo) } label: {
            VStack(alignment: .leading, spacing: 0) {
                Text(textos.blocoValor(n)).mxTipo(MX.Tipo.display).foregroundStyle(cor)
                Text(nome).mxTipo(MX.Tipo.legendaForte).foregroundStyle(cor).lineLimit(1).minimumScaleFactor(0.8)
            }
            .padding(.horizontal, 10)
            .frame(maxWidth: .infinity, minHeight: MX.Quadro.blocoAltura, alignment: .leading)
            .background(paleta.cartao, in: forma)
            .overlay(forma.stroke(ativo ? paleta.destaque : paleta.linha, lineWidth: ativo ? 2 : MX.Vidro.contornoOpacoEspessura))
            .contentShape(forma)
        }
        .buttonStyle(MXToque())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(nome)
        .accessibilityValue(textos.blocoValor(n))
        .accessibilityHint(textos.dicaDoFiltro)
        .accessibilityAddTraits(ativo ? .isSelected : [])
    }

    /// Texto grande: os quatro blocos viram um botão; a contagem passa para o título de cada seção.
    private func filtroEmMenu(_ dados: MXQuadroDados) -> some View {
        let forma = RoundedRectangle(cornerRadius: MX.Quadro.blocoRaio)
        let opcoes: [(MXQuadroFiltro, String)] = [(.todos, textos.mostrarTodos), (.voce, textos.voce), (.trabalhando, textos.trabalhando),
                                                   (.problema, textos.problema), (.livres, textos.livres)]
        let rotulo = filtro == .todos ? textos.filtrarTodos : textos.filtrar(textos.nomeDoFiltro(filtro))
        let rotuloVisual = HStack(spacing: MX.Espaco.dentroDeLinha) {
            Text(rotulo).mxTipo(MX.Tipo.nomeNoCartao).foregroundStyle(paleta.texto)
                .frame(maxWidth: .infinity, alignment: .leading)
            MXIconeDaContaView(icone: .ajustes, tamanho: 24).foregroundStyle(paleta.secundario)
        }
        .padding(.horizontal, MX.Conta.contaRespiro + 4)
        .frame(maxWidth: .infinity, minHeight: MX.Quadro.blocoAltura - 12, alignment: .leading)
        .background(paleta.cartao, in: forma)
        .overlay(forma.stroke(paleta.linha, lineWidth: MX.Vidro.contornoOpacoEspessura))
        .contentShape(forma)
        return Group {
            if semRolagem {
                // Só nas imagens de teste: o `ImageRenderer` não desenha um `Menu` (UIKit); mostra o mesmo botão, sem o menu.
                rotuloVisual
            } else {
                Menu {
                    Picker(rotulo, selection: $filtro) {
                        ForEach(opcoes, id: \.0) { opcao in
                            Text(opcao.0 == .todos ? opcao.1 : "\(opcao.1) · \(dados.itens(opcao.0))").tag(opcao.0)
                        }
                    }
                } label: { rotuloVisual }
            }
        }
        .accessibilityLabel(rotulo)
        .accessibilityHint(textos.dicaDoMenuDeFiltro)
    }

    // MARK: Painel

    private func painel(_ dados: MXQuadroDados, agora: Date) -> some View {
        let forma = UnevenRoundedRectangle(topLeadingRadius: MX.Conta.painelRaio, topTrailingRadius: MX.Conta.painelRaio)
        let lista = VStack(alignment: .leading, spacing: 0) {
            switch dados.estado {
            case .pronto: pronto(dados, agora: agora)
            case .carregando: estado(textos.carregandoTitulo, textos.carregandoTexto)
            case .semComputador:
                estado(textos.semComputadorTitulo, textos.semComputadorTexto, botao: textos.conectarUm, dica: textos.abreOsAjustes)
            case .semAcesso:
                estado(textos.semAcessoTitulo, textos.semAcessoTexto, botao: textos.abrirAjustes, dica: textos.abreOsAjustes)
            case .erro: estado(textos.erroTitulo, textos.erroTexto)
            }
        }
        .padding(MX.Conta.painelRespiro)
        .padding(.bottom, semRolagem ? 0 : MX.Medida.painelFolgaFinal)
        let conteudo = Group {
            if semRolagem {
                lista.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            } else {
                ScrollView { lista }
                    .scrollIndicators(.hidden)
                    .mask(alignment: .top) { mascaraDaBarra }
            }
        }
        return conteudo
            .background { fundoDoPainel(forma) }
            .clipShape(forma)
            .padding(.horizontal, MX.Espaco.margemTela)
            .ignoresSafeArea(edges: .bottom)
    }

    /// O que passa sob a barra de abas some (a borda de rolagem do desenho).
    private var mascaraDaBarra: some View {
        VStack(spacing: 0) {
            Rectangle().fill(Color.black)
            LinearGradient(colors: [.black, .clear], startPoint: .top, endPoint: .bottom).frame(height: MX.Quadro.mascaraDaBarra)
        }
        .ignoresSafeArea()
    }

    @ViewBuilder private func fundoDoPainel(_ forma: UnevenRoundedRectangle) -> some View {
        if paleta.escuro {
            forma.fill(paleta.painel).overlay(forma.stroke(paleta.linha, lineWidth: MX.Vidro.contornoOpacoEspessura))
        } else {
            Color.clear.mxVidro(.painel, em: forma)
        }
    }

    // MARK: Pronto

    @ViewBuilder private func pronto(_ dados: MXQuadroDados, agora: Date) -> some View {
        if MXQuadroRegras.ressalva(dados) != nil { cobertura(dados) }
        if MXQuadroRegras.mostra(.voce, com: filtro) {
            switch MXQuadroRegras.vazio(dados) {
            case .semBots:
                semItens(textos.semBotsTitulo, textos.semBotsTexto)
            case .semPedidos:
                secao(textos.secaoVoce, 0, cor: paleta.destaqueTexto)
                semItens(textos.vazioTitulo, textos.vazioTexto)
            case nil:
                secao(textos.secaoVoce, dados.voce.count, cor: paleta.destaqueTexto)
                ForEach(dados.voce) { item in cartao(item, agora: agora) }
                avisoDoAbrir
            }
        }
        if MXQuadroRegras.mostra(.trabalhando, com: filtro), !dados.trabalhando.isEmpty {
            secao(textos.secaoTrabalhando, dados.trabalhando.count, cor: paleta.ok)
            ForEach(dados.trabalhando) { item in linhaTrabalhando(item, agora: agora) }
        }
        if MXQuadroRegras.mostra(.problema, com: filtro), !dados.problema.isEmpty {
            secao(textos.secaoProblema, dados.problema.count, cor: paleta.erro)
            ForEach(dados.problema) { item in linhaDeProblema(item) }
        }
        if MXQuadroRegras.mostra(.livres, com: filtro), !dados.livres.isEmpty {
            livres(dados.livres)
        }
    }

    /// A ressalva de cobertura parcial: SEMPRE à vista em `.pronto`, com a lista cheia ou vazia (nunca "zero global").
    private func cobertura(_ dados: MXQuadroDados) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                MXIconeDaContaView(icone: .info, tamanho: MX.Conta.estadoIcone + 2).foregroundStyle(paleta.secundario)
                Text(textos.coberturaTitulo).mxTipo(MX.Tipo.legendaForte).foregroundStyle(paleta.secundario)
            }
            Text(textos.cobertura).mxTipo(MX.Tipo.legenda).foregroundStyle(paleta.secundario)
            if dados.atualizando {
                Text(textos.coberturaCarregando).mxTipo(MX.Tipo.legenda).foregroundStyle(paleta.secundario)
            }
        }
        .padding(.horizontal, MX.Conta.secaoLado)
        .padding(.top, 4)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(textos.coberturaTitulo)
        .accessibilityValue(dados.atualizando ? "\(textos.cobertura) \(textos.coberturaCarregando)" : textos.cobertura)
    }

    private var avisoDoAbrir: some View {
        Text(textos.dicaDoAbrir)
            .mxTipo(MX.Tipo.legenda)
            .foregroundStyle(paleta.secundario)
            .padding(.horizontal, MX.Conta.secaoLado)
            .padding(.top, 6)
            .accessibilityHidden(true)
    }

    private func secao(_ titulo: String, _ n: Int, cor: Color) -> some View {
        Text(textos.secaoComContagem(titulo, n))
            .mxTipo(MX.Tipo.corpoForte)
            .foregroundStyle(cor)
            .padding(.top, MX.Quadro.secaoTopo)
            .padding(.horizontal, MX.Conta.secaoLado)
            .padding(.bottom, MX.Quadro.secaoBaixo)
            .accessibilityLabel("\(titulo), \(n)")
            .accessibilityAddTraits(.isHeader)
    }

    /// Pronto e sem nada à vista: diz o que é (neste iPhone) e não promete que não há mais (cobertura parcial).
    private func semItens(_ titulo: String, _ texto: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(titulo).mxTipo(MX.Tipo.nomeNoCartao).foregroundStyle(paleta.texto)
            Text(texto).mxTipo(MX.Tipo.corpo).foregroundStyle(paleta.secundario)
        }
        .padding(MX.Conta.contaRespiro + 4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .modifier(MXCartaoDaConta(paleta: paleta))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(titulo)
        .accessibilityValue(texto)
    }

    // MARK: Cartão "Precisa de você"

    private func cartao(_ item: MXQuadroItem, agora: Date) -> some View {
        let motivo = item.motivo ?? .espera
        let texto: String = switch motivo {
        case .permissao: textos.pedePermissao
        case .pergunta: textos.temPergunta
        case .espera: textos.esperaVoce
        }
        let quando = item.desde.map { textos.ha($0, agora: agora) }
        let forma = RoundedRectangle(cornerRadius: MX.Conversa.cartaoRaio)
        let nome = Text(item.nome).mxTipo(MX.Tipo.nomeNoCartao).foregroundStyle(paleta.texto)
        let hora = quando.map { Text($0).mxTipo(MX.Tipo.legenda).foregroundStyle(paleta.secundario) }
        let pelucia = MXPelucia(rosto: item.rosto, estado: item.estado, tamanho: MX.Quadro.cartaoPelucia, comSelos: false)
        let corpo = Group {
            if acessivel {
                VStack(alignment: .leading, spacing: MX.Espaco.dentroDeLinha) {
                    pelucia
                    nome
                    if let hora { hora }
                    Text(texto).mxTipo(MX.Tipo.corpo).foregroundStyle(paleta.texto)
                    pilula(textos.abrir, cheia: true)
                }
            } else {
                HStack(alignment: .top, spacing: MX.Conta.linhaEntre) {
                    pelucia
                    VStack(alignment: .leading, spacing: 4) {
                        HStack(alignment: .firstTextBaseline) {
                            nome
                            Spacer(minLength: 4)
                            if let hora { hora }
                        }
                        Text(texto).mxTipo(MX.Tipo.corpo).foregroundStyle(paleta.texto)
                        pilula(textos.abrir, cheia: true).padding(.top, 4)
                    }
                }
            }
        }
        .padding(MX.Conversa.cartaoRespiro)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(paleta.cartao, in: forma)
        .overlay(forma.stroke(paleta.destaque, lineWidth: MX.Quadro.cartaoContorno))
        return Button { acoes.abrir(item.id) } label: { corpo.contentShape(forma) }
            .buttonStyle(MXToque())
            .padding(.bottom, MX.Espaco.entreBlocos)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(textos.leituraDoCartao(nome: item.nome, motivo: motivo, desde: item.desde, agora: agora))
            .accessibilityHint(textos.abrirConversa(item.nome) + ". " + textos.dicaDoAbrir)
            .accessibilityInputLabels([textos.abrirConversa(item.nome), item.nome])
    }

    /// O "botão" desenhado dentro de uma linha que já é um botão (um alvo só, não dois).
    private func pilula(_ titulo: String, cheia: Bool) -> some View {
        Text(titulo)
            .mxTipo(MX.Tipo.botaoGrande)
            .foregroundStyle(cheia ? paleta.sobreDestaque : paleta.texto)
            .padding(.horizontal, 28)
            .frame(minHeight: MX.Quadro.botaoAltura)
            .background {
                if cheia { Capsule().fill(paleta.destaque) } else { Capsule().stroke(paleta.borda, lineWidth: 1.5) }
            }
            .accessibilityHidden(true)
    }

    // MARK: Linhas

    private func linhaTrabalhando(_ item: MXQuadroItem, agora: Date) -> some View {
        let duracao = item.desde.map { textos.duracao($0, agora: agora) }
        let nome = Text(item.nome).mxTipo(MX.Tipo.sessaoPrincipal).foregroundStyle(paleta.texto)
        let detalhe = Text(item.detalhe.isEmpty ? textos.trabalhandoRotulo : item.detalhe).mxTipo(MX.Tipo.previa)
            .foregroundStyle(paleta.secundario).lineLimit(acessivel ? nil : 1)
        let relogio = HStack(spacing: 8) {
            if let duracao { Text(duracao).mxTipo(MX.Tipo.previa).foregroundStyle(paleta.secundario) }
            MXGomo(tamanho: MX.Quadro.gomo)
        }
        return linha(item, leitura: textos.leituraDoTrabalho(nome: item.nome, detalhe: item.detalhe, desde: item.desde, agora: agora)) {
            VStack(alignment: .leading, spacing: 0) { nome; detalhe }
        } fim: { relogio }
    }

    private func linhaDeProblema(_ item: MXQuadroItem) -> some View {
        let erro = HStack(alignment: .firstTextBaseline, spacing: 6) {
            MXIconeDaContaView(icone: .alerta, tamanho: MX.Conta.estadoIcone + 2).foregroundStyle(paleta.erro)
            Text(textos.parouComErro).mxTipo(MX.Tipo.previa).foregroundStyle(paleta.erro)
        }
        return linha(item, leitura: textos.leituraDoProblema(nome: item.nome)) {
            VStack(alignment: .leading, spacing: 0) {
                Text(item.nome).mxTipo(MX.Tipo.sessaoPrincipal).foregroundStyle(paleta.texto)
                erro
            }
        } fim: { pilula(textos.abrir, cheia: false) }
    }

    private func linhaDeLivre(_ item: MXQuadroItem) -> some View {
        let estado = item.desconhecido ? textos.estadoNaoInformado : (item.detalhe.isEmpty ? textos.semMensagens : item.detalhe)
        return linha(item, leitura: textos.leituraDoLivre(nome: item.nome, detalhe: item.detalhe, desconhecido: item.desconhecido)) {
            VStack(alignment: .leading, spacing: 0) {
                Text(item.nome).mxTipo(MX.Tipo.sessaoPrincipal).foregroundStyle(paleta.texto)
                Text(estado).mxTipo(MX.Tipo.previa).foregroundStyle(paleta.secundario).lineLimit(acessivel ? nil : 1)
            }
        } fim: { EmptyView() }
    }

    /// Uma linha de bot que abre a conversa: pelúcia, texto, e algo no fim (que no texto grande desce para baixo do texto).
    private func linha<Texto: View, Fim: View>(_ item: MXQuadroItem, leitura: String, @ViewBuilder texto: () -> Texto,
                                              @ViewBuilder fim: () -> Fim) -> some View {
        let pelucia = MXPelucia(rosto: item.rosto, estado: item.estado, tamanho: MX.Quadro.linhaPelucia, comSelos: false)
        let corpo = Group {
            if acessivel {
                VStack(alignment: .leading, spacing: MX.Espaco.dentroDeLinha) {
                    pelucia
                    texto()
                    fim()
                }
            } else {
                HStack(spacing: MX.Conta.linhaEntre) {
                    pelucia
                    texto().frame(maxWidth: .infinity, alignment: .leading)
                    fim()
                }
            }
        }
        .padding(.horizontal, MX.Conta.secaoLado)
        .padding(.vertical, MX.Conta.linhaRespiroV)
        .frame(maxWidth: .infinity, minHeight: MX.Quadro.linhaAlturaMinima, alignment: .leading)
        return Button { acoes.abrir(item.id) } label: { corpo.contentShape(Rectangle()) }
            .buttonStyle(MXToque())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(leitura)
            .accessibilityHint(textos.abrirConversa(item.nome))
            .accessibilityInputLabels([textos.abrirConversa(item.nome), item.nome])
    }

    // MARK: Livres (recolhido por padrão)

    private func livres(_ itens: [MXQuadroItem]) -> some View {
        let forma = RoundedRectangle(cornerRadius: MX.Quadro.blocoRaio)
        let primeiros = Array(itens.prefix(3))
        let cabecalho = Button { alternarLivres() } label: {
            HStack(spacing: MX.Conta.linhaEntre) {
                if !acessivel {
                    HStack(spacing: -MX.Quadro.livresSobreposicao) {
                        ForEach(primeiros) { item in
                            MXPelucia(rosto: item.rosto, estado: item.estado, tamanho: MX.Quadro.livresAvatar, comSelos: false)
                                .background(paleta.cartao, in: Circle())
                        }
                    }
                }
                Text(textos.secaoComContagem(textos.livres, itens.count))
                    .mxTipo(MX.Tipo.corpoForte)
                    .foregroundStyle(paleta.secundario)
                    .frame(maxWidth: .infinity, alignment: .leading)
                // Uma seta só: gira com a mola; com Reduzir movimento só troca de lado, sem giro (`MXTroca.interpola`).
                MXIconeView(.setaParaBaixo, tamanho: 18, traco: MX.Conta.traco)
                    .foregroundStyle(paleta.secundario)
                    .rotationEffect(.degrees(MXQuadroRegras.anguloDaSetaDosLivres(aberto: livresAbertos)))
            }
            .padding(.horizontal, MX.Conta.contaRespiro)
            .frame(maxWidth: .infinity, minHeight: MX.Quadro.linhaAlturaMinima, alignment: .leading)
            .background(paleta.tile, in: forma)
            .contentShape(forma)
        }
        .buttonStyle(MXToque())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(livresAbertos ? textos.livresAberto(itens.count) : textos.livresRecolhido(itens.count))
        .accessibilityHint(livresAbertos ? textos.dicaLivresFechar : textos.dicaLivresAbrir)
        return VStack(alignment: .leading, spacing: 0) {
            cabecalho.padding(.top, MX.Quadro.secaoTopo)
            if livresAbertos {
                // Com Reduzir movimento as linhas entram só por opacidade; com a mola, descem junto com o layout.
                ForEach(itens) { linhaDeLivre($0).transition(.opacity.animation(MX.Movimento.troca(reduzir: reduzirMovimento).animacao)) }
            }
        }
    }

    /// Abre ou recolhe Livres: com a mola, anima posição e giro; com Reduzir movimento, o layout pula e só a opacidade anima.
    private func alternarLivres() {
        if let animacao = MXQuadroRegras.animacaoDaPosicaoDosLivres(reduzir: reduzirMovimento) {
            withAnimation(animacao) { livresAbertos.toggle() }
        } else {
            livresAbertos.toggle()
        }
    }

    // MARK: Estados sem lista

    private func estado(_ titulo: String, _ texto: String, botao: String? = nil, dica: String = "") -> some View {
        VStack(alignment: .leading, spacing: MX.Conta.linhaEntre) {
            Text(titulo).mxTipo(MX.Tipo.nomeNoCartao).foregroundStyle(paleta.texto).accessibilityAddTraits(.isHeader)
            Text(texto).mxTipo(MX.Tipo.corpo).foregroundStyle(paleta.secundario)
            if let botao {
                Button { acoes.abrirAjustes() } label: {
                    Text(botao)
                        .mxTipo(MX.Tipo.corpoForte)
                        .foregroundStyle(paleta.sobreDestaque)
                        .padding(.horizontal, 20)
                        .frame(minHeight: MX.Espaco.alvoMinimo)
                        .frame(maxWidth: acessivel ? .infinity : nil)
                        .background(paleta.destaque, in: Capsule())
                        .contentShape(Capsule())
                }
                .buttonStyle(MXToque())
                .mxIndisponivel(!contaLigada, dica: dica)
            }
        }
        .padding(MX.Conta.contaRespiro + 4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .modifier(MXCartaoDaConta(paleta: paleta))
        .padding(.top, MX.Espaco.entreBlocos)
        .accessibilityElement(children: .contain)
    }
}
#endif
