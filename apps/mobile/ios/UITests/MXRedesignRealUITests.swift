import XCTest

/// Prova integrada do redesenho no app compilado: lança o `iOS Dev` com
/// `-MexericaRedesignReal`, pareia no Hub de teste local (WebSocket 127.0.0.1 no host,
/// dados sintéticos, processo separado — `hub-host/`) pelo gancho de Debug
/// `CODYNC_PAIR_URL` e dirige o fluxo só com gestos sintéticos do XCUITest:
/// abrir a conversa, puxar a linha para Sessões, segurar para Ajustes, abrir a sessão
/// extra, voltar ao destino certo e enviar na extra sem a mensagem aparecer na Principal.
///
/// O Hub precisa estar no host porque o simulador manda 127.0.0.1 para o loopback do Mac;
/// um listener dentro do runner ficaria inalcançável para o app. O estado do Hub chega
/// pelo canal de controle (ver MXHubControle).
final class MXRedesignRealUITests: XCTestCase {
    private var app: XCUIApplication!
    private let controle = MXHubControle()
    private var pairingURL = ""

    private func fioDoHub() -> String {
        controle.estado()?.fio.joined(separator: " · ") ?? "<sem contato com o hub>"
    }

    override func setUpWithError() throws {
        continueAfterFailure = false
        guard let estado = controle.estado() else {
            throw NSError(domain: "MXRedesignRealUITests", code: 1, userInfo: [
                NSLocalizedDescriptionKey: "o Hub de teste não está no ar no host; suba com `swiftc -O apps/mobile/ios/UITests/hub-host/*.swift -o mx-hub && ./mx-hub`",
            ])
        }
        pairingURL = estado.pairingURL
        app = XCUIApplication(bundleIdentifier: "app.mexerica.ios.dev")
        app.launchArguments = ["-MexericaRedesignReal"]
        app.launchEnvironment["CODYNC_PAIR_URL"] = pairingURL
        var safePairingURL = URLComponents(string: pairingURL)
        safePairingURL?.user = nil
        safePairingURL?.password = nil
        safePairingURL?.query = nil
        safePairingURL?.fragment = nil
        print("[hubteste] pairing URL: \(safePairingURL?.string ?? "<invalida>")")
        app.launch()
    }

    /// Reserva de pareamento: o orquestrador (shell) entrega o link por `simctl openurl`;
    /// aqui só aceitamos o diálogo "Abrir" do SpringBoard. `app.open` registra qualquer erro
    /// de Pairing/pair em `accounts.lastError`, que o RootView mostra como alerta legível.
    private func parearPeloLink() {
        do { try XCUIDevice.shared.system.open(URL(string: pairingURL)!) }
        catch { print("[hubteste] system.open falhou: \(error)") }
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let abrir = springboard.buttons["Abrir"]
        _ = abrir.waitForExistence(timeout: 30)
        if abrir.exists { abrir.tap() }
    }

    override func tearDownWithError() throws {
        app?.terminate()
        app = nil
    }

    // MARK: - Ajudantes

    private func esperar(_ elemento: XCUIElement, _ segundos: TimeInterval = 20, _ descricao: String) {
        XCTAssertTrue(elemento.waitForExistence(timeout: segundos),
                      "não apareceu: \(descricao) | fio do hub: \(fioDoHub())")
    }

    /// A linha do agente na aba Conversas: rótulo "Berga, Pessoal, <estado>, <mensagem>".
    private func linhaDoAgente() -> XCUIElement {
        app.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH[c] 'Berga,'"))
            .firstMatch
    }

    /// A linha da sessão extra na tela Sessões: "Sessão extra de teste, <info>".
    private func linhaDaExtra() -> XCUIElement {
        app.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH[c] 'Sessão extra de teste'"))
            .firstMatch
    }

    private func botaoVoltar() -> XCUIElement {
        app.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH[c] 'Voltar para'"))
            .firstMatch
    }

    private func capturar(_ nome: String) {
        let anexo = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        anexo.name = nome
        anexo.lifetime = .keepAlways
        add(anexo)
    }

    private func voltar() {
        esperar(botaoVoltar(), 5, "botão Voltar")
        botaoVoltar().tap()
    }

    /// O gesto da linha: arrastar para a esquerda passa do limiar e abre Sessões ao soltar.
    private func puxarParaSessoes(_ linha: XCUIElement) {
        let inicio = linha.coordinate(withNormalizedOffset: CGVector(dx: 0.85, dy: 0.5))
        let fim = linha.coordinate(withNormalizedOffset: CGVector(dx: 0.10, dy: 0.5))
        inicio.press(forDuration: 0.05, thenDragTo: fim, withVelocity: .default, thenHoldForDuration: 0.1)
    }

    /// A folha de Ajustes: o `.sheet` do SwiftUI aparece na árvore como `Other`
    /// ("Configurações"), não como `Sheet` — o marcador estável é o botão "Pronto".
    private func botaoProntoDaFolha() -> XCUIElement { app.buttons["Pronto"] }

    /// Segurar para Ajustes: toque contínuo de 0,7 s (limiar 0,55) seguido de um
    /// micro-arrasto de ~7 pt à direita. dx > 0 não abre Sessões; o deslocamento fica
    /// dentro do maximumDistance (10 pt) do LongPressGesture.
    private func segurarParaAjustes() -> XCUIElement {
        let de = linhaDoAgente().coordinate(withNormalizedOffset: CGVector(dx: 0.45, dy: 0.5))
        let ate = linhaDoAgente().coordinate(withNormalizedOffset: CGVector(dx: 0.47, dy: 0.5))
        de.press(forDuration: 0.7, thenDragTo: ate)
        return botaoProntoDaFolha()
    }

    // MARK: - O fluxo provado

    func testFluxoRealGestosSessaoExtraEEnvio() throws {
        // 1. Pareia no hub de teste e a lista real abre com o agente "Berga" (fonte real, não exemplo).
        //    O gancho CODYNC_PAIR_URL pareia no launch; se a linha não vier, entrega o link pelo
        //    sistema (onOpenURL) como reserva — mesmo Pairing, mesmo pair().
        if !linhaDoAgente().waitForExistence(timeout: 12) {
            parearPeloLink()
        }
        let linha = linhaDoAgente()
        if !linha.waitForExistence(timeout: 45) {
            let alerta = app.alerts.firstMatch
            var partes: [String] = []
            if alerta.exists { for e in alerta.staticTexts.allElementsBoundByIndex { partes.append(e.label) } }
            XCTFail("não apareceu: linha do agente Berga | alerta: \(partes.isEmpty ? "nenhum" : partes.joined(separator: " | ")) | fio do hub: \(fioDoHub())")
        }
        capturar("01-lista-com-berga")

        // 2. Abre a conversa tocando na linha.
        linha.tap()
        let caixa = app.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS[c] 'Mensagem para'")).firstMatch
        esperar(caixa, 15, "caixa de mensagem da conversa")
        XCTAssertTrue(app.staticTexts["Resposta do hub de teste"].waitForExistence(timeout: 10),
                      "mensagem semeada da Principal não apareceu | fio do hub: \(fioDoHub())")
        capturar("02-conversa-principal")
        voltar()
        esperar(linhaDoAgente(), 10, "volta à lista")

        // 3. Puxa a linha da direita para a esquerda: abre Sessões (gesto sintético do runner).
        puxarParaSessoes(linhaDoAgente())
        esperar(linhaDaExtra(), 15, "tela Sessões com a sessão extra do Hub")
        capturar("03-sessoes")
        voltar()
        esperar(linhaDoAgente(), 10, "volta à lista")

        // 4. Segura a linha: abre a folha de Ajustes (long-press 0,55 s do gesto).
        //    O press() puro do XCUI solta como tap no botão e abriu a conversa na 1ª
        //    corrida; segurar + micro-arrasto mantém o fluxo de toque real que o
        //    LongPressGesture reconhece. O arrasto é à direita e ~7 pt: dx > 0 não
        //    aciona Sessões e fica abaixo do maximumDistance (10) do long-press.
        var pronto = segurarParaAjustes()
        if !pronto.waitForExistence(timeout: 4) {
            // Se mesmo assim virou tap (conversa aberta), volta e tenta o press na
            // coordenada, fora do caminho de ativação do elemento acessível.
            if botaoVoltar().waitForExistence(timeout: 3) {
                voltar()
                esperar(linhaDoAgente(), 10, "volta à lista antes de repetir o segurar")
                linhaDoAgente().coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                    .press(forDuration: 1.2)
                pronto = botaoProntoDaFolha()
            }
        }
        esperar(pronto, 10, "folha de Ajustes pelo segurar")
        capturar("04-ajustes")
        pronto.tap()
        esperar(linhaDoAgente(), 10, "folha fechada, lista de volta")

        // 5. Puxa de novo e abre a sessão extra do Hub de teste.
        puxarParaSessoes(linhaDoAgente())
        esperar(linhaDaExtra(), 15, "Sessões")
        linhaDaExtra().tap()
        let mensagemDaExtra = app.staticTexts["Mensagem inicial da sessão extra"]
        esperar(mensagemDaExtra, 15, "conversa da sessão extra")
        capturar("05-conversa-sessao-extra")

        // 6. Envia na sessão extra: sai com sessionId e o hub responde na mesma sessão.
        let textoEnviado = "mensagem de teste na sessão extra"
        esperar(caixa, 10, "caixa da sessão extra")
        caixa.tap()
        app.typeText(textoEnviado)
        let enviar = app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'Enviar'")).firstMatch
        esperar(enviar, 5, "botão Enviar")
        enviar.tap()
        esperar(app.staticTexts["Resposta do hub de teste na sessão extra"], 15,
                "resposta automática do hub na sessão extra")
        capturar("06-enviada-na-extra")

        // 7. Volta: o destino correto é a tela Sessões (pilha lista → sessões → sessão).
        voltar()
        esperar(linhaDaExtra(), 10, "volta da sessão extra para Sessões")
        voltar()
        esperar(linhaDoAgente(), 10, "volta à lista")

        // 8. Abre a Principal: a mensagem enviada na extra NÃO pode estar aqui.
        linhaDoAgente().tap()
        esperar(caixa, 15, "conversa principal de volta")
        XCTAssertFalse(app.staticTexts[textoEnviado].exists,
                       "a mensagem da sessão extra vazou para a Principal")
        XCTAssertFalse(app.staticTexts["Resposta do hub de teste na sessão extra"].exists,
                       "a resposta da sessão extra vazou para a Principal")
        XCTAssertTrue(app.staticTexts["Resposta do hub de teste"].exists,
                      "a mensagem da Principal sumiu")
        capturar("07-principal-sem-a-extra")

        // 9. O hub registrou o send COM sessionId da extra — prova no fio, não só na tela.
        guard let estado = controle.estado() else {
            XCTFail("o Hub de teste parou de responder no canal de controle")
            return
        }
        let envios = estado.chamadas.filter { $0["m"] as? String == "send" }
        XCTAssertEqual(envios.count, 1, "o hub deveria ter recebido um send | fio: \(fioDoHub())")
        let corpo = envios.first?["b"] as? [String: Any]
        XCTAssertEqual(corpo?["sessionId"] as? String, "extra-1",
                       "o send saiu sem sessionId da sessão extra | fio: \(fioDoHub())")
    }
}
