#if DEBUG
import Foundation
import Testing
@testable import CodyncKit
@testable import CodyncUI

/// Interruptor "Novo visual (teste)" dos Ajustes (só Debug): padrão, persistência, o que o app abre e os textos.
struct RedesignNovoVisualTests {
    /// Um `UserDefaults` descartável, para nunca tocar o do app.
    private func padroes() throws -> (UserDefaults, String) {
        let nome = "RedesignNovoVisual.\(UUID().uuidString)"
        return (try #require(UserDefaults(suiteName: nome)), nome)
    }

    @Test func nasceDesligado() throws {
        let (p, nome) = try padroes()
        defer { p.removePersistentDomain(forName: nome) }
        #expect(MXNovoVisual.ligado(p) == false)
    }

    @Test func oEstadoFicaLembradoEntreAberturas() throws {
        let (p, nome) = try padroes()
        defer { p.removePersistentDomain(forName: nome) }
        p.set(true, forKey: MXNovoVisual.chave)
        // "Outra abertura": um objeto novo sobre o mesmo armazenamento.
        let depois = try #require(UserDefaults(suiteName: nome))
        #expect(MXNovoVisual.ligado(depois))
        depois.set(false, forKey: MXNovoVisual.chave)
        #expect(MXNovoVisual.ligado(p) == false)
    }

    @Test func aChaveEhAMesmaDoAppStorageDosAjustes() {
        // Os Ajustes e a casca usam `@AppStorage(MXNovoVisual.chave)`; mudar a chave perderia o estado de quem testa.
        #expect(MXNovoVisual.chave == "mexericaNovoVisualTeste")
    }

    @Test func desligadoSemArgumentoAbreOAppDeHoje() {
        #expect(MXNovoVisual.abre(interruptor: false, semLogin: false, real: false) == .antigo)
    }

    @Test func ligadoAbreOCaminhoRealComoOArgumentoReal() {
        #expect(MXNovoVisual.abre(interruptor: true, semLogin: false, real: false) == .real)
        #expect(MXNovoVisual.abre(interruptor: true, semLogin: false, real: false)
                    == MXNovoVisual.abre(interruptor: false, semLogin: false, real: true))
        // Com `-MexericaRedesign` junto, vence o real (o interruptor pede dados reais).
        #expect(MXNovoVisual.abre(interruptor: true, semLogin: true, real: false) == .real)
    }

    @Test func osArgumentosDeLancamentoContinuamValendoComOInterruptorDesligado() {
        #expect(MXNovoVisual.abre(interruptor: false, semLogin: true, real: false) == .exemplo)
        #expect(MXNovoVisual.abre(interruptor: false, semLogin: false, real: true) == .real)
        #expect(MXNovoVisual.abre(interruptor: false, semLogin: true, real: true) == .real)
    }

    @Test func voltarAoAntigoVenceOsArgumentosDeLancamento() {
        // Interruptor lembrado ligado + cada argumento de lançamento: o botão da faixa leva ao antigo.
        for (semLogin, real) in [(false, false), (true, false), (false, true), (true, true)] {
            #expect(MXNovoVisual.abre(interruptor: true, semLogin: semLogin, real: real) == .real)
            #expect(MXNovoVisual.abre(interruptor: false, semLogin: semLogin, real: real, voltouAoAntigo: true) == .antigo)
            // Mesmo que a chave continue lida como ligada (argumento `-mexericaNovoVisualTeste YES`), o botão vence.
            #expect(MXNovoVisual.abre(interruptor: true, semLogin: semLogin, real: real, voltouAoAntigo: true) == .antigo)
        }
    }

    @Test func fluxoDaFaixaComArgumentoDeLancamentoELigarDeNovo() throws {
        let (p, nome) = try padroes()
        defer { p.removePersistentDomain(forName: nome) }
        for (semLogin, real) in [(false, true), (true, false)] {
            p.set(true, forKey: MXNovoVisual.chave)
            var voltou = false
            #expect(MXNovoVisual.abre(interruptor: MXNovoVisual.ligado(p), semLogin: semLogin, real: real, voltouAoAntigo: voltou) == .real)
            // Toca em "Voltar ao visual antigo": grava desligado e marca a abertura.
            p.set(false, forKey: MXNovoVisual.chave)
            voltou = true
            #expect(MXNovoVisual.abre(interruptor: MXNovoVisual.ligado(p), semLogin: semLogin, real: real, voltouAoAntigo: voltou) == .antigo)
            // Ligar de novo nos Ajustes limpa a marca (`MXCascaDoApp.onChange`) e o redesenho volta.
            p.set(true, forKey: MXNovoVisual.chave)
            voltou = false
            #expect(MXNovoVisual.abre(interruptor: MXNovoVisual.ligado(p), semLogin: semLogin, real: real, voltouAoAntigo: voltou) == .real)
            p.set(false, forKey: MXNovoVisual.chave)
        }
    }

    @Test func semOBotaoOsArgumentosFuncionamComoAntes() {
        #expect(MXNovoVisual.abre(interruptor: false, semLogin: true, real: false, voltouAoAntigo: false) == .exemplo)
        #expect(MXNovoVisual.abre(interruptor: false, semLogin: false, real: true, voltouAoAntigo: false) == .real)
    }

    @Test func textosDoInterruptorNosDoisIdiomas() {
        #expect(MXTextosDebug.interruptor(ingles: false) == "Novo visual (em teste) — pode ter falhas")
        #expect(MXTextosDebug.interruptor(ingles: true) == "New look (testing) — may have bugs")
        #expect(MXTextosDebug.faixa(ingles: false) == "Novo visual (em teste)" && MXTextosDebug.faixa(ingles: true) == "New look (testing)")
        #expect(MXTextosDebug.voltar(ingles: false) == "Voltar ao visual antigo" && MXTextosDebug.voltar(ingles: true) == "Back to the old look")
        #expect(MXTextosDebug.ajuda(ingles: false).hasPrefix("Só nas versões de teste.") && MXTextosDebug.ajuda(ingles: true).hasPrefix("Test builds only."))
        #expect(MXTextosDebug.semComputador(ingles: false).hasPrefix("Nenhum computador conectado. Volte ao visual antigo"))
        #expect(MXTextosDebug.semComputador(ingles: true).hasPrefix("No computer connected. Go back to the old look"))
    }

    @Test func idiomaInglesSoParaEn() {
        #expect(MXTextosDebug.ingles("en") && MXTextosDebug.ingles("en-US") && MXTextosDebug.ingles("EN"))
        #expect(!MXTextosDebug.ingles("pt-BR") && !MXTextosDebug.ingles("de") && !MXTextosDebug.ingles(nil))
    }
}
#endif
