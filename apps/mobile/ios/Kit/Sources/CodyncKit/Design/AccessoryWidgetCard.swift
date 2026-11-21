// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import SwiftUI

/// Monochrome Lock Screen widgets; the system supplies the background and tint.
public struct AccessoryWidgetCard: View {
    public enum Kind { case bots, usage }
    public enum Family: String, CaseIterable { case circular = "Circle", rectangular = "Rectangle", inline = "Inline" }
    let kind: Kind
    let family: Family
    let bots: [Bot]
    let usage: Usage
    let paired: Bool
    @Environment(\.locale) private var locale

    public init(kind: Kind, family: Family, bots: [Bot] = [], usage: Usage = Usage(), paired: Bool = true) {
        self.kind = kind; self.family = family; self.bots = bots.filter { !$0.hidden }
        self.usage = usage; self.paired = paired
    }

    private var needs: Int { bots.filter(\.needsInput).count }
    private var working: Int { bots.filter { $0.isWorking && !$0.needsInput }.count }
    private var count: Int { needs > 0 ? needs : working > 0 ? working : bots.count }
    private var botLabel: String {
        if needs > 0 { return needs == 1 ? String(localized: "precisa de você (rótulo singular)", bundle: .module) : String(localized: "precisam de você (rótulo plural)", bundle: .module) }
        if working > 0 { return String(localized: "trabalhando", bundle: .module) }
        return bots.count == 1 ? String(localized: "bot (rótulo singular)", bundle: .module) : String(localized: "bots (rótulo plural)", bundle: .module)
    }
    private var top: (name: String, window: UsageWindow)? {
        usage.providers.flatMap { p in p.windows.map { (name: p.name, window: $0) } }
            .max { $0.window.percent < $1.window.percent }
    }
    private var headline: String {
        guard paired else { return String(localized: "Conectar um computador", bundle: .module) }
        if kind == .bots {
            if bots.isEmpty { return String(localized: "Nenhum bot ainda", bundle: .module) }
            if needs > 0 { return String(localized: "\(count) precisa de você", bundle: .module) }
            if working > 0 { return String(localized: "\(count) trabalhando", bundle: .module) }
            return String(localized: "\(count) bot", bundle: .module)
        }
        return top.map { "\($0.name) \(Int($0.window.percent.rounded()))%" } ?? String(localized: "Nenhum uso ainda", bundle: .module)
    }

    public var body: some View {
        Group {
            switch family {
            case .inline:
                Label(headline, systemImage: kind == .bots ? "bubble.left.and.bubble.right" : "chart.bar")
                    .font(.caption).lineLimit(1)
            case .circular:
                if !paired {
                    Image(systemName: "desktopcomputer").accessibilityLabel(headline)
                } else if kind == .usage, let top {
                    Gauge(value: min(100, max(0, top.window.percent)), in: 0...100) {
                        Text(top.name.prefix(2))
                    } currentValueLabel: {
                        Text("\(Int(top.window.percent.rounded()))").font(.system(size: 18, weight: .semibold))
                    }
                    .gaugeStyle(.accessoryCircularCapacity)
                    .accessibilityLabel(headline)
                } else {
                    VStack(spacing: 0) {
                        Text(kind == .bots ? count.formatted(.number.locale(locale)) : "—").font(.system(size: 24, weight: .semibold, design: .rounded))
                        Text(kind == .bots ? botLabel : String(localized: "uso", bundle: .module))
                            .font(.system(size: 9))
                    }
                }
            case .rectangular:
                VStack(alignment: .leading, spacing: 2) {
                    Text(headline).font(.system(size: 12, weight: .semibold)).lineLimit(1)
                    if paired, kind == .bots {
                        ForEach(bots.sorted { rank($0) < rank($1) }.prefix(2)) { bot in
                            Text("\(bot.name) · \(bot.needsInput ? String(localized: "Precisa de você", bundle: .module) : bot.isWorking ? String(localized: "Trabalhando", bundle: .module) : String(localized: "Disponível", bundle: .module))")
                                .font(.system(size: 10)).lineLimit(1)
                        }
                    } else if paired, let top {
                        Text(top.window.title).font(.system(size: 10)).lineLimit(1)
                        if let reset = top.window.resetsShort() {
                            Text(reset).font(.system(size: 9)).lineLimit(1)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private func rank(_ bot: Bot) -> Int { bot.needsInput ? 0 : bot.isWorking ? 1 : 2 }
}
