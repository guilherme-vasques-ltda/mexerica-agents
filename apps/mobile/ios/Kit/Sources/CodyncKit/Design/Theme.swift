// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import SwiftUI

import UIKit
private extension UIColor {
    convenience init(hex: UInt32, alpha: CGFloat = 1) {
        self.init(
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha
        )
    }
}

public extension Color {
    init(hex: UInt32) {
        self.init(red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255, blue: Double(hex & 0xFF) / 255)
    }

    /// Light/dark pair resolved by the system appearance.
    init(light: UInt32, dark: UInt32) {
        self.init(uiColor: UIColor { $0.userInterfaceStyle == .dark ? UIColor(hex: dark) : UIColor(hex: light) })
    }
}

/// Black and white: a pure black (or white) ground, grey bubbles, and color that
/// comes only from the bots; amber marks "needs you" and red marks errors.
public enum Palette {
    public static let background = Color(light: 0xFFFFFF, dark: 0x0A0A0A)
    public static let surface = Color(light: 0xF4F4F4, dark: 0x141414)
    public static let bubbleAgent = Color(light: 0xF0F0F0, dark: 0x1C1C1C)
    public static let bubbleUser = Color(light: 0xE2E2E2, dark: 0x363636)
    public static let border = Color(light: 0xE6E6E6, dark: 0x262626)
    public static let text = Color(light: 0x141414, dark: 0xF2F2F2)
    public static let secondary = Color(light: 0x6B6B6B, dark: 0x9A9A9A)
    public static let tertiary = Color(light: 0x767676, dark: 0x929292)
    /// Ink used for fills (primary buttons, the send button, unread badges).
    public static let accentFill = Color(light: 0x000000, dark: 0xFFFFFF)
    /// Ink readable as text and tint on the background.
    public static let accent = Color(light: 0x000000, dark: 0xFFFFFF)
    public static let onAccent = Color(light: 0xFFFFFF, dark: 0x000000)
    public static let accentDim = Color(light: 0xD9D9D9, dark: 0x333333)
    public static let danger = Color(light: 0xC23A2B, dark: 0xF0A7A7)
    public static let warning = Color(hex: 0xF0A030)
    /// A bot that needs you, in widgets and Live Activities: readable amber in both modes.
    public static let attention = Color(light: 0x936000, dark: 0xECAF52)
    public static let codeBackground = Color(light: 0xF4F4F4, dark: 0x111111)
    public static let added = Color(light: 0x2E7D32, dark: 0x8FD18B)
    public static let removed = Color(light: 0xC62828, dark: 0xF0A7A7)
}

public enum AvatarPalette {
    public struct Swatch: Identifiable, Hashable, Sendable {
        public let id: String
        public let label: String
        public let hex: UInt32
        public var color: Color { Color(hex: hex) }
    }

    public static var colors: [Swatch] { [
        .init(id: "black", label: String(localized: "Preto", bundle: .module), hex: 0x2B2B2B),
        .init(id: "brown", label: String(localized: "Marrom", bundle: .module), hex: 0x936439),
        .init(id: "red", label: String(localized: "Vermelho", bundle: .module), hex: 0xFF263C),
        .init(id: "orange", label: String(localized: "Laranja", bundle: .module), hex: 0xFF6700),
        .init(id: "yellow", label: String(localized: "Amarelo", bundle: .module), hex: 0xFF9800),
        .init(id: "green", label: String(localized: "Verde", bundle: .module), hex: 0x00C972),
        .init(id: "cyan", label: String(localized: "Ciano", bundle: .module), hex: 0x00BCA6),
        .init(id: "blue", label: String(localized: "Azul", bundle: .module), hex: 0x1084FE),
        .init(id: "violet", label: String(localized: "Violeta", bundle: .module), hex: 0x9159FE),
        .init(id: "magenta", label: String(localized: "Magenta", bundle: .module), hex: 0xFF309B),
        .init(id: "gray", label: String(localized: "Cinza", bundle: .module), hex: 0x777777),
    ] }

    public static let shapes = ["blob", "pebble", "squircle", "tablet", "wedge", "hex", "cloud", "teardrop"]

    public static func color(_ id: String) -> Color {
        (colors.first { $0.id == id } ?? colors[7]).color
    }
}

public enum RelativeTime {
    static var resourceBundle: Bundle { .module }
    /// Grok-style compact age: now · 5m · 3h · 2d · Sep 3
    public static func short(_ date: Date, now: Date = .now) -> String {
        short(date, now: now, bundle: .module)
    }

    static func short(_ date: Date, now: Date, bundle: Bundle) -> String {
        let s = max(0, now.timeIntervalSince(date))
        switch s {
        case ..<60: return String(localized: "agora", bundle: bundle)
        case ..<3600: return String(localized: "\(Int(s / 60))m", bundle: bundle)
        case ..<86_400: return String(localized: "\(Int(s / 3600))h", bundle: bundle)
        case ..<(86_400 * 7): return String(localized: "\(Int(s / 86_400))d", bundle: bundle)
        default: return date.formatted(.dateTime.month(.abbreviated).day())
        }
    }

    /// Roster stamp: 3:45 AM · Yesterday · Wednesday · Sep 16
    public static func day(_ date: Date, now: Date = .now) -> String {
        let cal = Calendar.current
        if cal.isDate(date, inSameDayAs: now) { return date.formatted(date: .omitted, time: .shortened) }
        if cal.isDateInYesterday(date) { return String(localized: "Ontem", bundle: .module) }
        if now.timeIntervalSince(date) < 6 * 86_400 { return date.formatted(.dateTime.weekday(.wide)) }
        return date.formatted(.dateTime.month(.abbreviated).day())
    }

    /// Chat separator: Today 3:27 AM · Yesterday 5:20 PM · Sep 16 9:02 AM
    public static func separator(_ date: Date) -> String {
        let cal = Calendar.current
        let day = cal.isDateInToday(date) ? String(localized: "Hoje", bundle: .module)
            : cal.isDateInYesterday(date) ? String(localized: "Ontem", bundle: .module)
            : date.formatted(.dateTime.month(.abbreviated).day())
        let time = date.formatted(date: .omitted, time: .shortened)
        return String(localized: "\(day) \(time)", bundle: .module)
    }

    public static func until(_ date: Date, now: Date = .now) -> String {
        until(date, now: now, bundle: .module)
    }

    static func until(_ date: Date, now: Date, bundle: Bundle) -> String {
        let s = max(0, date.timeIntervalSince(now))
        let h = Int(s / 3600), m = Int(s.truncatingRemainder(dividingBy: 3600) / 60)
        if h >= 48 { return String(localized: "\(h / 24)d", bundle: bundle) }
        if h > 0 { return String(localized: "\(h)h \(m)m", bundle: bundle) }
        return String(localized: "\(m)m", bundle: bundle)
    }
}

public enum BackendInfo {
    public static func name(_ id: String) -> String {
        switch id {
        case "claude": "Claude Code"
        case "codex": "Codex"
        case "opencode": "OpenCode"
        case "grok": "Grok Build"
        case "gemini": "Gemini CLI"
        default: String(localized: "Motor personalizado", bundle: .module)
        }
    }
}
