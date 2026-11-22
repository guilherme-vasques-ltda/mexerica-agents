// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

struct MarketSection<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.headline).foregroundStyle(Palette.text).padding(.leading, 4)
            content
        }
    }
}

/// Two columns on wide screens, one on a phone.
struct ItemGrid<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 300), spacing: 10)], spacing: 4) { content }
    }
}

struct AgentCard: View {
    let backend: Backend
    let action: () -> Void
    @State private var hovering = false

    private var status: String {
        if backend.installed != true, backend.curated == true { return String(localized: "Não instalado", bundle: .module) }
        if backend.needsNode { return String(localized: "Precisa do Node.js", bundle: .module) }
        if backend.installed != true { return String(localized: "Configura na primeira utilização", bundle: .module) }
        return backend.signedIn == false ? String(localized: "Sessão não iniciada", bundle: .module) : String(localized: "Instalado", bundle: .module)
    }

    var body: some View {
        Button(action: action) {
            VStack(spacing: 12) {
                AgentIcon(registry: backend.registry ?? backend.id, size: 34)
                    .frame(width: 64, height: 64)
                    .background(Palette.bubbleAgent, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                VStack(spacing: 2) {
                    Text(backend.name).font(.subheadline.weight(.semibold)).foregroundStyle(Palette.text).lineLimit(1)
                    Text(status)
                        .font(.caption)
                        .foregroundStyle(Palette.secondary)
                        .lineLimit(1)
                }
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 18)
            .padding(.horizontal, 10)
            .background(Palette.bubbleAgent.opacity(hovering ? 0.8 : 0.45), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        }
        .buttonStyle(PressScale())
        .onHover { h in withAnimation(Motion.hover) { hovering = h } }
        .help(backend.name)
    }
}

struct MarketRow<Icon: View>: View {
    let title: String
    let subtitle: String
    let added: Bool
    var busy = false
    var addLabel = String(localized: "Adicionar", bundle: .module)
    /// The whole row opens the item's details; without it the row does what its button does.
    var open: (() -> Void)?
    @ViewBuilder let icon: Icon
    let add: () -> Void
    @State private var hovering = false

    private var rowAction: (() -> Void)? { open ?? (added || busy ? nil : add) }

    var body: some View {
        HStack(spacing: 14) {
            icon.frame(width: 46, height: 46)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body.weight(.medium)).foregroundStyle(Palette.text).lineLimit(1)
                Text(subtitle).font(.subheadline).foregroundStyle(Palette.secondary).lineLimit(1)
            }
            Spacer(minLength: 8)
            if busy {
                ThinkingOrb(size: 18, color: Palette.secondary).frame(width: 60)
            } else if added {
                Label(String(localized: "Adicionado", bundle: .module), systemImage: "checkmark")
                    .labelStyle(.iconOnly)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Palette.secondary)
                    .frame(width: 60)
                    .accessibilityLabel(String(localized: "Adicionado", bundle: .module))
            } else {
                Button(addLabel, action: add)
                    .buttonStyle(.plain)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Palette.text)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 7)
                    .background(Palette.bubbleAgent, in: Capsule())
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(hovering && rowAction != nil ? Palette.bubbleAgent.opacity(0.6) : .clear, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .onTapGesture { rowAction?() }
        .accessibilityAddTraits(rowAction == nil ? [] : .isButton)
        .onHover { h in withAnimation(Motion.hover) { hovering = h } }
    }
}

/// Placeholder rows while a list loads, shaped like the real ones.
struct SkeletonGrid: View {
    @State private var dim = false

    var body: some View {
        ItemGrid {
            ForEach(0..<4, id: \.self) { _ in
                HStack(spacing: 14) {
                    RoundedRectangle(cornerRadius: 12, style: .continuous).frame(width: 46, height: 46)
                    VStack(alignment: .leading, spacing: 8) {
                        Capsule().frame(width: 120, height: 12)
                        Capsule().frame(width: 190, height: 10)
                    }
                    Spacer()
                }
                .foregroundStyle(Palette.bubbleAgent)
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
            }
        }
        .opacity(dim ? 0.5 : 1)
        .animation(.easeInOut(duration: 0.9).repeatForever(), value: dim)
        .onAppear { dim = true }
    }
}

/// Ícone local genérico, sem consultas externas de favicon.
struct ServiceLogo: View {
    let website: String?
    /// The service's own domain when it's known (a login's site), even github.com.
    var site: String?
    let name: String
    /// MCP Registry names are reverse-DNS ("com.notion/mcp"): the owner's domain.
    var registryName: String?
    var size: CGFloat = 46

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: size * 0.26, style: .continuous)
        ZStack {
            shape.fill(Palette.bubbleAgent)
            Image(systemName: "globe")
                .font(.system(size: size * 0.5))
                .foregroundStyle(Palette.text)
        }
        .frame(width: size, height: size)
        .clipShape(shape)
    }
}

struct TileIcon: View {
    let systemName: String

    var body: some View {
        RoundedRectangle(cornerRadius: 12, style: .continuous)
            .fill(Palette.bubbleAgent)
            .overlay(Image(systemName: systemName).font(.system(size: 18, weight: .medium)).foregroundStyle(Palette.text))
    }
}

struct SkillGlyph: View {
    var body: some View {
        RoundedRectangle(cornerRadius: 12, style: .continuous)
            .fill(Palette.bubbleAgent)
            .overlay(Image(systemName: "book.pages").foregroundStyle(Palette.text))
    }
}

/// Text that opens a web page (replaces the system `Link`).
struct WebLink: View {
    let title: String
    let url: URL
    @Environment(\.openURL) private var openURL

    init(_ title: String, url: URL) {
        self.title = title
        self.url = url
    }

    var body: some View {
        Button(title) { openURL(url) }
            .buttonStyle(.plain)
            .foregroundStyle(Palette.text)
            .underline()
            .help(url.absoluteString)
    }
}
