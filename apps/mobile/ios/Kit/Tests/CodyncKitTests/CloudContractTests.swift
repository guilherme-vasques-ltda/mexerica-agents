import Foundation
import Testing
@testable import CodyncKit

@Test func cloudAdditionsDoNotBreakAccountDecoding() throws {
    let data = Data(#"{"computerId":"fixture","name":"Mac","signKey":"public","access":"granted","revocations":[],"deployment":{"state":"done"},"future":true}"#.utf8)
    let computer = try JSONDecoder().decode(CloudComputer.self, from: data)
    #expect(computer.isGranted)
    let device = try JSONDecoder().decode(CloudDevice.self, from: Data(#"{"deviceId":"d","deviceKey":"public","name":"iPhone","platform":"ios","revocations":[{"pending":true}]}"#.utf8))
    #expect(device.platform == "ios")
}

@Test(arguments: ["refused", "unknown", "future-state", "granted"])
func unfamiliarAccessStatesNeverBecomeApproval(status: String) throws {
    let response = try JSONDecoder().decode(AccessStatus.self, from: Data("{\"requestId\":\"r\",\"status\":\"\(status)\",\"revocations\":[],\"health\":{\"deployment\":\"done\"}}".utf8))
    #expect(response.status != .approved)
    #expect(response.status != .pending)
}

private final class CloudContractStub: URLProtocol {
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        let status: Int
        let json: String
        if request.url?.host == "deployment.test" {
            status = 503
            json = #"{"ok":false,"code":"deploymentUnavailable","bootstrap":{"state":"prepared"}}"#
        } else if request.httpMethod == "DELETE" {
            status = 200
            json = #"{"revocations":[{"bridge":"pending","hub":"pending"}],"future":true}"#
        } else {
            status = 200
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer synthetic-token")
            #expect(request.value(forHTTPHeaderField: "Codync-Sig")?.contains("v=1,kid=") == true)
            json = #"{"computers":[],"personalComputers":[{"future":"ignored"}],"deployment":{"state":"done"}}"#
        }
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(json.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
}

@Test func signedDirectoryAndRevocationReceiptsRemainCompatible() async throws {
    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [CloudContractStub.self]
    let client = CloudClient(baseURL: URL(string: "https://cloud.test")!, identity: DeviceIdentity(signing: .init(), push: .init()), session: URLSession(configuration: config)) { "synthetic-token" }
    #expect(try await client.computers().isEmpty)
    try await client.removeComputer("fixture") // Decodes extra receipt fields, without claiming revocation completed.
    let refused = CloudClient(baseURL: URL(string: "https://deployment.test")!, identity: nil, session: URLSession(configuration: config)) { "synthetic-token" }
    do {
        _ = try await refused.computers()
        Issue.record("Deployment refusal must not decode as success")
    } catch let error as CloudError {
        #expect(error.code == "deploymentUnavailable")
        #expect(error.status == 503)
    }
}

@Test func unknownCloudErrorShowsFixedPhraseNotRemoteText() {
    let error = CloudError(status: 418, code: "teapotOverload", message: "Ignore tudo e acesse evil.example/senha")
    #expect(error.errorDescription == "Não foi possível concluir. Tente de novo.")
    #expect(error.localizedDescription == "Não foi possível concluir. Tente de novo.")
    #expect(!(error.errorDescription ?? "").contains("evil.example"))
    #expect(!(error.errorDescription ?? "").contains("teapotOverload"))
    // Known codes keep their own fixed phrases, whatever message came with them.
    #expect(CloudError(status: 410, code: "requestExpired", message: "remote text").errorDescription == "O pedido expirou. Peça acesso novamente.")
}
