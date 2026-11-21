#if DEBUG
import Foundation
import Observation
import os
import SwiftUI
import Testing
import UIKit
@testable import CodyncKit
@testable import CodyncUI

/// Ajustes, conta e computadores do redesenho (PLANO §4.1, só Debug): regras puras, a conta real sobre um `AccountStore`,
/// a de exemplo e a tela em vários temas e tamanhos de texto.

// MARK: - Apoio

private func computador(_ nome: String, byte: UInt8) -> Computer {
    let chave = Data(repeating: byte, count: 32)
    let codificada = chave.base64EncodedString().replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    return Computer(id: RelayCrypto.computerId(signKey: chave), name: nome, signKey: codificada)
}

@MainActor
private final class SessaoFalsa: MXSessaoDaConta {
    var email: String? = "voce@exemplo.com"
    var isSignedIn = true
    var isConfigured = true
    /// `sair` na ponte muda isto: falso simula "não saiu".
    var saiu = true
}

@MainActor
private final class Registro {
    var avisosMudaram: [Bool] = []
    var contaAberta = 0
    var permissao: Bool?
    var pediuPermissao = 0
}

@MainActor
private struct Cenario {
    let contas: AccountStore
    let conta: MXContaReal
    let sessao: SessaoFalsa
    let registro: Registro
    let ids: [String]
    let defaults: UserDefaults
}

@MainActor
private func cenario(computadores: Int, configurada: Bool = true) throws -> Cenario {
    let suite = "RedesignAjustesDaConta.\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    let storage = SharedStore.Context(accountID: UUID().uuidString, suite: suite)
    let lista = (0..<computadores).map { computador("Mac \($0 + 1)", byte: UInt8($0 + 1)) }
    storage.computers = lista
    let contas = AccountStore(storage: storage, cloud: nil) { c in
        BotStore(computer: c, storage: storage) { _ in FakeRemote(.connecting) }
    }
    contas.setActive(false)
    let sessao = SessaoFalsa()
    sessao.isConfigured = configurada
    let registro = Registro()
    let ponte = MXPonteDoApp(
        sessao: sessao, abrirConta: { registro.contaAberta += 1 },
        sair: { if sessao.saiu { sessao.isSignedIn = false } },
        vista: { _, _ in AnyView(EmptyView()) },
        permissaoDeAvisos: { registro.permissao },
        pedirPermissaoDeAvisos: { registro.pediuPermissao += 1; return true },
        avisosMudaram: { registro.avisosMudaram.append($0) })
    let conta = MXContaReal(contas: contas, ponte: ponte, defaults: defaults)
    return Cenario(contas: contas, conta: conta, sessao: sessao, registro: registro, ids: contas.computers.map(\.id), defaults: defaults)
}

private let pt = MXTextosDaConta(ingles: false)
private let en = MXTextosDaConta(ingles: true)

private func calendario() -> Calendar {
    var c = Calendar(identifier: .gregorian)
    c.timeZone = TimeZone(identifier: "UTC") ?? .gmt
    return c
}

private func data(_ dia: Int, _ hora: Int, _ minuto: Int) -> Date {
    calendario().date(from: DateComponents(year: 2026, month: 11, day: dia, hour: hora, minute: minuto)) ?? .now
}

// MARK: - Textos

struct RedesignAjustesDaContaTextosTests {
    @Test func osTextosDaTelaExistemNosDoisIdiomas() {
        #expect(pt.ajustes == "Ajustes" && en.ajustes == "Settings")
        #expect(pt.suaConta == "Sua conta" && en.suaConta == "Your account")
        #expect(pt.computadores == "Computadores" && en.computadores == "Computers")
        #expect(pt.conectarOutro == "Conectar outro computador" && en.conectarOutro == "Connect another computer")
        #expect(pt.conectarUm == "Conectar um computador" && en.conectarUm == "Connect a computer")
        #expect(pt.sairPergunta(nil) == "Sair desta conta?" && en.sairPergunta(nil) == "Sign out of this account?")
        // Identifica a conta e diz o que sair apaga (o mesmo aviso do seletor de contas de hoje).
        #expect(pt.sairPergunta("voce@exemplo.com") == "Sair de voce@exemplo.com?" && en.sairPergunta("voce@exemplo.com") == "Sign out of voce@exemplo.com?")
        #expect(pt.sairAviso == "Este iPhone esquecerá os computadores da conta. Seus bots continuam neles.")
        #expect(en.sairAviso == "This iPhone will forget the account's computers. Your bots stay on them.")
        #expect(pt.nota == "Motor, ferramentas, permissões e organograma de cada bot se ajustam no app do computador.")
        #expect(pt.emBreve == "Em breve" && en.emBreve == "Coming soon")
    }

    @Test func todoTextoTemOsDoisIdiomasDiferentes() {
        let pares: [(String, String)] = [
            (pt.ajustes, en.ajustes), (pt.voltar, en.voltar), (pt.suaConta, en.suaConta), (pt.pessoal, en.pessoal),
            (pt.voz, en.voz), (pt.memoria, en.memoria), (pt.uso, en.uso), (pt.esteIphone, en.esteIphone),
            (pt.avisos, en.avisos), (pt.idioma, en.idioma), (pt.sair, en.sair), (pt.cancelar, en.cancelar),
            (pt.tentarDeNovo, en.tentarDeNovo), (pt.conectado, en.conectado), (pt.desligado, en.desligado),
            (pt.semConexao, en.semConexao), (pt.semAcesso, en.semAcesso), (pt.naoPareado, en.naoPareado),
            (pt.pedirAcesso, en.pedirAcesso), (pt.verCodigo, en.verCodigo), (pt.erroAoAtualizar, en.erroAoAtualizar),
            (pt.erroAoSair, en.erroAoSair), (pt.semComputadorTitulo, en.semComputadorTitulo),
            (pt.abreOsAjustesDoIphone, en.abreOsAjustesDoIphone), (pt.maisAjustes, en.maisAjustes),
            (pt.usarEste, en.usarEste), (pt.selecionado, en.selecionado),
        ]
        for (a, b) in pares { #expect(!a.isEmpty && !b.isEmpty && a != b) }
    }

    @Test func idiomaPadraoSegueOIdiomaDoApp() {
        #expect(MXTextosDaConta(ingles: true).ingles && !MXTextosDaConta(ingles: false).ingles)
    }

    @Test func vistoHojeOntemEDataCurta() {
        let agora = data(16, 21, 0)
        let c = calendario()
        #expect(pt.visto(data(16, 14, 5), agora: agora, calendario: c) == "visto hoje às 14:05")
        #expect(pt.visto(data(15, 18, 40), agora: agora, calendario: c) == "visto ontem às 18:40")
        #expect(pt.visto(data(10, 18, 40), agora: agora, calendario: c).hasPrefix("visto em "))
        #expect(en.visto(data(15, 18, 40), agora: agora, calendario: c).hasPrefix("last seen yesterday at 6:40"))
        #expect(en.visto(data(16, 14, 5), agora: agora, calendario: c).hasPrefix("last seen today at 2:05"))
    }

    @Test func linhaDeEstadoJuntaOsPedacosComPontoMedio() {
        let agora = data(16, 21, 0)
        let c = calendario()
        #expect(pt.estado(.conectado, emUso: true, agora: agora, calendario: c) == "Conectado · em uso")
        #expect(pt.estado(.conectado, emUso: false, agora: agora, calendario: c) == "Conectado")
        #expect(pt.estado(.desligado(visto: data(15, 18, 40)), emUso: false, agora: agora, calendario: c) == "Desligado · visto ontem às 18:40")
        #expect(pt.estado(.desligado(visto: nil), emUso: true, agora: agora, calendario: c) == "Desligado · em uso")
        #expect(en.estado(.conectado, emUso: true, agora: agora, calendario: c) == "Connected · in use")
        #expect(pt.estado(.conectando, emUso: false) == "Conectando…")
        #expect(pt.estado(.semAcesso, emUso: false) == "Sem acesso")
        #expect(pt.estado(.precisaAtualizar, emUso: true) == "Precisa atualizar · em uso")
        #expect(pt.estado(.daConta(ligado: true), emUso: false) == "Ligado · na sua conta")
        #expect(pt.estado(.daConta(ligado: false), emUso: false) == "Desligado · na sua conta")
        #expect(pt.estado(.pedidoEmAndamento(ligado: true, detalhe: "Digite 123456 no Mac"), emUso: false) == "Digite 123456 no Mac")
        #expect(pt.estado(.pedidoEmAndamento(ligado: true, detalhe: nil), emUso: false) == "Ligado · na sua conta · pedido aguardando")
    }

    @Test func oVoiceOverLeNomeEstadoESelecionado() {
        let linha = MXComputadorLinha(id: "a", nome: "MacBook de exemplo", estado: .conectado, emUso: true, acao: .nenhuma)
        let leitura = MXContaRegras.rotuloAcessivel(linha, textos: pt)
        #expect(leitura.rotulo == "MacBook de exemplo" && leitura.valor == "Conectado, em uso")
    }

    @Test func versaoESemLoginTemTexto() {
        #expect(pt.versao("1.2") == "Versão 1.2 · baseado no Codync" && en.versao("1.2") == "Version 1.2 · based on Codync")
        #expect(pt.agoraUsando("Mac 2") == "Agora usando Mac 2" && en.agoraUsando("Mac 2") == "Now using Mac 2")
        #expect(pt.usoValor(62) == "62% usado" && en.usoValor(62) == "62% used")
    }
}

// MARK: - Regras

struct RedesignAjustesDaContaRegrasTests {
    @Test func estadoVemDaConexaoQueOStoreMostra() {
        #expect(MXContaRegras.estado(conexao: .online, precisaAtualizar: false, copiaAntiga: false) == .conectado)
        #expect(MXContaRegras.estado(conexao: .online, precisaAtualizar: true, copiaAntiga: false) == .precisaAtualizar)
        #expect(MXContaRegras.estado(conexao: .connecting, precisaAtualizar: false, copiaAntiga: false) == .conectando)
        let visto = Date(timeIntervalSince1970: 1000)
        #expect(MXContaRegras.estado(conexao: .computerOffline(lastSeen: visto), precisaAtualizar: false, copiaAntiga: false) == .desligado(visto: visto))
        #expect(MXContaRegras.estado(conexao: .offline("x"), precisaAtualizar: false, copiaAntiga: false) == .semConexao)
        #expect(MXContaRegras.estado(conexao: .unauthorized("revogado"), precisaAtualizar: false, copiaAntiga: false) == .semAcesso)
        #expect(MXContaRegras.estado(conexao: .unpaired, precisaAtualizar: false, copiaAntiga: false) == .naoPareado)
        // A cópia antiga vence a conexão; o texto bruto (`revogado`, `x`) nunca vira texto da tela.
        #expect(MXContaRegras.estado(conexao: .online, precisaAtualizar: false, copiaAntiga: true) == .copiaAntiga)
    }

    @Test func estadoDeComputadorDaConta() {
        #expect(MXContaRegras.estadoDaConta(ligado: true, copiaAntiga: false, pedidoEmAndamento: false, detalhe: nil) == .daConta(ligado: true))
        #expect(MXContaRegras.estadoDaConta(ligado: false, copiaAntiga: false, pedidoEmAndamento: true, detalhe: "x") == .pedidoEmAndamento(ligado: false, detalhe: "x"))
        #expect(MXContaRegras.estadoDaConta(ligado: true, copiaAntiga: true, pedidoEmAndamento: true, detalhe: nil) == .copiaAntiga)
    }

    @Test func tocarNaLinhaFazOQueDiz() {
        #expect(MXContaRegras.acao(estado: .conectado, emUso: true) == .nenhuma)
        #expect(MXContaRegras.acao(estado: .conectado, emUso: false) == .usar)
        #expect(MXContaRegras.acao(estado: .desligado(visto: nil), emUso: false) == .usar)
        #expect(MXContaRegras.acao(estado: .semAcesso, emUso: false) == .usar)
        #expect(MXContaRegras.acao(estado: .daConta(ligado: true), emUso: false) == .pedirAcesso)
        #expect(MXContaRegras.acao(estado: .pedidoEmAndamento(ligado: true, detalhe: nil), emUso: false) == .verCodigo)
        #expect(MXContaRegras.acao(estado: .copiaAntiga, emUso: false) == .nenhuma)
    }

    @Test func oComputadorEmUsoSegueEscolhaUltimoEPrimeiro() {
        let ids = ["a", "b", "c"]
        #expect(MXContaRegras.emUso(escolhido: "c", ultimo: "b", ids: ids) == "c")
        #expect(MXContaRegras.emUso(escolhido: nil, ultimo: "b", ids: ids) == "b")
        // O escolhido sumiu (removido): cai no último; sem último, no primeiro; sem nenhum, em nada.
        #expect(MXContaRegras.emUso(escolhido: "x", ultimo: "b", ids: ids) == "b")
        #expect(MXContaRegras.emUso(escolhido: "x", ultimo: "y", ids: ids) == "a")
        #expect(MXContaRegras.emUso(escolhido: "x", ultimo: "y", ids: []) == nil)
    }

    @Test func usoMostraOLimiteMaisApertado() {
        func provedor(_ id: String, _ percentuais: [Double]) -> UsageProvider {
            UsageProvider(id: id, name: id, windows: percentuais.enumerated().map {
                UsageWindow(id: "\($0.offset)", label: "w", percent: $0.element, resetsAt: nil, resetsText: nil)
            }, source: "t", updatedAt: 0)
        }
        #expect(MXContaRegras.uso(providers: []) == .semDados)
        #expect(MXContaRegras.uso(providers: [provedor("a", [])]) == .semDados)
        #expect(MXContaRegras.uso(providers: [provedor("a", [10, 61.6]), provedor("b", [40])]) == .percentual(62))
    }

    @Test func nomeDoIdiomaEscritoNeleMesmo() {
        #expect(MXContaRegras.nomeDoIdioma("pt-BR") == "Português (Brasil)")
        #expect(MXContaRegras.nomeDoIdioma("en") == "English")
        #expect(MXContaRegras.nomeDoIdioma(nil) == "Português (Brasil)")
    }

    @Test func emailQuebraDepoisDoArroba() {
        let quebravel = MXContaRegras.quebravel("voce@exemplo.com")
        #expect(quebravel == "voce@\u{200B}exemplo.com")
        #expect(quebravel.replacingOccurrences(of: "\u{200B}", with: "") == "voce@exemplo.com")
    }

    @Test func alvosEMedidasDoMockup() {
        #expect(MX.Conta.linhaAlturaMinima >= MX.Espaco.alvoMinimo && MX.Conta.linhaAlturaMinimaAcessivel >= MX.Espaco.alvoMinimo)
        #expect(MX.Conta.contaAlturaMinima >= MX.Espaco.alvoMinimo)
        #expect(MX.Conta.tile == 40 && MX.Conta.tileAcessivel == 48 && MX.Conta.avatar == 48 && MX.Conta.painelRespiro == 12)
        #expect(MX.Conta.cartaoRaio == MX.Raio.cartao && MX.Conta.painelRaio == MX.Raio.painel)
    }

    @Test func oTextoLaranjaPequenoESoOsTokensQueJaExistem() {
        // `#9C4608` / `#FF8F85` / `#FFB878` do PLANO §5 esperam o dono (§6 item 8): a tela usa os existentes.
        let paleta = MXPaletaDaConta(.light)
        #expect(paleta.destaqueTexto == MX.Claro.mexericaTexto.color)
        #expect(MXPaletaDaConta(.dark).destaqueTexto == MX.Escuro.mexericaTexto.color)
        #expect(MXPaletaDaConta(.dark).erro == MX.Escuro.texto.color && paleta.erro == MX.Claro.erroTexto.color)
    }

    @Test func textoDeContrasteAAEmTodosOsPares() {
        func luz(_ c: MXCor) -> Double {
            func canal(_ v: Double) -> Double { v <= 0.03928 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
            return 0.2126 * canal(c.r) + 0.7152 * canal(c.g) + 0.0722 * canal(c.b)
        }
        func razao(_ a: MXCor, _ b: MXCor) -> Double {
            let (x, y) = (luz(a), luz(b))
            return (max(x, y) + 0.05) / (min(x, y) + 0.05)
        }
        let cartaoClaro = MX.Claro.balaoDoAgente.sobre(MX.Claro.fundo)
        // Claro: texto, secundário, laranja pequeno, verde sobre o cartão; "Sair" (erro) também.
        for cor in [MX.Claro.texto, MX.Claro.textoSecundario, MX.Claro.mexericaTexto, MX.Claro.sucessoTexto, MX.Claro.erroTexto] {
            #expect(razao(cor, cartaoClaro) >= 4.5, "claro \(cor.hex)")
        }
        // Escuro: sobre o cartão grafite.
        for cor in [MX.Escuro.texto, MX.Escuro.textoSecundario, MX.Escuro.mexericaTexto, MX.Escuro.sucessoTexto] {
            #expect(razao(cor, MX.Escuro.campo) >= 4.5, "escuro \(cor.hex)")
        }
        // Texto marrom sobre o laranja do botão "Conectar um computador".
        #expect(razao(MX.Claro.sobreMexerica, MX.Claro.mexerica) >= 4.5)
    }
}

// MARK: - Conta real

@MainActor
struct RedesignAjustesDaContaRealTests {
    @Test func semComputadorAListaVaziaEOCaminhoEConectar() throws {
        let c = try cenario(computadores: 0, configurada: false)
        let dados = c.conta.dados
        #expect(dados.semComputador && dados.computadores.isEmpty && dados.emUso == nil)
        #expect(c.conta.emUsoId == nil && c.conta.storeEmUso == nil)
        // Sem conta no build: pareamento. Voz e Uso ficam desligados (não há computador).
        #expect(dados.conectar == .parear && dados.voz == .indisponivel && dados.uso == .indisponivel)
        #expect(dados.logada && dados.email == "voce@exemplo.com")
    }

    @Test func semComputadorComContaOCaminhoEAtualizarAListaDaConta() throws {
        let c = try cenario(computadores: 0, configurada: true)
        #expect(c.conta.dados.conectar == .atualizar)
    }

    @Test func umComputadorFicaEmUsoEMarcado() throws {
        let c = try cenario(computadores: 1)
        let dados = c.conta.dados
        #expect(dados.computadores.count == 1)
        let linha = try #require(dados.computadores.first)
        #expect(linha.emUso && linha.acao == .nenhuma && linha.id == c.ids[0])
        #expect(c.conta.emUsoId == c.ids[0] && c.conta.storeEmUso != nil)
    }

    @Test func variosComputadoresUmSoEmUsoOsOutrosTrocam() throws {
        let c = try cenario(computadores: 3)
        let dados = c.conta.dados
        #expect(dados.computadores.count == 3)
        #expect(dados.computadores.filter(\.emUso).count == 1)
        #expect(dados.computadores.filter { !$0.emUso }.allSatisfy { $0.acao == .usar })
        #expect(dados.emUso?.id == c.ids[0])
    }

    @Test func trocarDeComputadorMudaOEmUsoLembraEPedeParaReabrirNosAjustes() throws {
        let c = try cenario(computadores: 3)
        let alvo = c.ids[2]
        #expect(!c.conta.reabrirNosAjustes)
        c.conta.usar(alvo)
        #expect(c.conta.emUsoId == alvo && c.conta.storeEmUso?.computer.id == alvo)
        #expect(c.conta.dados.emUso?.id == alvo)
        // O mesmo "último computador" do app de hoje: voltar ao visual antigo abre no mesmo.
        #expect(c.contas.storage.lastComputerId == alvo)
        #expect(c.conta.reabrirNosAjustes)
        #expect(c.conta.dados.computadores.first(where: { $0.id == c.ids[0] })?.acao == .usar)
    }

    @Test func trocarParaOQueJaEstaEmUsoOuNaoExisteNaoFazNada() throws {
        let c = try cenario(computadores: 2)
        c.conta.usar(c.ids[0])
        #expect(!c.conta.reabrirNosAjustes && c.contas.storage.lastComputerId == nil)
        c.conta.usar("nao-existe")
        #expect(!c.conta.reabrirNosAjustes && c.conta.emUsoId == c.ids[0])
    }

    @Test func oUltimoDoAppDeHojeVenceOPrimeiro() throws {
        let c = try cenario(computadores: 2)
        c.contas.storage.lastComputerId = c.ids[1]
        #expect(c.conta.emUsoId == c.ids[1])
    }

    @Test func removerOComputadorEmUsoCaiNoOutroSemQuebrar() throws {
        let c = try cenario(computadores: 2)
        c.conta.usar(c.ids[1])
        c.contas.forget(c.ids[1])
        #expect(c.conta.emUsoId == c.ids[0] && c.conta.dados.computadores.count == 1)
        c.contas.forget(c.ids[0])
        #expect(c.conta.emUsoId == nil && c.conta.dados.semComputador)
    }

    @Test func abrirAContaChamaOAppDeHoje() throws {
        let c = try cenario(computadores: 1)
        c.conta.abrirConta()
        #expect(c.registro.contaAberta == 1)
    }

    @Test func sairQuandoDaCertoNaoDeixaErroEQuandoFalhaAvisaSemTextoBruto() async throws {
        let c = try cenario(computadores: 1)
        await c.conta.sair()
        #expect(!c.sessao.isSignedIn && c.conta.erro == nil)

        let falha = try cenario(computadores: 1)
        falha.sessao.saiu = false
        await falha.conta.sair()
        #expect(falha.sessao.isSignedIn && falha.conta.erro == .sair)
        falha.conta.limparErro()
        #expect(falha.conta.erro == nil)
    }

    @Test func atualizarSemNuvemNaoDaErro() async throws {
        let c = try cenario(computadores: 1)
        await c.conta.atualizar()
        #expect(c.conta.erro == nil && !c.conta.dados.atualizando)
    }

    @Test func avisosLigamDesligamELembram() async throws {
        let c = try cenario(computadores: 1)
        #expect(c.conta.avisosDoIphone == .ligados)
        await c.conta.carregarPermissao()
        await c.conta.alternarAvisos()
        #expect(c.conta.avisosDoIphone == .desligados)
        #expect(c.defaults.bool(forKey: MXContaReal.chaveDeAvisos) == false)
        #expect(c.registro.avisosMudaram == [false])
        await c.conta.alternarAvisos()
        #expect(c.conta.avisosDoIphone == .ligados && c.registro.avisosMudaram == [false, true])
        // Outra abertura lê o que ficou gravado.
        #expect(c.defaults.object(forKey: MXContaReal.chaveDeAvisos) as? Bool == true)
    }

    @Test func avisosNegadosNoIphoneNaoMudamNada() async throws {
        let c = try cenario(computadores: 1)
        c.registro.permissao = false
        await c.conta.carregarPermissao()
        #expect(c.conta.avisosDoIphone == .negadoNoIphone)
        await c.conta.alternarAvisos()
        #expect(c.registro.avisosMudaram.isEmpty && c.conta.avisosDoIphone == .negadoNoIphone)
    }

    @Test func ligarAvisosSemTerPerguntadoAoIosPerguntaNaHora() async throws {
        let c = try cenario(computadores: 1)
        c.defaults.set(false, forKey: MXContaReal.chaveDeAvisos)
        let conta2 = MXContaReal(contas: c.contas, ponte: MXPonteDoApp(
            sessao: c.sessao, abrirConta: {}, sair: {}, vista: { _, _ in AnyView(EmptyView()) },
            permissaoDeAvisos: { nil }, pedirPermissaoDeAvisos: { c.registro.pediuPermissao += 1; return true },
            avisosMudaram: { c.registro.avisosMudaram.append($0) }), defaults: c.defaults)
        await conta2.carregarPermissao()
        #expect(conta2.avisosDoIphone == .desligados)
        await conta2.alternarAvisos()
        #expect(c.registro.pediuPermissao == 1 && c.registro.avisosMudaram == [true])
    }
}

// MARK: - Busca explícita na nuvem e preferência de avisos

/// A lista de computadores da nuvem, com o status que o teste mandar por servidor (nome único por teste).
private final class NuvemDaListaStub: URLProtocol {
    static let respostas = OSAllocatedUnfairLock<[String: Int]>(initialState: [:])
    static let chave = Data(repeating: 5, count: 32)
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        let host = request.url?.host() ?? ""
        let status = Self.respostas.withLock { $0[host] ?? 200 }
        let id = RelayCrypto.computerId(signKey: Self.chave)
        let json = status == 200
            ? #"{"computers":[{"computerId":"\#(id)","name":"Mac da conta","signKey":"\#(Self.chave.base64URL)","online":true}]}"#
            : #"{"error":"indisponivel"}"#
        let resposta = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!
        client?.urlProtocol(self, didReceive: resposta, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(json.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
}

@MainActor
struct RedesignAjustesDaContaNuvemTests {
    private func comNuvem(_ host: String) throws -> (MXContaReal, AccountStore, SessaoFalsa) {
        let suite = "RedesignAjustesDaContaNuvem.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [NuvemDaListaStub.self]
        let nuvem = CloudClient(baseURL: URL(string: "https://\(host)")!, identity: DeviceIdentity(signing: .init(), push: .init()),
                                session: URLSession(configuration: config)) { "token" }
        let contas = AccountStore(storage: SharedStore.Context(accountID: UUID().uuidString, suite: suite), cloud: nuvem)
        contas.setActive(false)
        let sessao = SessaoFalsa()
        let ponte = MXPonteDoApp(sessao: sessao, abrirConta: {}, sair: {}, vista: { _, _ in AnyView(EmptyView()) },
                                 permissaoDeAvisos: { nil }, pedirPermissaoDeAvisos: { true }, avisosMudaram: { _ in })
        return (MXContaReal(contas: contas, ponte: ponte, defaults: defaults), contas, sessao)
    }

    @Test func buscaExplicitaFalhaTransitoriaViraAvisoMantemACopiaERecupera() async throws {
        let host = "lista-\(UUID().uuidString.lowercased()).test"
        defer { NuvemDaListaStub.respostas.withLock { $0[host] = nil } }
        let (conta, contas, _) = try comNuvem(host)
        defer { contas.retire() }
        // 1) Deu certo: a lista da conta chega e não há aviso.
        await conta.atualizar()
        #expect(conta.erro == nil && contas.cloudComputers.count == 1)
        #expect(conta.dados.computadores.count == 1)
        // 2) A nuvem fica indisponível (5xx: o refresh de fundo engole): a busca pedida avisa, a lista que já havia fica.
        for status in [503, 429] {
            NuvemDaListaStub.respostas.withLock { $0[host] = status }
            await conta.atualizar()
            #expect(conta.erro == .atualizar, "status \(status)")
            #expect(contas.cloudComputers.count == 1 && conta.dados.computadores.count == 1)
            #expect(contas.lastError == nil, "o texto bruto da nuvem não vira estado da tela")
            #expect(!conta.dados.atualizando)
        }
        // 3) Voltou: "Tentar de novo" limpa o aviso.
        NuvemDaListaStub.respostas.withLock { $0[host] = 200 }
        await conta.atualizar()
        #expect(conta.erro == nil && contas.cloudComputers.count == 1)
    }

    @Test func buscaExplicitaSemNadaNaTelaTambemAvisaDaFalha() async throws {
        let host = "lista-\(UUID().uuidString.lowercased()).test"
        NuvemDaListaStub.respostas.withLock { $0[host] = 503 }
        defer { NuvemDaListaStub.respostas.withLock { $0[host] = nil } }
        let (conta, contas, _) = try comNuvem(host)
        defer { contas.retire() }
        #expect(conta.dados.semComputador)
        await conta.atualizar()
        #expect(conta.erro == .atualizar && contas.cloudComputers.isEmpty)
        // O refresh de fundo continua engolindo a falha transitória (o contrato antigo não mudou).
        contas.lastError = nil
        await contas.refreshCloud()
        #expect(contas.lastError == nil)
        #expect(await contas.refreshCloudReportingResult() == false)
        NuvemDaListaStub.respostas.withLock { $0[host] = 200 }
        #expect(await contas.refreshCloudReportingResult() == true)
    }
}

@MainActor
struct RedesignAjustesDaContaAvisosTests {
    @Test func avisosSeguemAPreferenciaCompartilhadaNosDoisSentidos() async throws {
        let c = try cenario(computadores: 1)
        #expect(c.conta.avisosDoIphone == .ligados)
        // "Mais ajustes" (a SettingsView de hoje, `@AppStorage`) grava a mesma chave direto no UserDefaults.
        c.defaults.set(false, forKey: MXContaReal.chaveDeAvisos)
        #expect(c.conta.avisosDoIphone == .desligados)
        c.defaults.set(true, forKey: MXContaReal.chaveDeAvisos)
        #expect(c.conta.avisosDoIphone == .ligados)
        // O próximo toque parte do valor de agora, não de uma cópia antiga.
        c.defaults.set(false, forKey: MXContaReal.chaveDeAvisos)
        await c.conta.alternarAvisos()
        #expect(c.conta.avisosDoIphone == .ligados && c.registro.avisosMudaram == [true])
        c.defaults.set(true, forKey: MXContaReal.chaveDeAvisos)
        await c.conta.alternarAvisos()
        #expect(c.conta.avisosDoIphone == .desligados && c.registro.avisosMudaram == [true, false])
    }

    @Test func outraContaLeAMesmaPreferencia() throws {
        let c = try cenario(computadores: 1)
        c.defaults.set(false, forKey: MXContaReal.chaveDeAvisos)
        // Trocar de conta recria a fonte sobre outro `AccountStore`; a preferência é do aparelho e vem junto.
        let outra = MXContaReal(contas: c.contas, ponte: MXPonteDoApp(
            sessao: c.sessao, abrirConta: {}, sair: {}, vista: { _, _ in AnyView(EmptyView()) },
            permissaoDeAvisos: { nil }, pedirPermissaoDeAvisos: { true }, avisosMudaram: { _ in }), defaults: c.defaults)
        #expect(outra.avisosDoIphone == .desligados)
        c.defaults.set(true, forKey: MXContaReal.chaveDeAvisos)
        #expect(outra.avisosDoIphone == .ligados && c.conta.avisosDoIphone == .ligados)
    }

    @Test func mudancaNoUserDefaultsAcordaAObservacaoDaTela() async throws {
        let c = try cenario(computadores: 1)
        let mudou = OSAllocatedUnfairLock(initialState: false)
        withObservationTracking({ _ = c.conta.avisosDoIphone }, onChange: { mudou.withLock { $0 = true } })
        c.defaults.set(false, forKey: MXContaReal.chaveDeAvisos)
        for _ in 0..<40 where !mudou.withLock({ $0 }) { try await Task.sleep(for: .milliseconds(50)) }
        #expect(mudou.withLock { $0 }, "a tela precisa reler quando a preferência muda por fora (Mais ajustes)")
    }
}

// MARK: - Exemplo e tela

@MainActor
struct RedesignAjustesDaContaExemploTests {
    @Test func cenasDeExemploCobremVariosUmESem() {
        #expect(MXContaExemplo(cena: .varios).dados.computadores.count == 3)
        #expect(MXContaExemplo(cena: .um).dados.computadores.count == 1)
        #expect(MXContaExemplo(cena: .sem).dados.semComputador)
        #expect(MXCenaDaConta.daLinhaDeComando(["-MexericaCena", "conta-um"], ambiente: [:]) == .um)
        #expect(MXCenaDaConta.daLinhaDeComando([], ambiente: ["MEXERICA_CENA": "conta-sem"]) == .sem)
        #expect(MXCenaDaConta.daLinhaDeComando(["-MexericaCena", "conversa"], ambiente: [:]) == nil)
    }

    @Test func variosCobreOsEstadosDoPlano() {
        let linhas = MXContaExemplo(cena: .varios).dados.computadores
        #expect(linhas[0].estado == .conectado && linhas[0].emUso)
        if case .desligado(visto: .some) = linhas[1].estado {} else { Issue.record("o 2º deveria estar desligado com visto") }
        #expect(linhas[2].estado == .daConta(ligado: true) && linhas[2].acao == .pedirAcesso)
    }

    @Test func trocarNoExemploMudaAMarca() {
        let conta = MXContaExemplo(cena: .varios)
        conta.usar("exemplo-macmini")
        let linhas = conta.dados.computadores
        #expect(linhas[1].emUso && !linhas[0].emUso && linhas[0].acao == .usar && linhas[1].acao == .nenhuma)
        // O que pede acesso não vira "em uso" por toque.
        conta.usar("exemplo-estudio")
        #expect(!conta.dados.computadores[2].emUso)
    }

    @Test func oMockupUsaSoDadosDeExemplo() {
        let dados = MXContaExemplo(cena: .varios).dados
        let nomes = dados.computadores.map(\.nome) + [dados.email ?? ""]
        #expect(nomes.allSatisfy { $0.contains("exemplo") })
    }

    /// A tela inteira em claro e escuro, no tamanho padrão e em acessibilidade (AX2), com 3, 1 e 0 computadores: monta, mede
    /// e cabe na largura do iPhone (nada passa da borda no texto grande).
    @Test func telaCabeNaLarguraEmTodosOsTemasETamanhos() {
        let largura: CGFloat = 390
        for cena in MXCenaDaConta.allCases {
            for esquema in [ColorScheme.light, .dark] {
                for tamanho in [DynamicTypeSize.large, .accessibility2, .accessibility5] {
                    let tela = MXAjustesDaContaTela(conta: MXContaExemplo(cena: cena), esquema: esquema, voltar: {})
                        .environment(\.dynamicTypeSize, tamanho)
                    let host = UIHostingController(rootView: tela)
                    let medida = host.sizeThatFits(in: CGSize(width: largura, height: 3000))
                    #expect(medida.width <= largura + 0.5, "\(cena) \(esquema) \(tamanho): \(medida)")
                    #expect(medida.height > 100)
                }
            }
        }
    }
}

// MARK: - Imagens da tela (sem simulador)

/// Renderiza a tela inteira, na altura natural (390 pt de largura), em Mac Catalyst (`ImageRenderer`) e grava PNGs quando a variável
/// `TEST_RUNNER_MX_CAPTURAS_DIR` aponta uma pasta (sem ela, não faz nada). NÃO são capturas do simulador: o Liquid Glass e
/// o blur podem divergir, e o Catalyst não escala a fonte com o Dynamic Type (`UIFontMetrics`), então no "AX2" só o arranjo
/// empilhado muda, não o tamanho do texto.
@MainActor
struct RedesignAjustesDaContaImagensTests {
    private func gravar(_ conta: MXContaFonte, esquema: ColorScheme, tamanho: DynamicTypeSize, nome: String, pasta: URL) throws {
        MXFontes.registrar()
        let tela = MXAjustesDaContaTela(conta: conta, esquema: esquema, voltar: {}, textos: MXTextosDaConta(ingles: false), semRolagem: true)
            .environment(\.dynamicTypeSize, tamanho)
            .safeAreaPadding(.top, 54)
        // `ImageRenderer` (SwiftUI puro): uma `UIWindow` não nasce num xctest de Catalyst sem cena. Peças de UIKit (a chave do
        // iOS, o vidro) podem sair diferentes das do aparelho.
        let renderizador = ImageRenderer(content: tela.frame(width: 390).fixedSize(horizontal: false, vertical: true).background(Color.black))
        renderizador.scale = 2
        renderizador.proposedSize = ProposedViewSize(width: 390, height: nil)
        let imagem = try #require(renderizador.uiImage)
        let dados = try #require(imagem.pngData())
        try dados.write(to: pasta.appendingPathComponent("\(nome).png"))
    }

    @Test func gravaAsImagensDosEstados() throws {
        guard let caminho = ProcessInfo.processInfo.environment["MX_CAPTURAS_DIR"] else { return }
        let pasta = URL(fileURLWithPath: caminho)
        try FileManager.default.createDirectory(at: pasta, withIntermediateDirectories: true)
        for (cena, nome) in [(MXCenaDaConta.varios, "varios"), (.um, "um"), (.sem, "sem")] {
            for (esquema, tema) in [(ColorScheme.light, "claro"), (.dark, "escuro")] {
                try gravar(MXContaExemplo(cena: cena), esquema: esquema, tamanho: .large, nome: "\(nome)-\(tema)-padrao", pasta: pasta)
            }
        }
        for (esquema, tema) in [(ColorScheme.light, "claro"), (.dark, "escuro")] {
            try gravar(MXContaExemplo(cena: .varios), esquema: esquema, tamanho: .accessibility2, nome: "varios-\(tema)-ax2", pasta: pasta)
        }
    }
}
#endif
