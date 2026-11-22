import XCTest

/// O interruptor "Novo visual (teste)" no app compilado (Debug): "Voltar ao visual antigo" tem de levar ao app de hoje
/// mesmo com o interruptor lembrado ligado e um argumento de lançamento ativo, e os argumentos sozinhos continuam
/// funcionando. Sem Hub e sem computador pareado (o redesenho real abre nos Ajustes, com "Conectar um computador").
/// O interruptor lembrado vem de `-mexericaNovoVisualTeste YES` (domínio de argumentos do `UserDefaults`).
final class MXNovoVisualUITests: XCTestCase {
    private func abrir(_ argumentos: [String]) -> XCUIApplication {
        let app = XCUIApplication(bundleIdentifier: "app.mexerica.ios.dev")
        app.launchArguments = ["-AppleLanguages", "(pt-BR)", "-AppleLocale", "pt_BR"] + argumentos
        app.launch()
        return app
    }

    private func faixa(_ app: XCUIApplication) -> XCUIElement { app.buttons["Voltar ao visual antigo"] }
    /// O título do redesenho sem computador (hoje a raiz são os Ajustes, com "Conectar um computador"); o app de hoje nunca
    /// mostra esse texto.
    private func redesenhoSemComputador(_ app: XCUIApplication) -> XCUIElement { app.staticTexts["Ajustes"] }
    /// O botão "Computadores" do app de hoje (lista de bots vazia).
    private func appDeHoje(_ app: XCUIApplication) -> XCUIElement { app.buttons["Computadores"].firstMatch }

    private func conferirVoltaAoAntigo(_ argumentos: [String]) {
        let app = abrir(argumentos)
        let voltar = faixa(app)
        XCTAssertTrue(voltar.waitForExistence(timeout: 30), "a faixa deveria aparecer com o interruptor ligado")
        XCTAssertTrue(redesenhoSemComputador(app).exists, "deveria estar no redesenho")
        voltar.tap()
        XCTAssertTrue(appDeHoje(app).waitForExistence(timeout: 15), "deveria voltar ao app de hoje")
        XCTAssertFalse(faixa(app).exists)
        XCTAssertFalse(redesenhoSemComputador(app).exists, "não pode ficar no redesenho")
    }

    func testInterruptorLigadoComArgumentoRealVoltaAoAntigo() {
        conferirVoltaAoAntigo(["-MexericaRedesignReal", "-mexericaNovoVisualTeste", "YES"])
    }

    func testInterruptorLigadoComArgumentoExemploVoltaAoAntigo() {
        conferirVoltaAoAntigo(["-MexericaRedesign", "-mexericaNovoVisualTeste", "YES"])
    }

    func testInterruptorLigadoSemArgumentoVoltaAoAntigoSemComputador() {
        conferirVoltaAoAntigo(["-mexericaNovoVisualTeste", "YES"])
    }

    func testArgumentoRealSozinhoContinuaFuncionando() {
        let app = abrir(["-MexericaRedesignReal"])
        XCTAssertTrue(redesenhoSemComputador(app).waitForExistence(timeout: 30), "o argumento real abre o redesenho")
        XCTAssertFalse(faixa(app).exists, "sem o interruptor não há faixa")
    }

    func testArgumentoExemploSozinhoContinuaFuncionando() {
        let app = abrir(["-MexericaRedesign"])
        let berga = app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH[c] 'Berga,'")).firstMatch
        XCTAssertTrue(berga.waitForExistence(timeout: 30), "o argumento de exemplo abre o redesenho com os dados de exemplo")
        XCTAssertFalse(faixa(app).exists, "sem o interruptor não há faixa")
    }
}
