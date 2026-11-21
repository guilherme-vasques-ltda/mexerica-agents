#if DEBUG
import SwiftUI

/// Interruptor "Novo visual (teste)" dos Ajustes (só Debug/DevDebug; em Release este arquivo inteiro não existe, e os
/// textos também não: ficam no código abaixo, não nos catálogos, que entram em todos os builds).
/// Desligado por padrão e lembrado entre aberturas (`UserDefaults.standard`). Ligado, o app abre o redesenho com os
/// dados reais, como `-MexericaRedesignReal`, na hora e também nas próximas aberturas; desligado, abre o app de hoje.
/// Os argumentos de lançamento (`-MexericaRedesign`, `-MexericaRedesignReal`) continuam valendo, com ou sem o
/// interruptor; a única coisa que vence os argumentos é "Voltar ao visual antigo" (ver `abre`).
public enum MXNovoVisual {
    /// A chave no `UserDefaults`; é a mesma do `@AppStorage` dos Ajustes e da casca.
    public static let chave = "mexericaNovoVisualTeste"

    /// O que o app mostra por cima do app de hoje.
    public enum Abre: Equatable, Sendable {
        case antigo, exemplo, real
    }

    /// O estado lembrado; sem nada gravado, desligado.
    public static func ligado(_ padroes: UserDefaults = .standard) -> Bool {
        padroes.bool(forKey: chave)
    }

    /// A regra única, por precedência:
    /// 1. `voltouAoAntigo` (o botão "Voltar ao visual antigo" desta abertura) vence tudo, inclusive os argumentos de
    ///    lançamento, até o interruptor ser ligado de novo ou o app ser aberto outra vez;
    /// 2. o interruptor ou `-MexericaRedesignReal` abrem os dados reais;
    /// 3. só `-MexericaRedesign` abre o exemplo (sem login);
    /// 4. sem nenhum dos três, o app de hoje.
    public static func abre(interruptor: Bool, semLogin: Bool, real: Bool, voltouAoAntigo: Bool = false) -> Abre {
        if voltouAoAntigo { return .antigo }
        if real || interruptor { return .real }
        return semLogin ? .exemplo : .antigo
    }
}

/// Os textos do interruptor, só em Debug, em pt-BR e en (qualquer outro idioma cai no pt-BR, como o app).
public enum MXTextosDebug {
    public static func ingles(_ idioma: String? = Bundle.main.preferredLocalizations.first) -> Bool {
        idioma?.lowercased().hasPrefix("en") == true
    }

    public static func interruptor(ingles i: Bool = ingles()) -> String {
        i ? "New look (testing) — may have bugs" : "Novo visual (em teste) — pode ter falhas"
    }

    public static func ajuda(ingles i: Bool = ingles()) -> String {
        i ? "Test builds only. Opens the redesigned app with real data; turn off to go back to today's app."
          : "Só nas versões de teste. Abre o app redesenhado com os dados reais; desligue para voltar ao app de hoje."
    }

    public static func faixa(ingles i: Bool = ingles()) -> String {
        i ? "New look (testing)" : "Novo visual (em teste)"
    }

    public static func voltar(ingles i: Bool = ingles()) -> String {
        i ? "Back to the old look" : "Voltar ao visual antigo"
    }

    public static func semComputador(ingles i: Bool = ingles()) -> String {
        i ? "No computer connected. Go back to the old look, connect a computer, then turn the new look on again."
          : "Nenhum computador conectado. Volte ao visual antigo, conecte um computador e ligue o novo visual de novo."
    }
}

/// Faixa fina no topo do novo visual quando o interruptor está ligado: diz que é teste e é a saída para o app de hoje
/// (o redesenho ainda não tem Ajustes próprios, então sem ela o interruptor não teria como ser desligado).
struct MXFaixaDoNovoVisual: View {
    let voltar: () -> Void

    var body: some View {
        HStack(spacing: MX.Espaco.dentroDeLinha) {
            Text(verbatim: MXTextosDebug.faixa())
                .mxTipo(MX.Tipo.legenda)
                .foregroundStyle(MX.Claro.textoSecundario.color)
            Spacer(minLength: 0)
            Button(action: voltar) {
                Text(verbatim: MXTextosDebug.voltar())
                    .mxTipo(MX.Tipo.legenda)
                    .foregroundStyle(MX.Claro.mexericaTexto.color)
                    .frame(minHeight: MXAlvoDeToque.minimo)
                    .contentShape(Rectangle())
            }
            .buttonStyle(MXToque())
        }
        .padding(.horizontal, MX.Espaco.margemTela)
        .frame(maxWidth: .infinity)
        .background(MX.Claro.fundo.color)
    }
}
#endif
