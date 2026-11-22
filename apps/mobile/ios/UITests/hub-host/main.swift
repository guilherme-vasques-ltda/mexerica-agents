import Foundation

// Ponto de entrada do Hub de teste no host. Roda FORA do simulador:
// conexões do app para 127.0.0.1 caem no loopback do host, então o canal WebSocket
// precisa ser escutado aqui — um listener dentro do runner no simulador fica
// inalcançável para o app. Os dois listeners são sockets POSIX crus porque o
// NWListener do macOS falha com EINVAL nesta máquina.
//
// Uso: `swiftc -O apps/mobile/ios/UITests/hub-host/*.swift -o mx-hub && ./mx-hub` antes do
// `xcodebuild test`. O canal sobe em porta dinâmica; um segundo socket ("controle")
// na faixa 53997...54006 responde o estado atual em JSON e fecha — é como o teste
// descobre a URL de pareamento, as chamadas recebidas e o fio.

let hub = MXHubDeTeste()
do {
    try hub.iniciar()
} catch {
    FileHandle.standardError.write("falha ao subir o hub de teste: \(error)\n".data(using: .utf8)!)
    exit(2)
}

func abrirControle(_ faixa: ClosedRange<UInt16>) -> (Int32, UInt16)? {
    for tentativa in faixa {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { continue }
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &on, socklen_t(MemoryLayout<Int32>.size))
        var end = sockaddr_in()
        end.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        end.sin_family = sa_family_t(AF_INET)
        end.sin_port = tentativa.bigEndian
        end.sin_addr = in_addr(s_addr: INADDR_LOOPBACK.byteSwapped)
        let ligou = withUnsafePointer(to: &end) { p in
            p.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        if ligou == 0, listen(fd, 8) == 0 { return (fd, tentativa) }
        close(fd)
    }
    return nil
}

guard let (socketControle, portaControle) = abrirControle(53997 ... 54006) else {
    FileHandle.standardError.write("falha: nenhuma porta de controle livre em 53997...54006\n".data(using: .utf8)!)
    exit(2)
}

// Cada consulta recebe o estado inteiro em JSON e a conexão fecha. Sem HTTP de propósito:
// TCP puro não passa pela ATS do runner nem precisa de parsing de requisição.
DispatchQueue(label: "mx.hubcontrole").async {
    while true {
        var end = sockaddr()
        var tam = socklen_t(MemoryLayout<sockaddr>.size)
        let fd = accept(socketControle, &end, &tam)
        guard fd >= 0 else { continue }
        DispatchQueue.global().async {
            let json = Data(hub.estadoJSON().utf8)
            _ = json.withUnsafeBytes { send(fd, $0.baseAddress, $0.count, 0) }
            close(fd)
        }
    }
}

FileHandle.standardOutput.write("PORTA \(hub.portaPublica) CTRL \(portaControle)\n".data(using: .utf8)!)
RunLoop.main.run()
