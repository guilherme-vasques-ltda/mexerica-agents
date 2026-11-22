import SwiftUI

/// A navegação da aba Conversas: a lista e o que ela abre (a Conversa, destinos "Em breve").
///
/// Normal: `NavigationStack` do sistema. A tela entra e sai pela direita pelo mesmo caminho, o movimento é
/// interrompível e voltar arrastando da borda é o gesto nativo.
/// Reduzir movimento: nada desliza. As telas trocam com cross-fade curto (`MXTroca`) e arrastar da borda esquerda
/// passando de 90 pt volta (com o mesmo cross-fade, sem a tela acompanhar o dedo).
struct MXConversasNavegacao: View {
    let fonte: any MXFonte
    @Binding var caminho: [MXDestino]
    let cena: MXCena?
    /// A raiz da pilha é o Mission Board (só Debug; em Release a lista de Conversas é sempre a raiz).
    private let comoQuadro: Bool
    /// A folha de ajustes aberta (08), de qual agente.
    @State private var ajustes: MXFolhaDeAjustes?
    /// O destino que a folha pediu (Conversar, Sessões…): abre depois que ela desce.
    @State private var depoisDaFolha: MXDestino?
    @Environment(\.accessibilityReduceMotion) private var reduzirMovimento
    #if DEBUG
    /// Ajustes, conta e computadores (só Debug): a casca entrega a conta; sem ela o botão segue "Em breve".
    @Environment(\.mxConta) private var conta
    @Environment(\.mxEsquemaDoSistema) private var esquemaDoSistema
    #endif

    init(fonte: any MXFonte, caminho: Binding<[MXDestino]>, cena: MXCena?, quadro: Bool = false) {
        self.fonte = fonte
        _caminho = caminho
        self.cena = cena
        comoQuadro = quadro
        _ajustes = State(initialValue: MXNavegacaoRegras.ajustesIniciais(cena).map(MXFolhaDeAjustes.init))
    }

    var body: some View {
        if reduzirMovimento {
            pilhaCruzada
        } else {
            NavigationStack(path: $caminho) {
                raiz.navigationDestination(for: MXDestino.self) { destino($0) }
            }
            // Normal: a folha do sistema (sobe e desce pelo mesmo caminho, interrompível, puxar para baixo fecha).
            .sheet(item: $ajustes, onDismiss: abrirDepoisDaFolha) { folha in
                folhaDeAjustes(folha.id)
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
                    .presentationCornerRadius(MX.Ajustes.raio)
                    .presentationBackground(MX.CorDasSessoes.folha.color)
            }
        }
    }

    /// A raiz da pilha: a lista de Conversas ou, na aba Mission Board (só Debug), o quadro.
    @ViewBuilder private var raiz: some View {
        #if DEBUG
        if comoQuadro, let quadro = fonte as? any MXQuadroFonte {
            MXQuadroTela(fonte: quadro, esquema: esquemaDoSistema,
                         acoes: MXQuadroAcoes(abrir: { abrir(MXQuadroRegras.destinoDoAbrir($0)) }, abrirAjustes: { abrir(.emBreve(.conta)) }),
                         contaLigada: conta != nil)
                .toolbarVisibility(.hidden, for: .navigationBar)
                .toolbarVisibility(.hidden, for: .tabBar)
        } else {
            lista
        }
        #else
        lista
        #endif
    }

    private var lista: some View {
        MXConversasTela(dados: fonte.lista, acoes: MXAcoesDaLista(
            abrir: { abrir(.conversa($0)) },
            // Canal (10) é da etapa 4.
            abrirCanal: { _ in abrir(.emBreve(.canal)) },
            emBreve: { abrir(.emBreve($0)) },
            // Etapa 3: puxar o agente para a esquerda abre as Sessões; segurar 0,55 s abre os ajustes (folha).
            sessoes: { abrir(.sessoes($0)) },
            ajustes: { abrir(.ajustes($0)) }),
            capacidades: capacidades)
            .toolbarVisibility(.hidden, for: .navigationBar)
            .toolbarVisibility(.hidden, for: .tabBar)
    }

    /// O que a fonte tem de verdade, mais os Ajustes da conta quando a casca os tem (só Debug).
    private var capacidades: MXCapacidades {
        #if DEBUG
        var valor = fonte.capacidades
        valor.abreConta = conta != nil
        return valor
        #else
        return fonte.capacidades
        #endif
    }

    @ViewBuilder private func destino(_ d: MXDestino) -> some View {
        Group {
            switch d {
            case let .conversa(id):
                MXConversaTela(fonte: fonte, id: id, cena: cena, rotuloVoltar: rotuloVoltar(de: d), voltar: voltar, irPara: abrir)
            case let .emBreve(e):
                #if DEBUG
                if e == .conta, let conta {
                    MXAjustesDaContaTela(conta: conta, esquema: esquemaDoSistema, voltar: voltar)
                } else {
                    emBreve(e)
                }
                #else
                emBreve(e)
                #endif
            case let .sessoes(id):
                MXSessoesTela(fonte: fonte, id: id, rotuloVoltar: rotuloVoltar(de: d), cena: cena, voltar: voltar, irPara: abrir)
            case let .sessao(agente, sessao):
                // 07b: a conversa da sessão, com voltar para Sessões; a que não abre no iPhone leva a "Em breve".
                if let conversa = fonte.conversaDaSessao(sessao, em: agente) {
                    MXConversaTela(fonte: fonte, id: conversa, cena: cena, rotuloVoltar: rotuloVoltar(de: d), voltar: voltar,
                                   irPara: abrir)
                } else {
                    MXEmBreveTela(titulo: MXEmBreve.sessaoExtra.titulo, texto: MXEmBreve.sessaoExtra.texto ?? "", voltar: voltar)
                }
            case .ajustes:
                // A folha não entra na pilha (`abrir` a apresenta); nunca chega aqui.
                EmptyView()
            }
        }
        .toolbarVisibility(.hidden, for: .tabBar)
    }

    @ViewBuilder private func emBreve(_ e: MXEmBreve) -> some View {
        if let texto = e.texto {
            MXEmBreveTela(titulo: e.titulo, texto: texto, voltar: voltar)
        } else {
            MXEmBreveTela(titulo: e.titulo, voltar: voltar)
        }
    }

    /// O que o voltar desta tela diz: a tela de baixo dela na pilha (o destino real de voltar).
    private func rotuloVoltar(de d: MXDestino) -> String {
        let abaixo = caminho.lastIndex(of: d).flatMap { $0 > 0 ? caminho[$0 - 1] : nil }
        #if DEBUG
        if comoQuadro, abaixo == nil { return MXTextosDoQuadro().titulo }
        #endif
        return MXNavegacaoRegras.rotuloVoltar(abaixo: abaixo) { fonte.conversa($0)?.nome }
    }

    // MARK: Folha de ajustes

    private func folhaDeAjustes(_ id: String) -> some View {
        MXAjustesTela(fonte: fonte, id: id, acoes: MXAcoesDosAjustes(
            fechar: { fecharAjustes(depois: nil) },
            irPara: { destino in
                fecharAjustes(depois: MXNavegacaoRegras.destinoDepoisDosAjustes(destino, topo: caminho.last))
            },
            excluido: {
                fecharAjustes(depois: nil)
                caminho.removeAll()
            }))
    }

    private func fecharAjustes(depois destino: MXDestino?) {
        if reduzirMovimento {
            withAnimation(MX.Movimento.troca(reduzir: true).animacao) { ajustes = nil }
            if let destino { abrir(destino) }
        } else {
            depoisDaFolha = destino
            ajustes = nil
        }
    }

    private func abrirDepoisDaFolha() {
        guard let destino = depoisDaFolha else { return }
        depoisDaFolha = nil
        abrir(destino)
    }

    private func abrir(_ d: MXDestino) {
        if case let .ajustes(id) = d {
            let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
            if troca.interpola {
                ajustes = MXFolhaDeAjustes(id: id)
            } else {
                withAnimation(troca.animacao) { ajustes = MXFolhaDeAjustes(id: id) }
            }
            return
        }
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        if troca.interpola {
            caminho.append(d) // o NavigationStack anima o push do sistema
        } else {
            withAnimation(troca.animacao) { caminho.append(d) }
        }
    }

    private func voltar() {
        guard !caminho.isEmpty else { return }
        let troca = MX.Movimento.troca(reduzir: reduzirMovimento)
        if troca.interpola {
            caminho.removeLast()
        } else {
            withAnimation(troca.animacao) { _ = caminho.removeLast() }
        }
    }

    // MARK: Reduzir movimento

    private var pilhaCruzada: some View {
        ZStack {
            raiz
            if let topo = caminho.last {
                destino(topo)
                    .id(topo)
                    .transition(.opacity)
                    .overlay(alignment: .leading) { bordaDeVoltar }
            }
            // Reduzir movimento: a folha não sobe; aparece por cross-fade sobre o véu, sem deslizar.
            if let folha = ajustes {
                ZStack(alignment: .bottom) {
                    Button { fecharAjustes(depois: nil) } label: {
                        MX.CorDasSessoes.veuDaFolha.color.ignoresSafeArea()
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(String(localized: "Fechar configurações", bundle: .module))
                    folhaDeAjustes(folha.id)
                        .clipShape(UnevenRoundedRectangle(topLeadingRadius: MX.Ajustes.raio, topTrailingRadius: MX.Ajustes.raio))
                        .padding(.top, MX.Ajustes.topo)
                        .ignoresSafeArea(edges: .bottom)
                }
                .transition(.opacity)
                .accessibilityAddTraits(.isModal)
            }
        }
    }

    /// Faixa fina na borda esquerda (fora do botão voltar): arrastar para a direita passando do limiar volta.
    private var bordaDeVoltar: some View {
        Color.clear
            .frame(width: MX.Conversa.topoMargem)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: MX.Espaco.dentroDeLinha).onEnded { valor in
                if MXNavegacaoRegras.voltaArrastando(valor.translation.width) { voltar() }
            })
            .accessibilityHidden(true)
    }
}

public enum MXNavegacaoRegras {
    /// Arrastar da esquerda para a direita volta quando passa de 90 pt (`voltarArrastando` do JSON).
    public static func voltaArrastando(_ deslocamento: CGFloat) -> Bool {
        deslocamento > MX.Movimento.voltarArrastandoLimiar
    }

    /// A barra de abas some dentro de uma tela empurrada de Conversas (a Conversa ocupa a tela, como no desenho); o mesmo vale
    /// para a Conversa que o Mission Board abre (`caminho` é a pilha da aba aberta).
    public static func mostraBarra(aba: MXAba, caminho: [MXDestino]) -> Bool {
        (aba != .conversas && aba != .missionBoard) || caminho.isEmpty
    }

    /// Onde a casca começa numa cena de captura: dentro da Conversa do Byte (05–06), nas Sessões dele (07), numa
    /// sessão aberta (07b) ou na lista com a folha de ajustes (08); sem cena, na lista.
    public static func caminhoInicial(_ cena: MXCena?) -> [MXDestino] {
        switch cena {
        case nil, .ajustesDoAgente: []
        case .conversa, .filaEditando, .menu, .aprovar: [.conversa("byte")]
        case .sessoes: [.sessoes("byte")]
        case .sessaoAberta: [.sessoes("byte"), .sessao(agente: "byte", sessao: "byte-suite")]
        }
    }

    /// O nome do voltar de uma tela empurrada: o da tela que fica embaixo dela (para onde volta). A lista, "Conversas";
    /// as Sessões, "Sessões"; uma sessão aberta, "Sessão"; a conversa de um agente, o nome dele.
    public static func rotuloVoltar(abaixo: MXDestino?, nomeDoAgente: (String) -> String?) -> String {
        switch abaixo {
        case nil, .ajustes: String(localized: "Conversas", bundle: .module)
        case let .conversa(id): nomeDoAgente(id) ?? String(localized: "Conversa", bundle: .module)
        case .sessoes: String(localized: "Sessões", bundle: .module)
        case .sessao: String(localized: "Sessão", bundle: .module)
        case let .emBreve(e): e.titulo
        }
    }

    /// A folha de ajustes aberta de início (cena 08).
    public static func ajustesIniciais(_ cena: MXCena?) -> String? { cena == .ajustesDoAgente ? "byte" : nil }

    /// O que a folha de ajustes abre ao fechar: nada se já é a tela do topo (Conversar dentro da conversa do agente ou
    /// da Principal aberta pelas Sessões, Sessões dentro das sessões dele); senão, o destino. Conversar dentro de uma
    /// sessão extra abre a Principal (é outra conversa).
    public static func destinoDepoisDosAjustes(_ destino: MXDestino, topo: MXDestino?) -> MXDestino? {
        switch (destino, topo) {
        case let (.conversa(a), .conversa(b)) where a == b: nil
        case let (.conversa(a), .sessao(b, s)) where a == b && s == b: nil
        case let (.sessoes(a), .sessoes(b)) where a == b: nil
        default: destino
        }
    }
}

/// A folha de ajustes aberta (para `sheet(item:)`).
struct MXFolhaDeAjustes: Identifiable, Equatable {
    let id: String
}
