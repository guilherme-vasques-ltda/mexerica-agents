// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import AppKit
import ApplicationServices
import CoreGraphics
import Darwin
import Foundation
import Network
@preconcurrency import ScreenCaptureKit
@preconcurrency import XPC

struct HelperError: LocalizedError {
    let errorDescription: String?
    init(_ message: String) { errorDescription = message }
}

/// JSON-RPC 2.0, one message per line, over the host's `~/.mexerica/screen.sock`.
/// The host sends requests; we answer them and send notifications. Reconnects forever.
@MainActor
final class HostLink {
    typealias Handler = @MainActor (_ method: String, _ params: [String: Any]) async throws -> Any

    /// Answers the host's requests.
    var handler: Handler?
    private var connection: NWConnection?
    private var buffer = Data()
    private(set) var isConnected = false
    /// Runs on every (re)connect: the host forgets us when we drop.
    var onConnect: (@MainActor () -> Void)?

    // Capturado uma vez: reconnect não procura outra raiz nem dados antigos.
    static let socketPath: String? = {
        let variables = ProcessInfo.processInfo.environment
        guard variables["DONNABOT_HOME"] == nil, variables["CODYNC_HOME"] == nil else { return nil }
        if variables["MEXERICA_HOME"] == "" { return nil }
        // CodyncDataFolder preserva apenas a identidade técnica Main/Dev do projeto gerado.
        let identity = Bundle.main.object(forInfoDictionaryKey: "CodyncDataFolder") as? String
        let folder = identity == ".donnabot-dev" ? ".mexerica-dev" : ".mexerica"
        let selected = variables["MEXERICA_HOME"] ?? NSHomeDirectory() + "/" + folder
        let absolute = selected.hasPrefix("/") ? selected : FileManager.default.currentDirectoryPath + "/" + selected
        let base = (absolute as NSString).standardizingPath
        for candidate in [base, (base as NSString).resolvingSymlinksInPath] {
            let home = NSHomeDirectory().lowercased() + "/"
            if candidate.lowercased().hasPrefix(home) {
                let first = candidate.lowercased().dropFirst(home.count).split(separator: "/").first ?? ""
                if first.hasPrefix(".donnabot") || first.hasPrefix(".codync") { return nil }
            }
        }
        return base + "/screen.sock"
    }()

    func start() {
        guard let socket = Self.socketPath else { return }
        let c = NWConnection(to: .unix(path: socket), using: .tcp)
        connection = c
        c.stateUpdateHandler = { [weak self] state in
            MainActor.assumeIsolated { self?.changed(state, of: c) }
        }
        c.start(queue: .main)
    }

    private func changed(_ state: NWConnection.State, of c: NWConnection) {
        guard c === connection else { return }
        switch state {
        case .ready:
            isConnected = true
            buffer = Data()
            receive(on: c)
            onConnect?()
        case .waiting, .failed:
            // The host isn't running (yet): try again shortly.
            drop(c)
        default:
            break
        }
    }

    private func drop(_ c: NWConnection) {
        guard c === connection else { return }
        isConnected = false
        connection = nil
        c.stateUpdateHandler = nil
        c.cancel()
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(2))
            self?.start()
        }
    }

    private func receive(on c: NWConnection) {
        c.receive(minimumIncompleteLength: 1, maximumLength: 1 << 20) { [weak self] data, _, done, error in
            MainActor.assumeIsolated {
                guard let self, c === self.connection else { return }
                if let data {
                    self.buffer.append(data)
                    self.drain()
                }
                if done || error != nil {
                    self.drop(c)
                } else {
                    self.receive(on: c)
                }
            }
        }
    }

    private func drain() {
        while let nl = buffer.firstIndex(of: 0x0A) {
            let line = buffer[buffer.startIndex..<nl]
            buffer.removeSubrange(buffer.startIndex...nl)
            guard let msg = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any],
                  let method = msg["method"] as? String
            else { continue }
            let id = msg["id"].map { "\($0)" }
            let params = msg["params"] as? [String: Any] ?? [:]
            Task { await self.run(method, params, id: id) }
        }
    }

    private func run(_ method: String, _ params: [String: Any], id: String?) async {
        do {
            guard let handler else { throw HelperError("not ready") }
            let result = try await handler(method, params)
            if let id { send(["jsonrpc": "2.0", "id": Int(id) ?? 0, "result": result]) }
        } catch {
            if let id {
                send(["jsonrpc": "2.0", "id": Int(id) ?? 0, "error": ["code": -32000, "message": error.localizedDescription]])
            }
        }
    }

    func notify(_ method: String, _ params: [String: Any]) {
        send(["jsonrpc": "2.0", "method": method, "params": params])
    }

    private func send(_ msg: [String: Any]) {
        guard let connection, isConnected, var data = try? JSONSerialization.data(withJSONObject: msg) else { return }
        data.append(0x0A)
        connection.send(content: data, completion: .contentProcessed { _ in })
    }
}

@MainActor
final class ForkLocalScreenXPC {
    static let service = "app.donnabot.dev.screen.xpc"
    private var listener: xpc_connection_t?
    private var peers: [ObjectIdentifier: xpc_connection_t] = [:]
    private let instance = UUID().uuidString

    func start() {
        guard Bundle.main.object(forInfoDictionaryKey: "MexericaScreenService") as? String == Self.service,
              let requirement = Self.hubRequirement() else { return }
        let listener = xpc_connection_create_mach_service(Self.service, .main, UInt64(XPC_CONNECTION_MACH_SERVICE_LISTENER))
        self.listener = listener
        xpc_connection_set_event_handler(listener) { [weak self] object in
            MainActor.assumeIsolated {
                guard let self, xpc_get_type(object) == XPC_TYPE_CONNECTION,
                      let peer = object as? xpc_connection_t else { return }
                self.accept(peer, requirement: requirement)
            }
        }
        xpc_connection_activate(listener)
        if !CGPreflightScreenCaptureAccess() {
            Task {
                while !Task.isCancelled {
                    try? await Task.sleep(for: .seconds(3))
                    if await ScreenHelper.captureGrantedNow() { exit(0) }
                }
            }
        }
    }

    private func accept(_ peer: xpc_connection_t, requirement: String) {
        xpc_connection_set_target_queue(peer, .main)
        guard xpc_connection_set_peer_code_signing_requirement(peer, requirement) == 0 else {
            xpc_connection_cancel(peer)
            return
        }
        let id = ObjectIdentifier(peer)
        peers[id] = peer
        xpc_connection_set_event_handler(peer) { [weak self, weak peer] message in
            MainActor.assumeIsolated {
                guard let self, let peer else { return }
                if xpc_get_type(message) == XPC_TYPE_ERROR {
                    xpc_connection_cancel(peer)
                    self.peers.removeValue(forKey: id)
                    return
                }
                guard xpc_get_type(message) == XPC_TYPE_DICTIONARY,
                      let reply = xpc_dictionary_create_reply(message),
                      let raw = xpc_dictionary_get_string(message, "json"),
                      strnlen(raw, 32_768) < 32_768,
                      let body = try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8)) as? [String: Any]
                else { return }
                Task { @MainActor in
                    let response: [String: Any]
                    do {
                        response = ["result": try await self.handle(body)]
                    } catch {
                        response = ["error": error.localizedDescription]
                    }
                    guard let data = try? JSONSerialization.data(withJSONObject: response),
                          data.count < 4 * 1024 * 1024 else { return }
                    xpc_dictionary_set_string(reply, "json", String(decoding: data, as: UTF8.self))
                    xpc_connection_send_message(peer, reply)
                }
            }
        }
        xpc_connection_activate(peer)
    }

    private func handle(_ message: [String: Any]) async throws -> [String: Any] {
        switch message["method"] as? String {
        case "status":
            return ["capture": CGPreflightScreenCaptureAccess(), "input": AXIsProcessTrusted(),
                    "instance": instance, "display": CGMainDisplayID()]
        case "requestPermission":
            guard let permission = message["permission"] as? String else { throw HelperError("Missing permission.") }
            try ScreenPermissions.request(permission)
            return [:]
        case "screenshot":
            let display = try authorize(message)
            let bounds = CGDisplayBounds(display)
            let filter = try await Capture.filter(display)
            let config = SCStreamConfiguration()
            let scale = min(1, 1280 / max(bounds.width, bounds.height))
            config.width = max(1, Int(bounds.width * scale))
            config.height = max(1, Int(bounds.height * scale))
            _ = try authorize(message)
            let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
            _ = try authorize(message)
            guard bounds == CGDisplayBounds(display) else { throw HelperError("The display changed.") }
            let bitmap = NSBitmapImageRep(cgImage: image)
            guard let jpeg = bitmap.representation(using: .jpeg, properties: [.compressionFactor: 0.75]) else {
                throw HelperError("Screen capture could not be encoded.")
            }
            let data = jpeg.base64EncodedString()
            _ = try authorize(message)
            return ["data": data, "width": bounds.width, "height": bounds.height]
        default:
            throw HelperError("Unsupported screen request.")
        }
    }

    private func authorize(_ message: [String: Any]) throws -> CGDirectDisplayID {
        let now = clock_gettime_nsec_np(CLOCK_MONOTONIC_RAW)
        guard let raw = message["deadlineNs"] as? String, let deadline = UInt64(raw),
              now > 0, now < deadline else { throw HelperError("Screen capture approval expired.") }
        guard message["instance"] as? String == instance, CGPreflightScreenCaptureAccess(), AXIsProcessTrusted(),
              let display = message["display"] as? UInt32, display != 0, display == CGMainDisplayID() else {
            throw HelperError("The helper or display changed, or screen permissions are missing.")
        }
        return display
    }

    private static func hubRequirement() -> String? {
        let contents = Bundle.main.bundleURL.deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent()
        let outer = contents.deletingLastPathComponent()
        guard contents.lastPathComponent == "Contents", outer.pathExtension == "app" else { return nil }
        let verify = Process()
        verify.executableURL = URL(fileURLWithPath: "/usr/bin/codesign")
        verify.arguments = ["--verify", "--strict", outer.path]
        verify.environment = [:]
        verify.standardOutput = FileHandle.nullDevice
        verify.standardError = FileHandle.nullDevice
        do { try verify.run() } catch { return nil }
        verify.waitUntilExit()
        guard verify.terminationStatus == 0,
              let file = try? FileHandle(forReadingFrom: contents.appending(path: "Resources/screen-identity.cdhashes"))
        else { return nil }
        defer { try? file.close() }
        guard let data = try? file.read(upToCount: 83), data.count == 82,
              let pin = String(data: data, encoding: .utf8)
        else { return nil }
        let lines = pin.split(separator: "\n", omittingEmptySubsequences: false)
        guard lines.count == 3, lines[2].isEmpty, lines[0] != lines[1],
              lines.prefix(2).allSatisfy({ $0.utf8.count == 40 && $0.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) } })
        else { return nil }
        return "cdhash H\"\(lines[1])\""
    }
}
