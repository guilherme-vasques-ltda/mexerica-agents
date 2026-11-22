import Foundation
import Network

/// Cliente do canal de controle do Hub de teste. O Hub roda num processo no
/// host — o app disca 127.0.0.1 para o loopback do host, onde um listener do runner dentro
/// do simulador não é alcançável — e este cliente pergunta por TCP puro o estado atual em
/// JSON: URL de pareamento, chamadas recebidas e fio de diagnóstico.
///
/// TCP puro, sem HTTP: não passa pela ATS do runner e não precisa de parser de requisição
/// no host. Cada consulta abre uma conexão, lê o JSON até o fim e fecha.
final class MXHubControle: @unchecked Sendable {
    static let faixaControle: ClosedRange<UInt16> = 53997 ... 54006

    struct Estado {
        var pairingURL: String
        var chamadas: [[String: Any]]
        var fio: [String]
    }

    /// Pergunta cada porta da faixa até achar o Hub; nil quando ele não está no ar.
    /// Conexões recusadas falham rápido, então varrer a faixa toda custa milissegundos.
    func estado(timeout: TimeInterval = 3) -> Estado? {
        for porta in Self.faixaControle {
            guard let dados = ler(porta: porta, timeout: timeout),
                  let obj = try? JSONSerialization.jsonObject(with: dados) as? [String: Any],
                  obj["hub"] as? String == "mexerica-teste",
                  let url = obj["pairingURL"] as? String else { continue }
            return Estado(pairingURL: url,
                          chamadas: obj["chamadas"] as? [[String: Any]] ?? [],
                          fio: obj["fio"] as? [String] ?? [])
        }
        return nil
    }

    private final class Caixa: @unchecked Sendable {
        var dados = Data()
    }

    /// Lê a resposta inteira de uma porta até o host fechar; nil em recusa/vazio.
    private func ler(porta: UInt16, timeout: TimeInterval) -> Data? {
        guard let p = NWEndpoint.Port(rawValue: porta) else { return nil }
        let conn = NWConnection(host: "127.0.0.1", port: p, using: .tcp)
        let semaforo = DispatchSemaphore(value: 0)
        let caixa = Caixa()
        @Sendable func receber() {
            conn.receive(minimumIncompleteLength: 1, maximumLength: 1 << 20) { dados, _, completo, erro in
                if let dados { caixa.dados += dados }
                if completo || erro != nil {
                    semaforo.signal()
                } else {
                    receber()
                }
            }
        }
        conn.stateUpdateHandler = { estado in
            switch estado {
            case .ready:
                receber()
            case .failed, .cancelled:
                semaforo.signal()
            default:
                break
            }
        }
        conn.start(queue: DispatchQueue(label: "mx.hubcontrole.cliente"))
        _ = semaforo.wait(timeout: .now() + timeout)
        conn.cancel()
        return caixa.dados.isEmpty ? nil : caixa.dados
    }
}
