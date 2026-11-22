import Foundation

// O avatar de um bot, igual ao do app de desktop.
//
// O Hub manda dois campos no JSON de cada bot:
//   - `avatarConfig`: o avatar legado (matriz de pontos), sempre presente num Hub atual;
//   - `mexeriquinha`: o avatar da Mexeriquinha (v2 simples, v2 3d, v3 3d), quando existe.
// O desktop decide em `apps/desktop/src/shared/fork-mexeriquinha-config.ts` (`avatarDoBot` e
// `normalizarAvatarDoBot`) e este arquivo repete a mesma regra. `avatarColor` e `avatarShape`
// continuam existindo só como aproximação para clientes antigos e não entram na decisão.

/// Chave qualquer, para contar as chaves que o JSON traz (o desktop recusa chaves a mais: `additionalProperties: false`).
private struct QualquerChave: CodingKey {
    var stringValue: String
    var intValue: Int? { nil }
    init?(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { return nil }
}

private extension KeyedDecodingContainer {
    /// Como `decodeIfPresent`, mas `null` falha (no desktop `null` não é um valor aceito) em vez de virar ausente.
    func estrito<T: Decodable>(_ type: T.Type, _ key: Key) throws -> T? {
        contains(key) ? try decode(type, forKey: key) : nil
    }

    /// O JSON não pode ter chave fora das de `Key` (as que o decodificador sintetizado jogaria fora).
    func exigirSemChavesExtras(_ decoder: Decoder) throws {
        let todas = try decoder.container(keyedBy: QualquerChave.self)
        if todas.allKeys.count != allKeys.count {
            throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "chaves desconhecidas"))
        }
    }
}

/// Aparência "Simples": cinco escolhas por id (as mesmas do desktop, em português).
public struct MexeriquinhaAparencia: Codable, Hashable, Sendable {
    public var cor: String
    public var olhos: String
    public var roupa: String
    public var acessorio: String
    public var objeto: String

    public init(cor: String, olhos: String, roupa: String, acessorio: String, objeto: String) {
        self.cor = cor; self.olhos = olhos; self.roupa = roupa; self.acessorio = acessorio; self.objeto = objeto
    }

    /// Estrito como o schema do desktop: as cinco chaves, nenhuma a mais.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        try c.exigirSemChavesExtras(decoder)
        cor = try c.decode(String.self, forKey: .cor)
        olhos = try c.decode(String.self, forKey: .olhos)
        roupa = try c.decode(String.self, forKey: .roupa)
        acessorio = try c.decode(String.self, forKey: .acessorio)
        objeto = try c.decode(String.self, forKey: .objeto)
    }

    static let cores: Set<String> = ["laranja", "lima", "limao", "toranja", "roxo", "celeste"]
    static let olhosIds: Set<String> = ["ponto", "sonolentos", "brilhantes", "oculos-redondos", "oculos-escuros", "piscando", "oculos-verdes"]
    static let roupas: Set<String> = ["nenhuma", "blazer", "colete", "moletom", "avental", "jaleco", "polo"]
    static let acessorios: Set<String> = ["nenhum", "coroa", "fone", "capacete", "boina", "bone", "flor", "cartola", "capuz"]
    static let objetos: Set<String> = ["nenhum", "gravata", "moeda", "chave", "megafone", "notebook", "lupa", "pincel", "caixa", "carrinho", "coracao", "ferramenta"]

    var valida: Bool {
        Self.cores.contains(cor) && Self.olhosIds.contains(olhos) && Self.roupas.contains(roupa)
            && Self.acessorios.contains(acessorio) && Self.objetos.contains(objeto)
    }
}

/// O campo `mexeriquinha` do bot, como o Hub o manda. Os campos que não existem numa versão ficam nulos.
public struct MexeriquinhaConfig: Codable, Hashable, Sendable {
    public var v: Int
    /// `simples` ou `3d`.
    public var estilo: String
    public var modeloId: String?
    public var aparencia: MexeriquinhaAparencia?
    /// v2 3d: um conjunto do catálogo fixo.
    public var conjuntoId: String?
    /// v3 3d: conjunto gerado pelo Hub. O iPhone não tem como baixar os quadros dele (ver `BotAvatar.pelucia`).
    public var assetId: String?
    public var fallbackConjuntoId: String?

    public init(v: Int, estilo: String, modeloId: String? = nil, aparencia: MexeriquinhaAparencia? = nil,
                conjuntoId: String? = nil, assetId: String? = nil, fallbackConjuntoId: String? = nil) {
        self.v = v; self.estilo = estilo; self.modeloId = modeloId; self.aparencia = aparencia
        self.conjuntoId = conjuntoId; self.assetId = assetId; self.fallbackConjuntoId = fallbackConjuntoId
    }

    /// Estrito como o schema do desktop: chave desconhecida ou `null` falha (o roster trata a falha como "sem avatar").
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        try c.exigirSemChavesExtras(decoder)
        v = try c.decode(Int.self, forKey: .v)
        estilo = try c.decode(String.self, forKey: .estilo)
        modeloId = try c.estrito(String.self, .modeloId)
        aparencia = try c.estrito(MexeriquinhaAparencia.self, .aparencia)
        conjuntoId = try c.estrito(String.self, .conjuntoId)
        assetId = try c.estrito(String.self, .assetId)
        fallbackConjuntoId = try c.estrito(String.self, .fallbackConjuntoId)
    }

    /// uuid v4 canônico em minúsculas: o único formato aceito para conjuntos gerados.
    static func assetIdValido(_ id: String) -> Bool {
        guard id.utf8.count == 36 else { return false }  // antes de qualquer varredura: o id vem de fora e pode ser enorme
        let parts = id.split(separator: "-", omittingEmptySubsequences: false)
        guard parts.map(\.count) == [8, 4, 4, 4, 12] else { return false }
        let hex = Set("0123456789abcdef")
        guard id.allSatisfy({ $0 == "-" || hex.contains($0) }) else { return false }
        let version = parts[2].first, variant = parts[3].first
        return version == "4" && variant.map { "89ab".contains($0) } == true
    }

    /// O teto do desktop para a `mexeriquinha` serializada (`validMexeriquinha`).
    static let maxBytes = 1024

    /// Confere o que o desktop confere em `validMexeriquinha`: o schema (incluindo o `modeloId` do catálogo) e o
    /// limite de 1024 bytes. Chaves desconhecidas já falharam na decodificação (`init(from:)`).
    /// A forma vem primeiro: ela só aceita ids de catálogo (curtos), então o JSON só é montado para o que já é pequeno.
    public var isValid: Bool {
        guard formaValida, let json = try? JSONEncoder().encode(self) else { return false }
        return json.count <= Self.maxBytes
    }

    /// Esta configuração se já valer (e couber no teto), senão nil: é o que o `Bot` guarda e leva ao snapshot.
    var ifValid: MexeriquinhaConfig? { isValid ? self : nil }

    private var formaValida: Bool {
        let modeloValido = modeloId.map { MexeriquinhaData.modelos.contains($0) } ?? true
        switch (v, estilo) {
        case (2, "simples"):
            return modeloValido && aparencia?.valida == true && conjuntoId == nil && assetId == nil && fallbackConjuntoId == nil
        case (2, "3d"):
            return modeloValido && conjuntoId.map { MexeriquinhaData.conjuntos[$0] != nil } == true && aparencia == nil
                && assetId == nil && fallbackConjuntoId == nil
        case (3, "3d"):
            return assetId.map(Self.assetIdValido) == true
                && fallbackConjuntoId.map { MexeriquinhaData.conjuntos[$0] != nil } == true
                && aparencia == nil && conjuntoId == nil && modeloId == nil
        default:
            return false
        }
    }

    /// O conjunto do catálogo que desenha este avatar 3D (no v3, o de reserva).
    var conjuntoVisivel: String? {
        guard estilo == "3d" else { return nil }
        return v == 3 ? fallbackConjuntoId : conjuntoId
    }
}

/// O campo `avatarConfig` (avatar legado de pontos). Só serve de reserva e de prova de consistência.
public struct LegacyAvatarConfig: Codable, Hashable, Sendable {
    public var shape: String
    public var color: String
    public var eye: String
    public var head: String
    public var face: String
    public var ears: String
    public var chin: String
    public var neck: String
    public var cheeks: String
    public var badge: String
    public var badgeStyle: String

    static let padrao = LegacyAvatarConfig(shape: "cubo", color: "#3B82F6", eye: "auto", head: "none", face: "none", ears: "none",
                                           chin: "none", neck: "none", cheeks: "none", badge: "none", badgeStyle: "light")

    private static let shapes: Set<String> = ["oval", "pilula", "cubo", "gota", "ciclope", "tela", "urso", "cupula", "losango", "nuvem", "hexa", "gato", "totem"]
    private static let eyes: Set<String> = ["auto", "pill", "round", "pixel", "bar", "cat", "cyc"]
    private static let heads: Set<String> = ["none", "cap", "helmet", "phones", "headset", "beret", "eyeshade", "bucket", "beanie", "flower", "pencil"]
    private static let faces: Set<String> = ["none", "glasses", "shades"]
    private static let earsIds: Set<String> = ["none", "hoops", "pearls"]
    private static let chins: Set<String> = ["none", "beard", "stache"]
    private static let necks: Set<String> = ["none", "bowtie", "tie", "scarf", "lanyard", "necklace"]
    private static let cheeksIds: Set<String> = ["none", "blush"]
    private static let badges: Set<String> = [
        "none", "sparkle", "clipboard", "briefcase", "kanban", "calendar", "notes", "search", "globe", "translate", "code", "bug",
        "bag", "box", "ship", "cart", "warehouse", "lifebuoy", "megaphone", "target", "chat", "pen", "seo", "video", "palette",
        "chart", "phone", "coin", "calc", "ledger", "people", "heart", "mail", "bolt", "rocket", "bulb", "mic",
    ]
    private static let badgeStyles: Set<String> = ["light", "dark", "color"]

    /// Os onze campos existem e valem o que o schema `avatar-config.schema.json` permite.
    public var isValid: Bool {
        Self.shapes.contains(shape) && Self.hexa(color) != nil && Self.eyes.contains(eye) && Self.heads.contains(head)
            && Self.faces.contains(face) && Self.earsIds.contains(ears) && Self.chins.contains(chin) && Self.necks.contains(neck)
            && Self.cheeksIds.contains(cheeks) && Self.badges.contains(badge) && Self.badgeStyles.contains(badgeStyle)
    }

    /// Esta configuração se for válida, senão nil: é o que o `Bot` guarda e leva ao snapshot (valida ⇒ só ids curtos).
    var ifValid: LegacyAvatarConfig? { isValid ? self : nil }

    /// `#RRGGBB` como três componentes de 0 a 255; nulo se o texto não tiver exatamente esse formato.
    static func hexa(_ text: String) -> [Int]? {
        guard text.utf8.count == 7 else { return nil }  // `#RRGGBB` é ASCII: texto de outro tamanho nem é varrido
        let chars = Array(text)
        guard chars.count == 7, chars[0] == "#", chars.dropFirst().allSatisfy({ $0.isASCII && $0.isHexDigit }) else { return nil }
        // Só dígitos ASCII chegam aqui: `Int(_, radix:)` aceitaria sinais ("+f", "-1"), que o regex do desktop recusa.
        return stride(from: 1, to: 7, by: 2).compactMap { Int(String(chars[$0 ... $0 + 1]), radix: 16) }
    }
}

extension LegacyAvatarConfig {
    /// Estrito como `validAvatar` do desktop: as onze chaves e nenhuma a mais (numa extensão, para manter o `init` de membros).
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        try c.exigirSemChavesExtras(decoder)
        self.init(shape: try c.decode(String.self, forKey: .shape), color: try c.decode(String.self, forKey: .color),
                  eye: try c.decode(String.self, forKey: .eye), head: try c.decode(String.self, forKey: .head),
                  face: try c.decode(String.self, forKey: .face), ears: try c.decode(String.self, forKey: .ears),
                  chin: try c.decode(String.self, forKey: .chin), neck: try c.decode(String.self, forKey: .neck),
                  cheeks: try c.decode(String.self, forKey: .cheeks), badge: try c.decode(String.self, forKey: .badge),
                  badgeStyle: try c.decode(String.self, forKey: .badgeStyle))
    }
}

/// O que o iPhone desenha para um bot.
public enum BotAvatar: Hashable, Sendable {
    /// A Mexeriquinha desenhada em vetor (o estilo "Simples" do desktop; também o mascote padrão).
    case simples(MexeriquinhaAparencia)
    /// A Mexeriquinha de feltro: quadros prontos de um conjunto do catálogo. `assetId` é o id de um conjunto
    /// gerado pelo Hub (v3): o iPhone ainda não recebe os quadros dele e mostra o conjunto de reserva.
    case pelucia(conjunto: String, assetId: String?)

    /// O mascote padrão: a Mexeriquinha laranja, sem roupa nem acessório (`padrao` do contrato v2).
    public static let padrao = BotAvatar.simples(MexeriquinhaData.padrao)

    /// A regra do desktop (`avatarDoBot` + `normalizarAvatarDoBot`).
    /// 1. `mexeriquinha` válida vale, se não houver `avatarConfig` válido ou se ele for a projeção dela.
    /// 2. Senão, vale o `avatarConfig` válido, aproximado para a Mexeriquinha Simples.
    /// 3. Senão, o mascote padrão. (O desktop tenta antes adivinhar uma função pelo nome do bot; essa tabela
    ///    de 30 funções não foi portada. Um Hub atual sempre manda `avatarConfig`, então não chega aqui.)
    public static func resolve(avatarConfig: LegacyAvatarConfig?, mexeriquinha: MexeriquinhaConfig?) -> BotAvatar {
        let legado = avatarConfig.flatMap { $0.isValid ? $0 : nil }
        if let mx = mexeriquinha, mx.isValid, legado == nil || legado == projetar(mx) {
            return from(mx)
        }
        if let legado { return .simples(aproximar(legado)) }
        return .padrao
    }

    static func from(_ mx: MexeriquinhaConfig) -> BotAvatar {
        if mx.estilo == "3d", let conjunto = mx.conjuntoVisivel {
            return .pelucia(conjunto: conjunto, assetId: mx.v == 3 ? mx.assetId : nil)
        }
        return .simples(mx.aparencia ?? MexeriquinhaData.padrao)
    }

    /// `projetarAvatarLegado`: o que o Hub guarda em `avatarConfig` para uma `mexeriquinha`.
    static func projetar(_ mx: MexeriquinhaConfig) -> LegacyAvatarConfig? {
        let aparencia = mx.estilo == "simples" ? mx.aparencia : mx.conjuntoVisivel.flatMap { MexeriquinhaData.conjuntos[$0] }
        guard let aparencia, let cor = MexeriquinhaData.cores[aparencia.cor] else { return nil }
        var old = LegacyAvatarConfig.padrao
        old.shape = "oval"
        old.color = cor
        old.eye = switch aparencia.olhos {
        case "sonolentos": "bar"
        case "brilhantes": "round"
        case "piscando": "cat"
        default: "auto"
        }
        old.face = switch aparencia.olhos {
        case "oculos-redondos": "glasses"
        case "oculos-escuros", "oculos-verdes": "shades"
        default: "none"
        }
        return old
    }

    /// A cor da paleta mais perto, com o mesmo desempate do desktop e do Hub (menor id em ordem de bytes).
    static func corProxima(_ hex: String) -> String {
        guard let alvo = LegacyAvatarConfig.hexa(hex) else { return MexeriquinhaData.padrao.cor }
        var melhor: (id: String, distancia: Int)?
        for (id, cor) in MexeriquinhaData.cores {
            guard let rgb = LegacyAvatarConfig.hexa(cor) else { continue }
            let distancia = zip(rgb, alvo).reduce(0) { $0 + ($1.0 - $1.1) * ($1.0 - $1.1) }
            if let atual = melhor, (atual.distancia, atual.id) <= (distancia, id) { continue }
            melhor = (id, distancia)
        }
        return melhor?.id ?? MexeriquinhaData.padrao.cor
    }

    /// `normalizarAvatarDoBot` para um avatar legado: aproxima, sem inventar instruções nem função.
    static func aproximar(_ old: LegacyAvatarConfig) -> MexeriquinhaAparencia {
        var a = MexeriquinhaData.padrao
        a.cor = corProxima(old.color)
        a.olhos = old.face == "glasses" ? "oculos-redondos" : old.face == "shades" ? "oculos-escuros"
            : MexeriquinhaData.olhosLegados[old.eye] ?? "ponto"
        a.acessorio = MexeriquinhaData.acessoriosLegados[old.head] ?? "nenhum"
        if old.neck == "tie" || old.neck == "bowtie" { a.roupa = "blazer"; a.objeto = "gravata" }
        if let badge = MexeriquinhaData.badgesLegados[old.badge] { a.roupa = badge.roupa; a.objeto = badge.objeto }
        return a
    }
}

/// Os cinco estados do avatar, iguais para todos os bots (`estado.ts` do desktop).
public enum MexeriquinhaState: String, Hashable, Sendable {
    case dormindo, livre, trabalhando, precisaResposta = "precisa-resposta", erro

    /// Parado há mais de 8 horas, estritamente, o bot dorme.
    static let inatividadeMs: Int64 = 8 * 60 * 60 * 1000

    /// A precedência do desktop (`resolveState`): erro, precisa de resposta, trabalhando, dormindo, livre.
    /// `unavailable` não tem estado próprio no iPhone e desenha como livre.
    /// `hasPendingQuestions` (o bot tem pergunta pendente ao dono) pesa como `needsInput`, acima de `working`.
    public static func of(status: String, lastAt: Int64, createdAt: Int64, startedAt: Int64?, hasPendingQuestions: Bool = false, now: Date) -> MexeriquinhaState {
        if status == "unavailable" { return .livre }  // o Hub não comprovou o runtime: sem sobras, nem perguntas
        if status == "error" { return .erro }
        if status == "needsInput" || hasPendingQuestions { return .precisaResposta }
        if status == "working" { return .trabalhando }
        let ultima = [lastAt, createdAt, startedAt ?? 0].filter { $0 > 0 }.max()
        if let ultima, Int64(now.timeIntervalSince1970 * 1000) - ultima > inatividadeMs { return .dormindo }
        return .livre
    }

    /// O estado do avatar na fase de uma Live Activity.
    public init(phase: BotActivityPresentation.Phase) {
        switch phase {
        case .working: self = .trabalhando
        case .needsInput: self = .precisaResposta
        case .failed: self = .erro
        case .sending, .queued, .completed, .stale, .waiting: self = .livre
        }
    }

    /// O selo do estado: zzz, ? ou ×.
    var selo: String? {
        switch self {
        case .dormindo: "zzz"
        case .precisaResposta: "?"
        case .erro: "×"
        case .livre, .trabalhando: nil
        }
    }

    /// O primeiro quadro de cada estado na Mexeriquinha de feltro.
    var quadro: String {
        switch self {
        case .livre: "livre-1"
        case .trabalhando: "trabalhando-1"
        case .dormindo: "dormindo-1"
        case .precisaResposta: "precisa-resposta"
        case .erro: "erro"
        }
    }

    /// O rótulo falado para acessibilidade.
    var nome: String {
        switch self {
        case .dormindo: "Dormindo"
        case .livre: "Livre"
        case .trabalhando: "Trabalhando"
        case .precisaResposta: "Precisa de você"
        case .erro: "Erro"
        }
    }
}

public extension Bot {
    /// O avatar que o desktop mostra para este bot.
    var avatar: BotAvatar { BotAvatar.resolve(avatarConfig: avatarConfig, mexeriquinha: mexeriquinha) }

    func avatarState(now: Date = Date()) -> MexeriquinhaState {
        MexeriquinhaState.of(status: status, lastAt: lastAt, createdAt: createdAt, startedAt: startedAt,
                              hasPendingQuestions: hasPendingQuestions, now: now)
    }
}
