import CodyncKit
import Foundation
import SwiftUI

// Etapa 2 do redesenho: o que a Conversa (05), a fila (05a), o menu do agente (05b) e o cartão de aprovação (06)
// mostram, e as regras deles. Só dados e funções puras (testadas em `RedesignConversaTests`); as views ficam em
// `MXConversaTela`, `MXCaixaDeMensagem`, `MXMenuDoAgente` e `MXCartaoDeAprovacao`.

// MARK: - Conexão

/// Estado da ligação com o computador, como as telas do redesenho precisam dele.
///
/// Ponto de encaixe da queda curta: a fonte real
/// preenche `.conectado` por enquanto; quando o `BotStore.estadoConexao` da outra frente entrar, a fonte real só
/// traduz aquele valor para este. Sem conexão, enviar e aprovar ficam à vista, desligados, com o motivo.
public enum MXEstadoConexao: Equatable, Sendable {
    case conectado
    /// Queda curta: a tela fica, a faixa aparece, enviar e aprovar desligam.
    case reconectando(desde: Date)
    /// Sem ligação (prazo longo ou falha definitiva).
    case desconectado

    /// Enviar, aprovar e mexer na fila exigem o Hub: só conectado.
    public var podeAgir: Bool { self == .conectado }
}

/// Textos da queda curta, iguais aos do desktop (`conversa.mensagens.queda.*`, pt-BR).
public enum MXTextoDeQueda {
    public static var titulo: String { String(localized: "Sem conexão — reconectando…", bundle: .module) }
    public static var detalhe: String { String(localized: "Suas conversas continuam aqui. Enviar e aprovar voltam quando reconectar; o que você digitar fica guardado.", bundle: .module) }
    public static var semEnvio: String { String(localized: "Sem conexão: o texto fica guardado e você envia quando reconectar.", bundle: .module) }
    public static var semAprovar: String { String(localized: "Sem conexão: permitir e recusar voltam quando reconectar.", bundle: .module) }
    /// O link caiu ou foi trocado entre o toque e a resposta: nada foi aplicado aqui.
    public static var linkMudou: String { String(localized: "Sem conexão — tente de novo.", bundle: .module) }
    /// O pedido mudou entre a tela e a resposta (revisão nova, deixou de estar pendente, opções novas).
    public static var pedidoMudou: String { String(localized: "O pedido mudou. Confira de novo antes de responder.", bundle: .module) }
    /// Permitir bloqueado por tamanho (`MXPedidoDeAprovacao.bloqueadoPorTamanho`).
    public static var grandeDemais: String { String(localized: "Pedido grande demais para conferir no iPhone. Aprove no computador.", bundle: .module) }
    /// Fixar ainda a caminho do Hub: um por vez por agente.
    public static var fixarACaminho: String { String(localized: "Aguarde: a mudança anterior ainda está a caminho.", bundle: .module) }
    /// Outras ações que exigem o Hub (fila, parar, fixar).
    public static var semAcao: String { String(localized: "Sem conexão: tente de novo quando reconectar.", bundle: .module) }
    /// Faixa quando não está tentando reconectar (computador desligado, acesso encerrado).
    public static var tituloDesconectado: String { String(localized: "Sem conexão com o computador", bundle: .module) }

    /// "Tentar agora" na faixa (como `FaixaDeQueda`), e enquanto tenta.
    public static var tentar: String { String(localized: "Tentar agora", bundle: .module) }
    public static var tentando: String { String(localized: "Tentando…", bundle: .module) }

    /// O título da faixa: reconectando, só depois da carência (`faixa`, o `faixaDeQueda` do store); desconectado, sempre.
    public static func tituloDaFaixa(_ conexao: MXEstadoConexao, faixa: Bool) -> String? {
        switch conexao {
        case .conectado: return nil
        case .reconectando: return faixa ? titulo : nil
        case .desconectado: return tituloDesconectado
        }
    }
}

// MARK: - O que a Conversa mostra

/// Um passo do agente (ferramenta): "Leu src/pace.js".
public struct MXPasso: Identifiable, Equatable, Sendable {
    public let id: String
    public let texto: String
    public let concluido: Bool

    public init(id: String, texto: String, concluido: Bool) {
        self.id = id
        self.texto = texto
        self.concluido = concluido
    }
}

/// Os passos recolhidos de uma resposta: "3 passos · Leitura, Edição, Comando · 2 min".
public struct MXPassos: Equatable, Sendable {
    public let resumo: String
    public let itens: [MXPasso]

    public init(resumo: String, itens: [MXPasso]) {
        self.resumo = resumo
        self.itens = itens
    }
}

/// Balão laranja do usuário.
public struct MXMensagemDoUsuario: Equatable, Sendable {
    public let texto: String
    /// Linha pequena embaixo ("Enviando…", "Não enviada"…), quando há.
    public let nota: String?
    /// Não foi enviada: mostra Reenviar.
    public let podeReenviar: Bool

    public init(texto: String, nota: String? = nil, podeReenviar: Bool = false) {
        self.texto = texto
        self.nota = nota
        self.podeReenviar = podeReenviar
    }
}

/// Balão branco do agente.
public struct MXMensagemDoAgente: Equatable, Sendable {
    public let autor: String
    public let hora: String?
    /// Nome e hora em cima (primeira mensagem de uma sequência do agente).
    public let cabecalho: Bool
    public let texto: String
    public let passos: MXPassos?
    /// Identidade do texto para o cache do markdown (id e revisão da entrada): texto inalterado não é reinterpretado.
    public let chave: String

    public init(autor: String, hora: String?, cabecalho: Bool, texto: String, passos: MXPassos? = nil, chave: String = "") {
        self.autor = autor
        self.hora = hora
        self.cabecalho = cabecalho
        self.texto = texto
        self.passos = passos
        self.chave = chave
    }
}

/// O balão de quem está trabalhando agora (com o gomo) ou esperando a decisão no cartão.
public struct MXTrabalhoEmCurso: Equatable, Sendable {
    public let autor: String
    /// "Adicionando a guarda de data inválida".
    public let atividade: String
    public let passos: MXPassos?
    /// Parado no cartão de aprovação acima: "Esperando a sua decisão acima", sem gomo.
    public let esperandoDecisao: Bool
    /// Depois da decisão, no exemplo: "✓ Você permitiu".
    public let decisao: String?

    public init(autor: String, atividade: String, passos: MXPassos? = nil, esperandoDecisao: Bool = false, decisao: String? = nil) {
        self.autor = autor
        self.atividade = atividade
        self.passos = passos
        self.esperandoDecisao = esperandoDecisao
        self.decisao = decisao
    }
}

/// Manter a Conversa no fim quando a caixa muda de altura (a fila que entra, a edição, um aviso).
/// `defaultScrollAnchor(.bottom, for: .sizeChanges)` cobre o conteúdo crescendo, não a margem de baixo: sem isto, a
/// fila que mede depois da primeira passada cobre as últimas mensagens. "Está no fim" só muda quando a rolagem da
/// pessoa para (a geometria no meio de um ajuste de layout ou de uma animação não diz se ela rolou).
enum MXFimDaRolagem {
    /// Distância do fim que ainda conta como "no fim".
    static let tolerancia: CGFloat = 24

    /// Quanto do conteúdo fica abaixo da área visível (descontada a caixa embaixo).
    static func falta(conteudo: CGFloat, deslocamento: CGFloat, janela: CGFloat, margemDeBaixo: CGFloat) -> CGFloat {
        conteudo - (deslocamento + janela - margemDeBaixo)
    }

    static func noFim(falta: CGFloat) -> Bool { falta <= tolerancia }
}

/// Cortes por bytes UTF-8, ANTES de qualquer trabalho por caractere. Um `Character` pode juntar um número sem limite
/// de escalares (uma letra com milhares de marcas combinantes é um caractere só), então contar ou cortar por
/// caractere não limita nem o tamanho nem o trabalho. Corta sempre na fronteira de escalar (nunca no meio de um).
enum MXUTF8 {
    /// O começo de `texto` com até `bytes` bytes, e se cortou. O trabalho é limitado a `bytes`.
    static func comeco<S: StringProtocol>(_ texto: S, bytes: Int) -> (texto: String, cortou: Bool)
    where S.UTF8View: BidirectionalCollection {
        let u = texto.utf8
        guard let corte = u.index(u.startIndex, offsetBy: max(0, bytes), limitedBy: u.endIndex), corte != u.endIndex else {
            return (String(texto), false)
        }
        var fim = corte
        while fim > u.startIndex, UTF8.isContinuation(u[fim]) { fim = u.index(before: fim) }
        return (String(decoding: u[u.startIndex..<fim], as: UTF8.self), true)
    }

    /// O fim de `texto` com até `bytes` bytes.
    static func fim(_ texto: String, bytes: Int) -> String {
        let u = texto.utf8
        guard let corte = u.index(u.endIndex, offsetBy: -max(0, bytes), limitedBy: u.startIndex), corte != u.startIndex else {
            return texto
        }
        var inicio = corte
        while inicio < u.endIndex, UTF8.isContinuation(u[inicio]) { inicio = u.index(after: inicio) }
        return String(decoding: u[inicio..<u.endIndex], as: UTF8.self)
    }

    /// "9 bytes", "12 KB", "3,4 MB" (o que ficou de fora da inspeção).
    static func tamanho(_ bytes: Int) -> String {
        let n = max(0, bytes)
        if n < 1000 { return n == 1 ? "1 byte" : "\(n) bytes" } // unidade igual nos dois idiomas
        if n < 1_000_000 { return "\((n + 999) / 1000) KB" }
        let decimos = (n + 50_000) / 100_000
        return String.localizedStringWithFormat(NSLocalizedString("%lld,%lld MB", bundle: .module, comment: "Tamanho em MB com uma casa decimal"), decimos / 10, decimos % 10)
    }
}

/// Um texto do pedido de aprovação (comando, saída, título, caminho, patch) que pode ser grande: a prévia curta vai na
/// tela; o inteiro fica disponível sob demanda ("Mostrar tudo"), até o limite absoluto de inspeção. Os limites são em
/// bytes UTF-8 (`utf8.count` é imediato): nada percorre o texto inteiro para decidir, e a prévia só lê o começo.
public struct MXTextoLongo: Equatable, Sendable {
    /// O texto inteiro, como veio do Hub.
    public let texto: String
    /// O tamanho em bytes UTF-8.
    public let bytes: Int
    let limitePrevia: Int
    public let cortadoNaPrevia: Bool
    /// Passa do limite absoluto: não dá para conferir inteiro no iPhone.
    public let excedeLimite: Bool

    public init(_ texto: String, previa limitePrevia: Int = MX.Conversa.detalhesMaximo,
                limite: Int = MX.Conversa.inspecaoMaxima) {
        self.texto = texto
        bytes = texto.utf8.count
        self.limitePrevia = limitePrevia
        cortadoNaPrevia = bytes > limitePrevia
        excedeLimite = bytes > limite
    }

    /// O começo (até `limitePrevia` bytes, com "…" se cortou). Montado quando a tela pede; lê só o começo.
    public var previa: String { cortadoNaPrevia ? MXUTF8.comeco(texto, bytes: limitePrevia).texto + "…" : texto }

    /// O que "Mostrar tudo" mostra: o texto inteiro; acima do limite absoluto, o começo e o fim (metade do limite cada)
    /// e quanto ficou de fora (nunca negativo: começo e fim não se sobrepõem).
    public func paraInspecao(limite: Int = MX.Conversa.inspecaoMaxima) -> String {
        guard bytes > limite else { return texto }
        let cabeca = MXUTF8.comeco(texto, bytes: limite / 2).texto
        let cauda = MXUTF8.fim(texto, bytes: limite / 2)
        let fora = max(0, bytes - cabeca.utf8.count - cauda.utf8.count)
        return cabeca + "\n\n" + String(localized: "… \(MXUTF8.tamanho(fora)) ficaram de fora …", bundle: .module) + "\n\n" + cauda
    }

    /// A nota embaixo da prévia cortada.
    public var notaDoCorte: String? {
        guard cortadoNaPrevia else { return nil }
        return excedeLimite ? String(localized: "Grande demais para conferir no iPhone: \"Mostrar tudo\" mostra o começo e o fim.", bundle: .module)
            : String(localized: "Prévia cortada: só o começo. \"Mostrar tudo\" abre o resto.", bundle: .module)
    }
}

/// Uma alteração de arquivo do pedido. Montar é imediato (só guarda caminho e patch e conta bytes); as linhas da
/// prévia só são preparadas quando a alteração aparece em Detalhes aberto (`linhasDaPrevia`), com trabalho limitado
/// ANTES de processar: só o começo do patch é lido. O patch e o caminho ficam inteiros para "Mostrar tudo" (com o
/// mesmo limite absoluto dos outros textos); acima dele, bloqueiam permitir.
public struct MXDiffPrevia: Identifiable, Equatable, Sendable {
    /// A posição no pedido (identidade estável; não o conteúdo).
    public let id: Int
    /// O caminho do arquivo, inteiro. Em Detalhes aparece inteiro com quebra de linha (`caminhoNaTela`), sem
    /// depender do corte do patch.
    public let caminho: MXTextoLongo
    public let adicionadas: Int
    public let removidas: Int
    public let novo: Bool
    public let patch: MXTextoLongo

    public init(_ d: FileDiff, indice: Int) {
        let m = MX.Conversa.self
        id = indice
        caminho = MXTextoLongo(d.path, previa: m.diffCaminhoBytes)
        adicionadas = d.added
        removidas = d.removed
        novo = d.isNew
        patch = MXTextoLongo(d.patch, previa: m.diffPreviaBytes)
    }

    /// O caminho como aparece em Detalhes: inteiro (até `diffCaminhoBytes`); maior que isso, o começo, e o inteiro
    /// quando a pessoa pede (começo e fim acima do limite absoluto, que bloqueia permitir).
    public func caminhoNaTela(inteiro: Bool) -> String { inteiro ? caminho.paraInspecao() : caminho.previa }

    /// O caminho é grande demais para a tela de cara: aparece "Mostrar o caminho inteiro" junto dele.
    public var caminhoPedeMostrarTudo: Bool { caminho.cortadoNaPrevia }

    /// As primeiras linhas do patch e se a prévia não mostra o patch inteiro. Lê no máximo
    /// `MX.Conversa.diffPreviaBytes` do patch, por bytes antes de separar linhas; cada linha também por bytes.
    public func linhasDaPrevia() -> MXLinhasDoDiff {
        let m = MX.Conversa.self
        let comeco = MXUTF8.comeco(patch.texto, bytes: m.diffPreviaBytes)
        let todas = comeco.texto.split(separator: "\n", omittingEmptySubsequences: false)
        var linhaCortada = false
        let linhas = todas.prefix(m.diffLinhas).map { l in
            let corte = MXUTF8.comeco(l, bytes: m.diffLinhaMaxima)
            if corte.cortou { linhaCortada = true }
            return corte.cortou ? corte.texto + "…" : corte.texto
        }
        let cortado = comeco.cortou || todas.count > m.diffLinhas || linhaCortada
        return MXLinhasDoDiff(linhas: linhas, cortado: cortado)
    }
}

/// O que a prévia de uma alteração mostra.
public struct MXLinhasDoDiff: Equatable, Sendable {
    public let linhas: [String]
    /// A prévia não mostra tudo: aparece "Mostrar tudo".
    public let cortado: Bool
}

/// Uma resposta possível no cartão de aprovação (a opção do Hub, com o rótulo do desenho).
public struct MXOpcaoDeAprovacao: Identifiable, Equatable, Sendable {
    public let id: String
    public let rotulo: String
    /// Permite (segurar para confirmar) ou recusa (um toque).
    public let permite: Bool
    /// A permissão mais estreita: o botão laranja grande ("Permitir uma vez").
    public let principal: Bool

    public init(id: String, rotulo: String, permite: Bool, principal: Bool) {
        self.id = id
        self.rotulo = rotulo
        self.permite = permite
        self.principal = principal
    }
}

/// Como o pedido terminou.
public enum MXResultadoDaAprovacao: Equatable, Sendable {
    case pendente
    case permitido(String)
    case recusado(String)
    /// Cancelado ou expirou.
    case encerrado(String)
}

/// O cartão "Esperando você": o mesmo pedido e as mesmas opções do app atual e do Mac ("uma autorização só").
public struct MXPedidoDeAprovacao: Equatable, Sendable {
    public let id: String
    /// A revisão da entrada que está na tela (`entryRev`): a resposta só sai se o pedido ainda for este.
    public let revisao: Int64
    /// "Byte quer mudar arquivos".
    public let titulo: String
    /// O nome técnico do Hub, em monoespaçada ("Edit src/pace.js"): a prévia no topo; inteiro em Detalhes se cortou.
    public let detalhe: MXTextoLongo?
    public let acao: String
    public let onde: String
    public let opcoes: [MXOpcaoDeAprovacao]
    public let resultado: MXResultadoDaAprovacao
    /// Opção cuja resposta está a caminho do computador.
    public let respondendo: String?
    /// Comando e saída, em Detalhes (prévia e "Mostrar tudo").
    public let detalhes: [MXTextoLongo]
    /// As alterações de arquivos do pedido (`EntryData.diffs`), todas (montar é imediato). Em Detalhes aberto aparece
    /// uma página por vez (`MX.Conversa.diffsMaximo`), e só as dela preparam a prévia.
    public let diffs: [MXDiffPrevia]

    public init(id: String, revisao: Int64 = 0, titulo: String, detalhe: MXTextoLongo?, acao: String, onde: String, opcoes: [MXOpcaoDeAprovacao],
                resultado: MXResultadoDaAprovacao, respondendo: String? = nil, detalhes: [MXTextoLongo] = [],
                diffs: [MXDiffPrevia] = []) {
        self.id = id
        self.revisao = revisao
        self.titulo = titulo
        self.detalhe = detalhe
        self.acao = acao
        self.onde = onde
        self.opcoes = opcoes
        self.resultado = resultado
        self.respondendo = respondendo
        self.detalhes = detalhes
        self.diffs = diffs
    }

    public var pendente: Bool { resultado == .pendente }

    /// O que identifica o pedido como apresentado: id, revisão, se está pendente e as opções. Se muda, a confirmação
    /// aberta fecha e um segurar em curso é cancelado (a pessoa vê o pedido novo antes de responder).
    public var versao: String {
        "\(id)#\(revisao)#\(pendente)#\(opcoes.map(\.id).joined(separator: ","))"
    }

    /// Há o que mostrar em Detalhes (comando, saída, alterações ou o título inteiro).
    public var temDetalhes: Bool { !detalhes.isEmpty || !diffs.isEmpty || (detalhe?.cortadoNaPrevia ?? false) }

    /// Algum texto do pedido passa do limite absoluto de inspeção: permitir (uma vez ou sempre) fica bloqueado com a
    /// explicação; recusar continua.
    public var bloqueadoPorTamanho: Bool {
        (detalhe?.excedeLimite ?? false) || detalhes.contains(where: \.excedeLimite)
            || diffs.contains { $0.patch.excedeLimite || $0.caminho.excedeLimite }
    }
}

/// Uma linha da Conversa.
public enum MXConteudo: Equatable, Sendable {
    /// Chip "Hoje 19:22".
    case separador(String)
    case usuario(MXMensagemDoUsuario)
    case agente(MXMensagemDoAgente)
    case trabalhando(MXTrabalhoEmCurso)
    case aprovacao(MXPedidoDeAprovacao)
    /// Aviso do computador (linha pequena centralizada); `erro` em vermelho.
    case aviso(String, erro: Bool)
}

public struct MXItemDaConversa: Identifiable, Equatable, Sendable {
    public let id: String
    public let conteudo: MXConteudo

    public init(id: String, _ conteudo: MXConteudo) {
        self.id = id
        self.conteudo = conteudo
    }
}

/// Uma mensagem na fila desta sessão ("Na fila · N").
public struct MXItemDaFila: Identifiable, Equatable, Sendable {
    public let id: String
    public let texto: String
    /// Revisão da entrada no Hub (`expectedRevision` de `queuedEdit`).
    public let revisao: Int64

    public init(id: String, texto: String, revisao: Int64) {
        self.id = id
        self.texto = texto
        self.revisao = revisao
    }
}

/// Tudo o que a tela da Conversa de um agente mostra.
public struct MXConversaDados: Equatable, Sendable {
    /// O id da conversa (o do agente na Principal; o de `MXIdDaConversa` numa sessão extra).
    public let id: String
    /// O agente dono da conversa (ajustes, fixar e sessões são dele).
    public let agenteId: String
    public let nome: String
    /// Função ("Desenvolvedor"); vazia quando não se sabe.
    public let funcao: String
    public let rosto: MXRosto
    public let estado: MexeriquinhaState
    /// "Pedir" ou "Liberar"; nulo quando não se sabe.
    public let autonomia: String?
    public let fixado: Bool
    public let itens: [MXItemDaConversa]
    public let fila: [MXItemDaFila]
    /// Trabalhando nesta conversa: o que se manda entra na fila.
    public let trabalhando: Bool
    /// Há um cartão de aprovação esperando: a caixa diz "Entra depois da sua decisão".
    public let esperandoDecisao: Bool
    /// "Claude · Opus".
    public let modelo: String?
    /// "% do contexto"; nulo quando o Hub não informa (some do rodapé).
    public let contexto: Int?
    /// A ordem da fila é a do Hub: só então a posição numerada ("a 2ª da fila") aparece.
    public let ordemDaFilaConfiavel: Bool
    /// O botão do WhatsApp no topo (a captura 06 não tem).
    public let mostraWhatsApp: Bool
    /// Por que esta conversa não aceita mensagem agora (sessão extra que o Hub não confirmou): a caixa fica desligada
    /// com este texto. Nulo: manda normalmente.
    public let semEnvio: String?

    public init(id: String, agenteId: String? = nil, nome: String, funcao: String, rosto: MXRosto, estado: MexeriquinhaState,
                autonomia: String?, fixado: Bool, itens: [MXItemDaConversa], fila: [MXItemDaFila], trabalhando: Bool,
                esperandoDecisao: Bool, modelo: String?, contexto: Int?, ordemDaFilaConfiavel: Bool = true,
                mostraWhatsApp: Bool = true, semEnvio: String? = nil) {
        self.id = id
        self.agenteId = agenteId ?? id
        self.semEnvio = semEnvio
        self.nome = nome
        self.funcao = funcao
        self.rosto = rosto
        self.estado = estado
        self.autonomia = autonomia
        self.fixado = fixado
        self.itens = itens
        self.fila = fila
        self.trabalhando = trabalhando
        self.esperandoDecisao = esperandoDecisao
        self.modelo = modelo
        self.contexto = contexto
        self.ordemDaFilaConfiavel = ordemDaFilaConfiavel
        self.mostraWhatsApp = mostraWhatsApp
    }
}

/// O id de uma conversa: o do agente (a Principal) ou o do agente e o de uma sessão extra (07b), num texto só para as
/// telas e a fonte levarem como sempre. Regra pura, testada.
public struct MXIdDaConversa: Equatable, Hashable, Sendable {
    public let agente: String
    /// Nulo: a Principal (o Hub dá à Principal o id do próprio agente).
    public let sessao: String?

    /// Separa agente e sessão no texto (o separador de unidade não aparece em ids do Hub).
    static let separador: Character = "\u{1F}"

    public init(agente: String, sessao: String?) {
        self.agente = agente
        self.sessao = sessao == agente || sessao?.isEmpty == true ? nil : sessao
    }

    public init(_ texto: String) {
        if let i = texto.firstIndex(of: Self.separador) {
            self.init(agente: String(texto[..<i]), sessao: String(texto[texto.index(after: i)...]))
        } else {
            self.init(agente: texto, sessao: nil)
        }
    }

    public var texto: String { sessao.map { "\(agente)\(Self.separador)\($0)" } ?? agente }
}

/// O que a aba Conversas mostra.
public struct MXConversasDados: Equatable, Sendable {
    public let berga: MXBergaResumo?
    public let space: String
    public let canais: [MXCanalResumo]
    public let agentes: [MXAgenteResumo]

    public init(berga: MXBergaResumo?, space: String, canais: [MXCanalResumo], agentes: [MXAgenteResumo]) {
        self.berga = berga
        self.space = space
        self.canais = canais
        self.agentes = agentes
    }
}

// MARK: - Estado do agente

public enum MXEstadoRegras {
    /// O texto do estado na cápsula e no menu.
    public static func texto(_ estado: MexeriquinhaState) -> String {
        switch estado {
        case .livre: String(localized: "Livre", bundle: .module)
        case .trabalhando: String(localized: "Trabalhando", bundle: .module)
        case .precisaResposta: String(localized: "Precisa de você", bundle: .module)
        case .dormindo: String(localized: "Dormindo", bundle: .module)
        case .erro: String(localized: "Erro", bundle: .module)
        }
    }

    /// A cor do estado: verde trabalhando, laranja precisa de você, vermelho erro, marrom secundário o resto.
    public static func cor(_ estado: MexeriquinhaState) -> MXCor {
        switch estado {
        case .trabalhando: MX.Claro.sucessoTexto
        case .precisaResposta: MX.Claro.mexericaTexto
        case .erro: MX.Claro.erroTexto
        case .livre, .dormindo: MX.Claro.textoSecundario
        }
    }
}

// MARK: - Caixa de mensagem

/// O botão da direita na cápsula da caixa.
public enum MXBotaoDaCaixa: Equatable, Sendable {
    /// Sem texto: a onda laranja, que abre a ligação.
    case aoVivo
    /// Com texto: a seta laranja.
    case enviar
    /// Editando uma mensagem da fila: ✓.
    case salvarEdicao
    /// Sem texto e esperando a decisão no cartão: o quadrado, que para o agente.
    case parar
}

/// Estado da caixa de mensagem (regra pura, testada).
public struct MXCaixa: Equatable, Sendable {
    public let botao: MXBotaoDaCaixa
    /// Desligado: visível, mas sem ação (sem conexão, ou ✓ sem texto).
    public let ligado: Bool
    /// O texto do campo vazio.
    public let dica: String
    /// Rótulo do botão no VoiceOver.
    public let rotulo: String
    /// Desligado por não existir nesta fonte (o ao vivo sem ligação): "Em breve", valor acessível "Indisponível".
    public var emBreve = false

    /// `aoVivo`: a fonte tem o ao vivo (`MXCapacidades.ligado(.aoVivo)`); sem ele, a onda fica desligada.
    /// `semEnvio`: esta conversa não aceita mensagem agora (o motivo vira a dica); enviar e o ao vivo desligam. Editar
    /// e parar seguem pela ligação (não mandam mensagem nova).
    public static func estado(texto: String, editando: Bool, esperandoDecisao: Bool, trabalhando: Bool,
                              conexao: MXEstadoConexao, nome: String, salvando: Bool = false, aoVivo: Bool = true,
                              semEnvio: String? = nil) -> MXCaixa {
        let temTexto = !texto.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let conectado = conexao.podeAgir
        let dica = semEnvio ?? (!conectado ? MXTextoDeQueda.semEnvio
            : esperandoDecisao ? String(localized: "Entra depois da sua decisão", bundle: .module)
            : String(localized: "Mensagem para \(nome)", bundle: .module))
        if editando {
            // Um salvar por vez: enquanto o Hub não responde, o ✓ fica desligado.
            return MXCaixa(botao: .salvarEdicao, ligado: conectado && temTexto && !salvando, dica: dica, rotulo: String(localized: "Salvar a mensagem da fila", bundle: .module))
        }
        if temTexto {
            let rotulo = trabalhando || esperandoDecisao ? String(localized: "Enviar (\(nome) está trabalhando: entra na fila)", bundle: .module) : String(localized: "Enviar", bundle: .module)
            return MXCaixa(botao: .enviar, ligado: conectado && semEnvio == nil, dica: dica, rotulo: rotulo)
        }
        if esperandoDecisao {
            return MXCaixa(botao: .parar, ligado: conectado, dica: dica, rotulo: String(localized: "Parar \(nome)", bundle: .module))
        }
        return MXCaixa(botao: .aoVivo, ligado: aoVivo && semEnvio == nil, dica: dica, rotulo: String(localized: "Falar ao vivo com \(nome)", bundle: .module),
                       emBreve: !aoVivo)
    }
}

// MARK: - Fila

public enum MXFilaRegras {
    /// A fila aparece quando há mensagem esperando.
    public static func visivel(_ fila: [MXItemDaFila]) -> Bool { !fila.isEmpty }

    /// Passou de 2: rola com barra e o fim esmaece.
    public static func rola(_ fila: [MXItemDaFila]) -> Bool { fila.count > MX.Conversa.filaLinhasVisiveis }

    /// "Na fila · 3".
    public static func titulo(_ fila: [MXItemDaFila]) -> String { String(localized: "Na fila · \(fila.count)", bundle: .module) }

    /// Recolhida, o cabeçalho mostra a primeira: "· Depois roda o lint também".
    public static func resumoRecolhida(_ fila: [MXItemDaFila]) -> String? { fila.first.map { "· \($0.texto)" } }

    /// Posição (1, 2, 3…) da mensagem na fila, ou nulo se ela saiu.
    public static func posicao(_ id: String, em fila: [MXItemDaFila]) -> Int? {
        fila.firstIndex { $0.id == id }.map { $0 + 1 }
    }

    /// "Editando a 2ª da fila"; sem ordem confiável (posição nula), "Editando uma mensagem da fila".
    public static func textoEditando(posicao: Int?) -> String {
        posicao.map { String(localized: "Editando a \($0)ª da fila", bundle: .module) } ?? String(localized: "Editando uma mensagem da fila", bundle: .module)
    }

    /// A revisão que vai no `queuedEdit`: a de quando a edição começou; se só a ordem mudou desde então (o texto é
    /// o mesmo), vale a atual (como no Mac, `BandejaFila.tsx`). Um texto trocado em outro aparelho invalida a edição.
    public static func revisaoParaSalvar(_ original: MXItemDaFila, fila: [MXItemDaFila]) -> Int64 {
        guard let atual = fila.first(where: { $0.id == original.id }), atual.texto == original.texto else { return original.revisao }
        return atual.revisao
    }

    /// Por que a ação na fila não valeu. O Hub põe o código no começo da mensagem; o resto nunca vai à tela.
    /// Textos do Mac (`conversa.fila.erro.*`).
    public static func falha(_ mensagem: String?) -> String {
        let codigo = mensagem.map { m in String(m.prefix { c in c.isLowercase || c == "_" }) } ?? ""
        switch codigo {
        case "queued_entry_dispatched", "queued_entry_not_found": return String(localized: "Essa mensagem já saiu da fila.", bundle: .module)
        case "queued_revision_conflict": return String(localized: "A fila mudou enquanto isso. Confira e tente de novo.", bundle: .module)
        case "not_allowed": return String(localized: "Este aparelho não pode mais mexer na fila.", bundle: .module)
        case "queued_control_pending": return String(localized: "O computador não confirmou a tempo. Confira a fila.", bundle: .module)
        default: return String(localized: "Não foi possível mudar a fila.", bundle: .module)
        }
    }
}

/// Os salvamentos de mensagens da fila a caminho do Hub, por agente e mensagem. Fica na FONTE (compartilhada), fora
/// da edição e da tela: reabrir a edição, ou sair e reabrir a conversa, não libera um segundo salvar da mesma
/// mensagem enquanto o primeiro não voltar.
public struct MXSalvamentosPendentes: Equatable, Sendable {
    struct Chave: Hashable, Sendable {
        let conversa: String
        let mensagem: String
    }
    private var reservas: Set<Chave> = []

    public init() {}

    public func pendente(_ mensagem: String, em conversa: String) -> Bool {
        reservas.contains(Chave(conversa: conversa, mensagem: mensagem))
    }

    /// Reserva o salvar desta mensagem; falso se já há um em curso.
    public mutating func iniciar(_ mensagem: String, em conversa: String) -> Bool {
        reservas.insert(Chave(conversa: conversa, mensagem: mensagem)).inserted
    }

    /// O Hub respondeu (certo ou errado), ou o salvar nem saiu.
    public mutating func concluir(_ mensagem: String, em conversa: String) {
        reservas.remove(Chave(conversa: conversa, mensagem: mensagem))
    }
}

/// Um salvar da edição a caminho do Hub: de qual edição (geração) e com que texto.
public struct MXSalvamento: Equatable, Sendable {
    public let geracao: Int
    public let texto: String
}

/// A edição de uma mensagem da fila: o texto dela vai para a caixa; cancelar ou salvar devolve o que estava escrito.
/// Um salvar por vez; a resposta só encerra a edição se for desta edição e a caixa ainda tiver o texto enviado.
public struct MXEdicaoDaFila: Equatable, Sendable {
    /// A mensagem em edição, como estava quando a edição começou (ou no último salvar que valeu).
    public private(set) var item: MXItemDaFila?
    /// O que estava na caixa antes da edição (volta ao cancelar ou salvar).
    public private(set) var guardado = ""
    /// Muda a cada começo e fim de edição: resposta de um salvar antigo não mexe numa edição nova.
    public private(set) var geracao = 0
    /// O salvar em curso, se há.
    public private(set) var salvando: MXSalvamento?

    public init() {}

    public var ativa: Bool { item != nil }

    /// Começa a editar `item`; devolve o texto da caixa. Trocar de mensagem no meio mantém o rascunho original.
    public mutating func comecar(_ item: MXItemDaFila, caixa: String) -> String {
        if self.item == nil { guardado = caixa }
        self.item = item
        geracao += 1
        salvando = nil
        return item.texto
    }

    /// Cancela (ou conclui) a edição; devolve o texto que volta para a caixa.
    public mutating func encerrar() -> String {
        let volta = guardado
        item = nil
        guardado = ""
        geracao += 1
        salvando = nil
        return volta
    }

    /// Começa a salvar `texto`. Nulo se não há edição ou se já há um salvar em curso (um por vez).
    public mutating func iniciarSalvar(_ texto: String) -> MXSalvamento? {
        guard item != nil, salvando == nil else { return nil }
        let s = MXSalvamento(geracao: geracao, texto: texto)
        salvando = s
        return s
    }

    /// O Hub respondeu ao salvar `s`. Encerra a edição (e devolve o rascunho guardado) só se é o salvar desta
    /// edição, deu certo e a caixa ainda tem o texto enviado. Se a pessoa continuou digitando, a edição continua com o
    /// texto salvo como base. Se deu erro, a edição continua intacta.
    public mutating func concluirSalvar(_ s: MXSalvamento, ok: Bool, caixa: String) -> String? {
        guard s.geracao == geracao, salvando == s, let atual = item else { return nil }
        salvando = nil
        guard ok else { return nil }
        guard caixa == s.texto else {
            item = MXItemDaFila(id: atual.id, texto: s.texto, revisao: atual.revisao)
            return nil
        }
        return encerrar()
    }

    /// A fila mudou: se a mensagem em edição saiu (mandada, removida em outro aparelho), a edição acaba aqui e
    /// devolve o texto da caixa; senão, nulo.
    public mutating func filaMudou(_ fila: [MXItemDaFila]) -> String? {
        guard let item, MXFilaRegras.posicao(item.id, em: fila) == nil else { return nil }
        return encerrar()
    }
}

// MARK: - Segurar para permitir

/// Como o botão de permitir mostra o segurar. Em todos os modos, permitir exige segurar 0,9 s (decisão da revisão de
/// segurança da etapa 2): nunca é um toque só, nem com Reduzir movimento, nem "Sempre permitir".
public enum MXModoDePermitir: Equatable, Sendable {
    /// Segurar com o preenchimento animado.
    case segurarAnimado
    /// Reduzir movimento: o mesmo segurar, sem animação; o rótulo diz "Segure…" enquanto segura.
    case segurarParado
}

public enum MXSegurarRegras {
    public static func modo(reduzirMovimento: Bool) -> MXModoDePermitir { reduzirMovimento ? .segurarParado : .segurarAnimado }

    /// Ativar pelo VoiceOver ou Controle por Botão nunca aprova direto: abre uma confirmação deliberada.
    public static let ativacaoAcessivelAprovaDireto = false

    /// A pergunta da confirmação acessível: "Permitir uma vez?", "Sempre permitir?".
    public static func pergunta(_ rotulo: String) -> String { "\(rotulo)?" }

    /// Um segurar (ou a confirmação acessível) só confirma se o pedido ainda é o que estava na tela quando começou.
    public static func confirmaAinda(versaoInicial: String?, versaoAtual: String) -> Bool { versaoInicial == versaoAtual }

    /// O que o botão diz enquanto segura com Reduzir movimento (progresso estático, sem animação).
    public static var rotuloSegurando: String { String(localized: "Segure…", bundle: .module) }

    /// Quanto do segurar já passou (0…1), desde `inicio`.
    public static func progresso(desde inicio: Date?, agora: Date, duracao: Double = MX.Movimento.segurarParaPermitir) -> Double {
        guard let inicio, duracao > 0 else { return 0 }
        return min(1, max(0, agora.timeIntervalSince(inicio) / duracao))
    }

    /// Soltar confirma só se segurou o tempo todo.
    public static func confirma(segurouPor tempo: Double, duracao: Double = MX.Movimento.segurarParaPermitir) -> Bool {
        tempo >= duracao
    }

    /// A dica embaixo dos botões (igual nos dois modos: sempre se segura).
    public static func dica(_ modo: MXModoDePermitir) -> String { String(localized: "Segure para permitir; soltar antes cancela.", bundle: .module) }

    /// O que o VoiceOver diz do botão: ativar abre a confirmação.
    public static func dicaAcessivel(_ modo: MXModoDePermitir) -> String { String(localized: "Ative para pedir a confirmação.", bundle: .module) }
}

// MARK: - Horas

/// Horas e datas como o desenho escreve ("09:12", "Ontem", "8 nov", "Hoje 19:22"); "Ontem", "Hoje" e os meses vêm do catálogo (pt-BR/en).
public enum MXHora {
    static var meses: [String] {
        [String(localized: "jan", bundle: .module),
         String(localized: "fev", bundle: .module),
         String(localized: "mar", bundle: .module),
         String(localized: "abr", bundle: .module),
         String(localized: "mai", bundle: .module),
         String(localized: "jun", bundle: .module),
         String(localized: "jul", bundle: .module),
         String(localized: "ago", bundle: .module),
         String(localized: "set", bundle: .module),
         String(localized: "out", bundle: .module),
         String(localized: "nov", bundle: .module),
         String(localized: "dez", bundle: .module)]
    }

    /// "19:22".
    public static func relogio(_ data: Date, calendario: Calendar = .current) -> String {
        let c = calendario.dateComponents([.hour, .minute], from: data)
        let dois = { (n: Int) in n < 10 ? "0\(n)" : "\(n)" }
        return "\(dois(c.hour ?? 0)):\(dois(c.minute ?? 0))"
    }

    /// "8 nov", ou "8 nov 2025" em outro ano.
    static func dia(_ data: Date, agora: Date, calendario: Calendar) -> String {
        let c = calendario.dateComponents([.year, .month, .day], from: data)
        let ano = calendario.component(.year, from: agora)
        let mes = meses[max(0, min(11, (c.month ?? 1) - 1))]
        let base = "\(c.day ?? 1) \(mes)"
        return c.year == ano ? base : "\(base) \(c.year ?? ano)"
    }

    /// Hora na lista: hoje "09:12", ontem "Ontem", antes "8 nov".
    public static func naLista(_ data: Date, agora: Date, calendario: Calendar = .current) -> String {
        if calendario.isDate(data, inSameDayAs: agora) { return relogio(data, calendario: calendario) }
        if let ontem = calendario.date(byAdding: .day, value: -1, to: agora), calendario.isDate(data, inSameDayAs: ontem) { return String(localized: "Ontem", bundle: .module) }
        return dia(data, agora: agora, calendario: calendario)
    }

    /// Chip entre mensagens: "Hoje 19:22", "Ontem 08:10", "8 nov 14:00".
    public static func separador(_ data: Date, agora: Date, calendario: Calendar = .current) -> String {
        let hora = relogio(data, calendario: calendario)
        if calendario.isDate(data, inSameDayAs: agora) { return String(localized: "Hoje \(hora)", bundle: .module) }
        if let ontem = calendario.date(byAdding: .day, value: -1, to: agora), calendario.isDate(data, inSameDayAs: ontem) { return String(localized: "Ontem \(hora)", bundle: .module) }
        return "\(dia(data, agora: agora, calendario: calendario)) \(hora)"
    }
}

// MARK: - Busca e layout

public extension MXConversasRegras {
    /// A busca ignora maiúsculas e acentos.
    static func normalizar(_ s: String) -> String {
        s.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: Locale(identifier: "pt_BR"))
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Agentes que batem com a busca (nome ou função). Busca vazia: todos.
    static func filtrar(_ agentes: [MXAgenteResumo], busca: String) -> [MXAgenteResumo] {
        let b = normalizar(busca)
        guard !b.isEmpty else { return agentes }
        return agentes.filter { normalizar($0.nome).contains(b) || normalizar($0.funcao).contains(b) }
    }

    /// Canais que aparecem: com busca, os que batem (mesmo recolhidos, para a busca achar); sem busca, todos se
    /// abertos, nenhum se recolhidos.
    static func canaisVisiveis(_ canais: [MXCanalResumo], abertos: Bool, busca: String) -> [MXCanalResumo] {
        let b = normalizar(busca)
        guard !b.isEmpty else { return abertos ? canais : [] }
        return canais.filter { normalizar($0.nome).contains(b) }
    }

    /// Sem resultado: "Nada encontrado para “x”."
    static func semResultado(_ busca: String) -> String {
        String(localized: "Nada encontrado para “\(busca.trimmingCharacters(in: .whitespacesAndNewlines))”.", bundle: .module)
    }
}

/// Política de links nas mensagens: o texto vem do Hub (e de quem escreveu para o agente); só abre o que é web ou
/// e-mail. Outros esquemas (de outros apps, `tel:`, `file:`…) não abrem.
public enum MXLinks {
    public static let esquemas: Set<String> = ["http", "https", "mailto"]

    public static func permitido(_ url: URL) -> Bool {
        guard let esquema = url.scheme?.lowercased() else { return false }
        return esquemas.contains(esquema)
    }
}

public extension MXTroca {
    /// Com Reduzir movimento o conteúdo que entra e sai leva a própria animação (cross-fade curto), porque a troca
    /// não anima posição; com a mola, vai junto com ela.
    var conteudoAnimaSozinho: Bool { !interpola }
}

/// O item Fixar/Desafixar do menu do agente.
public struct MXItemDeFixar: Equatable, Sendable {
    public let titulo: String
    /// Sem autoridade (sem conexão), fica à vista e desligado; tocar não faz nada e o menu não fecha.
    public let ligado: Bool
    /// O que o VoiceOver diz do estado ("Indisponível sem conexão"); nulo quando ligado.
    public let valorAcessivel: String?

    public init(fixado: Bool, conexao: MXEstadoConexao) {
        titulo = fixado ? String(localized: "Desafixar", bundle: .module) : String(localized: "Fixar no topo", bundle: .module)
        ligado = conexao.podeAgir
        valorAcessivel = ligado ? nil : String(localized: "Indisponível sem conexão", bundle: .module)
    }
}

/// Um item do menu do agente (regra pura, testada). Ligado só quando faz o que diz nesta fonte (`MXCapacidades`) e,
/// Fixar, com autoridade. O que só levaria a "Em breve" fica à vista e desligado, com "Em breve" à direita e valor
/// acessível "Indisponível"; tocar nele não faz nada e o menu não fecha.
public struct MXItemDoMenu: Identifiable, Equatable, Sendable {
    public let controle: MXControle
    public let titulo: String
    /// O texto à direita: o valor (a autonomia) ou "Em breve".
    public let valor: String?
    public let ligado: Bool
    /// Desligado por não existir nesta fonte (não por falta de conexão).
    public let emBreve: Bool
    public let rotuloAcessivel: String
    public let valorAcessivel: String?
    public let dicaAcessivel: String?
    /// A linha fina vem antes deste item.
    public let divisoriaAntes: Bool

    public var id: MXControle { controle }
}

public enum MXMenuRegras {
    /// Os itens do menu, na ordem do desenho: Rotinas, Ajustes do <nome>, Autonomia, —, Nova sessão, Fixar no topo.
    public static func itens(_ dados: MXConversaDados, conexao: MXEstadoConexao, capacidades: MXCapacidades) -> [MXItemDoMenu] {
        func item(_ controle: MXControle, _ titulo: String, valor: String? = nil, divisoria: Bool = false) -> MXItemDoMenu {
            let ligado = capacidades.ligado(controle)
            return MXItemDoMenu(
                controle: controle, titulo: titulo, valor: ligado ? valor : MXIndisponivel.emBreve, ligado: ligado,
                emBreve: !ligado, rotuloAcessivel: ligado ? valor.map { "\(titulo): \($0)" } ?? titulo : titulo,
                valorAcessivel: ligado ? nil : MXIndisponivel.valorAcessivel, dicaAcessivel: ligado ? nil : MXIndisponivel.emBreve,
                divisoriaAntes: divisoria)
        }
        let fixar = MXItemDeFixar(fixado: dados.fixado, conexao: conexao)
        return [
            item(.rotinas, String(localized: "Rotinas", bundle: .module)),
            item(.ajustesDoAgente, String(localized: "Ajustes do \(dados.nome)", bundle: .module)),
            item(.autonomia, String(localized: "Autonomia", bundle: .module), valor: dados.autonomia),
            item(.novaSessao, String(localized: "Nova sessão", bundle: .module), divisoria: true),
            MXItemDoMenu(controle: .fixar, titulo: fixar.titulo, valor: nil, ligado: fixar.ligado, emBreve: false,
                         rotuloAcessivel: fixar.titulo, valorAcessivel: fixar.valorAcessivel, dicaAcessivel: nil, divisoriaAntes: false),
        ]
    }
}

/// A linha da fila no tamanho de texto (regra pura, testada). No padrão: uma linha de texto e as três ações ao lado,
/// como sempre. Nos tamanhos de acessibilidade: até 2 linhas de texto e as ações (Mandar agora, Editar, Remover)
/// numa linha embaixo, cada uma com 44 pt e o mesmo afastamento.
public struct MXArranjoDaFila: Equatable, Sendable {
    public let linhasDoTexto: Int
    public let acoesEmbaixo: Bool

    public init(acessibilidade: Bool) {
        linhasDoTexto = acessibilidade ? 2 : 1
        acoesEmbaixo = acessibilidade
    }

    /// A altura de uma mensagem na lista, para as visíveis antes de rolar: no padrão, o passo da linha; com as ações
    /// embaixo, as linhas de texto mais a linha das ações.
    public func alturaDaLinha(linha: CGFloat, passo: CGFloat, acao: CGFloat) -> CGFloat {
        acoesEmbaixo ? CGFloat(linhasDoTexto) * linha + acao : passo
    }
}

/// A caixa de mensagem no tamanho de texto (regra pura, testada). No padrão, nada muda. Nos tamanhos de
/// acessibilidade: o campo rola por dentro depois de 3 linhas e a caixa inteira para em metade da tela, rolando por
/// dentro (o texto em edição não sai por baixo).
public struct MXArranjoDaCaixa: Equatable, Sendable {
    public let acessibilidade: Bool

    public init(acessibilidade: Bool) { self.acessibilidade = acessibilidade }

    public var linhasDoCampo: Int { acessibilidade ? MX.Conversa.campoLinhasMaximasAcessivel : MX.Conversa.campoLinhasMaximas }

    /// Texto grande: a dica do campo vazio ("Entra depois da sua decisão") é desenhada aqui, com até 2 linhas e
    /// encolhendo um pouco (o `prompt` do campo não quebra linha). No padrão, o `prompt` de sempre.
    public var dicaPropria: Bool { acessibilidade }
    public var linhasDaDica: Int { acessibilidade ? 2 : 1 }
    public var escalaMinimaDaDica: CGFloat { acessibilidade ? 0.85 : 1 }

    /// A altura máxima da caixa para a altura da tela; nula (sem limite) no padrão ou antes de medir a tela.
    public func alturaMaxima(tela: CGFloat) -> CGFloat? {
        guard acessibilidade, tela > 0 else { return nil }
        return tela * MX.Conversa.caixaFracaoMaximaDaTela
    }
}

public enum MXConversaRegras {
    /// O que o VoiceOver lê na cápsula do topo: nome, função (se houver) e estado ("Byte, Dev, Trabalhando").
    public static func rotuloDaCapsula(nome: String, funcao: String, estado: String) -> String {
        [nome, funcao, estado].filter { !$0.isEmpty }.joined(separator: ", ")
    }

    /// Quantas alterações aparecem: as primeiras 20, ou todas depois de "Mostrar os outros".
    /// Quantas páginas de alterações há em Detalhes.
    public static func paginasDeDiffs(_ total: Int) -> Int {
        let n = MX.Conversa.diffsMaximo
        return max(1, (max(0, total) + n - 1) / n)
    }

    /// As alterações da página (uma página por vez: o trabalho preparado nunca passa de
    /// `diffsMaximo × diffPreviaBytes`, por maior que seja o pedido). Página fora do intervalo vai para a mais próxima.
    public static func paginaDeDiffs(total: Int, pagina: Int) -> Range<Int> {
        let n = MX.Conversa.diffsMaximo
        let p = paginaNormalizada(total: total, pagina: pagina)
        let inicio = min(max(0, total), p * n)
        return inicio..<min(max(0, total), inicio + n)
    }

    /// A página efetivamente exibida: a guardada, trazida para dentro do intervalo (o pedido pode ter encolhido).
    public static func paginaNormalizada(total: Int, pagina: Int) -> Int {
        min(max(0, pagina), paginasDeDiffs(total) - 1)
    }

    /// "Anteriores": a partir da página exibida, não da guardada.
    public static func paginaAnterior(total: Int, pagina: Int) -> Int {
        max(0, paginaNormalizada(total: total, pagina: pagina) - 1)
    }

    /// "Próximos": a partir da página exibida, sem passar da última.
    public static func proximaPagina(total: Int, pagina: Int) -> Int {
        min(paginasDeDiffs(total) - 1, paginaNormalizada(total: total, pagina: pagina) + 1)
    }

    /// "Arquivos 21–40 de 523".
    public static func rotuloDaPagina(_ faixa: Range<Int>, total: Int) -> String {
        String(localized: "Arquivos \(faixa.lowerBound + 1)–\(faixa.upperBound) de \(total)", bundle: .module)
    }

    /// A conversa conta como lida enquanto está visível numa cena ativa.
    public static func lendo(visivel: Bool, cenaAtiva: Bool) -> Bool { visivel && cenaAtiva }

    /// Texto muito grande entra cortado; "Mostrar tudo" abre o resto sob demanda.
    public static func cortar(_ texto: String, limite: Int = MX.Conversa.textoLimite) -> (texto: String, cortado: Bool) {
        let corte = MXUTF8.comeco(texto, bytes: limite)
        return corte.cortou ? (corte.texto + "…", true) : (texto, false)
    }

    /// A página anterior veio: a fronteira do histórico (o menor `seq` do Hub carregado) recuou, ou o histórico
    /// acabou; senão, falhou. Mensagem nova que chega pelo stream no meio não conta: ela entra no fim, não no começo.
    public static func carregou(fronteiraAntes: Int64?, fronteiraDepois: Int64?, aindaHaMais: Bool) -> Bool {
        guard aindaHaMais else { return true }
        guard let depois = fronteiraDepois else { return false }
        guard let antes = fronteiraAntes else { return true }
        return depois < antes
    }

    /// Voltou a ligação com o topo à vista (o indicador não ganha outro `onAppear`): tenta de novo sozinha.
    public static func recarregaAoReconectar(topoVisivel: Bool, conexao: MXEstadoConexao, pode: Bool, carregando: Bool) -> Bool {
        topoVisivel && carregaAnteriores(pode: pode, carregando: carregando, conexao: conexao)
    }

    /// Chegou ao topo: carrega a página anterior se há mais, se não está carregando e se há ligação.
    public static func carregaAnteriores(pode: Bool, carregando: Bool, conexao: MXEstadoConexao) -> Bool {
        pode && !carregando && conexao.podeAgir
    }

    /// Código no balão: monoespaçada relativa ao estilo `footnote` (13 pt no tamanho padrão, como o desenho), que
    /// escala com o Dynamic Type junto do texto do balão, sem teto.
    public static var fonteDoCodigo: Font { .system(.footnote, design: .monospaced) }

    /// Largura máxima do balão: a do desenho; nos tamanhos de acessibilidade, a tela toda (o texto grande não vira
    /// uma coluna estreita).
    public static func larguraMaxima(_ base: CGFloat, _ tamanho: DynamicTypeSize) -> CGFloat {
        tamanho.isAccessibilitySize ? .infinity : base
    }

    /// Cartão de aprovação em uma coluna (rótulo em cima do valor, botões empilhados) nos tamanhos de acessibilidade.
    public static func umaColuna(_ tamanho: DynamicTypeSize) -> Bool { tamanho.isAccessibilitySize }

    /// "1 passo", "3 passos".
    public static func passos(_ n: Int) -> String { n == 1 ? String(localized: "1 passo", bundle: .module) : String(localized: "\(n) passos", bundle: .module) }

    /// O texto do balão com markdown simples (negrito, itálico, `código`, links); o código ganha fundo e monoespaçada.
    /// Texto que não é markdown válido fica como veio.
    public static func textoRico(_ texto: String) -> AttributedString {
        let opcoes = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        guard var rico = try? AttributedString(markdown: texto, options: opcoes) else { return AttributedString(texto) }
        let codigos = rico.runs.filter { $0.inlinePresentationIntent?.contains(.code) == true }.map(\.range)
        for faixa in codigos {
            // Chaves explícitas do SwiftUI: `font` também existe no escopo do UIKit (seria ambíguo).
            rico[faixa][AttributeScopes.SwiftUIAttributes.FontAttribute.self] = MXConversaRegras.fonteDoCodigo
            rico[faixa][AttributeScopes.SwiftUIAttributes.BackgroundColorAttribute.self] = MX.CorDaConversa.neutro.color
        }
        return rico
    }
}

// MARK: - Alvo de toque

/// Espaçamento da fila de mensagens (ver `MX.Conversa.filaEspacamento`).
public enum MXEspacamentoDaFila: Equatable, Sendable {
    /// Compacta, como no desenho e no Mac.
    case desenho
    /// Passo de 44 pt (padrão da Apple).
    case apple
}

/// Alvo de toque de 44 pt em tudo (conforme as recomendações da Apple). O desenho fica do
/// tamanho desenhado; só a área de toque, transparente, cresce até 44. Vizinhos nunca se sobrepõem.
public enum MXAlvoDeToque {
    public static var minimo: CGFloat { MX.Espaco.alvoMinimo }

    /// O lado da área de toque de um desenho: o próprio desenho, ou 44 se ele for menor.
    public static func lado(_ desenhado: CGFloat) -> CGFloat { max(desenhado, minimo) }

    /// Distância entre centros de alvos lado a lado para cada um ter 44 sem sobrepor o vizinho. Na fila, use
    /// `passoNaFila`, que segue o espaçamento escolhido (`MX.Conversa.filaEspacamento`).
    public static func passo(_ desenhado: CGFloat) -> CGFloat { lado(desenhado) }

    /// As áreas de toque de alvos alinhados, de centros `centros` (em ordem): cada uma cresce até 44 em volta do
    /// centro e, quando o vizinho está a menos de 44, a área é dividida no meio do caminho. Nunca se sobrepõem.
    public static func areas(centros: [CGFloat], desenhado: CGFloat) -> [ClosedRange<CGFloat>] {
        let meio = lado(desenhado) / 2
        return centros.indices.map { i in
            var inicio = centros[i] - meio
            var fim = centros[i] + meio
            if i > centros.startIndex { inicio = max(inicio, (centros[i - 1] + centros[i]) / 2) }
            if i < centros.index(before: centros.endIndex) { fim = min(fim, (centros[i] + centros[i + 1]) / 2) }
            return inicio...max(inicio, fim)
        }
    }

    /// O passo de uma medida da fila (linha 31, cabeçalho 22, ação 28) no espaçamento escolhido: o desenhado, ou 44.
    public static func passoNaFila(_ desenhado: CGFloat, _ espacamento: MXEspacamentoDaFila = MX.Conversa.filaEspacamento) -> CGFloat {
        espacamento == .apple ? lado(desenhado) : desenhado
    }

    /// As áreas de toque das três ações de uma linha da fila (Mandar agora, Editar, Remover), em x, a partir da
    /// borda esquerda da primeira. No desenho: 36 / 28 / 36 (as das pontas crescem para fora, a do meio fica com o
    /// próprio desenho; nenhuma se sobrepõe). No padrão da Apple: 44 / 44 / 44.
    public static func areasDasAcoesDaFila(_ espacamento: MXEspacamentoDaFila = MX.Conversa.filaEspacamento) -> [ClosedRange<CGFloat>] {
        let passo = passoNaFila(MX.Conversa.filaAcao, espacamento)
        return areas(centros: centros(MX.Conversa.filaAcoes, passo: passo), desenhado: MX.Conversa.filaAcao)
    }

    /// Centros de `n` alvos lado a lado, a partir de `inicio`, com o passo dado.
    public static func centros(_ n: Int, passo: CGFloat, inicio: CGFloat = 0) -> [CGFloat] {
        (0..<max(0, n)).map { inicio + passo / 2 + CGFloat($0) * passo }
    }
}

/// Cache do markdown dos balões, por id e revisão da entrada (e se é o texto cortado ou o inteiro): redesenhar a
/// conversa (cada tecla, cada evento) não reinterpreta texto que não mudou.
@MainActor
enum MXCacheDeTexto {
    private static var itens: [String: AttributedString] = [:]
    /// Quantas vezes interpretou markdown (testes).
    private(set) static var interpretacoes = 0
    static let maximo = 500

    static func rico(_ texto: String, chave: String) -> AttributedString {
        guard !chave.isEmpty else { return interpretar(texto) }
        if let pronto = itens[chave] { return pronto }
        if itens.count >= maximo { itens.removeAll(keepingCapacity: true) }
        let rico = interpretar(texto)
        itens[chave] = rico
        return rico
    }

    private static func interpretar(_ texto: String) -> AttributedString {
        interpretacoes += 1
        return MXConversaRegras.textoRico(texto)
    }

    static func limpar() { itens.removeAll() }
}
