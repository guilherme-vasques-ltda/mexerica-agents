import Foundation

// Compile together with AccountConfiguration.swift: the build and unit tests share the merger.
do {
    let args = CommandLine.arguments
    guard args.count == 7 else { throw CocoaError(.fileReadInvalidFileName) }
    let base = try Data(contentsOf: URL(fileURLWithPath: args[1]))
    let localExists = FileManager.default.fileExists(atPath: args[2])
    let local = localExists ? try Data(contentsOf: URL(fileURLWithPath: args[2])) : nil
    let merged = try AccountConfiguration.merge(base: base, local: local)
    let config = try PropertyListDecoder().decode(AccountConfiguration.self, from: merged)
    try config.validateForBuild(environment: args[4], isDebug: args[5].hasSuffix("Debug"),
                                isDevice: args[6] == "iphoneos", localFileExists: localExists)
    try merged.write(to: URL(fileURLWithPath: args[3]), options: .atomic)
} catch {
    // Do not print plist contents or values on failure.
    let message = (error as? AccountConfiguration.ConfigurationError)?.errorDescription
        ?? "Configuração de conta ilegível ou inválida; confira os plists do ambiente."
    FileHandle.standardError.write(Data("error: \(message)\n".utf8))
    exit(1)
}
