import CoreText
import Foundation
import SwiftUI
import Testing
import UIKit
@testable import CodyncKit
@testable import CodyncUI

/// Texto do catálogo do CodyncUI no idioma em que o teste roda (o mesmo que o código usa): a comparação não depende do
/// idioma do aparelho ou do simulador. Interpolações entram como no código (`\(2) passos`), para achar a chave certa.
func catalogo(_ chave: String.LocalizationValue) -> String {
    String(localized: chave, bundle: ConnectionSummary.resourceBundle)
}

/// Etapa 1 do redesenho iOS v1: o que não é visual (tokens, regras da pelúcia, contadores, gomo, fontes).
struct RedesignTokensTests {
    @Test func coresDoClaroBatemComOJSON() {
        let esperado: [(MXCor, String)] = [
            (MX.Claro.fundo, "#F6EEDC"), (MX.Claro.texto, "#2A1C14"), (MX.Claro.textoSecundario, "#6E5A4C"),
            (MX.Claro.borda, "#94785F"), (MX.Claro.mexerica, "#F58A1F"), (MX.Claro.mexericaForte, "#DD6F10"),
            (MX.Claro.mexericaTexto, "#B4530A"), (MX.Claro.sucesso, "#5C9B3A"), (MX.Claro.sucessoTexto, "#3F7A26"),
            (MX.Claro.erro, "#D92D20"), (MX.Claro.erroTexto, "#B42318"), (MX.Claro.esperaTexto, "#6B4E00"),
            (MX.Claro.rodandoTexto, "#3F7A26"), (MX.Claro.gomo, "#FFC170"), (MX.Claro.balaoDoUsuario, "#F58A1F"),
        ]
        for (cor, hex) in esperado {
            #expect(cor.hex == hex)
            #expect(cor.a == 1)
        }
        #expect(MX.Claro.linha.hex == "#2A1C14" && MX.Claro.linha.a == 0.08)
        #expect(MX.Claro.esperaFundo.hex == "#F5C518" && MX.Claro.esperaFundo.a == 0.26)
        #expect(MX.Claro.rodandoFundo.hex == "#5C9B3A" && MX.Claro.rodandoFundo.a == 0.16)
        #expect(MX.Claro.balaoDoAgente.hex == "#FFFFFF" && MX.Claro.balaoDoAgente.a == 0.88)
    }

    @Test func escuroDeclarado() {
        #expect(MX.Escuro.fundo.hex == "#1E1E21")
        #expect(MX.Escuro.superficie.hex == "#2D2D32")
        #expect(MX.Escuro.texto.hex == "#F5F5F7")
        #expect(MX.Escuro.mexericaTexto.hex == "#FFA24D")
        #expect(MX.Escuro.mexerica == MX.Claro.mexerica) // destaques sempre em laranja
    }

    @Test func textoSobreLaranjaEhMarromNuncaBranco() {
        #expect(MX.Claro.sobreMexerica == MX.Claro.texto)
        #expect(MX.Claro.sobreMexerica.hex == "#2A1C14")
        #expect(MX.Claro.sobreMexerica.hex != "#FFFFFF")
    }

    @Test func raiosEspacosEMovimento() {
        #expect([MX.Raio.chip, MX.Raio.linha, MX.Raio.cartao, MX.Raio.painel, MX.Raio.folha, MX.Raio.capsula] == [12, 14, 18, 22, 28, 999])
        #expect([MX.Espaco.margemTela, MX.Espaco.entreBlocos, MX.Espaco.dentroDeLinha, MX.Espaco.alvoMinimo] == [12, 10, 8, 44])
        #expect(MX.Movimento.molaPadrao == MXMola(amortecimento: 1.0, resposta: 0.35))
        #expect(MX.Movimento.molaFolha == MXMola(amortecimento: 0.86, resposta: 0.32))
        #expect(MX.Movimento.molaPuxar == MXMola(amortecimento: 0.8, resposta: 0.3))
        #expect(MX.Movimento.toqueEscala == 0.97 && MX.Movimento.toqueDuracao == 0.1)
        #expect(MX.Movimento.gomoQuantidade == 8 && MX.Movimento.gomoCiclo == 1.2)
        #expect(MX.Movimento.gomoAtraso == 0.15 && MX.Movimento.gomoVolta == 8)
        #expect(MX.Movimento.peluciaCiclo == 1.8 && MX.Movimento.peluciaRotacao == 2)
        #expect(MX.Movimento.peluciaEscalaX == 1.03 && MX.Movimento.peluciaEscalaY == 0.97)
        #expect(MX.Movimento.segurarParaAjustes == 0.55)
        #expect(MX.Movimento.puxarParaSessoesLimiar == 80 && MX.Movimento.puxarParaSessoesMaximo == 150)
        #expect(MX.Movimento.puxarSessaoLimiar == 70 && MX.Movimento.voltarArrastandoLimiar == 90)
        #expect(MX.Medida.barraAbasDaBorda == 26)
    }

    @Test func vidroOpacoEhACorDoProtótipoPousadaNoCreme() {
        // rgba(255,255,255,0.72) sobre #F6EEDC.
        #expect(MX.Vidro.opacoBotao.hex == "#FCFAF5")
        #expect(MX.Vidro.opacoBotao.a == 1)
        #expect(MX.Vidro.opacoBarra.a == 1 && MX.Vidro.opacoCampo.a == 1)
    }
}

struct RedesignPeluciaTests {
    @Test(arguments: [
        (MexeriquinhaState.livre, "livre-1"), (.trabalhando, "trabalhando-1"), (.precisaResposta, "precisa-resposta"),
        (.dormindo, "dormindo-1"), (.erro, "erro"),
    ])
    func estadoParaQuadro(estado: MexeriquinhaState, quadro: String) {
        #expect(MXPeluciaRegras.quadro(estado) == quadro)
    }

    @Test func piscarSoNaLivre() {
        #expect(MXPeluciaRegras.quadro(.livre, piscando: true) == "piscar")
        #expect(MXPeluciaRegras.quadro(.dormindo, piscando: true) == "dormindo-1")
        #expect(MXPeluciaRegras.quadro(.trabalhando, piscando: true) == "trabalhando-1")
    }

    @Test func selos() {
        #expect(MXPeluciaRegras.selos(.livre, sessoesRodando: 0) == [])
        #expect(MXPeluciaRegras.selos(.trabalhando, sessoesRodando: 0) == [.gomo])
        // Byte no desenho: precisa de você e tem uma sessão rodando → gomo.
        #expect(MXPeluciaRegras.selos(.precisaResposta, sessoesRodando: 1) == [.gomo])
        #expect(MXPeluciaRegras.selos(.dormindo, sessoesRodando: 0) == [.sono])
        #expect(MXPeluciaRegras.selos(.erro, sessoesRodando: 0) == [])
        #expect(MXPeluciaRegras.balanca(.livre, sessoesRodando: 0) == false)
        #expect(MXPeluciaRegras.balanca(.precisaResposta, sessoesRodando: 1))
    }

    @Test func balancoVaiDeMenos2AMais2ComEscalaNoMeio() {
        let inicio = MXPeluciaRegras.balanco(tempo: 0)
        #expect(abs(inicio.graus + 2) < 1e-9)
        #expect(abs(inicio.escalaX - 1) < 1e-9 && abs(inicio.escalaY - 1) < 1e-9)
        let meio = MXPeluciaRegras.balanco(tempo: 0.9)
        #expect(abs(meio.graus - 2) < 1e-9)
        #expect(abs(meio.escalaX - 1.03) < 1e-9 && abs(meio.escalaY - 0.97) < 1e-9)
        let volta = MXPeluciaRegras.balanco(tempo: 1.8)
        #expect(abs(volta.graus + 2) < 1e-9)
    }

    @Test func piscaEntre93e96PorCentoDoCiclo() {
        #expect(!MXPeluciaRegras.piscando(tempo: 0))
        #expect(!MXPeluciaRegras.piscando(tempo: 4.6))
        #expect(MXPeluciaRegras.piscando(tempo: 4.7))
        #expect(!MXPeluciaRegras.piscando(tempo: 4.85))
        #expect(MXPeluciaRegras.piscando(tempo: 9.7))
        #expect(MXPeluciaRegras.piscando(tempo: 0.7, atraso: 4))
    }

    @Test func agendaDoPiscarAcordaSoNasBordas() {
        let agenda = MXAgendaDoPiscar(atraso: 0)
        let inicio = Date(timeIntervalSinceReferenceDate: 10) // fase 0 do ciclo de 5 s
        let datas = Array(agenda.entries(from: inicio, mode: .normal).prefix(5)).map(\.timeIntervalSinceReferenceDate)
        #expect(datas.count == 5)
        let esperado: [Double] = [10, 14.65, 14.8, 19.65, 19.8]
        for (d, e) in zip(datas, esperado) { #expect(abs(d - e) < 1e-9) }
    }

    @Test(arguments: ["berga", "vendas", "base"])
    func agendaComDatasReaisAbreEFechaNaOrdemCerta(conjunto: String) {
        // Regressão: com o tempo absoluto de hoje, a piscada saía invertida (olhos fechados quase o tempo todo).
        let atraso = MXPeluciaRegras.atraso(conjunto)
        let inicio = Date(timeIntervalSinceReferenceDate: 815_000_000.123)
        let datas = Array(MXAgendaDoPiscar(atraso: atraso).entries(from: inicio, mode: .normal).prefix(7)).dropFirst()
        let estados = datas.map { MXPeluciaRegras.piscando(tempo: $0.timeIntervalSinceReferenceDate, atraso: atraso) }
        #expect(estados == [true, false, true, false, true, false])
    }

    @MainActor @Test func todosOsConjuntosTemOsSeisQuadros() {
        let conjuntos = ["amazon", "assistente", "base", "berga", "ceo", "chefe", "chiperica", "colheitor", "compras",
                         "designer", "developer", "dono", "financeiro", "hacker", "holofruta", "marketing", "pesquisa",
                         "pomarca", "rh", "suporte", "tesourango", "vendas"]
        let quadros = ["livre-1", "piscar", "trabalhando-1", "precisa-resposta", "dormindo-1", "erro"]
        for c in conjuntos {
            for q in quadros { #expect(MXQuadros.imagem(conjunto: c, quadro: q) != nil, "\(c)/\(q)") }
        }
    }
}

struct RedesignConversasTests {
    @Test func contador() {
        #expect(MXConversasRegras.contador(0) == nil)
        #expect(MXConversasRegras.contador(-1) == nil)
        #expect(MXConversasRegras.contador(1) == "1")
        #expect(MXConversasRegras.contador(99) == "99")
        #expect(MXConversasRegras.contador(100) == "99+")
    }

    @Test func resumoDosCanais() {
        #expect(MXConversasRegras.resumoDosCanais(MXExemplo.canais) == catalogo("\(3) · \(3) novas"))
        let um = [MXCanalResumo(nome: "a", naoLidas: 1, sessoesRodando: 0, sessoesEsperando: 0)]
        #expect(MXConversasRegras.resumoDosCanais(um) == catalogo("\(1) · \(1) nova"))
        let nenhum = [MXCanalResumo(nome: "a", naoLidas: 0, sessoesRodando: 0, sessoesEsperando: 0)]
        #expect(MXConversasRegras.resumoDosCanais(nenhum) == "1")
    }

    @Test func horaEDescricao() throws {
        let byte = try #require(MXExemplo.agentes.first { $0.id == "byte" })
        #expect(MXConversasRegras.horaDestacada(byte))
        #expect(MXConversasRegras.descricao(byte) == "Byte, Dev, 09:12, Precisa da sua aprovação, " + [catalogo("\(1) sessão rodando"), catalogo("\(1) esperando você"), catalogo("\(1) não lida")].joined(separator: ", "))
        let trato = try #require(MXExemplo.agentes.first { $0.id == "trato" })
        #expect(!MXConversasRegras.horaDestacada(trato))
        #expect(MXConversasRegras.descricao(trato) == "Trato, Vendas, Ontem, Mandei a proposta para o fornecedor.")
    }

    @Test func arranjoDaListaNoTextoGrande() {
        // Tamanho normal: o desenho (uma linha de prévia e de mensagem; nome e Space lado a lado; contagem ao lado).
        let normal = MXArranjoDaLista(acessibilidade: false)
        #expect(!normal.nomeESpaceEmpilhados && !normal.contagemDosCanaisEmbaixo && !normal.funcaoEmLinhaPropria)
        #expect(normal.linhasDaPrevia == 1 && normal.linhasDaMensagemDoBerga == 1)
        // Acessibilidade: empilha, mensagem do Berga sem limite, prévia até 3, função em linha própria.
        let grande = MXArranjoDaLista(acessibilidade: true)
        #expect(grande.nomeESpaceEmpilhados && grande.contagemDosCanaisEmbaixo && grande.funcaoEmLinhaPropria)
        #expect(grande.linhasDaPrevia == 3 && grande.linhasDaMensagemDoBerga == nil)
        #expect(MXConversasRegras.escalaMinimaDoNome >= 0.7 && MXConversasRegras.escalaMinimaDoNome < 1)
    }

    @Test func iconesDoTopoCrescemComTetoESemPerderOAlvo() {
        // Tamanho padrão: exatamente o desenho (ícone 22 em círculo de 44).
        #expect(MXConversasRegras.iconeDoBotaoRedondo(escalado: 22) == 22)
        #expect(MXConversasRegras.ladoDoBotaoRedondo(icone: 22) == 44)
        // Texto menor não encolhe abaixo do desenho; texto grande cresce até o teto, e o círculo junto.
        #expect(MXConversasRegras.iconeDoBotaoRedondo(escalado: 18) == 22)
        #expect(MXConversasRegras.iconeDoBotaoRedondo(escalado: 25) == 25)
        #expect(MXConversasRegras.iconeDoBotaoRedondo(escalado: 60) == MX.Medida.botaoRedondoIconeTeto)
        #expect(MXConversasRegras.ladoDoBotaoRedondo(icone: MX.Medida.botaoRedondoIconeTeto) == 50)
        #expect(MXConversasRegras.ladoDoBotaoRedondo(icone: 10) >= MX.Espaco.alvoMinimo)
    }

    @Test func abasNaOrdemDoDesenho() {
        #expect(MXAba.allCases.map(\.titulo) == [catalogo("Mission Board"), catalogo("Conversas"), catalogo("Organograma"), catalogo("Rotinas"), catalogo("Estado")])
        #expect(MXAba.missionBoard.largura == 84 && MXAba.conversas.largura == 64)
        #expect(MXAba.missionBoard.descricaoDoContador(2) == catalogo("\(2) esperam você"))
        #expect(MXAba.conversas.descricaoDoContador(1) == catalogo("\(1) nova"))
        #expect(MXExemplo.pendencias[.estado] == nil)
        // VoiceOver: o nome e o contador inteiro no rótulo (mesmo quando a aba mostra "9+").
        #expect(MXAba.conversas.rotuloAcessivel(12) == catalogo("Conversas") + ", " + catalogo("\(12) novas"))
        #expect(MXAba.missionBoard.rotuloAcessivel(1) == catalogo("Mission Board") + ", " + catalogo("\(1) espera você"))
        #expect(MXAba.estado.rotuloAcessivel(0) == catalogo("Estado"))
    }
}

struct RedesignGomoTests {
    @Test func gomoAcendeEmRoda() {
        #expect(MXGomo.cor(indice: 0, tempo: 0).hex == "#F58A1F")
        #expect(MXGomo.cor(indice: 0, tempo: 0.48).hex == "#FFB45E") // 40% do ciclo
        #expect(MXGomo.cor(indice: 1, tempo: 0.15).hex == "#F58A1F") // 0,15 s depois, o seguinte
        #expect(MXGomo.cor(indice: 7, tempo: 1.05).hex == "#F58A1F")
        #expect(MXGomo.cor(indice: 0, tempo: 1.2).hex == "#F58A1F") // ciclo de 1,2 s
    }

    @Test func rodaGiraEm8Segundos() {
        #expect(MXGomo.giro(tempo: 0) == 0)
        #expect(abs(MXGomo.giro(tempo: 2) - 90) < 1e-9)
        #expect(abs(MXGomo.giro(tempo: 8)) < 1e-9)
    }

    @Test func primeiroGomoBateComOSVG() {
        let caixa = MXGomo.caminho(indice: 0).boundingRect
        // M12.07 10.20 L12.33 3.61 A8.4 … 17.70 5.83 L13.22 10.68 Z
        #expect(abs(caixa.minX - 12.07) < 0.05)
        #expect(abs(caixa.minY - 3.6) < 0.05)
        #expect(abs(caixa.maxX - 17.70) < 0.05)
        #expect(abs(caixa.maxY - 10.68) < 0.05)
    }
}

struct RedesignFontesTests {
    @Test func nunitoEDMSansRegistramEAplicamPeso() {
        #expect(MXFontes.registrar())
        let titulo = MXFontes.ctFont(MX.Tipo.nomeNoCartao, tamanho: 22)
        #expect(CTFontCopyFamilyName(titulo) as String == "Nunito")
        let corpo = MXFontes.ctFont(MX.Tipo.corpoForte, tamanho: 15)
        #expect(CTFontCopyFamilyName(corpo) as String == "DM Sans")
        let eixos = CTFontCopyVariation(corpo) as? [Int: Double] ?? [:]
        #expect(eixos[MXFontes.eixoPeso] == 600)
        #expect(eixos[MXFontes.eixoOptico] == 15)
    }

    @Test func tamanhoEscalaComDynamicType() {
        let normal = MXFontes.tamanho(MX.Tipo.corpo, categoria: .large)
        let grande = MXFontes.tamanho(MX.Tipo.corpo, categoria: .accessibilityExtraLarge)
        #expect(normal == 15)
        #expect(grande > normal)
    }

    @Test func licencasAcompanhamAsFontes() {
        for nome in ["Nunito-OFL.txt", "DMSans-OFL.txt"] {
            #expect(MXFontes.recurso(nome) != nil, "\(nome)")
        }
    }
}

/// Revisão da etapa 1: Reduzir movimento, Dynamic Type na barra de abas e tokens novos.
struct RedesignAcessibilidadeTests {
    @Test func reduzirMovimentoViraCrossFadeSemGirarNemDeslizar() {
        let normal = MX.Movimento.troca(reduzir: false)
        #expect(normal == .mola(MX.Movimento.molaPadrao))
        #expect(normal.interpola)
        let reduzido = MX.Movimento.troca(reduzir: true)
        #expect(reduzido == .crossFade(duracao: MX.Movimento.crossFadeCurto))
        #expect(!reduzido.interpola)
        #expect(MX.Movimento.crossFadeCurto == 0.2)
    }

    @Test func reduzirMovimentoNaoAnimaPosicaoAoAbrirCanais() {
        // Normal: a troca anima o layout com a mola (a lista de agentes desce junto).
        let normal = MX.Movimento.troca(reduzir: false)
        #expect(normal.animacaoDaPosicao == MX.Movimento.molaPadrao.animacao)
        // Reduzir movimento: nenhuma animação de posição; só a opacidade, presa à transição das linhas.
        let reduzido = MX.Movimento.troca(reduzir: true)
        #expect(reduzido.animacaoDaPosicao == nil)
        #expect(reduzido.animacao == MX.Movimento.crossFade)
    }

    @Test func reduzirMovimentoRecolheCanaisEsmaecendoNoLugar() {
        // Reduzir movimento: ao recolher, as linhas esmaecem primeiro e só depois saem (sem animar posição).
        #expect(MX.Movimento.troca(reduzir: true).saidaEmDuasFases)
        // Normal: saem junto com a mola.
        #expect(!MX.Movimento.troca(reduzir: false).saidaEmDuasFases)
    }

    @Test func barraMantemORotuloAteMuitoGrande() {
        let normais: [DynamicTypeSize] = [.xSmall, .small, .medium, .large, .xLarge]
        for tamanho in normais {
            #expect(MXApresentacaoDaBarra.para(tamanho) == .iconeERotulo, "\(tamanho)")
        }
        // Igual ao desenho (capturas/04-conversas.webp).
        let desenho = MXApresentacaoDaBarra.iconeERotulo
        #expect(desenho.icone == 22)
        #expect(desenho.tetoDoRotulo == .xLarge)
        #expect(desenho.largura(.missionBoard) == 84 && desenho.largura(.conversas) == 64)
        #expect(desenho.altura == 52)
        #expect(desenho.larguraTotal == 358)
    }

    @Test func textoGrandeComoABarraNativaNomeVisivelComTeto() {
        // Até "extra extra extra grande": o desenho, com o nome parando de crescer em "muito grande".
        for tamanho in [DynamicTypeSize.xxLarge, .xxxLarge] {
            #expect(MXApresentacaoDaBarra.para(tamanho) == .iconeERotulo, "\(tamanho)")
        }
        // Acessibilidade: abas de 68 (revisão da etapa 1), nome no tamanho do desenho.
        for tamanho in [DynamicTypeSize.accessibility1, .accessibility2, .accessibility3, .accessibility4, .accessibility5] {
            #expect(MXApresentacaoDaBarra.para(tamanho) == .acessivel, "\(tamanho)")
        }
        #expect(MXApresentacaoDaBarra.acessivel.tetoDoRotulo == .large)
        // O ícone não cresce (o grande fica no visualizador de conteúdo grande).
        #expect(MXApresentacaoDaBarra.acessivel.icone == MXApresentacaoDaBarra.iconeERotulo.icone)
        for aba in MXAba.allCases {
            #expect(MXApresentacaoDaBarra.acessivel.largura(aba) == MX.Medida.abaLarguraAcessivel)
        }
    }

    /// Os nomes das abas nos dois idiomas do app, direto do catálogo compilado (sem depender do idioma do aparelho).
    private func nomesDasAbas(idioma: String) -> [(aba: MXAba, rotulo: String)] {
        let pasta = ConnectionSummary.resourceBundle.path(forResource: idioma, ofType: "lproj")
        let pacote = pasta.flatMap(Bundle.init(path:))
        #expect(pacote != nil, "sem \(idioma).lproj")
        func texto(_ chave: String) -> String { pacote?.localizedString(forKey: chave, value: nil, table: nil) ?? chave }
        return MXAba.allCases.map { aba in
            (aba, texto(chaveDoRotulo(aba)))
        }
    }

    /// A chave do rótulo que cada aba mostra na barra (a do título, salvo as duas que têm rótulo curto).
    private func chaveDoRotulo(_ aba: MXAba) -> String {
        switch aba {
        case .equipes: "Organograma na barra"
        case .conversas: "Conversas na barra"
        default: chaveDoTitulo(aba)
        }
    }

    /// A chave (texto em pt-BR) do título de cada aba no catálogo.
    private func chaveDoTitulo(_ aba: MXAba) -> String {
        switch aba {
        case .missionBoard: "Mission Board"
        case .conversas: "Conversas"
        case .equipes: "Organograma"
        case .rotinas: "Rotinas"
        case .estado: "Estado"
        }
    }

    /// Largura do rótulo no tamanho de texto escolhido, com o teto da apresentação (DM Sans 600 medida).
    private func larguraDoRotulo(_ texto: String, tamanho: DynamicTypeSize, apresentacao: MXApresentacaoDaBarra) -> CGFloat {
        let teto = apresentacao.tetoDoRotulo
        let efetivo = tamanho < teto ? tamanho : teto
        let tipo = MX.Tipo.rotuloAba
        let fonte = MXFontes.ctFont(tipo, tamanho: MXFontes.tamanho(tipo, categoria: UIContentSizeCategory(efetivo)))
        let linha = CTLineCreateWithAttributedString(
            NSAttributedString(string: texto, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): fonte]))
        return CGFloat(CTLineGetTypographicBounds(linha, nil, nil, nil))
    }

    @Test func rotuloDaAbaCabeEmTodosOsTamanhosNosDoisIdiomasSemPisoEspecial() {
        #expect(MXFontes.registrar())
        // Piso único para todas as abas (nenhuma encolhe abaixo de 80%), inclusive Organograma ("Bots" na barra).
        let piso = MX.Medida.abaRotuloEscalaMinima
        #expect(piso == 0.8)
        let tamanhos: [DynamicTypeSize] = [.large, .xLarge, .xxxLarge, .accessibility1, .accessibility3, .accessibility5]
        for idioma in ["pt-BR", "en"] {
            let nomes = nomesDasAbas(idioma: idioma)
            for tamanho in tamanhos {
                let apresentacao = MXApresentacaoDaBarra.para(tamanho)
                for (aba, rotulo) in nomes {
                    let nome = larguraDoRotulo(rotulo, tamanho: tamanho, apresentacao: apresentacao)
                    let largura = apresentacao.largura(aba)
                    #expect(nome * piso <= largura, "\(idioma) \(tamanho) \(aba) '\(rotulo)': \(nome) em \(largura)")
                    // No tamanho do desenho (padrão) o nome cabe sem encolher.
                    if tamanho == .large { #expect(nome <= largura, "\(idioma) padrão \(aba) '\(rotulo)': \(nome) em \(largura)") }
                }
            }
        }
    }

    @Test func abasComRotuloCurtoLeemONomeCompleto() {
        // pt-BR e en no idioma do teste: o rótulo é curto; o título (VoiceOver, visualizador grande) é o completo.
        #expect(MXAba.equipes.rotulo == catalogo("Organograma na barra"))
        #expect(MXAba.equipes.titulo == catalogo("Organograma"))
        #expect(MXAba.equipes.rotuloAcessivel(0) == catalogo("Organograma"))
        #expect(MXAba.equipes.rotuloAcessivel(3) == catalogo("Organograma") + ", " + catalogo("\(3) novas"))
        #expect(MXAba.conversas.rotulo == catalogo("Conversas na barra"))
        #expect(MXAba.conversas.titulo == catalogo("Conversas"))
        for aba in [MXAba.missionBoard, .rotinas, .estado] { #expect(aba.rotulo == aba.titulo) }
        func rotulo(_ idioma: String, _ aba: MXAba) -> String? { nomesDasAbas(idioma: idioma).first { $0.aba == aba }?.rotulo }
        #expect(rotulo("pt-BR", .equipes) == "Bots" && rotulo("en", .equipes) == "Bots")
        // O título da tela (e o que o VoiceOver lê) segue Organograma / Org chart.
        func titulo(_ idioma: String) -> String? {
            let pasta = ConnectionSummary.resourceBundle.path(forResource: idioma, ofType: "lproj")
            return pasta.flatMap(Bundle.init(path:))?.localizedString(forKey: "Organograma", value: nil, table: nil)
        }
        #expect(titulo("pt-BR") == "Organograma" && titulo("en") == "Org chart")
        #expect(rotulo("pt-BR", .conversas) == "Conversas" && rotulo("en", .conversas) == "Chats")
    }

    @Test func barraCabeEm390ComAlvoDeToque() {
        let larguraUtil: CGFloat = 390 - 2 * MX.Espaco.margemTela
        for apresentacao in [MXApresentacaoDaBarra.iconeERotulo, .acessivel] {
            #expect(apresentacao.larguraTotal <= larguraUtil)
            #expect(apresentacao.altura >= MX.Espaco.alvoMinimo)
            for aba in MXAba.allCases { #expect(apresentacao.largura(aba) >= MX.Espaco.alvoMinimo) }
        }
        #expect(MX.Medida.abaRotuloAte == .xLarge)
    }

    @Test func contadorDaAbaMudaDeLugarNosTamanhosDeAcessibilidade() {
        for tamanho in [DynamicTypeSize.xSmall, .large, .xLarge, .xxLarge, .xxxLarge] {
            #expect(MXPosicaoDoContador.para(tamanho) == .noCantoDoIcone, "\(tamanho)")
        }
        for tamanho in [DynamicTypeSize.accessibility1, .accessibility2, .accessibility3, .accessibility4, .accessibility5] {
            #expect(MXPosicaoDoContador.para(tamanho) == .abaixoDoIcone, "\(tamanho)")
        }
    }

    /// Largura do selo (texto medido na DM Sans 700 + respiro e contorno dos dois lados).
    private func larguraDoSelo(_ texto: String, categoria: UIContentSizeCategory) -> CGFloat {
        let tipo = MX.Tipo.contadorAba
        let fonte = MXFontes.ctFont(tipo, tamanho: MXFontes.tamanho(tipo, categoria: categoria))
        let linha = CTLineCreateWithAttributedString(
            NSAttributedString(string: texto, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): fonte]))
        let m = MX.Medida.self
        return CGFloat(CTLineGetTypographicBounds(linha, nil, nil, nil)) + 2 * (m.abaContadorRespiro + m.abaContadorContorno)
    }

    @Test func contadorDaAbaAbreviaSoNosTamanhosDeAcessibilidade() {
        // Fora da acessibilidade: igual aos outros contadores.
        for tamanho in [DynamicTypeSize.large, .xLarge, .xxxLarge] {
            #expect(MXConversasRegras.contadorDaAba(0, tamanho: tamanho) == nil)
            #expect(MXConversasRegras.contadorDaAba(9, tamanho: tamanho) == "9")
            #expect(MXConversasRegras.contadorDaAba(12, tamanho: tamanho) == "12")
            #expect(MXConversasRegras.contadorDaAba(150, tamanho: tamanho) == "99+")
        }
        // Acessibilidade: "9+" acima de 9; o VoiceOver segue com o número inteiro.
        for tamanho in [DynamicTypeSize.accessibility1, .accessibility3, .accessibility5] {
            #expect(MXConversasRegras.contadorDaAba(0, tamanho: tamanho) == nil)
            #expect(MXConversasRegras.contadorDaAba(9, tamanho: tamanho) == "9")
            #expect(MXConversasRegras.contadorDaAba(10, tamanho: tamanho) == "9+")
            #expect(MXConversasRegras.contadorDaAba(150, tamanho: tamanho) == "9+")
        }
        #expect(MXAba.conversas.descricaoDoContador(150).contains("150"))
    }

    /// Todos os textos que o contador pode mostrar no tamanho.
    private func textosPossiveis(_ tamanho: DynamicTypeSize) -> [String] {
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 99, 100, 150].compactMap { MXConversasRegras.contadorDaAba($0, tamanho: tamanho) }
    }

    @Test func numeroDoContadorCresceSemTetoECabeSemEncolher() {
        #expect(MXFontes.registrar())
        let m = MX.Medida.self
        let tipo = MX.Tipo.contadorAba
        let pares: [(DynamicTypeSize, UIContentSizeCategory)] = [
            (.large, .large), (.xLarge, .extraLarge), (.xxLarge, .extraExtraLarge), (.xxxLarge, .extraExtraExtraLarge),
            (.accessibility1, .accessibilityMedium), (.accessibility2, .accessibilityLarge),
            (.accessibility3, .accessibilityExtraLarge), (.accessibility4, .accessibilityExtraExtraLarge),
            (.accessibility5, .accessibilityExtraExtraExtraLarge),
        ]
        // Sem teto: a fonte pedida cresce em todos os tamanhos, até o último.
        let tamanhos = pares.map { MXFontes.tamanho(tipo, categoria: $0.1) }
        for (menor, maior) in zip(tamanhos, tamanhos.dropFirst()) { #expect(maior > menor) }

        for (tamanho, categoria) in pares {
            let apresentacao = MXApresentacaoDaBarra.para(tamanho)
            let aba = MXAba.conversas
            // O selo mais largo possível, medido na fonte pedida, sem escala.
            let maior = textosPossiveis(tamanho).map { larguraDoSelo($0, categoria: categoria) }.max() ?? 0
            switch MXPosicaoDoContador.para(tamanho) {
            case .noCantoDoIcone:
                // No canto do ícone: o selo fica dentro da própria aba (nem passa da borda direita nem da esquerda).
                let direita = (apresentacao.largura(aba) + apresentacao.icone) / 2 + m.abaContadorDireita
                #expect(direita <= apresentacao.largura(aba), "\(tamanho)")
                #expect(direita - maior >= 0, "\(tamanho)")
            case .abaixoDoIcone:
                // Embaixo do ícone: cabe na largura da aba, com folga dos dois lados.
                #expect(maior <= MXPosicaoDoContador.larguraMaxima(aba, apresentacao), "\(tamanho): \(maior)")
            }
            // A barra cabe em 390 pt.
            #expect(apresentacao.larguraTotal <= 390 - 2 * MX.Espaco.margemTela, "\(tamanho)")
        }
        // Número marrom sobre o laranja.
        #expect(MX.Claro.sobreMexerica.hex == "#2A1C14")
    }

    @Test func listaSoMudaDeArranjoNosTamanhosDeAcessibilidade() {
        for tamanho in [DynamicTypeSize.xSmall, .large, .xLarge, .xxxLarge] {
            #expect(MXArranjoDaLista(acessibilidade: tamanho.isAccessibilitySize) == MXArranjoDaLista(acessibilidade: false), "\(tamanho)")
        }
        for tamanho in [DynamicTypeSize.accessibility1, .accessibility3, .accessibility5] {
            #expect(MXArranjoDaLista(acessibilidade: tamanho.isAccessibilitySize) == MXArranjoDaLista(acessibilidade: true), "\(tamanho)")
        }
    }

    @Test func tokensDeEspessuraEEscala() {
        #expect(MX.Vidro.contornoOpacoEspessura == 1)
        #expect(MX.Medida.abaRotuloEscalaMinima == 0.8)
        #expect([MX.Medida.iconeTracoFino, MX.Medida.iconeTraco, MX.Medida.iconeTracoMais, MX.Medida.iconeTracoSeta] == [1.8, 2, 2.2, 2.4])
        #expect(MXIcone.conta.traco == MX.Medida.iconeTracoFino)
        #expect(MXIcone.busca.traco == MX.Medida.iconeTraco && MXIcone.robo.traco == MX.Medida.iconeTraco)
        #expect(MXIcone.mais.traco == MX.Medida.iconeTracoMais)
        #expect(MXIcone.setaParaDireita.traco == MX.Medida.iconeTracoSeta && MXIcone.setaParaBaixo.traco == MX.Medida.iconeTracoSeta)
        #expect(MXIcone.conversas.traco == MX.Medida.abaIconeTraco)
    }
}

/// Os textos do redesenho vêm do catálogo do CodyncUI, com tradução de verdade em pt-BR e em inglês.
struct RedesignTextosEmDoisIdiomasTests {
    private func bundle(_ idioma: String) throws -> Bundle {
        let caminho = try #require(ConnectionSummary.resourceBundle.path(forResource: idioma, ofType: "lproj"))
        return try #require(Bundle(path: caminho))
    }

    @Test func textosSimplesTemPortuguesEIngles() throws {
        let casos: [(chave: String, pt: String, en: String)] = [
            ("Fechar menu", "Fechar menu", "Close menu"),
            ("Tentar de novo", "Tentar de novo", "Try again"),
            ("Conversas", "Conversas", "Conversations"),
            ("Voltar para Conversas", "Voltar para Conversas", "Back to Conversations"),
            ("Esperando você", "Esperando você", "Waiting for you"),
            ("Sempre permitir", "Sempre permitir", "Always allow"),
            ("1 passo", "1 passo", "1 step"),
        ]
        for idioma in ["pt-BR", "en"] {
            let b = try bundle(idioma)
            for caso in casos {
                #expect(NSLocalizedString(caso.chave, bundle: b, comment: "") == (idioma == "pt-BR" ? caso.pt : caso.en), "\(idioma): \(caso.chave)")
            }
        }
    }

    @Test func pluraisEInterpolacoesSaemCertosNosDoisIdiomas() throws {
        let pt = try bundle("pt-BR")
        let en = try bundle("en")
        func formatar(_ chave: String, _ b: Bundle, _ args: CVarArg...) -> String {
            String(format: NSLocalizedString(chave, bundle: b, comment: ""), arguments: args)
        }
        // Pares singular/plural explícitos, escolhidos pelo número no código.
        #expect(formatar("%lld passos", pt, 2) == "2 passos" && formatar("%lld passos", en, 2) == "2 steps")
        #expect(formatar("%lld nova", pt, 1) == "1 nova" && formatar("%lld nova", en, 1) == "1 new")
        #expect(formatar("%lld não lidas", pt, 3) == "3 não lidas" && formatar("%lld não lidas", en, 3) == "3 unread")
        #expect(formatar("%lld sessão rodando", en, 1) == "1 session running" && formatar("%lld sessões rodando", en, 2) == "2 sessions running")
        // Separador decimal e porcentagem.
        #expect(formatar("%lld,%lld MB", pt, 3, 4) == "3,4 MB" && formatar("%lld,%lld MB", en, 3, 4) == "3.4 MB")
        #expect(formatar("%lld%% do contexto", pt, 42) == "42% do contexto" && formatar("%lld%% do contexto", en, 42) == "42% of context")
        // Mesma ordem dos argumentos nas duas línguas.
        #expect(formatar("Enviar (%@ está trabalhando: entra na fila)", pt, "Byte") == "Enviar (Byte está trabalhando: entra na fila)")
        #expect(formatar("Enviar (%@ está trabalhando: entra na fila)", en, "Byte") == "Send (Byte is working: goes to the queue)")
        #expect(formatar("Arquivos %lld–%lld de %lld", en, 21, 23, 23) == "Files 21–23 of 23")
    }
}
