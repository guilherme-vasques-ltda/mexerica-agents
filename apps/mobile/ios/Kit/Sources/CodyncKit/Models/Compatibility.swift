// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import Foundation

/// Phone/host compatibility: each side names the oldest version of the other it still works
/// with (docs/reference/compatibility.md). The host sends `minApp` in `hello`; this app keeps
/// `minHost`. Mirrors `host/src/compat.rs`.
public enum VersionMismatch: Equatable, Sendable {
    /// This app is older than the host's `minApp`.
    case updateApp(minimum: String)
    /// The host is older than `minHost`.
    case updateHost(version: String, minimum: String)

    /// The oldest host this app works with. Raise it only when the app starts depending on
    /// something older hosts don't have (an optional feature hides its control instead).
    public static let minHost = "2.20.1"

    /// Checks this app (`app`) against a host's version and `minApp` (nil from hosts that predate it).
    public static func check(app: String, minHost: String = minHost, hostVersion: String, minApp: String?) -> VersionMismatch? {
        if let minApp, AppVersion.isBelow(app, minApp) { return .updateApp(minimum: minApp) }
        if AppVersion.isBelow(hostVersion, minHost) { return .updateHost(version: hostVersion, minimum: minHost) }
        return nil
    }
}

/// The two fields a compatibility check needs, readable even from a `hello` whose other
/// fields this app version can't decode.
public struct HostVersion: Codable, Equatable, Sendable {
    public var version: String
    public var minApp: String?

    public init(version: String, minApp: String? = nil) {
        self.version = version
        self.minApp = minApp
    }
}

public enum AppVersion {
    /// This build's marketing version ("" when the bundle has none, e.g. in tests: never blocks).
    public static let current = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? ""

    /// Shared parse contract with Android and the Hub (`host/src/compat.rs`):
    /// `major.minor.patch`, trimming spaces AND line breaks, ignoring ONE leading `v`
    /// (`vv2.18.0` stays unreadable here — the host strips every `v`; like the desktop we strip
    /// one, choosing the side that never blocks) and any pre-release or build suffix; 1–3
    /// ASCII-digit parts read as 64-bit (the host's u64 accepts a few values above Int64.max,
    /// which stay unreadable here — again never blocking); missing parts count as 0.
    /// `nil` for anything else, and an unreadable version never blocks anyone.
    public static func parse(_ version: String) -> [Int]? {
        var core = Substring(version.trimmingCharacters(in: .whitespacesAndNewlines))
        if core.first == "v" { core = core.dropFirst() }
        guard let numbers = core.split(separator: "-", maxSplits: 1, omittingEmptySubsequences: false).first?
            .split(separator: "+", maxSplits: 1, omittingEmptySubsequences: false).first else { return nil }
        let parts = numbers.split(separator: ".", omittingEmptySubsequences: false)
        guard (1...3).contains(parts.count) else { return nil }
        var result: [Int] = []
        for part in parts {
            guard !part.isEmpty, part.allSatisfy(\.isASCII), part.allSatisfy(\.isNumber), let n = Int(part) else { return nil }
            result.append(n)
        }
        return result + Array(repeating: 0, count: 3 - result.count)
    }

    /// Release order across lines: major 0 is the Mexerica line, which comes after every inherited
    /// Codync version (2.x), so `0.1.0` is newer than `2.20.3`. Within a line the usual numeric
    /// order applies. Leaving major 0 needs this rule revisited (mirrors `rank` in host/src/compat.rs).
    private static func rank(_ v: [Int]) -> [Int] {
        [v[0] == 0 ? 1 : 0] + v
    }

    /// Whether `version` is below `minimum` in release order; an unreadable version never is.
    public static func isBelow(_ version: String, _ minimum: String) -> Bool {
        guard let v = parse(version), let m = parse(minimum) else { return false }
        return rank(v).lexicographicallyPrecedes(rank(m))
    }
}

/// Newer releases worth a (dismissible) reminder while everything still works together.
public enum UpdateReminder {
    /// The App Store has a newer app than this one, and it wasn't dismissed.
    public static func app(current: String, store: String?, dismissed: String?) -> String? {
        guard let store, AppVersion.isBelow(current, store), store != dismissed else { return nil }
        return store
    }
}
