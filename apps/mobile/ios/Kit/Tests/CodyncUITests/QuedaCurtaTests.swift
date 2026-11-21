import Foundation
import Testing
import CodyncKit
@testable import CodyncUI

/// The short-drop rules and texts, without a store or a clock.

@MainActor @Test func dropStatesFollowTheConnection() {
    let t = Date(timeIntervalSince1970: 1_000)
    #expect(EstadoConexao.proximo(.conectado, nova: .connecting, agora: t) == .reconectando(desde: t))
    #expect(EstadoConexao.proximo(.conectado, nova: .offline("x"), agora: t) == .reconectando(desde: t))
    #expect(EstadoConexao.proximo(.conectado, nova: .computerOffline(lastSeen: nil), agora: t) == .reconectando(desde: t))
    // A drop keeps counting from when it began.
    let later = t.addingTimeInterval(60)
    #expect(EstadoConexao.proximo(.reconectando(desde: t), nova: .offline("x"), agora: later) == .reconectando(desde: t))
    #expect(EstadoConexao.proximo(.reconectando(desde: t), nova: .online, agora: later) == .conectado)
    // Definitive failures don't wait.
    #expect(EstadoConexao.proximo(.conectado, nova: .unauthorized("revoked"), agora: t) == .desconectado(.acessoEncerrado("revoked")))
    #expect(EstadoConexao.proximo(.reconectando(desde: t), nova: .unauthorized("revoked"), agora: t) == .desconectado(.acessoEncerrado("revoked")))
    // Disconnected leaves only by coming back.
    #expect(EstadoConexao.proximo(.desconectado(.prazoLongo), nova: .connecting, agora: t) == .desconectado(.prazoLongo))
    #expect(EstadoConexao.proximo(.desconectado(.prazoLongo), nova: .online, agora: t) == .conectado)
}

@Test func dropDeadlineIsFiveMinutes() {
    let drop = EstadoConexao.reconectando(desde: Date(timeIntervalSince1970: 1_000))
    #expect(drop.depoisDe(.seconds(4 * 60 + 59)) == drop)
    #expect(drop.depoisDe(.seconds(5 * 60)) == .desconectado(.prazoLongo))
    #expect(EstadoConexao.conectado.depoisDe(.seconds(3600)) == .conectado)
}

@Test func refusalsThatEndAccess() {
    #expect(HostError.unauthorized("revoked").recusaDefinitiva(identificando: false) == "revoked")
    #expect(HostError.http(401, "").recusaDefinitiva(identificando: false) != nil)
    #expect(HostError.http(403, "Revoked.").recusaDefinitiva(identificando: true) == "Revoked.")
    // One action refused by the Hub is not the end of access.
    #expect(HostError.http(403, "Only on the computer.").recusaDefinitiva(identificando: false) == nil)
    #expect(HostError.unreachable.recusaDefinitiva(identificando: true) == nil)
}

@Test func mirrorIsReusedOnlyForTheSameAccountComputerAndKey() {
    let same = DonoDoEspelho(conta: "user_1", computador: "c1", chave: "k1")
    #expect(DonoDoEspelho.podeReaproveitar(same, same))
    #expect(!DonoDoEspelho.podeReaproveitar(same, DonoDoEspelho(conta: "user_2", computador: "c1", chave: "k1")))
    #expect(!DonoDoEspelho.podeReaproveitar(same, DonoDoEspelho(conta: nil, computador: "c1", chave: "k1")))
    #expect(!DonoDoEspelho.podeReaproveitar(same, DonoDoEspelho(conta: "user_1", computador: "c2", chave: "k1")))
    #expect(!DonoDoEspelho.podeReaproveitar(same, DonoDoEspelho(conta: "user_1", computador: "c1", chave: "k2")))
}

@Test func dropTextsInTheSixLanguages() throws {
    #expect(Set(QuedaTextos.porIdioma.keys) == ["pt-BR", "en", "es", "fr", "zh-CN", "ar-AE"])
    let expected: [(locale: String, key: String, titulo: String, tentar: String, tentando: String)] = [
        ("pt_BR", "pt-BR", "Sem conexão — reconectando…", "Tentar agora", "Tentando…"),
        ("en_US", "en", "No connection — reconnecting…", "Try now", "Trying…"),
        ("es_ES", "es", "Sin conexión — reconectando…", "Intentar ahora", "Intentando…"),
        ("fr_FR", "fr", "Pas de connexion — reconnexion…", "Réessayer maintenant", "Nouvelle tentative…"),
        ("zh_Hans_CN", "zh-CN", "无连接——正在重新连接…", "立即重试", "正在重试…"),
        ("ar_AE", "ar-AE", "لا يوجد اتصال — جارٍ إعادة الاتصال…", "حاول الآن", "جارٍ المحاولة…"),
    ]
    for row in expected {
        let textos = QuedaTextos.para(Locale(identifier: row.locale))
        #expect(textos == QuedaTextos.porIdioma[row.key])
        #expect(textos.titulo == row.titulo)
        #expect(textos.tentar == row.tentar)
        #expect(textos.tentando == row.tentando)
        for text in [textos.detalhe, textos.semEnvio, textos.semAprovar] { #expect(!text.isEmpty) }
    }
    let pt = QuedaTextos.para(Locale(identifier: "pt_BR"))
    #expect(pt.detalhe == "Suas conversas continuam aqui. Enviar e aprovar voltam quando reconectar; o que você digitar fica guardado.")
    #expect(pt.semEnvio == "Sem conexão: o texto fica guardado e você envia quando reconectar.")
    #expect(pt.semAprovar == "Sem conexão: permitir e recusar voltam quando reconectar.")
    let en = QuedaTextos.para(Locale(identifier: "en_US"))
    #expect(en.detalhe == "Your conversations stay here. Sending and approving come back once it reconnects; what you type is kept.")
    #expect(en.semEnvio == "No connection: your text is kept, send it once it reconnects.")
    #expect(en.semAprovar == "No connection: allowing and denying come back once it reconnects.")
    // Any other language reads English.
    #expect(QuedaTextos.para(Locale(identifier: "de_DE")) == en)
}
