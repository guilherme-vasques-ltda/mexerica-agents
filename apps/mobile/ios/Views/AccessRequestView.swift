// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import CodyncUI
import SwiftUI

/// Asking a computer in the account for access (spec §4.2 B): once both sides committed, this iPhone
/// shows a 6-digit code. The person types it on the Mac, which proves that whoever approves there
/// is holding this iPhone. Once the Mac saves this iPhone it shows a second code, typed back here.
/// Only a match saves the computer, so a cloud can't swap in another one.
struct AccessRequestView: View {
    let computer: CloudComputer
    @Environment(AccountStore.self) private var accounts
    @Environment(\.dismissModal) private var dismiss
    @State private var error: String?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// A code was shown: when it goes away without an approval, the request is over (denied or expired).
    @State private var sawTicket: Bool
    /// Why it ended, taken from `accounts.lastError` when the code went: shown here, so the computers
    /// list under this sheet doesn't say it again.
    @State private var outcome: String?
    /// The Mac's confirmation code as typed, ASCII digits only.
    @State private var confirmDigits = ""
    @State private var confirmMessage: String?
    @State private var confirming = false

    init(computer: CloudComputer, pending: Bool) {
        self.computer = computer
        _sawTicket = State(initialValue: pending)
    }

    private var ticket: AccessTicket? { accounts.pendingAccess[computer.computerId] }
    private var approved: Bool { accounts.store(for: computer.computerId) != nil }

    var body: some View {
        ScrollView {
        VStack(spacing: 24) {
            ComputerBadge(Computer(id: computer.computerId, name: computer.name, signKey: computer.signKey, device: computer.device), size: 64)
                .padding(.top, 32)

            if approved {
                Label("\(computer.name) aprovou este iPhone", systemImage: "checkmark.circle.fill")
                    .font(.headline)
                    .foregroundStyle(Palette.added)
            } else if accounts.accessConfirm[computer.computerId] != nil {
                confirmContent()
            } else if let ticket {
                ticketContent(ticket)
            } else if let error {
                Text(error)
                    .foregroundStyle(Palette.danger)
                    .multilineTextAlignment(.center)
                Button("Tentar de novo") { withAnimation(Motion.reduced(Motion.layout, reduceMotion)) { self.error = nil } }
                    .buttonStyle(.secondary)
            } else if sawTicket {
                Text(outcome ?? String(localized: "\(computer.name) não aprovou este pedido."))
                    .foregroundStyle(Palette.secondary)
                    .multilineTextAlignment(.center)
                Button("Pedir de novo") {
                    withAnimation(Motion.reduced(Motion.layout, reduceMotion)) {
                        outcome = nil
                        sawTicket = false
                    }
                }
                .buttonStyle(.secondary)
            } else {
                VStack(spacing: 10) {
                    Spinner()
                    Text(computer.isOnline ? String(localized: "Pedindo acesso a \(computer.name)…") : String(localized: "\(computer.name) está desligado. Ligue o computador para continuar."))
                        .font(.subheadline)
                        .foregroundStyle(Palette.secondary)
                        .multilineTextAlignment(.center)
                }
                .task { await request() }
            }
            Spacer()
        }
        .padding(24)
        .frame(maxWidth: .infinity)
        }
        .background(Palette.background)
        // Closing keeps waiting in the background; the computer shows up once approved.
        .safeAreaInset(edge: .top, spacing: 0) { ModalHeader("Acesso") }
        .onChange(of: ticket?.requestId) { old, id in
            // Whatever was typed for the Mac's code belonged to the request that just ended or changed.
            confirmDigits = ""
            confirmMessage = nil
            if id != nil {
                sawTicket = true
            } else if old != nil, !approved {
                outcome = accounts.lastError
                accounts.lastError = nil
            }
        }
        .onChange(of: approved) { _, done in
            guard done else { return }
            Task {
                try? await Task.sleep(for: .seconds(1))
                dismiss()
            }
        }
    }

    /// The code to type on the Mac, while the Mac hasn't approved yet.
    @ViewBuilder private func ticketContent(_ ticket: AccessTicket) -> some View {
        VStack(spacing: 10) {
            Text("Pedido enviado ao \(computer.name)")
                .font(.headline)
                .foregroundStyle(Palette.text)
            Text("Digite este código no seu Mac")
                .font(.subheadline)
                .foregroundStyle(Palette.secondary)
                .multilineTextAlignment(.center)
            Text(spaced(ticket.code))
                .font(.system(size: 52, weight: .semibold, design: .monospaced))
                .foregroundStyle(Palette.text)
                .textSelection(.enabled)
                .accessibilityLabel("Código \(ticket.code.map(String.init).joined(separator: " "))")
            Text("Se você não pediu isto, cancele.")
                .font(.footnote)
                .foregroundStyle(Palette.secondary)
                .multilineTextAlignment(.center)
        }
        HStack(spacing: 8) {
            Spinner()
            Text("Esperando o Mac…").foregroundStyle(Palette.secondary)
            IconButton("Cancelar pedido", systemImage: "xmark.circle.fill") {
                Task {
                    await accounts.cancelAccess(computer.computerId)
                    dismiss()
                }
            }
        }
        .font(.subheadline)
    }

    /// The Mac approved and shows its own code: typed here, it saves the computer only if it matches.
    @ViewBuilder private func confirmContent() -> some View {
        VStack(spacing: 12) {
            Text(AccessConfirmText.text(.prompt))
                .font(.headline)
                .foregroundStyle(Palette.text)
                .multilineTextAlignment(.center)
            TextField("000 000", text: Binding(
                get: { spacedDigits(confirmDigits) },
                // Pasted text works too: only its digits count, at most six.
                set: { confirmDigits = String(AccessConfirmation.digits(in: $0).prefix(6)) }
            ))
            .keyboardType(.numberPad)
            .font(.system(size: 40, weight: .semibold, design: .monospaced))
            .foregroundStyle(Palette.text)
            .multilineTextAlignment(.center)
            // Digits read left to right in every language, Arabic included.
            .environment(\.layoutDirection, .leftToRight)
            .accessibilityLabel(AccessConfirmText.text(.field))
            .disabled(confirming)
            if let confirmMessage {
                Text(confirmMessage)
                    .font(.footnote)
                    .foregroundStyle(Palette.danger)
                    .multilineTextAlignment(.center)
            }
            Button(AccessConfirmText.text(.confirm)) { Task { await confirm() } }
                .buttonStyle(.primary)
                .disabled(confirmDigits.count != 6 || confirming)
            Text(AccessConfirmText.text(.oldMac))
                .font(.footnote)
                .foregroundStyle(Palette.secondary)
                .multilineTextAlignment(.center)
            Button(AccessConfirmText.text(.startOver)) { startOver() }
                .buttonStyle(.secondary)
        }
    }

    private func confirm() async {
        confirming = true
        let result = await accounts.confirmAccess(computer.computerId, code: confirmDigits)
        confirming = false
        switch result {
        case .incomplete:
            break
        case let .wrong(attemptsLeft):
            confirmDigits = ""
            confirmMessage = AccessConfirmText.text(.wrong, attemptsLeft: attemptsLeft)
        case .saved, .tooManyAttempts, .ended:
            // Saved shows the approval; the others end the request, and its reason shows as the outcome.
            confirmDigits = ""
            confirmMessage = nil
        }
    }

    /// Withdraws this request and asks again right away, with a new code.
    private func startOver() {
        confirmDigits = ""
        confirmMessage = nil
        outcome = nil
        sawTicket = false
        Task { await accounts.cancelAccess(computer.computerId) }
    }

    /// "123 456" while typing: the first three digits, a space, the rest.
    private func spacedDigits(_ digits: String) -> String {
        guard digits.count > 3 else { return digits }
        return "\(digits.prefix(3)) \(digits.dropFirst(3))"
    }

    private func spaced(_ code: String) -> String {
        guard code.count == 6 else { return code }
        return "\(code.prefix(3)) \(code.suffix(3))"
    }

    /// Runs until the computer answered with its nonce; the code then shows through `pendingAccess`.
    /// Closing the sheet leaves the request, and this wait, running to its end with the result
    /// ignored; the code then shows in the banner.
    private func request() async {
        do {
            try await accounts.requestAccess(computer.computerId)
        } catch is CancellationError {
            return
        } catch {
            guard !Task.isCancelled else { return }
            // A 409 (approved earlier, never finished here) reads in the person's language.
            self.error = AccessConfirmText.requestErrorText(error)
        }
    }
}
