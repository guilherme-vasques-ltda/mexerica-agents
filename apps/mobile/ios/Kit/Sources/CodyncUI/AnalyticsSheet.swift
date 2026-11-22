// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import CodyncKit
import SwiftUI

public struct AnalyticsSheet: View {
    public init() {}
    public var body: some View {
        VStack(spacing: 0) {
            ModalHeader(String(localized: "Análise de uso", bundle: .module))
            CardForm {
                CardSection { Text(String(localized: "A análise de uso e o envio de falhas estão desativados na Mexerica.", bundle: .module)) }
            }
        }
        .background(Palette.background)
    }
}
