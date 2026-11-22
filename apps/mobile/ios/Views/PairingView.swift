// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import CodyncUI
import SwiftUI
import VisionKit

/// Pair with a computer running codync-host, one thing per page: install it, then scan its code.
struct PairingView<Leading: View>: View {
    /// Adding another computer from the computers sheet: has a close button; closes once paired.
    var inModal = false
    /// Finish onboarding without pairing; modal pairing simply closes when skipped.
    var onSkip: (() -> Void)? = nil
    /// Top-left on the first page (back to the welcome, the account switcher).
    @ViewBuilder var leading: Leading

    private enum Step { case install, scan }
    @State private var step = Step.install
    @Environment(\.dismissModal) private var dismissModal
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(spacing: 0) {
            ScreenHeader {
                if step == .scan { BackButton { go(.install) } } else { leading }
            } title: {
                HStack(spacing: 6) {
                    Capsule().fill(Palette.text).frame(width: 18, height: 4)
                    Capsule().fill(step == .scan ? Palette.text : Palette.accentDim).frame(width: 18, height: 4)
                }
                .accessibilityElement()
                .accessibilityLabel(step == .install ? String(localized: "Etapa 1 de 2") : String(localized: "Etapa 2 de 2"))
            } trailing: {
                if inModal { IconButton(String(localized: "Fechar"), systemImage: "xmark") { dismissModal() } }
            }

            ZStack {
                switch step {
                case .install:
                    InstallPage() { go(.scan) }
                        .transition(.move(edge: .leading).combined(with: .opacity))
                case .scan:
                    ScanPage() { if inModal { dismissModal() } }
                        .transition(.move(edge: .trailing).combined(with: .opacity))
                }
            }
            .frame(maxHeight: .infinity)
            .clipped()

            if step == .scan {
                Button {
                    withAnimation(Motion.reduced(Motion.layout, reduceMotion)) {
                        if inModal { dismissModal() } else { onSkip?() }
                    }
                } label: {
                    Text(String(localized: "Pular")).font(.headline).frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.secondary)
                .accessibilityHint(String(localized: "Pareie um computador depois em Computadores e ajustes."))
                .padding(.horizontal, 24)
                .padding(.bottom, 12)
                .transition(.opacity)
            }
        }
        .background(Palette.background)
        // First run, each page is an onboarding step; afterwards, the whole sheet is one screen.
        .tracked(inModal ? .screenViewed(.pairing) : .onboardingStepViewed(step == .install ? .install : .scan))
    }

    private func go(_ next: Step) {
        withAnimation(Motion.reduced(.spring(duration: 0.45, bounce: 0), reduceMotion)) { step = next }
    }
}

extension PairingView where Leading == EmptyView {
    init(inModal: Bool) { self.init(inModal: inModal) { EmptyView() } }
}

private enum ComputerOS: Hashable { case mac, linux }

// MARK: - Install

private struct InstallPage: View {
    let next: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    // A bot on the computer's screen: that's where it lives.
                    ZStack {
                        Image(systemName: "laptopcomputer")
                            .font(.system(size: 150, weight: .ultraLight))
                            .foregroundStyle(Palette.tertiary)
                        CharacterAvatar(shape: "blob", color: "blue", size: 52, mood: .working)
                            .offset(y: -12)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.top, 8)

                    VStack(alignment: .leading, spacing: 10) {
                        Text(String(localized: "Abra a Mexerica\nno seu computador"))
                            .font(.system(size: 30, weight: .semibold))
                            .tracking(-0.5)
                            .foregroundStyle(Palette.text)
                        Text(String(localized: "Seus bots funcionam lá. Use este iPhone para conversar com eles."))
                            .font(.body)
                            .foregroundStyle(Palette.secondary)
                    }

                    Text(String(localized: "Abra a Mexerica no computador e entre com a mesma conta quando a entrada estiver disponível. Você também pode parear localmente usando o código QR."))
                        .font(.subheadline)
                        .foregroundStyle(Palette.secondary)
                }
                .padding(.horizontal, 24)
                .padding(.bottom, 16)
            }

            Button(action: next) {
                Text(String(localized: "Continuar")).font(.headline).frame(maxWidth: .infinity, minHeight: 50)
            }
            .buttonStyle(.primary)
            .padding(.horizontal, 24)
            .padding(.bottom, 12)
        }
    }
}

// MARK: - Scan

private struct ScanPage: View {
    let paired: () -> Void
    @Environment(AppStore.self) private var app
    @Environment(AccountSession.self) private var account
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.openURL) private var openURL
    @State private var pairing = false
    @State private var pasted = ""
    @State private var error: String?
    /// Bumped after a failed pair so the scanner starts looking again.
    @State private var attempt = 0
    @State private var tailscaleOn = Tailscale.isConnected

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                VStack(alignment: .leading, spacing: 10) {
                    Text(String(localized: "Escaneie o código de pareamento"))
                        .font(.system(size: 30, weight: .semibold))
                        .tracking(-0.5)
                        .foregroundStyle(Palette.text)
                    Text(String(localized: "Abra a Mexerica no computador e mostre o código de pareamento."))
                        .font(.body)
                        .foregroundStyle(Palette.secondary)
                }

                viewfinder

                HStack {
                    TextField(String(localized: "Ou cole um link \(AppEnvironment.current.urlScheme)://pair"), text: $pasted)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .font(.callout.monospaced())
                        .onSubmit { pair(pasted) }
                    IconButton(String(localized: "Parear"), systemImage: "arrow.right") { pair(pasted) }
                        .disabled(pasted.isEmpty || pairing)
                }
                .padding(.leading, 14)
                .padding(.trailing, 4)
                .padding(.vertical, 4)
                .background(Palette.surface, in: RoundedRectangle(cornerRadius: 12))

                if let error {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(error).font(.footnote).foregroundStyle(Palette.danger)
                        // The usual cause: the phone has no road to the computer yet.
                        Label(String(localized: "Confira se este iPhone e o computador estão no mesmo Wi-Fi ou ambos no Tailscale, ou se “Acessar de qualquer lugar” está ativado na Mexerica no computador."),
                              systemImage: "wifi.exclamationmark")
                            .font(.footnote)
                            .foregroundStyle(Palette.secondary)
                    }
                    .transition(.opacity)
                }

                // Signed in, account computers show up in Computers, where a tap asks one for access.
                if account.isConfigured && !account.isSignedIn { SignInButtons() }

                // How it connects is never a choice here: every paired computer gets every route.
                VStack(alignment: .leading, spacing: 8) {
                    Label(String(localized: "Conecta por Wi-Fi ou Tailscale. Um relay configurado permite a conexão de qualquer lugar. Criptografia de ponta a ponta."),
                          systemImage: "lock.fill")
                    if tailscaleOn {
                        Label(String(localized: "O Tailscale está ativo: a Mexerica se conecta diretamente por ele."), systemImage: "checkmark.circle.fill")
                    } else {
                        Button {
                            Analytics.shared.capture(.linkOpened(.tailscale))
                            openURL(Tailscale.downloadURL)
                        } label: {
                            Label(String(localized: "Usa Tailscale? Conexão direta e mais rápida fora de casa."), systemImage: "arrow.up.right")
                        }
                        .buttonStyle(PressScale())
                    }
                }
                .font(.footnote)
                .foregroundStyle(Palette.tertiary)
            }
            .padding(.horizontal, 24)
            .padding(.bottom, 24)
        }
        .scrollDismissesKeyboard(.interactively)
        // Coming back from the Tailscale app: show whether it's connected now.
        .onChange(of: scenePhase) { _, phase in if phase == .active { tailscaleOn = Tailscale.isConnected } }
    }

    /// The camera, right on the page: no extra sheet between the instruction and the scan.
    private var viewfinder: some View {
        ZStack {
            if DataScannerViewController.isSupported {
                QRScanner { code in pair(code) }
                    .id(attempt)
            } else {
                VStack(spacing: 10) {
                    Image(systemName: "qrcode.viewfinder").font(.system(size: 44, weight: .light))
                    Text(String(localized: "Câmera indisponível. Cole o link abaixo.")).font(.footnote)
                }
                .foregroundStyle(Palette.tertiary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Palette.surface)
            }
            if pairing {
                Palette.background.opacity(0.7)
                Spinner(size: 28)
            }
        }
        .aspectRatio(1, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
        .animation(Motion.fade, value: pairing)
    }

    private func pair(_ text: String) {
        let p: Pairing
        do {
            p = try app.pairing(from: text.trimmingCharacters(in: .whitespacesAndNewlines))
        } catch {
            show(error)
            return
        }
        withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { error = nil }
        pairing = true
        Task {
            defer { pairing = false }
            do {
                _ = try await app.pair(p, via: .qr)
                pasted = ""
                paired()
            } catch {
                show(error)
            }
        }
    }

    private func show(_ error: Error) {
        attempt += 1
        withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { self.error = error.localizedDescription }
    }
}

/// VisionKit live QR scanner.
struct QRScanner: UIViewControllerRepresentable {
    let onCode: (String) -> Void

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let vc = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .fast,
            isHighlightingEnabled: true
        )
        vc.delegate = context.coordinator
        try? vc.startScanning()
        return vc
    }

    func updateUIViewController(_ vc: DataScannerViewController, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator(onCode: onCode) }

    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        let onCode: (String) -> Void
        private var done = false

        init(onCode: @escaping (String) -> Void) { self.onCode = onCode }

        func dataScanner(_ scanner: DataScannerViewController, didAdd items: [RecognizedItem], allItems: [RecognizedItem]) {
            for case let .barcode(code) in items {
                if let value = code.payloadStringValue, value.hasPrefix(AppEnvironment.current.urlScheme + "://"), !done {
                    done = true
                    scanner.stopScanning()
                    onCode(value)
                }
            }
        }
    }
}
