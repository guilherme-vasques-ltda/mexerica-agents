import CryptoKit
import Foundation

/// Hub de teste descartável para a prova integrada do redesenho: um "computador"
/// falso, gerado no próprio teste e sem nenhum dado sensível, que fala o canal criptografado do
/// Hub (§6: handshake Ed25519/X25519/HKDF, frames ChaCha20-Poly1305) sobre um WebSocket local em
/// 127.0.0.1. O app pareia por `CODYNC_PAIR_URL` (gancho de Debug) e liga como num computador real.
///
/// Serve um agente "Berga" com uma sessão extra aberta ("Sessão extra de teste"), mensagens
/// sintéticas nas duas conversas e eco automático no `send` — o suficiente para exercitar gestos,
/// sessões, ajustes, envio por sessão e a árvore acessível.
final class MXHubDeTeste: @unchecked Sendable {
    // MARK: - base64url (sem padding), como o RelayCrypto

    static func b64(_ data: Data) -> String {
        data.base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }

    static func b64(_ s: String) -> Data? {
        var t = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while t.count % 4 != 0 { t += "=" }
        return Data(base64Encoded: t)
    }

    // MARK: - Identidade do "computador" (descartável, gerada no teste)

    private let signKey = Curve25519.Signing.PrivateKey()
    private let boxKey = Curve25519.KeyAgreement.PrivateKey()
    private let pairingCode = Data((0 ..< 16).map { _ in UInt8.random(in: .min ... .max) })

    private var cidRaw: Data { Data(SHA256.hash(data: signKey.publicKey.rawRepresentation)).prefix(16) }
    var computerId: String { Self.b64(cidRaw) }

    private var porta: UInt16 = 0
    private let fila = DispatchQueue(label: "mx.hubdeteste")

    /// O link de pareamento injetado no app (`CODYNC_PAIR_URL`): só vale nesta execução.
    var pairingURL: String {
        "donnabot-dev://pair?v=3&name=Hub%20de%20teste&id=\(computerId)" +
            "&sk=\(Self.b64(signKey.publicKey.rawRepresentation))" +
            "&bk=\(Self.b64(boxKey.publicKey.rawRepresentation))" +
            "&code=\(Self.b64(pairingCode))" +
            "&urls=http://127.0.0.1:\(porta)"
    }

    /// Chamadas recebidas, em ordem — a asserção do teste confere `send` com `sessionId`.
    /// Escritas na `fila`; a leitura sincroniza nela.
    private var _chamadas: [(String, [String: Any])] = []
    var chamadas: [(String, [String: Any])] { fila.sync { _chamadas } }

    /// Diagnóstico do fio: quantas conexões chegaram e o que cada uma trouxe.
    private var _fio: [String] = []
    var fio: [String] { fila.sync { _fio } }
    private func anotar(_ texto: String) {
        _fio.append(texto)
        print("[hubteste] \(texto)")
    }

    var portaPublica: UInt16 { fila.sync { porta } }

    /// Estado para o canal de controle no host: porta do canal, URL de pareamento, chamadas
    /// recebidas e o fio de diagnóstico — tudo serializável em JSON. O teste lê isto por TCP.
    func estadoJSON() -> String {
        fila.sync {
            let cham = _chamadas.map { ["m": $0.0, "b": $0.1] as [String: Any] }
            let obj: [String: Any] = [
                "hub": "mexerica-teste",
                "porta": Int(porta),
                "pairingURL": pairingURL,
                "chamadas": cham,
                "fio": _fio,
            ]
            guard let dados = try? JSONSerialization.data(withJSONObject: obj) else { return "{}" }
            return String(decoding: dados, as: UTF8.self)
        }
    }

    // MARK: - Estado sintético do Hub

    private var rev: Int64 = 100
    private var seq: Int64 = 0
    private var turno: Int64 = 0
    private var entradas: [[String: Any]] = []

    static let botId = "berga"
    static let sessaoExtra = "extra-1"

    private func agora() -> Int64 { Int64(Date().timeIntervalSince1970 * 1000) }

    private func novaEntrada(_ kind: String, dados: [String: Any], sessao: String?) -> [String: Any] {
        seq += 1
        rev += 1
        var e: [String: Any] = [
            "id": "e\(seq)", "seq": seq, "botId": Self.botId, "rev": rev, "kind": kind, "turn": turno,
            "data": dados, "createdAt": agora(), "updatedAt": agora(),
        ]
        if let sessao { e["sessionId"] = sessao }
        return e
    }

    private func semear() {
        turno += 1
        entradas.append(novaEntrada("user", dados: ["text": "Olá do teste", "status": "sent"], sessao: nil))
        entradas.append(novaEntrada("agent", dados: ["text": "Resposta do hub de teste", "final": true], sessao: nil))
        entradas.append(novaEntrada("agent", dados: ["text": "Mensagem inicial da sessão extra", "final": true],
                                                 sessao: Self.sessaoExtra))
    }

    // MARK: - Ciclo de vida

    /// Sobe o servidor WebSocket sobre socket POSIX cru: o NWListener do macOS falha com
    /// EINVAL nesta máquina, então o accept e o upgrade RFC 6455 são feitos à mão.
    /// Devolve quando a porta está no ar (ou falha).
    func iniciar() throws {
        semear()
        socketCanal = try abrirEscuta(porta: 0)
        aceitando = true
        filaAceite.async { [self] in lacoAceite() }
    }

    func parar() {
        aceitando = false
        if socketCanal >= 0 { close(socketCanal); socketCanal = -1 }
    }

    // MARK: - Socket POSIX (canal WebSocket)

    private var socketCanal: Int32 = -1
    private var aceitando = false
    private let filaAceite = DispatchQueue(label: "mx.hubdeteste.aceite")

    /// `bind` em 127.0.0.1; porta 0 = dinâmica. Devolve o descritor e a porta real.
    private func abrirEscuta(porta pedida: UInt16) throws -> Int32 {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &on, socklen_t(MemoryLayout<Int32>.size))
        var end = sockaddr_in()
        end.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        end.sin_family = sa_family_t(AF_INET)
        end.sin_port = pedida.bigEndian
        end.sin_addr = in_addr(s_addr: INADDR_LOOPBACK.byteSwapped)
        let ligou = withUnsafePointer(to: &end) { p in
            p.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        guard ligou == 0, listen(fd, 16) == 0 else {
            let e = errno
            close(fd)
            throw NSError(domain: NSPOSIXErrorDomain, code: Int(e))
        }
        var real = sockaddr_in()
        var tam = socklen_t(MemoryLayout<sockaddr_in>.size)
        _ = withUnsafeMutablePointer(to: &real) { p in
            p.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &tam) }
        }
        porta = UInt16(bigEndian: real.sin_port)
        return fd
    }

    private func lacoAceite() {
        while true {
            var end = sockaddr()
            var tam = socklen_t(MemoryLayout<sockaddr>.size)
            let fd = accept(socketCanal, &end, &tam)
            if fd < 0 {
                if !aceitando || errno == EBADF { return }
                continue
            }
            let cx = Conexao(fd: fd)
            cx.fila.async { [self] in upgradeWS(cx) }
        }
    }

    private func lerExato(_ fd: Int32, _ n: Int) -> Data? {
        var dados = Data(count: n)
        var lidos = 0
        while lidos < n {
            let r = dados.withUnsafeMutableBytes { recv(fd, $0.baseAddress! + lidos, n - lidos, 0) }
            guard r > 0 else { return nil }
            lidos += r
        }
        return dados
    }

    private func enviarBytes(_ fd: Int32, _ dados: Data) -> Bool {
        dados.withUnsafeBytes { buf in
            var escritos = 0
            while escritos < buf.count {
                let r = send(fd, buf.baseAddress! + escritos, buf.count - escritos, 0)
                guard r > 0 else { return false }
                escritos += r
            }
            return true
        }
    }

    // MARK: - Upgrade RFC 6455 e frames à mão

    /// Lê a requisição de upgrade, responde 101 e entra no loop de frames.
    private func upgradeWS(_ cx: Conexao) {
        var bruto = Data()
        var buf = [UInt8](repeating: 0, count: 4096)
        while !bruto.contains(Data([13, 10, 13, 10])), bruto.count < 8192 {
            let r = recv(cx.fd, &buf, buf.count, 0)
            guard r > 0 else { cx.fechar(); return }
            bruto.append(contentsOf: buf[0 ..< r])
        }
        guard let requisicao = String(data: bruto, encoding: .utf8),
              requisicao.contains("\r\n\r\n"),
              let chave = requisicao.split(separator: "\r\n")
                  .first(where: { $0.lowercased().hasPrefix("sec-websocket-key:") })?
                  .split(separator: ":").last?.trimmingCharacters(in: .whitespaces) else {
            cx.fechar()
            return
        }
        let aceite = Data(Insecure.SHA1.hash(data: Data((chave + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").utf8)))
            .base64EncodedString()
        let resposta = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n" +
            "Connection: Upgrade\r\nSec-WebSocket-Accept: \(aceite)\r\n\r\n"
        guard enviarBytes(cx.fd, Data(resposta.utf8)) else { cx.fechar(); return }
        fila.async { self.anotar("conexão aceita (upgrade 101)") }
        loopFrames(cx)
    }

    /// Loop bloqueante de frames WS na fila da conexão; mensagens de texto completas vão
    /// para `tratar` na `fila` central. Ping → pong; close/erro encerra.
    private func loopFrames(_ cx: Conexao) {
        var fragmento = Data()
        var opcodeDaMensagem: UInt8 = 0
        while true {
            guard let cab = lerExato(cx.fd, 2) else { break }
            let final = cab[0] & 0x80 != 0
            let opcode = cab[0] & 0x0F
            var tamanho = UInt64(cab[1] & 0x7F)
            if tamanho == 126 {
                guard let ext = lerExato(cx.fd, 2) else { break }
                tamanho = UInt64(ext.withUnsafeBytes { $0.load(as: UInt16.self) }.bigEndian)
            } else if tamanho == 127 {
                guard let ext = lerExato(cx.fd, 8) else { break }
                tamanho = ext.withUnsafeBytes { $0.load(as: UInt64.self) }.bigEndian
            }
            guard tamanho <= (4 << 20) else { break }
            var mascara: Data?
            if cab[1] & 0x80 != 0 {
                mascara = lerExato(cx.fd, 4)
                guard mascara != nil else { break }
            }
            guard var carga = lerExato(cx.fd, Int(tamanho)) else { break }
            if let mascara { for i in carga.indices { carga[i] ^= mascara[i % 4] } }
            switch opcode {
            case 0x8: // close
                cx.fechar()
                fila.async { self.limpar(cx) }
                return
            case 0x9: // ping → pong
                _ = enviarFrameWS(cx.fd, opcode: 0xA, carga: carga)
            case 0x1, 0x2, 0x0:
                if opcode != 0 { fragmento = Data(); opcodeDaMensagem = opcode }
                fragmento += carga
                if final, opcodeDaMensagem == 0x1, let texto = String(data: fragmento, encoding: .utf8) {
                    let msg = texto
                    fila.async { self.tratar(cx, texto: msg) }
                }
                if final { fragmento = Data() }
            default:
                break
            }
        }
        cx.fechar()
        fila.async { self.limpar(cx) }
    }

    /// Frame WS de servidor: FIN + opcode, sem máscara.
    private func enviarFrameWS(_ fd: Int32, opcode: UInt8, carga: Data) -> Bool {
        var quadro = Data([0x80 | opcode])
        if carga.count < 126 {
            quadro.append(UInt8(carga.count))
        } else if carga.count <= UInt16.max {
            quadro.append(126)
            withUnsafeBytes(of: UInt16(carga.count).bigEndian) { quadro.append(contentsOf: $0) }
        } else {
            quadro.append(127)
            withUnsafeBytes(of: UInt64(carga.count).bigEndian) { quadro.append(contentsOf: $0) }
        }
        quadro.append(carga)
        return enviarBytes(fd, quadro)
    }

    // MARK: - Conexão (handshake + canal)

    private final class Conexao: @unchecked Sendable {
        let fd: Int32
        let fila = DispatchQueue(label: "mx.hubdeteste.cx")
        var efemera: Curve25519.KeyAgreement.PrivateKey?
        var d2h: SymmetricKey?
        var h2d: SymmetricKey?
        var cD2h: UInt64 = 0
        var cH2d: UInt64 = 0
        var buffer = Data()
        var pareando = false
        init(fd: Int32) { self.fd = fd }
        func fechar() { close(fd) }
    }

    private var conexaoComStream: Conexao?
    private var streamId: Int?

    private func limpar(_ cx: Conexao) {
        if conexaoComStream === cx { conexaoComStream = nil; streamId = nil }
    }

    private func enviar(_ cx: Conexao, _ obj: [String: Any]) {
        guard let dados = try? JSONSerialization.data(withJSONObject: obj) else { return }
        _ = enviarFrameWS(cx.fd, opcode: 0x1, carga: dados)
    }

    // MARK: - Fio do canal (hello → welcome; "f" = frame cifrado)

    private func tratar(_ cx: Conexao, texto: String) {
        guard let dados = texto.data(using: .utf8),
              let wire = try? JSONSerialization.jsonObject(with: dados) as? [String: Any],
              let t = wire["t"] as? String else { return }
        switch t {
        case "hello":
            anotar("hello recebido (pair=\(wire["pair"] as? Bool == true))")
            guard let dk = (wire["dk"] as? String).flatMap(Self.b64),
                  let ekD = (wire["ek"] as? String).flatMap(Self.b64),
                  let n = (wire["n"] as? String).flatMap(Self.b64) else { return }
            cx.pareando = wire["pair"] as? Bool == true
            do { try responderWelcome(cx, dk: dk, ekD: ekD, n: n) }
            catch { enviar(cx, ["t": "reject", "code": "crypto"]) }
        case "f":
            guard let c = (wire["c"] as? NSNumber)?.uint64Value,
                  let d = (wire["d"] as? String).flatMap(Self.b64),
                  let chave = cx.d2h, c == cx.cD2h,
                  let plano = try? abrirFrame(chave, c: c, d: d) else { return }
            cx.cD2h += 1
            cx.buffer += plano.chunk
            guard plano.final else { return }
            let mensagem = cx.buffer
            cx.buffer = Data()
            tratarInterno(cx, dados: mensagem)
        default:
            break
        }
    }

    /// O welcome do Hub: ekH efêmera do "computador" + assinatura Ed25519 do transcript hs2,
    /// depois deriva as duas direções de chave como o app (HKDF th → d2h / h2d).
    private func responderWelcome(_ cx: Conexao, dk: Data, ekD: Data, n: Data) throws {
        let efemera = Curve25519.KeyAgreement.PrivateKey()
        let ekH = efemera.publicKey.rawRepresentation
        let th = sha256("codync/hs2/v1", cidRaw, dk, ekD, n, ekH)
        let sig = try signKey.signature(for: th)
        let publica = try Curve25519.KeyAgreement.PublicKey(rawRepresentation: ekD)
        let compartilhado = try efemera.sharedSecretFromKeyAgreement(with: publica)
        let ss = compartilhado.withUnsafeBytes { Data($0) }
        let prk = HKDF<SHA256>.extract(inputKeyMaterial: SymmetricKey(data: ss), salt: th)
            .withUnsafeBytes { Data($0) }
        cx.efemera = efemera
        cx.d2h = expandir(prk, "codync/d2h/v1")
        cx.h2d = expandir(prk, "codync/h2d/v1")
        enviar(cx, ["t": "welcome", "ek": Self.b64(ekH), "sig": Self.b64(sig)])
        anotar("welcome enviado")
    }

    private func sha256(_ rotulo: String, _ partes: Data...) -> Data {
        var h = SHA256()
        h.update(data: Data(rotulo.utf8))
        for p in partes { h.update(data: p) }
        return Data(h.finalize())
    }

    private func expandir(_ prk: Data, _ info: String) -> SymmetricKey {
        HKDF<SHA256>.expand(pseudoRandomKey: SymmetricKey(data: prk),
                            info: Data(info.utf8), outputByteCount: 32)
    }

    private func u64be(_ v: UInt64) -> Data { withUnsafeBytes(of: v.bigEndian) { Data($0) } }

    private func selarFrame(_ chave: SymmetricKey, c: UInt64, final: Bool, chunk: Data) throws -> Data {
        let nonce = try ChaChaPoly.Nonce(data: Data(count: 4) + u64be(c))
        let caixa = try ChaChaPoly.seal(Data([final ? 0 : 1]) + chunk, using: chave, nonce: nonce,
                                        authenticating: Data("codync/frame/v1".utf8) + u64be(c))
        return caixa.ciphertext + caixa.tag
    }

    private func abrirFrame(_ chave: SymmetricKey, c: UInt64, d: Data) throws -> (final: Bool, chunk: Data) {
        let nonce = try ChaChaPoly.Nonce(data: Data(count: 4) + u64be(c))
        let caixa = try ChaChaPoly.SealedBox(nonce: nonce, ciphertext: d.dropLast(16), tag: d.suffix(16))
        let plano = try ChaChaPoly.open(caixa, using: chave,
                                        authenticating: Data("codync/frame/v1".utf8) + u64be(c))
        return (plano.first == 0, plano.dropFirst())
    }

    private func enviarInterno(_ cx: Conexao, _ obj: [String: Any]) {
        guard let chave = cx.h2d, let dados = try? JSONSerialization.data(withJSONObject: obj) else { return }
        // As mensagens do teste são pequenas: um frame final por mensagem.
        guard let frame = try? selarFrame(chave, c: cx.cH2d, final: true, chunk: dados) else { return }
        let c = cx.cH2d
        cx.cH2d += 1
        enviar(cx, ["t": "f", "c": c, "d": Self.b64(frame)])
    }

    // MARK: - Chamadas e streams

    private func tratarInterno(_ cx: Conexao, dados: Data) {
        guard let msg = try? JSONSerialization.jsonObject(with: dados) as? [String: Any],
              let id = (msg["id"] as? NSNumber)?.intValue else { return }
        if let sub = msg["sub"] as? String {
            anotar("sub \(sub)")
            if sub == "events" { assinarEventos(cx, id: id) }
            return
        }
        if msg["cancel"] as? Bool == true { anotar("cancel id \(id)") }
        guard let m = msg["m"] as? String else { return }
        let corpo = msg["b"] as? [String: Any] ?? [:]
        _chamadas.append((m, corpo))
        anotar("chamada \(m)")
        if cx.pareando, m != "pair" {
            enviarInterno(cx, ["id": id, "err": ["status": 403, "message": "pairing"]])
            return
        }
        switch m {
        case "pair":
            enviarInterno(cx, ["id": id, "ok": ["computerId": computerId, "name": "Hub de teste"]])
        case "hello":
            enviarInterno(cx, ["id": id, "ok": hello()])
        case "usage":
            enviarInterno(cx, ["id": id, "ok": ["providers": [Any]()]])
        case "sessions":
            enviarInterno(cx, ["id": id, "ok": sessoes()])
        case "history":
            enviarInterno(cx, ["id": id, "ok": historico(corpo)])
        case "send":
            responderSend(cx, id: id, corpo: corpo)
        case "screenStatus":
            enviarInterno(cx, ["id": id, "ok": ["enabled": false, "connected": false, "platform": "",
                                              "capture": false, "input": false, "displays": [Any](),
                                              "userControl": false, "computerUse": false, "viewers": 0]])
        case "markRead", "stop", "react", "reorderBots", "queuedSendNow", "queuedEdit", "queuedRemove":
            enviarInterno(cx, ["id": id, "ok": [String: Any]()])
        default:
            enviarInterno(cx, ["id": id, "err": ["status": 404, "message": "metodo desconhecido no hub de teste"]])
        }
    }

    private func assinarEventos(_ cx: Conexao, id: Int) {
        conexaoComStream = cx
        streamId = id
        rev += 1
        let eventos: [[String: Any]] = [
            ["type": "hello", "hostId": computerId, "rev": rev, "usage": ["providers": [Any]()]],
            ["type": "bot", "bot": bot()],
        ] + entradas.map { ["type": "entry", "entry": $0] }
        for ev in eventos { enviarInterno(cx, ["id": id, "ev": ev]) }
    }

    private func publicar(_ evento: [String: Any]) {
        guard let cx = conexaoComStream, let id = streamId else { return }
        enviarInterno(cx, ["id": id, "ev": evento])
    }

    // MARK: - Respostas (modelos do Hub)

    private func hello() -> [String: Any] {
        [
            "hostId": computerId, "name": "Hub de teste", "version": "9.9.9-teste",
            "os": "macos", "device": "macmini",
            "backends": [["id": "claude", "name": "Claude Code", "available": true,
                          "command": "claude", "installHint": "", "curated": true, "signedIn": true]],
            "rev": rev, "urls": ["http://127.0.0.1:\(porta)"],
            "computerId": computerId,
            "signKey": Self.b64(signKey.publicKey.rawRepresentation),
            "boxKey": Self.b64(boxKey.publicKey.rawRepresentation),
        ]
    }

    private func bot() -> [String: Any] {
        [
            "id": Self.botId, "kind": "agent", "name": "Berga", "title": "Pessoal",
            "description": "", "avatarColor": "blue", "avatarShape": "blob",
            "backend": "claude", "permission": "auto", "model": "opus",
            "pinned": false, "hidden": false, "connectors": [Any](), "skills": [Any](),
            "computer": false, "createdAt": agora(), "rev": rev,
            "status": "idle", "activity": "", "unread": 0, "lastAt": agora(),
            "hasPendingQuestions": false, "needsInput": false,
        ]
    }

    private func sessoes() -> [String: Any] {
        [
            "sessions": [
                ["id": Self.botId, "chatId": Self.botId, "principal": true,
                 "depth": 0, "title": "Principal", "state": "open", "revision": 1],
                // Raiz (sem parentSession): a tela abre "tudo recolhido", então a extra
                // precisa estar no topo para a linha ficar visível sem expandir nada.
                ["id": Self.sessaoExtra, "chatId": Self.botId, "principal": false,
                 "depth": 0, "title": "Sessão extra de teste",
                 "state": "open", "revision": 1],
            ],
            "sessionActions": ["send"],
        ]
    }

    private func historico(_ corpo: [String: Any]) -> [String: Any] {
        let sessao = corpo["sessionId"] as? String
        let antesDe = (corpo["beforeSeq"] as? NSNumber)?.int64Value ?? .max
        let limite = (corpo["limit"] as? NSNumber)?.intValue ?? 100
        let filtradas = entradas.filter { e in
            guard (e["seq"] as? Int64 ?? .max) < antesDe else { return false }
            let daEntrada = e["sessionId"] as? String
            // A Principal mostra entradas sem sessão ou com sessionId == botId.
            return sessao == nil ? (daEntrada == nil || daEntrada == Self.botId) : daEntrada == sessao
        }.suffix(limite)
        return ["entries": Array(filtradas)]
    }

    /// `send` com `sessionId` cai na sessão extra (nunca na Principal): a resposta traz a entrada e
    /// o stream publica a do usuário + uma resposta automática do agente, ambas na sessão certa.
    private func responderSend(_ cx: Conexao, id: Int, corpo: [String: Any]) {
        let texto = corpo["text"] as? String ?? ""
        let sessao = corpo["sessionId"] as? String
        let nonce = corpo["clientNonce"] as? String
        turno += 1
        var dados: [String: Any] = ["text": texto, "status": "sent"]
        if let nonce { dados["clientNonce"] = nonce }
        let doUsuario = novaEntrada("user", dados: dados, sessao: sessao)
        entradas.append(doUsuario)
        enviarInterno(cx, ["id": id, "ok": ["entry": doUsuario]])
        publicar(["type": "entry", "entry": doUsuario])
        let resposta = novaEntrada("agent",
                                 dados: ["text": sessao == nil
                                         ? "Resposta do hub de teste na Principal"
                                         : "Resposta do hub de teste na sessão extra",
                                         "final": true],
                                 sessao: sessao)
        entradas.append(resposta)
        publicar(["type": "entry", "entry": resposta])
    }
}
