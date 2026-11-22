import Foundation
import Testing
@testable import CodyncKit

@Test func localAccountConfigurationOverridesWithoutCrossingEnvironments() throws {
    let base = try PropertyListSerialization.data(fromPropertyList: [
        "cloudURL": "https://api.example.invalid", "clerkPublishableKey": "",
        "clerkTokenOrigin": "https://app.example.invalid", "pushRelayURL": ""
    ], format: .xml, options: 0)
    let local = try PropertyListSerialization.data(fromPropertyList: [
        "clerkPublishableKey": "synthetic-public-key", "cloudURL": "", "futureField": "kept"
    ], format: .xml, options: 0)
    let merged = try AccountConfiguration.merge(base: base, local: local)
    let config = try PropertyListDecoder().decode(AccountConfiguration.self, from: merged)
    #expect(config.clerkPublishableKey == "synthetic-public-key")
    #expect(config.cloudURL == "")
    #expect(config.clerkTokenOrigin == "https://app.example.invalid")
    let raw = try PropertyListSerialization.propertyList(from: merged, format: nil) as? [String: String]
    #expect(raw?["futureField"] == "kept")
    let dev = try PropertyListSerialization.data(fromPropertyList: ["cloudURL": "", "clerkPublishableKey": ""], format: .xml, options: 0)
    let devConfig = try PropertyListDecoder().decode(AccountConfiguration.self, from: AccountConfiguration.merge(base: dev, local: nil))
    #expect(devConfig.cloudURL == "")
    #expect(devConfig.clerkPublishableKey == "")
    #expect(throws: (any Error).self) { try AccountConfiguration.merge(base: base, local: Data("bad plist".utf8)) }
}

@Test func loginGateDoesNotDependOnSavedOnboardingOrComputers() {
    #expect(AccountConfiguration.requiresLogin(configured: true, signedIn: false))
    #expect(!AccountConfiguration.requiresLogin(configured: true, signedIn: true))
    #expect(!AccountConfiguration.requiresLogin(configured: false, signedIn: false))
    #expect(!AccountConfiguration.requiresLogin(configured: false, signedIn: true))
}

private let publicTestKey = "pk_test_" + Data("clerk.example.test$".utf8).base64EncodedString().replacingOccurrences(of: "=", with: "")

@Test func tokenPostUsesConfiguredOriginAndPreservesNativeAuthentication() throws {
    let policy = try ClerkTokenRequest(publishableKey: publicTestKey, origin: "https://app.example.invalid")
    var request = URLRequest(url: URL(string: "https://clerk.example.test/v1/client/sessions/sess_fixture/tokens?_is_native=true")!)
    request.httpMethod = "POST"
    request.httpBody = Data("organization_id=&force_origin=true".utf8)
    request.setValue("synthetic-native-credential", forHTTPHeaderField: "Authorization")
    let original = request
    policy.prepare(&request)
    #expect(request.value(forHTTPHeaderField: "Origin") == "https://app.example.invalid")
    #expect(request.value(forHTTPHeaderField: "Authorization") == original.value(forHTTPHeaderField: "Authorization"))
    #expect(request.url == original.url)
    #expect(request.httpBody == original.httpBody)
    #expect(!request.httpShouldHandleCookies)
}

@Test(arguments: [
    "https://other.example.test/v1/client/sessions/sess_fixture/tokens",
    "https://clerk.example.test/v1/client/sign_ins",
    "https://clerk.example.test/v1/client/sessions/sess_fixture/tokens/template",
    "http://clerk.example.test/v1/client/sessions/sess_fixture/tokens",
    "https://clerk.example.test:444/v1/client/sessions/sess_fixture/tokens"
]) func tokenOriginIsNeverAddedToOtherRequests(url: String) throws {
    let policy = try ClerkTokenRequest(publishableKey: publicTestKey, origin: "https://app.example.invalid")
    var request = URLRequest(url: URL(string: url)!)
    request.httpMethod = "POST"
    policy.prepare(&request)
    #expect(request.value(forHTTPHeaderField: "Origin") == nil)
}

@Test(arguments: ["", "http://app.example.invalid", "https://app.example.invalid/path", "https://app.example.invalid:443", "https://user@app.example.invalid", "https://app.example.invalid?q=1"])
func invalidTokenOriginFailsClosed(origin: String) {
    #expect(throws: (any Error).self) { try ClerkTokenRequest(publishableKey: publicTestKey, origin: origin) }
}

@Test(arguments: ["", " \n", "https://cloud.example.test"])
func mainReleaseNeverFallsBackToQR(cloud: String) throws {
    for key in ["", " \n", "synthetic-public-key"] {
        #expect(AccountConfiguration.isConfigured(cloudURL: cloud, publishableKey: key, environment: "main", isDebug: false))
        #expect(AccountConfiguration.requiresLogin(configured: true, signedIn: false))
    }
}

@Test func missingOrUnreadableConfigurationKeepsMainReleaseClosed() {
    for data in [Data?.none, Data("bad plist".utf8), Data("<plist><array/></plist>".utf8)] {
        let config = data.flatMap { try? PropertyListDecoder().decode(AccountConfiguration.self, from: $0) }
        #expect(config == nil)
        #expect(AccountConfiguration.isConfigured(cloudURL: config?.cloudURL ?? "", publishableKey: config?.clerkPublishableKey ?? "",
                                                 environment: "main", isDebug: false))
    }
}

@Test func emptyConfigurationAllowsQROnlyInDebugOrDev() {
    #expect(!AccountConfiguration.isConfigured(cloudURL: "", publishableKey: "", environment: "main", isDebug: true))
    #expect(!AccountConfiguration.isConfigured(cloudURL: "", publishableKey: "", environment: "dev", isDebug: false))
    #expect(!AccountConfiguration.isConfigured(cloudURL: "", publishableKey: "", environment: "dev", isDebug: true))
    #expect(AccountConfiguration.isConfigured(cloudURL: "", publishableKey: "", environment: "unknown", isDebug: false))
    #expect(AccountConfiguration.isConfigured(cloudURL: "https://cloud.example.test", publishableKey: "synthetic",
                                             environment: "dev", isDebug: true))
}

@Test func processOverridesApplyOnlyInDebug() {
    for bundled in [String?.none, "", "bundled"] {
        #expect(AccountConfiguration.value(bundled, debugOverride: " process ", isDebug: false) == (bundled ?? ""))
        #expect(AccountConfiguration.value(bundled, debugOverride: " process ", isDebug: true) == "process")
        #expect(AccountConfiguration.value(bundled, debugOverride: "", isDebug: true) == "")
    }
}

@Test func mainReleaseBuildRejectsEmptyKeyAndCloud() throws {
    func config(_ key: String, _ cloud: String) throws -> AccountConfiguration {
        let data = try PropertyListSerialization.data(fromPropertyList: ["clerkPublishableKey": key, "cloudURL": cloud], format: .xml, options: 0)
        return try PropertyListDecoder().decode(AccountConfiguration.self, from: data)
    }
    for empty in ["", " \n"] {
        let noKey = try config(empty, "https://cloud.example.test")
        #expect(throws: AccountConfiguration.ConfigurationError.missingKey) { try noKey.validateForBuild(environment: "main", isDebug: false) }
        let noCloud = try config("synthetic", empty)
        #expect(throws: AccountConfiguration.ConfigurationError.missingCloud) { try noCloud.validateForBuild(environment: "main", isDebug: false) }
        try noKey.validateForBuild(environment: "main", isDebug: true)
        try noCloud.validateForBuild(environment: "dev", isDebug: false)
    }
    try config("synthetic", "https://cloud.example.test").validateForBuild(environment: "main", isDebug: false)
}

@Test func deviceAndReleaseBuildsFailWithoutLocalFileOrKey() throws {
    func config(_ key: String, _ cloud: String = "https://cloud.example.test") throws -> AccountConfiguration {
        let data = try PropertyListSerialization.data(fromPropertyList: ["clerkPublishableKey": key, "cloudURL": cloud], format: .xml, options: 0)
        return try PropertyListDecoder().decode(AccountConfiguration.self, from: data)
    }
    let empty = try config("")
    for isDebug in [true, false] {
        #expect(throws: AccountConfiguration.ConfigurationError.missingLocalFile(environment: "main")) {
            try empty.validateForBuild(environment: "main", isDebug: isDebug, isDevice: true, localFileExists: false)
        }
    }
    #expect(throws: AccountConfiguration.ConfigurationError.missingLocalFile(environment: "main")) {
        try empty.validateForBuild(environment: "main", isDebug: false, isDevice: false, localFileExists: false)
    }
    // Simulador em Debug e qualquer build Dev seguem permitindo configuração vazia.
    try empty.validateForBuild(environment: "main", isDebug: true, isDevice: false, localFileExists: false)
    try empty.validateForBuild(environment: "dev", isDebug: false, isDevice: true, localFileExists: false)
    // Com o arquivo presente, a falha aponta a chave; preenchida, o build segue.
    #expect(throws: AccountConfiguration.ConfigurationError.missingKey) {
        try empty.validateForBuild(environment: "main", isDebug: true, isDevice: true, localFileExists: true)
    }
    try config("synthetic").validateForBuild(environment: "main", isDebug: true, isDevice: true, localFileExists: true)
}

@Test func localFileIsRequiredBeforeKeyValidationInDeviceAndRelease() throws {
    func config(_ key: String, _ cloud: String = "https://cloud.example.test") throws -> AccountConfiguration {
        let data = try PropertyListSerialization.data(fromPropertyList: ["clerkPublishableKey": key, "cloudURL": cloud], format: .xml, options: 0)
        return try PropertyListDecoder().decode(AccountConfiguration.self, from: data)
    }
    // Chave preenchida no plist base não dispensa o .local.plist: a existência do arquivo vem antes da chave.
    let withKey = try config("synthetic-public-key")
    for (isDebug, isDevice) in [(true, true), (false, true), (false, false)] {
        #expect(throws: AccountConfiguration.ConfigurationError.missingLocalFile(environment: "main")) {
            try withKey.validateForBuild(environment: "main", isDebug: isDebug, isDevice: isDevice, localFileExists: false)
        }
    }
    // Debug de simulador e qualquer build Dev seguem dispensados do arquivo.
    try withKey.validateForBuild(environment: "main", isDebug: true, isDevice: false, localFileExists: false)
    try withKey.validateForBuild(environment: "dev", isDebug: false, isDevice: true, localFileExists: false)
    // Com arquivo e chave, os três caminhos protegidos passam.
    for (isDebug, isDevice) in [(true, true), (false, true), (false, false)] {
        try withKey.validateForBuild(environment: "main", isDebug: isDebug, isDevice: isDevice, localFileExists: true)
    }
}
