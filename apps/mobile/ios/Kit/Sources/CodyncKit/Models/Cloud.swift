// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

// Wire types for the Codync cloud (`/v1`, cloud/src/api.ts) and the relay mailbox. Times are epoch milliseconds.
// Decoding is lenient: unknown fields are ignored and anything the server may omit is optional.

/// A computer in the account (`GET /v1/computers`). Its `boxKey` is for display only.
public struct CloudComputer: Codable, Hashable, Sendable, Identifiable {
    public var computerId: ComputerID
    public var name: String
    public var platform: String?
    public var device: String?
    /// Delivered by the cloud: trusted only after the SAS matched (`AccessTicket.signKey`).
    public var signKey: String
    public var boxKey: String?
    public var version: String?
    public var online: Bool?
    public var lastSeenAt: Int64?
    public var claimedAt: Int64?
    /// `none` · `pending` · `granted` (only when the request carried this device's signature).
    public var access: String?

    public var id: ComputerID { computerId }
    public var isOnline: Bool { online ?? false }
    /// This device was approved on it through the account.
    public var isGranted: Bool { access == "granted" }
}

/// A started access request, after both nonces were exchanged: `code` is ready to show.
public struct AccessTicket: Codable, Hashable, Sendable, Identifiable {
    public var requestId: String
    public var expiresAt: Int64
    /// The 6-digit SAS: this iPhone shows it and the person types it on the Mac.
    public var code: String
    public var computerId: ComputerID
    /// The host key the SAS was computed with; pinned once the request is approved.
    public var signKey: String
    public var hostNonce: String?
    public var id: String { requestId }

    public init(requestId: String, expiresAt: Int64, code: String, computerId: ComputerID, signKey: String, hostNonce: String? = nil) {
        self.requestId = requestId
        self.expiresAt = expiresAt
        self.code = code
        self.computerId = computerId
        self.signKey = signKey
        self.hostNonce = hostNonce
    }
}

/// What the Mac's confirmation code is bound to, for one access request: the host key the SAS
/// was computed with, this device's key, both nonces and the request id. Memory only: not Codable, and it
/// never prints or mirrors its bytes. The code itself is only ever the one the person types here.
public struct AccessConfirmation: Sendable, CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public let requestId: String
    let hostSignPub: Data
    let deviceKey: Data
    let deviceNonce: Data
    let hostNonce: Data

    init(requestId: String, hostSignPub: Data, deviceKey: Data, deviceNonce: Data, hostNonce: Data) {
        self.requestId = requestId
        self.hostSignPub = hostSignPub
        self.deviceKey = deviceKey
        self.deviceNonce = deviceNonce
        self.hostNonce = hostNonce
    }

    public enum Verdict: Sendable, Equatable {
        case match
        case mismatch
        /// The expected code can't be computed (bad request id, key or nonce): nothing may be saved.
        case unusable
    }

    /// Computed here, never taken from the network.
    var expectedCode: String? {
        RelayCrypto.accessConfirmCode(hostSignPub: hostSignPub, deviceKey: deviceKey, deviceNonce: deviceNonce,
                                      hostNonce: hostNonce, requestId: requestId)
    }

    /// Same request, same host key (the one pinned) and same host nonce as the ticket.
    public func isBound(to ticket: AccessTicket) -> Bool {
        requestId == ticket.requestId && hostSignPub.base64URL == ticket.signKey && hostNonce.base64URL == ticket.hostNonce
    }

    /// The typed code against the expected one, compared in constant time.
    public func verify(_ typed: String) -> Verdict {
        guard let expected = expectedCode else { return .unusable }
        guard let digits = Self.normalize(typed) else { return .mismatch }
        let a = Array(expected.utf8)
        let b = Array(digits.utf8)
        guard a.count == 6, b.count == 6 else { return .mismatch }
        var diff: UInt8 = 0
        for i in 0..<6 { diff |= a[i] ^ b[i] }
        return diff == 0 ? .match : .mismatch
    }

    /// Exactly six ASCII digits from what was typed or pasted: spaces and invisible direction marks are
    /// dropped, Arabic-Indic digits read as 0–9. Anything else, or another count: nil.
    public static func normalize(_ typed: String) -> String? {
        var out = ""
        for scalar in typed.unicodeScalars {
            if scalar.properties.isWhitespace || scalar.properties.generalCategory == .format { continue }
            guard let digit = asciiDigit(scalar) else { return nil }
            out.append(digit)
        }
        return out.count == 6 ? out : nil
    }

    /// Only the digits of `text`, as ASCII (Arabic-Indic digits included), in order: for a code field
    /// that accepts pasted text like "123 456".
    public static func digits(in text: String) -> String {
        String(text.unicodeScalars.compactMap(asciiDigit))
    }

    private static func asciiDigit(_ scalar: Unicode.Scalar) -> Character? {
        let v = scalar.value
        let zero: UInt32
        switch v {
        case 0x30...0x39: zero = 0x30
        case 0x660...0x669: zero = 0x660 // Arabic-Indic
        case 0x6F0...0x6F9: zero = 0x6F0 // Extended Arabic-Indic
        default: return nil
        }
        return Character(Unicode.Scalar(UInt8(0x30 + v - zero)))
    }

    public var description: String { "AccessConfirmation(redacted)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: [Mirror.Child]()) }
}

/// What identifies an approval of one request (request, computer, host nonce), without its secrets:
/// for reading the request back once after it ended here, to revoke a grant this iPhone never saw.
public struct AccessApprovalCheck: Sendable {
    public let requestId: String
    let computerId: ComputerID
    let hostNonce: String

    /// The grant of an approval of this very request; nil for anything else. Only for revoking it:
    /// nothing is ever saved from this answer.
    public func grant(in status: AccessStatus) -> String? {
        guard status.status == .approved, status.requestId == requestId, status.computerId == computerId,
              !hostNonce.isEmpty, status.hostNonce == hostNonce,
              let grant = status.grantId, RelayCrypto.isAccessRequestId(grant) else { return nil }
        return grant
    }
}

extension AccessConfirmation {
    /// This request's approval check: its id, its computer (from the host key) and the host nonce.
    public var approvalCheck: AccessApprovalCheck {
        AccessApprovalCheck(requestId: requestId, computerId: RelayCrypto.computerId(signKey: hostSignPub),
                            hostNonce: hostNonce.isEmpty ? "" : hostNonce.base64URL)
    }
}

/// A started access request: the ticket to show, and what the Mac's confirmation code is checked against.
public struct AccessAttempt: Sendable {
    public let ticket: AccessTicket
    public let confirmation: AccessConfirmation
}

public enum AccessState: String, Codable, Sendable {
    case pending, approved, denied, expired, cancelled, refused, unknown

    public init(from decoder: Decoder) throws {
        // A state this version doesn't know is over as far as it can tell.
        self = AccessState(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .unknown
    }
}

/// `GET /v1/access-requests/{id}`.
public struct AccessStatus: Codable, Hashable, Sendable {
    public var requestId: String
    public var computerId: ComputerID?
    public var status: AccessState
    public var expiresAt: Int64?
    public var hostNonce: String?
    public var grantId: String?
}

/// `GET /v1/devices`.
public struct CloudDevice: Codable, Hashable, Sendable, Identifiable {
    public var deviceId: String
    public var deviceKey: String
    public var name: String
    public var platform: String?
    public var createdAt: Int64?
    public var lastUsedAt: Int64?
    public var revoked: Bool?
    public var id: String { deviceId }
}

/// `GET /v1/computers/{id}/grants`.
public struct CloudGrant: Codable, Hashable, Sendable, Identifiable {
    public var grantId: String
    public var deviceId: String
    public var deviceName: String?
    public var platform: String?
    public var scopes: [String]?
    public var createdAt: Int64?
    public var id: String { grantId }
}

/// A message waiting in the relay mailbox for a computer that is offline.
public struct QueuedItem: Codable, Hashable, Sendable, Identifiable {
    public var nonce: String
    public var exp: Int64?
    /// `queued`, or `delivering` (handed to the computer, not acknowledged yet).
    public var state: String
    public var id: String { nonce }
}

public enum MailboxEvent: Sendable, Equatable {
    case delivered(nonce: String)
    case failed(nonce: String, code: String)
    case expired(nonce: String)

    public var nonce: String {
        switch self {
        case let .delivered(nonce), let .failed(nonce, _), let .expired(nonce): nonce
        }
    }
}

public enum MailboxCancel: Sendable, Equatable {
    /// Removed before the computer saw it.
    case cancelled
    /// Already handed to the computer: it will run.
    case delivering
    /// The relay doesn't have it (delivered, expired, or not reachable right now).
    case unknown
}

/// Why the relay refused a mailbox message.
public enum MailboxError: LocalizedError, Sendable, Equatable {
    /// The computer came online: send over the channel instead.
    case hostOnline
    case rejected(String)

    public var errorDescription: String? {
        switch self {
        case .hostOnline: String(localized: "Seu computador acabou de ficar disponível. Envie novamente.", bundle: .module)
        case .rejected("full"): String(localized: "Há mensagens demais aguardando este computador.", bundle: .module)
        case .rejected("tooLarge"): String(localized: "Esta mensagem é longa demais para guardar enquanto o computador está sem conexão.", bundle: .module)
        case let .rejected(code): String(localized: "Não foi possível guardar a mensagem para seu computador (\(code)).", bundle: .module)
        }
    }
}
