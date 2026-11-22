import SwiftUI

/// O que o menu do agente faz.
struct MXAcoesDoMenu {
    var emBreve: (MXEmBreve) -> Void
    /// Ajustes do agente e Autonomia: a folha de ajustes (08).
    var ajustes: () -> Void
    var novaSessao: () -> Void
    var alternarFixar: () -> Void
}

/// Menu de vidro ancorado na cápsula do agente (`capturas/05b-menu-do-agente.webp`): função e estado, Rotinas,
/// Ajustes do agente, Autonomia, Nova sessão, Fixar no topo. Ajustes e Autonomia abrem a folha de ajustes (08);
/// Nova sessão cria e abre quando a fonte sabe; Fixar no topo vale já. O que a fonte não tem (Rotinas, e Nova sessão
/// na real) fica à vista e desligado, com "Em breve" (`MXMenuRegras`).
struct MXMenuDoAgente: View {
    let dados: MXConversaDados
    /// A ligação: sem autoridade, Fixar/Desafixar fica desligado.
    let conexao: MXEstadoConexao
    /// O que a fonte tem de verdade.
    let capacidades: MXCapacidades
    let acoes: MXAcoesDoMenu

    init(dados: MXConversaDados, conexao: MXEstadoConexao, capacidades: MXCapacidades, acoes: MXAcoesDoMenu) {
        self.dados = dados
        self.conexao = conexao
        self.capacidades = capacidades
        self.acoes = acoes
    }

    private var c: MX.Conversa.Type { MX.Conversa.self }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            cabecalho
            ForEach(MXMenuRegras.itens(dados, conexao: conexao, capacidades: capacidades)) { i in
                if i.divisoriaAntes {
                    Rectangle()
                        .fill(MX.Claro.linha.color)
                        .frame(height: MX.Medida.divisoria)
                        .padding(c.menuDivisoriaRespiro)
                        .accessibilityHidden(true)
                }
                item(i)
            }
        }
        .padding(c.menuRespiro)
        .mxVidro(.menu, em: RoundedRectangle(cornerRadius: c.menuRaio))
        // O menu segura o toque: tocar num item desligado (ou entre os itens) não passa para o véu, que fecharia o menu.
        .contentShape(RoundedRectangle(cornerRadius: c.menuRaio))
        .onTapGesture {}
        .shadow(color: MX.CorDaConversa.sombraDoMenu.color, radius: c.menuSombraRaio, y: c.menuSombraY)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "Opções de \(dados.nome)", bundle: .module))
        .accessibilityAddTraits(.isModal)
    }

    private var cabecalho: some View {
        HStack(spacing: 0) {
            if !dados.funcao.isEmpty {
                Text("\(dados.funcao) · ").mxTipo(MX.TipoDaConversa.menuCabecalho).foregroundStyle(MX.Claro.textoSecundario.color)
            }
            Text(MXEstadoRegras.texto(dados.estado))
                .mxTipo(MX.TipoDaConversa.menuCabecalhoForte)
                .foregroundStyle(MXEstadoRegras.cor(dados.estado).color)
        }
        .lineLimit(2)
        .padding(c.menuCabecalhoRespiro)
        .accessibilityElement(children: .combine)
    }

    private func icone(_ controle: MXControle) -> MXIcone {
        switch controle {
        case .rotinas: .rotinas
        case .ajustesDoAgente: .ajustes
        case .autonomia: .escudo
        case .novaSessao: .mais
        default: .alfinete
        }
    }

    private func acao(_ controle: MXControle) -> () -> Void {
        switch controle {
        case .rotinas: { acoes.emBreve(.rotinas) }
        case .ajustesDoAgente, .autonomia: acoes.ajustes
        case .novaSessao: acoes.novaSessao
        default: acoes.alternarFixar
        }
    }

    private func item(_ i: MXItemDoMenu) -> some View {
        Button(action: acao(i.controle)) {
            HStack(spacing: c.menuItemEntre) {
                MXIconeView(icone(i.controle), tamanho: c.menuItemIcone, traco: c.iconeTraco)
                Text(i.titulo).mxTipo(MX.TipoDaConversa.menuItem)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let valor = i.valor {
                    Text(valor).mxTipo(MX.TipoDaConversa.menuValor).foregroundStyle(MX.Claro.textoSecundario.color)
                }
            }
            .foregroundStyle(MX.Claro.texto.color)
            .padding(.horizontal, c.menuItemRespiro)
            .frame(minHeight: c.menuItemAltura)
            .contentShape(RoundedRectangle(cornerRadius: c.menuItemRaio))
        }
        .buttonStyle(MXToque())
        .disabled(!i.ligado)
        .opacity(i.ligado ? 1 : c.desligadoOpacidade)
        .accessibilityLabel(i.rotuloAcessivel)
        .accessibilityValue(i.valorAcessivel ?? "")
        .accessibilityHint(i.dicaAcessivel ?? "")
    }
}
