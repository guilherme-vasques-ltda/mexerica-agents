#if DEBUG
import CodyncKit
import CodyncUI
import SwiftUI

/// A ponte entre os Ajustes do redesenho (só Debug, atrás do interruptor "Novo visual (em teste)") e o que só o app tem:
/// sessão de login, pareamento, pedido de acesso, uso e os ajustes de hoje. Em Release este arquivo não existe.
extension AccountSession: MXSessaoDaConta {}

@MainActor
enum MXPonteApp {
    static func criar(app: AppStore) -> MXPonteDoApp {
        MXPonteDoApp(
            sessao: app.account,
            // A conta de hoje (trocar de conta, entrar): é a folha que o app de hoje já abre pelo seletor de contas.
            abrirConta: { app.account.showSwitcher = true },
            sair: { await app.signOut() },
            vista: { destino, computador in vista(app, destino, computador) },
            permissaoDeAvisos: { await PushRegistrar.shared.isAllowed() },
            pedirPermissaoDeAvisos: { await PushRegistrar.shared.requestAuthorization(from: .settings) },
            avisosMudaram: { ligados in
                PushRegistrar.shared.resync()
                Analytics.shared.capture(.settingChanged(.push, on: ligados))
            })
    }

    /// A tela de hoje para cada destino, com os mesmos objetos de ambiente que o app dá a ela (a folha da ponte não herda
    /// o ambiente da raiz: o redesenho cobre o app como uma camada à parte).
    private static func vista(_ app: AppStore, _ destino: MXDestinoDaConta, _ computador: String?) -> AnyView {
        let store = computador.flatMap { app.accounts.store(for: $0) }
        switch destino {
        case .conectar:
            return comAmbiente(app, PairingView(inModal: true))
        case let .acesso(id):
            guard let alvo = app.accounts.cloudComputers.first(where: { $0.computerId == id }) else { return AnyView(EmptyView()) }
            return comAmbiente(app, AccessRequestView(computer: alvo, pending: app.accounts.pendingAccess[id] != nil))
        case .voz:
            guard let store else { return AnyView(EmptyView()) }
            return comAmbiente(app, VoiceChatSettingsView().environment(store))
        case .uso:
            guard let store else { return AnyView(EmptyView()) }
            return comAmbiente(app, VStack(spacing: 0) {
                ModalHeader(String(localized: "Uso"))
                UsageView().environment(store).id(store.computer.id)
            }.background(Palette.background))
        case .maisAjustes:
            // Os ajustes de hoje inteiros (bots ocultos, remover computador, cor, rota, recomeçar), como o app abre
            // "Computadores e ajustes".
            return comAmbiente(app, NavigationStack { SettingsView() })
        }
    }

    private static func comAmbiente(_ app: AppStore, _ vista: some View) -> AnyView {
        AnyView(vista
            .environment(app)
            .environment(app.accounts)
            .environment(app.account)
            .tint(Palette.accent))
    }
}
#endif
