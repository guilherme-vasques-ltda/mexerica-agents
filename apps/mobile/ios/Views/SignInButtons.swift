// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import CodyncUI
import SwiftUI

/// Existing-account sign-in; credentials stay only in form memory and the Clerk request.
struct SignInButtons: View {
    @Environment(AccountSession.self) private var account
    @State private var email = ""
    @State private var password = ""
    @State private var code = ""

    var body: some View {
        VStack(spacing: 10) {
            if account.needsEmailCode {
                Text("Confira seu e-mail para autorizar este iPhone.").font(.subheadline)
                TextField("Código recebido por e-mail", text: $code)
                    .keyboardType(.numberPad).textContentType(.oneTimeCode)
                    .textFieldStyle(.roundedBorder)
                Button("Confirmar código") {
                    let value = code
                    code = ""
                    Task { await account.verifyEmailCode(value) }
                }
                .buttonStyle(.primary)
                .disabled(code.isEmpty || account.isBusy)
                Button("Entrar novamente") { account.cancelEmailCode(); code = "" }
                    .disabled(account.isBusy)
            } else {
                TextField("E-mail", text: $email)
                    .keyboardType(.emailAddress).textContentType(.username)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                    .textFieldStyle(.roundedBorder)
                SecureField("Senha", text: $password).textContentType(.password)
                    .textFieldStyle(.roundedBorder)
                Button(account.isBusy ? "Entrando…" : "Entrar com e-mail e senha") {
                    let value = password
                    password = ""
                    Task { await account.signIn(email: email.trimmingCharacters(in: .whitespacesAndNewlines), password: value) }
                }
                .buttonStyle(.primary)
                .disabled(email.isEmpty || password.isEmpty || account.isBusy || !account.isReady)

                if AppEnvironment.supportsAppleSignIn {
                    Button { Task { await account.signIn(provider: .apple) } } label: {
                        ZStack {
                            HStack(spacing: 10) {
                                Image(systemName: "apple.logo").font(.system(size: 20))
                                Text("Continuar com Apple")
                            }
                            .opacity(account.isBusy ? 0 : 1)
                            if account.isBusy { Spinner(size: 18) }
                        }
                        .font(.headline)
                        .frame(maxWidth: .infinity, minHeight: 50)
                    }
                    .buttonStyle(.primary)
                    .disabled(account.isBusy || !account.isReady)
                    .animation(Motion.fade, value: account.isBusy)
                }
                GoogleSignInButton().disabled(!account.isReady)
            }
        }
        .onDisappear { password = ""; code = "" }
        .onChange(of: account.isSignedIn) { _, signedIn in
            if signedIn { password = ""; code = ""; email = "" }
        }
    }
}
