// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

/// An error from the Codync cloud: `{"error":{"code","message"}}`.
public struct CloudError: LocalizedError, Sendable, Equatable {
    public var status: Int
    /// `notFound`, `conflict`, `requestExpired`, … (spec §8.3).
    public var code: String
    /// Text the cloud sent. Never shown to the person: `errorDescription` only uses fixed phrases.
    public var message: String?

    public init(status: Int, code: String, message: String? = nil) {
        self.status = status
        self.code = code
        self.message = message
    }

    public var errorDescription: String? {
        switch code {
        case "requestExpired": "O pedido expirou. Peça acesso novamente."
        case "accountDeleted": "Esta conta foi apagada."
        case "rateLimited": "Muitos pedidos em pouco tempo. Tente de novo daqui a pouco."
        // Access-request flow: fixed Portuguese phrases, whatever message the cloud or the code sent.
        case "noDeviceKey": "Não foi possível acessar a chave deste iPhone. Entre na sua conta de novo."
        case "badSignKey": "A chave deste computador não confere com a identidade dele. Peça acesso de novo."
        case "badNonce", "invalidAccessResponse": "A resposta da nuvem veio incompleta. Peça acesso novamente."
        case "denied": "O computador recusou o pedido."
        case "cancelled": "O pedido foi cancelado."
        case "refused": "A nuvem recusou este pedido. Nenhum vínculo foi salvo."
        case "approved", "unknown": "O resultado do pedido não foi confirmado. Confira no Mac antes de pedir acesso novamente."
        // Raised here, never from the cloud: fixed phrases too.
        case "accountRequired": "Use os computadores da sua conta."
        case "computerNotInAccount": "Esse computador não está na sua conta."
        case "deviceKeyMismatch": "A nuvem não confirmou a chave deste iPhone."
        // A 409 when asking for access: an approval from an earlier request is still active.
        case "deviceAlreadyApproved": "Este iPhone já foi aprovado nesse Mac, mas não terminou a ligação. No Mac, em Ajustes › Computadores e dispositivos, remova este iPhone e peça de novo."
        // Any other code, including whatever the cloud invents: nothing remote reaches the screen.
        default: "Não foi possível concluir. Tente de novo."
        }
    }

    /// A busy or failing cloud, or a session token that wasn't fresh yet: trying again later
    /// settles it, so background work (launch, foreground refresh) keeps it out of the error dialog.
    public var isTransient: Bool { status >= 500 || status == 429 || status == 401 }
}

/// The user-facing `/v1` API (§8.4): account, devices, computers, access requests, claims.
/// The Clerk session token comes from the app; kit doesn't depend on ClerkKit.
public struct CloudClient: Sendable {
    public let baseURL: URL
    let identity: DeviceIdentity?
    let token: @Sendable () async throws -> String
    let session: URLSession
    /// How often `requestAccess` polls for the host's nonce.
    var pollInterval: Duration = .seconds(2)

    public init(baseURL: URL, identity: DeviceIdentity?, token: @escaping @Sendable () async throws -> String) {
        self.init(baseURL: baseURL, identity: identity, session: .shared, token: token)
    }

    init(baseURL: URL, identity: DeviceIdentity?, session: URLSession, token: @escaping @Sendable () async throws -> String) {
        self.baseURL = baseURL
        self.identity = identity
        self.session = session
        self.token = token
    }

    // MARK: account and devices


    /// Registers this device key with the account (Clerk + proof of the key).
    public func registerDevice(name: String, platform: String) async throws {
        guard let identity else { throw CloudError(status: 0, code: "noDeviceKey") }
        struct Res: Decodable { var device: CloudDevice }
        let result: Res = try await request("POST", "v1/devices", body: ["name": name, "platform": platform], signed: true)
        guard result.device.deviceKey == identity.publicKey else {
            throw CloudError(status: 0, code: "deviceKeyMismatch")
        }
    }

    public func devices() async throws -> [CloudDevice] {
        struct Res: Decodable { var devices: [CloudDevice] }
        let res: Res = try await request("GET", "v1/devices")
        return res.devices
    }

    // MARK: computers

    /// With a device identity, each computer says whether this device has access.
    public func computers() async throws -> [CloudComputer] {
        struct Res: Decodable { var computers: [CloudComputer] }
        let res: Res = try await request("GET", "v1/computers", signed: identity != nil)
        return res.computers
    }

    /// Removes the computer from the account; every device's access to it through the account ends.
    public func removeComputer(_ id: ComputerID) async throws {
        try await send("DELETE", "v1/computers/\(id)")
    }

    public func grants(_ id: ComputerID) async throws -> [CloudGrant] {
        struct Res: Decodable { var grants: [CloudGrant] }
        let res: Res = try await request("GET", "v1/computers/\(id)/grants")
        return res.grants
    }

    public func revokeGrant(_ id: ComputerID, grantId: String) async throws {
        try await send("DELETE", "v1/computers/\(id)/grants/\(grantId)")
    }

    // MARK: access requests (§4.2 B, commit-then-reveal SAS)

    /// Commit → wait for the host's nonce → reveal. Returns once `code` can be shown.
    /// The device nonce lives only in memory here, so a cloud can't learn it before the host committed;
    /// it stays in memory in the attempt's `confirmation`, for the Mac's confirmation code.
    public func requestAccess(_ id: ComputerID, signKey: String) async throws -> AccessAttempt {
        guard let identity else { throw CloudError(status: 0, code: "noDeviceKey") }
        guard let hostKey = Data(base64URL: signKey), hostKey.count == 32, RelayCrypto.computerId(signKey: hostKey) == id else {
            throw CloudError(status: 0, code: "badSignKey")
        }
        let nD = Data.random(32)
        let commit = RelayCrypto.sasCommit(deviceKey: identity.deviceKey, deviceNonce: nD).base64URL
        struct Created: Decodable { var requestId: String; var expiresAt: Int64 }
        let created: Created
        do {
            created = try await request("POST", "v1/computers/\(id)/access-requests", body: ["commit": commit], signed: true)
        } catch let error as CloudError where error.status == 409 {
            // Decided by this route's status alone, never by the cloud's text: this device still holds an
            // active grant on the computer from an approval that never finished here.
            throw CloudError(status: 409, code: "deviceAlreadyApproved")
        }
        // The confirmation code is bound to the id; one outside the contract ends the request here.
        guard RelayCrypto.isAccessRequestId(created.requestId) else { throw CloudError(status: 0, code: "invalidAccessResponse") }

        var hostNonce: Data?
        while hostNonce == nil {
            try Task.checkCancellation()
            guard Int64(Date.now.timeIntervalSince1970 * 1000) < created.expiresAt else {
                throw CloudError(status: 410, code: "requestExpired")
            }
            let status = try await accessStatus(created.requestId)
            guard status.requestId == created.requestId,
                  status.computerId == id else { throw CloudError(status: 0, code: "invalidAccessResponse") }
            guard status.status == .pending else {
                throw CloudError(status: 0, code: status.status == .expired ? "requestExpired" : status.status.rawValue)
            }
            if let raw = status.hostNonce {
                guard let n = Data(base64URL: raw), n.count == 32 else { throw CloudError(status: 0, code: "badNonce", message: nil) }
                hostNonce = n
            } else {
                guard Int64(Date.now.timeIntervalSince1970 * 1000) < created.expiresAt else {
                    throw CloudError(status: 410, code: "requestExpired", message: nil)
                }
                try await Task.sleep(for: pollInterval)
            }
        }
        try await send("POST", "v1/access-requests/\(created.requestId)/reveal", body: ["nonce": nD.base64URL], signed: true)
        let code = RelayCrypto.sasCode(hostSignKey: hostKey, deviceKey: identity.deviceKey, deviceNonce: nD, hostNonce: hostNonce ?? Data())
        let ticket = AccessTicket(requestId: created.requestId, expiresAt: created.expiresAt, code: code, computerId: id, signKey: signKey, hostNonce: hostNonce?.base64URL)
        let confirmation = AccessConfirmation(requestId: created.requestId, hostSignPub: hostKey, deviceKey: identity.deviceKey,
                                              deviceNonce: nD, hostNonce: hostNonce ?? Data())
        return AccessAttempt(ticket: ticket, confirmation: confirmation)
    }

    public func accessStatus(_ requestId: String) async throws -> AccessStatus {
        try await request("GET", "v1/access-requests/\(requestId)")
    }

    public func cancelAccess(_ requestId: String) async throws {
        try await send("DELETE", "v1/access-requests/\(requestId)")
    }

    // MARK: claims (§4.2 A)

    // MARK: plumbing

    private struct NoBody: Encodable {}
    private struct Failure: Decodable {
        struct Detail: Decodable { var code: String; var message: String? }
        var error: Detail?
        // Deployment refusals in index.ts use a flat envelope, before the /v1 handler.
        var code: String?
        var message: String?
    }
    private struct Ignored: Decodable {}

    private func send(_ method: String, _ path: String, body: (some Encodable)? = NoBody?.none, signed: Bool = false) async throws {
        let _: Ignored = try await request(method, path, body: body, signed: signed)
    }

    func request<T: Decodable>(_ method: String, _ path: String, body: (some Encodable)? = NoBody?.none, signed: Bool = false) async throws -> T {
        let url = baseURL.appending(path: path)
        var req = URLRequest(url: url, timeoutInterval: 20)
        req.httpMethod = method
        let data = try body.map { try JSONEncoder().encode($0) } ?? Data()
        if body != nil {
            req.httpBody = data
            req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        req.setValue("Bearer \(try await token())", forHTTPHeaderField: "Authorization")
        if signed {
            guard let identity else { throw CloudError(status: 0, code: "noDeviceKey") }
            let header = try identity.signatureHeader(method: method, authority: RelayCrypto.authority(of: url),
                                                      pathAndQuery: RelayCrypto.pathAndQuery(of: url), body: data)
            req.setValue(header, forHTTPHeaderField: "Codync-Sig")
        }
        let (bytes, response) = try await session.data(for: req)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else {
            let e = try? JSONDecoder().decode(Failure.self, from: bytes)
            throw CloudError(status: status, code: e?.error?.code ?? e?.code ?? "http\(status)", message: e?.error?.message ?? e?.message)
        }
        return try JSONDecoder().decode(T.self, from: bytes.isEmpty ? Data("{}".utf8) : bytes)
    }
}
