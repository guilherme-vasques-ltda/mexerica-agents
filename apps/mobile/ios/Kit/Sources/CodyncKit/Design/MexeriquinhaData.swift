// Dados do contrato Mexeriquinhas v2, gerados de apps/desktop/src/shared/fork-mexeriquinha-data.json
// Veja apps/desktop/src/renderer/lib/mexeriquinhas/. Mantenha em sincronia com o desktop e com o Hub.
// Os 119 modelos entram so como ids (`modelos`): eles escolhem aparencia e nao mudam o desenho.
import Foundation

enum MexeriquinhaData {
    /// Os ids de `modeloId` que o schema do desktop aceita (o `enum` de `modeloId`). Só rotulam: não mudam o desenho,
    /// mas um id fora daqui invalida a `mexeriquinha` inteira, como no desktop.
    static let modelos: Set<String> = [
        "berga", "pomarca", "colheitor", "tesourango", "holofruta", "chiperica",
        "futurango", "maracuja-lunar", "jabuticlausula", "leiranja", "licitacai", "feirante",
        "datamelao", "planilhaca", "sabioliva", "pitchaya", "feirometro", "espiaranja",
        "lancafruta", "feirinha", "amazoja", "mercaju", "shopequi", "baiacu",
        "estocado", "caquifrete", "lichia-boy", "etiquetamara", "carambolinha", "trocaja",
        "vendoca", "prospecaqui", "alogoiaba", "gentileza", "zapim", "discoco",
        "cifrao-azedo", "contamora", "livrinho", "lupima", "notinha", "leaoranja",
        "carimbacaxi", "cartangerina", "boletinho", "pixtache", "gentarina", "cacapera",
        "tradoiaba", "bolsaranja", "macahattan", "gritamelo", "anuncia", "metuva",
        "googaba", "polpinha", "instapera", "tiktomate", "tubacate", "xuxu",
        "famora", "comissao", "fofoquiwi", "fucaranja", "pitanguel", "palavrango",
        "humanana", "rankiwi", "cortelao", "roteiruva", "papaia", "figuinho",
        "imaginanas", "clicacau", "brandamora", "pincelima", "figmigo", "liquidifruta",
        "retoquinho", "molduraca", "codaba", "bugerica", "limao-sombrio", "cachopo",
        "automango", "sitiozinho", "guardacoco", "jamelao", "dutoranja", "cajuca",
        "pomita", "brotinho", "agendrica", "rabisco", "pautinha", "prontopera",
        "mangueira", "linguim", "bananafit", "saladinha", "mestre-jaca", "carreirola",
        "gavetinha", "cofrinha", "cripitanga", "provinha", "lancheira", "chacarinha",
        "doutora-acerola", "cardapinha", "togaca", "pomarzao", "avulsa", "telegoiaba",
        "gravatanga", "xerifruta", "quermesse", "calmaca", "abracai",
    ]

    static let cores: [String: String] = [
        "laranja": "#f28a1e",
        "lima": "#94c630",
        "limao": "#f2c41e",
        "toranja": "#ec5a5e",
        "roxo": "#8d56d8",
        "celeste": "#2f9cf0",
    ]

    static let padrao = MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "nenhuma", acessorio: "nenhum", objeto: "nenhum")

    static let olhosLegados: [String: String] = ["bar": "sonolentos", "round": "brilhantes", "cat": "piscando"]
    static let acessoriosLegados: [String: String] = ["cap": "bone", "helmet": "capacete", "phones": "fone", "headset": "fone", "beret": "boina", "bucket": "bone", "beanie": "capuz", "flower": "flor"]
    /// badge legado: (roupa, objeto)
    static let badgesLegados: [String: (roupa: String, objeto: String)] = [
        "code": ("moletom", "notebook"),
        "bug": ("moletom", "ferramenta"),
        "search": ("jaleco", "lupa"),
        "coin": ("colete", "moeda"),
        "calc": ("colete", "moeda"),
        "ledger": ("colete", "moeda"),
        "palette": ("avental", "pincel"),
        "pen": ("avental", "pincel"),
        "box": ("polo", "caixa"),
        "warehouse": ("polo", "caixa"),
        "cart": ("avental", "carrinho"),
        "bag": ("blazer", "carrinho"),
        "people": ("polo", "coracao"),
        "heart": ("polo", "coracao"),
        "lifebuoy": ("blazer", "ferramenta"),
        "megaphone": ("colete", "megafone"),
        "briefcase": ("blazer", "gravata"),
    ]

    /// Conjuntos prontos do catalogo (o enum de conjuntoId/fallbackConjuntoId) e sua aparencia.
    static let conjuntos: [String: MexeriquinhaAparencia] = [
        "base": MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "nenhuma", acessorio: "nenhum", objeto: "nenhum"),
        "ceo": MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "blazer", acessorio: "coroa", objeto: "gravata"),
        "assistente": MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "jaleco", acessorio: "fone", objeto: "nenhum"),
        "financeiro": MexeriquinhaAparencia(cor: "lima", olhos: "oculos-redondos", roupa: "colete", acessorio: "nenhum", objeto: "moeda"),
        "marketing": MexeriquinhaAparencia(cor: "toranja", olhos: "oculos-escuros", roupa: "moletom", acessorio: "nenhum", objeto: "nenhum"),
        "developer": MexeriquinhaAparencia(cor: "roxo", olhos: "ponto", roupa: "moletom", acessorio: "fone", objeto: "notebook"),
        "pesquisa": MexeriquinhaAparencia(cor: "limao", olhos: "ponto", roupa: "jaleco", acessorio: "nenhum", objeto: "lupa"),
        "vendas": MexeriquinhaAparencia(cor: "celeste", olhos: "piscando", roupa: "blazer", acessorio: "nenhum", objeto: "nenhum"),
        "compras": MexeriquinhaAparencia(cor: "lima", olhos: "ponto", roupa: "avental", acessorio: "bone", objeto: "carrinho"),
        "rh": MexeriquinhaAparencia(cor: "toranja", olhos: "brilhantes", roupa: "polo", acessorio: "flor", objeto: "coracao"),
        "suporte": MexeriquinhaAparencia(cor: "limao", olhos: "ponto", roupa: "blazer", acessorio: "fone", objeto: "ferramenta"),
        "designer": MexeriquinhaAparencia(cor: "roxo", olhos: "ponto", roupa: "avental", acessorio: "boina", objeto: "pincel"),
        "amazon": MexeriquinhaAparencia(cor: "celeste", olhos: "ponto", roupa: "polo", acessorio: "capacete", objeto: "caixa"),
        "dono": MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "blazer", acessorio: "cartola", objeto: "chave"),
        "chefe": MexeriquinhaAparencia(cor: "toranja", olhos: "ponto", roupa: "colete", acessorio: "fone", objeto: "megafone"),
        "hacker": MexeriquinhaAparencia(cor: "laranja", olhos: "oculos-verdes", roupa: "moletom", acessorio: "capuz", objeto: "nenhum"),
        "berga": MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "blazer", acessorio: "cartola", objeto: "chave"),
        "pomarca": MexeriquinhaAparencia(cor: "laranja", olhos: "ponto", roupa: "blazer", acessorio: "coroa", objeto: "gravata"),
        "colheitor": MexeriquinhaAparencia(cor: "lima", olhos: "ponto", roupa: "avental", acessorio: "capacete", objeto: "caixa"),
        "tesourango": MexeriquinhaAparencia(cor: "limao", olhos: "oculos-redondos", roupa: "colete", acessorio: "nenhum", objeto: "moeda"),
        "holofruta": MexeriquinhaAparencia(cor: "toranja", olhos: "oculos-escuros", roupa: "moletom", acessorio: "nenhum", objeto: "nenhum"),
        "chiperica": MexeriquinhaAparencia(cor: "celeste", olhos: "ponto", roupa: "moletom", acessorio: "fone", objeto: "notebook"),
    ]
}
