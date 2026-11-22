// swift-tools-version: 6.0
// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import PackageDescription

let package = Package(
    name: "CodyncKit",
    defaultLocalization: "pt-BR",
    platforms: [.iOS("26.0")], // redesenho: Liquid Glass nativo do iOS 26
    products: [
        .library(name: "CodyncKit", targets: ["CodyncKit"]),
        .library(name: "CodyncUI", targets: ["CodyncUI"]),
    ],
    dependencies: [
        // libwebrtc for the remote screen viewer (hardware H.264 over WebRTC) and realtime voice calls.
        .package(url: "https://github.com/stasel/WebRTC.git", exact: "153.0.0"),
        // Terminal emulator for agent install and sign-in. 1.19+ adds a build-tool
        // plugin every Xcode build would have to trust.
        .package(url: "https://github.com/migueldeicaza/SwiftTerm.git", exact: "1.18.0"),
    ],
    targets: [
        .target(name: "CodyncKit", resources: [.copy("Resources/ThirdPartyNotices"), .process("Resources/ProviderIcons.xcassets"), .process("Resources/Mexeriquinhas.xcassets"), .process("Resources/Localizable.xcstrings")]),
        .target(
            name: "CodyncUI",
            dependencies: [
                "CodyncKit",
                .product(name: "WebRTC", package: "WebRTC"),
                .product(name: "SwiftTerm", package: "SwiftTerm"),
            ],
            resources: [.process("Resources")]
        ),
        .testTarget(name: "CodyncKitTests", dependencies: ["CodyncKit"], resources: [.copy("Fixtures")]),
        .testTarget(name: "CodyncUITests", dependencies: ["CodyncUI", "CodyncKit"]),
    ]
)
