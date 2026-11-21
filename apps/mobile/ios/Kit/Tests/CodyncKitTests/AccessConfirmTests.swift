import Foundation
import Testing
@testable import CodyncKit

// The Mac's confirmation code. Inputs are the shared vector keys and nonces
// (the same fields the SAS uses); the expected codes come from the contract and were checked apart.

private let v = Vectors.shared
private var hostSignPub: Data { v.b64("keys", "hostSignPub") }
private var dk: Data { v.b64("keys", "deviceSignPub") }
private var nD: Data { v.b64("sas", "deviceNonce") }
private var nH: Data { v.b64("sas", "hostNonce") }

private func confirmation(_ requestId: String, hostSignPub key: Data = hostSignPub) -> AccessConfirmation {
    AccessConfirmation(requestId: requestId, hostSignPub: key, deviceKey: dk, deviceNonce: nD, hostNonce: nH)
}

@Test func accessConfirmCodeVectors() {
    // Same inputs as the SAS vector: the fields are read the way the SAS reads them.
    #expect(RelayCrypto.sasCode(hostSignKey: hostSignPub, deviceKey: dk, deviceNonce: nD, hostNonce: nH) == "919055")
    #expect(RelayCrypto.accessConfirmCode(hostSignPub: hostSignPub, deviceKey: dk, deviceNonce: nD, hostNonce: nH,
                                          requestId: "req_vector_1") == "244257")
    #expect(RelayCrypto.accessConfirmCode(hostSignPub: hostSignPub, deviceKey: dk, deviceNonce: nD, hostNonce: nH,
                                          requestId: "req_vector_2") == "729851")
    // A swapped host key gives another code (leading zero kept), which never matches the real one.
    let swapped = RelayCrypto.accessConfirmCode(hostSignPub: dk, deviceKey: dk, deviceNonce: nD, hostNonce: nH, requestId: "req_vector_1")
    #expect(swapped == "050343")
    #expect(swapped != "244257")
    #expect(confirmation("req_vector_1", hostSignPub: dk).verify("244257") == .mismatch)
    #expect(confirmation("req_vector_1", hostSignPub: dk).verify("050343") == .match)
}

@Test func accessConfirmVerifiesOnlyTheTypedCode() {
    let c = confirmation("req_vector_1")
    #expect(c.verify("244257") == .match)
    #expect(c.verify("244 257") == .match)
    #expect(c.verify(" 244\u{00A0}257\n") == .match)
    #expect(c.verify("٢٤٤٢٥٧") == .match) // Arabic-Indic digits
    #expect(c.verify("\u{200E}244 257") == .match) // a direction mark from pasted text
    #expect(c.verify("244258") == .mismatch)
    #expect(c.verify("729851") == .mismatch) // the other request's code
    #expect(c.verify("24425") == .mismatch)
    #expect(c.verify("2442570") == .mismatch)
    #expect(c.verify("244-257") == .mismatch)
    #expect(c.verify("") == .mismatch)
    #expect(AccessConfirmation.digits(in: "Código: 244 257") == "244257")
    #expect(AccessConfirmation.normalize("12a456") == nil)
}

@Test(arguments: ["", "req.1", "req 1", "req/1", "réq_1", "req_1\n", String(repeating: "a", count: 65)])
func invalidRequestIdFailsClosed(requestId: String) {
    #expect(!RelayCrypto.isAccessRequestId(requestId))
    #expect(RelayCrypto.accessConfirmCode(hostSignPub: hostSignPub, deviceKey: dk, deviceNonce: nD, hostNonce: nH,
                                          requestId: requestId) == nil)
    #expect(confirmation(requestId).verify("244257") == .unusable)
}

@Test func accessConfirmBoundariesAndShortInputs() {
    #expect(RelayCrypto.isAccessRequestId(String(repeating: "a", count: 64)))
    #expect(RelayCrypto.isAccessRequestId("A-z_0"))
    // A key or nonce of the wrong size gives no code at all.
    #expect(RelayCrypto.accessConfirmCode(hostSignPub: hostSignPub, deviceKey: dk, deviceNonce: nD, hostNonce: Data(),
                                          requestId: "req_vector_1") == nil)
    #expect(RelayCrypto.accessConfirmCode(hostSignPub: hostSignPub.prefix(31), deviceKey: dk, deviceNonce: nD, hostNonce: nH,
                                          requestId: "req_vector_1") == nil)
}

@Test func accessConfirmationNeverPrintsItsBytes() {
    let c = confirmation("req_vector_1")
    let printed = [String(describing: c), String(reflecting: c), "\(c)"]
    for text in printed {
        #expect(!text.contains(nD.base64URL))
        #expect(!text.contains("244257"))
    }
    #expect(Mirror(reflecting: c).children.isEmpty)
}
