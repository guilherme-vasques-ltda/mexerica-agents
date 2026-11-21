import CodyncKit
import Foundation
import Observation

/// Mapeamento puro dos modelos do app (`Bot`, `Entry`) para o que as telas da etapa 2 mostram. Testado em
/// `RedesignConversaTests` com entradas montadas à mão; nada aqui fala com o Hub.
public enum MXMapeamento {
    /// O Berga é o bot principal; o Hub reserva o nome "Berga" para ele (`donna_reserved`). O app ainda não recebe
    /// o `primaryBotId`, então reconhece pelo nome reservado.
    public static func ehBerga(_ bot: Bot) -> Bool { !bot.isGroup && bot.name == "Berga" }

    /// "Pedir" (ask) ou "Liberar" (auto).
    public static func autonomia(_ bot: Bot) -> String? {
        switch bot.permission {
        case "ask": return String(localized: "Pedir", bundle: .module)
        case "auto": return String(localized: "Liberar", bundle: .module)
        default: return nil
        }
    }

    /// "Claude Code · opus"; só o motor quando não há modelo escolhido.
    public static func modelo(motor: String, modelo: String?) -> String? {
        let m = modelo?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if motor.isEmpty { return m.isEmpty ? nil : m }
        return m.isEmpty ? motor : "\(motor) · \(m)"
    }

    /// Há um cartão de aprovação esperando nesta conversa.
    public static func temAprovacaoPendente(_ entries: [Entry]) -> Bool {
        entries.contains { $0.kind == "permission" && $0.data.status == "pending" }
    }

    /// A prévia e o tom da linha do agente: o que pede você primeiro, depois o que está fazendo, depois a última fala.
    public static func previa(_ bot: Bot, aprovacaoPendente: Bool) -> (texto: String, tom: MXTomDaPrevia) {
        if aprovacaoPendente { return (String(localized: "Precisa da sua aprovação", bundle: .module), .alerta) }
        if bot.hasPendingQuestions { return (String(localized: "Tem uma pergunta para você", bundle: .module), .alerta) }
        if bot.needsInput { return (bot.activity.isEmpty ? String(localized: "Precisa de você", bundle: .module) : bot.activity, .alerta) }
        if bot.isWorking { return (bot.activity.isEmpty ? String(localized: "Trabalhando…", bundle: .module) : bot.activity, .ativo) }
        let ultima = MessagePreview.text(bot.lastMessage ?? "")
        if bot.status == "error" { return (ultima.isEmpty ? String(localized: "Algo deu errado", bundle: .module) : ultima, .normal) }
        return (ultima.isEmpty ? String(localized: "Sem mensagens ainda", bundle: .module) : ultima, .normal)
    }

    /// Uma linha de agente. Contagem de sessões rodando e esperando: o Hub ainda não manda (pílulas ocultas, 0).
    /// Função: `Bot.title` do Hub (vazia, a etiqueta fica oculta).
    public static func agente(_ bot: Bot, aprovacaoPendente: Bool, agora: Date, calendario: Calendar = .current) -> MXAgenteResumo {
        let p = previa(bot, aprovacaoPendente: aprovacaoPendente)
        return MXAgenteResumo(id: bot.id, nome: bot.name, funcao: funcaoDoHub(bot), rosto: MXRosto(bot.avatar),
                              estado: bot.avatarState(now: agora),
                              hora: MXHora.naLista(Date(milliseconds: bot.lastAt), agora: agora, calendario: calendario),
                              previa: p.texto, tom: p.tom, sessoesRodando: 0, sessoesEsperando: 0, naoLidas: max(0, bot.unread))
    }

    /// Um grupo do app vira um canal (vários agentes e você numa conversa).
    public static func canal(_ bot: Bot) -> MXCanalResumo {
        MXCanalResumo(id: bot.id, nome: bot.name, naoLidas: max(0, bot.unread), sessoesRodando: 0, sessoesEsperando: 0)
    }

    public static func berga(_ bot: Bot, space: String, aprovacaoPendente: Bool, agora: Date) -> MXBergaResumo {
        let estado = bot.avatarState(now: agora)
        return MXBergaResumo(id: bot.id, nome: bot.name, space: space, rosto: MXRosto(bot.avatar), estado: estado,
                             estadoTexto: MXEstadoRegras.texto(estado),
                             mensagem: previa(bot, aprovacaoPendente: aprovacaoPendente).texto)
    }

    /// A posição da mensagem na fila do Hub (`EntryData.queueOrder`); nula de um Hub antigo.
    public static func ordemDoHub(_ e: Entry) -> Int64? { e.data.queueOrder }

    /// A função do agente ("Dev", "Desenvolvedor"): `Bot.title` do Hub. Vazia, a etiqueta fica oculta.
    public static func funcaoDoHub(_ bot: Bot) -> String { bot.title.trimmingCharacters(in: .whitespacesAndNewlines) }

    /// As mensagens na fila, na ordem do Hub (`Entry.ordenadaPelaFila(_:)`: `queueOrder`, `seq` quando ausente,
    /// empate por `seq`). A ordem só é confiável quando todas trazem a ordem do Hub (ou há uma só): sem ela (Hub
    /// antigo), `seq` é só uma reserva, e reordenar no Mac muda a ordem de verdade sem o iPhone saber; aí a posição
    /// numerada não aparece.
    public static func ordenarFila(_ entries: [Entry]) -> (itens: [MXItemDaFila], confiavel: Bool) {
        let naFila = Entry.ordenadaPelaFila(entries.filter { $0.kind == "user" && $0.data.status == "queued" })
        let confiavel = naFila.count <= 1 || naFila.allSatisfy { ordemDoHub($0) != nil }
        return (naFila.map { MXItemDaFila(id: $0.id, texto: $0.data.text ?? "", revisao: $0.rev) }, confiavel)
    }

    /// As mensagens na fila desta conversa, na ordem do Hub.
    public static func fila(_ entries: [Entry]) -> [MXItemDaFila] { ordenarFila(entries).itens }

    // MARK: Passos

    /// Nome do tipo de passo no resumo ("Leitura, Edição, Comando").
    public static func tipoDoPasso(_ toolKind: String?) -> String {
        switch toolKind {
        case "read": String(localized: "Leitura", bundle: .module)
        case "edit": String(localized: "Edição", bundle: .module)
        case "execute": String(localized: "Comando", bundle: .module)
        case "search": String(localized: "Busca", bundle: .module)
        case "fetch": String(localized: "Web", bundle: .module)
        case "delete": String(localized: "Exclusão", bundle: .module)
        case "move": String(localized: "Mudança", bundle: .module)
        case "think": String(localized: "Raciocínio", bundle: .module)
        default: String(localized: "Ferramenta", bundle: .module)
        }
    }

    /// O caminho mais curto de mostrar: relativo à pasta do agente quando está dentro dela.
    static func caminho(_ path: String, pasta: String) -> String {
        guard !pasta.isEmpty else { return path }
        let base = pasta.hasSuffix("/") ? pasta : pasta + "/"
        return path.hasPrefix(base) ? String(path.dropFirst(base.count)) : path
    }

    /// A frase de um passo: "Leu src/pace.js", "Rodou npm test".
    public static func textoDoPasso(_ e: Entry, pasta: String) -> String {
        let d = e.data
        let alvo = d.locations?.first.map { caminho($0.path, pasta: pasta) }
        let titulo = d.title?.trimmingCharacters(in: .whitespacesAndNewlines).nilSeVazio
        switch d.toolKind {
        case "read": return String(localized: "Leu \(alvo ?? titulo ?? String(localized: "um arquivo", bundle: .module))", bundle: .module)
        case "edit": return String(localized: "Editou \(alvo ?? titulo ?? String(localized: "um arquivo", bundle: .module))", bundle: .module)
        case "delete": return String(localized: "Apagou \(alvo ?? titulo ?? String(localized: "um arquivo", bundle: .module))", bundle: .module)
        case "move": return String(localized: "Moveu \(alvo ?? titulo ?? String(localized: "um arquivo", bundle: .module))", bundle: .module)
        case "search": return String(localized: "Buscou \(titulo ?? alvo ?? String(localized: "nos arquivos", bundle: .module))", bundle: .module)
        case "fetch": return String(localized: "Abriu \(titulo ?? String(localized: "uma página", bundle: .module))", bundle: .module)
        case "execute": return String(localized: "Rodou \(d.command?.trimmingCharacters(in: .whitespacesAndNewlines).nilSeVazio ?? titulo ?? String(localized: "um comando", bundle: .module))", bundle: .module)
        default: return titulo ?? String(localized: "Usou uma ferramenta", bundle: .module)
        }
    }

    /// "2 min" ou "40 s"; nulo sem duração.
    public static func duracao(_ ms: Int64) -> String? {
        guard ms >= 1000 else { return nil }
        let s = ms / 1000
        return s < 60 ? "\(s) s" : "\(s / 60) min"
    }

    /// Os passos de uma resposta já dada.
    public static func passos(_ ferramentas: [Entry], pasta: String) -> MXPassos? {
        guard !ferramentas.isEmpty else { return nil }
        var tipos: [String] = []
        for f in ferramentas {
            let t = tipoDoPasso(f.data.toolKind)
            if !tipos.contains(t) { tipos.append(t) }
        }
        let inicio = ferramentas.map(\.createdAt).min() ?? 0
        let fim = ferramentas.map { max($0.updatedAt, $0.createdAt) }.max() ?? inicio
        let partes = [MXConversaRegras.passos(ferramentas.count), tipos.joined(separator: ", ")] + [duracao(fim - inicio)].compactMap { $0 }
        return MXPassos(resumo: partes.joined(separator: " · "), itens: itensDosPassos(ferramentas, pasta: pasta))
    }

    /// Os passos do trabalho em curso: "Trabalhando · 1 passo".
    public static func passosEmCurso(_ ferramentas: [Entry], pasta: String) -> MXPassos? {
        guard !ferramentas.isEmpty else { return nil }
        return MXPassos(resumo: String(localized: "Trabalhando · \(MXConversaRegras.passos(ferramentas.count))", bundle: .module), itens: itensDosPassos(ferramentas, pasta: pasta))
    }

    static func itensDosPassos(_ ferramentas: [Entry], pasta: String) -> [MXPasso] {
        ferramentas.map { MXPasso(id: $0.id, texto: textoDoPasso($0, pasta: pasta), concluido: $0.data.status == "completed") }
    }

    // MARK: Aprovação

    /// "quer mudar arquivos" etc. (textos do Mac, `conversa.aprovacao.pedidoDe.*`).
    public static func pedido(_ toolKind: String?) -> String {
        switch toolKind {
        case "execute": String(localized: "quer rodar um comando", bundle: .module)
        case "edit", "delete", "move": String(localized: "quer mudar arquivos", bundle: .module)
        case "fetch": String(localized: "quer acessar a web", bundle: .module)
        case "read", "search": String(localized: "quer ler arquivos", bundle: .module)
        case "mcp_memory": String(localized: "quer usar uma ferramenta de memória", bundle: .module)
        default: String(localized: "quer usar uma ferramenta", bundle: .module)
        }
    }

    /// A linha "Ação" (textos do Mac, `conversa.aprovacao.acao.*`).
    public static func acao(_ toolKind: String?) -> String {
        switch toolKind {
        case "execute": String(localized: "Comando", bundle: .module)
        case "edit": String(localized: "Edição de arquivos", bundle: .module)
        case "delete": String(localized: "Apagar arquivos", bundle: .module)
        case "move": String(localized: "Mover arquivos", bundle: .module)
        case "fetch": String(localized: "Acesso à web", bundle: .module)
        case "read": String(localized: "Leitura", bundle: .module)
        case "search": String(localized: "Busca", bundle: .module)
        case "mcp_memory": String(localized: "Ferramenta de memória", bundle: .module)
        default: String(localized: "Ferramenta", bundle: .module)
        }
    }

    static func naSessao(_ o: PermissionOption) -> Bool {
        o.kind == "allow_always" && ["allow_for_session", "allow_session", "allow_permissions_session"].contains(o.optionId)
    }

    /// O rótulo da opção (Mac: `conversa.aprovacao.opcao.*`).
    public static func rotulo(_ o: PermissionOption) -> String {
        if naSessao(o) { return String(localized: "Permitir nesta sessão", bundle: .module) }
        switch o.kind {
        case "allow_once": return String(localized: "Permitir uma vez", bundle: .module)
        case "allow_always": return String(localized: "Sempre permitir", bundle: .module)
        case "reject_once": return String(localized: "Recusar", bundle: .module)
        case "reject_always": return String(localized: "Nunca", bundle: .module)
        default: return o.name
        }
    }

    /// O resultado de um pedido já respondido (Mac: `conversa.aprovacao.resultado.*`).
    public static func resultado(_ e: Entry) -> MXResultadoDaAprovacao {
        let d = e.data
        switch d.status {
        case "pending": return .pendente
        case "answered":
            let escolhida = d.options?.first { $0.optionId == d.selected }
            if let escolhida, naSessao(escolhida) { return .permitido(String(localized: "Permitido nesta sessão", bundle: .module)) }
            switch escolhida?.kind {
            case "allow_once": return .permitido(String(localized: "Permitido uma vez", bundle: .module))
            case "allow_always": return .permitido(String(localized: "Sempre permitido", bundle: .module))
            case "reject_once": return .recusado(String(localized: "Recusado", bundle: .module))
            case "reject_always": return .recusado(String(localized: "Nunca permitido", bundle: .module))
            default: return .encerrado(escolhida?.name ?? String(localized: "Respondido", bundle: .module))
            }
        case "cancelled": return .encerrado(String(localized: "Cancelado", bundle: .module))
        default: return .encerrado(String(localized: "Expirou — o bot seguiu sem a resposta", bundle: .module))
        }
    }

    /// A resposta confere com o que a pessoa viu: mesma entrada e revisão, ainda pendente, a opção existe e as opções
    /// são as mesmas da tela; permitir um pedido grande demais para conferir no iPhone nunca confere.
    public static func respostaConfere(_ e: Entry, apresentado p: MXPedidoDeAprovacao, opcao: String) -> Bool {
        let opcoes = e.data.options ?? []
        guard e.id == p.id, e.rev == p.revisao, e.kind == "permission", e.data.status == "pending", p.pendente,
              opcoes.contains(where: { $0.optionId == opcao }),
              Set(opcoes.map(\.optionId)) == Set(p.opcoes.map(\.id)) else { return false }
        let permite = p.opcoes.first { $0.id == opcao }?.permite ?? true
        return !(permite && p.bloqueadoPorTamanho)
    }

    /// O cartão: as opções do Hub na ordem do Mac (permitir uma vez, sempre, recusar, nunca); a permissão mais
    /// estreita é a principal. Mesmo pedido, mesmas opções e mesma resposta do app atual.
    public static func aprovacao(_ e: Entry, autor: String, computador: String, respondendo: String?) -> MXPedidoDeAprovacao {
        let d = e.data
        let ordem = ["allow_once": 0, "allow_always": 1, "reject_once": 2, "reject_always": 3]
        let opcoes = (d.options ?? []).sorted { (ordem[$0.kind] ?? 9) < (ordem[$1.kind] ?? 9) }
        let principal = (opcoes.first { $0.kind == "allow_once" } ?? opcoes.first { $0.kind.hasPrefix("allow") })?.optionId
        let pasta = d.cwd.map { ($0 as NSString).lastPathComponent }.flatMap(\.nilSeVazio)
        // Inteiros (a prévia e o "Mostrar tudo" ficam com o cartão); nada é descartado antes de autorizar.
        let detalhes = [d.command, d.detail].compactMap { $0?.nilSeVazio }.map { MXTextoLongo($0) }

        return MXPedidoDeAprovacao(
            id: e.id,
            revisao: e.rev,
            titulo: "\(autor) \(pedido(d.toolKind))",
            detalhe: d.title?.nilSeVazio.map { MXTextoLongo($0) },
            acao: acao(d.toolKind),
            onde: [computador, pasta].compactMap { $0 }.joined(separator: " · "),
            opcoes: opcoes.map { MXOpcaoDeAprovacao(id: $0.optionId, rotulo: rotulo($0), permite: $0.kind.hasPrefix("allow"), principal: $0.optionId == principal) },
            resultado: resultado(e),
            respondendo: respondendo,
            detalhes: detalhes,
            diffs: (d.diffs ?? []).enumerated().map { MXDiffPrevia($0.element, indice: $0.offset) }
        )
    }

    // MARK: Conversa

    /// A nota embaixo do balão do usuário pelo estado da entrega.
    public static func nota(_ status: String?) -> (texto: String?, reenviar: Bool) {
        switch status {
        case "sending": return (String(localized: "Enviando…", bundle: .module), false)
        case "failed": return (String(localized: "Não enviada", bundle: .module), true)
        case "waiting": return (String(localized: "Esperando o computador ligar", bundle: .module), false)
        case "delivering": return (String(localized: "Entregue ao computador", bundle: .module), false)
        case "cancelled": return (String(localized: "Não enviada — parada", bundle: .module), false)
        default: return (nil, false)
        }
    }

    /// Mais de uma hora entre duas mensagens: chip de hora (como o app atual).
    static let intervaloDoSeparador: TimeInterval = 3600

    /// As linhas da Conversa a partir das entradas do bot: balões, passos recolhidos, cartões de aprovação,
    /// avisos, chips de hora e, no fim, o balão de quem trabalha. As mensagens na fila ficam fora (vão para a fila).
    /// `mostraTrabalho`: o balão "Trabalhando…" do turno em curso. Numa sessão extra, falso: o Hub não diz em que
    /// sessão o agente trabalha, e o trabalho seria o da Principal.
    public static func itens(_ entries: [Entry], bot: Bot, computador: String, respondendo: [String: String],
                             agora: Date, calendario: Calendar = .current, mostraTrabalho: Bool = true) -> [MXItemDaConversa] {
        let ferramentas = entries.filter { $0.kind == "tool" }.sorted { $0.seq < $1.seq }
        let porTurno = Dictionary(grouping: ferramentas, by: \.turn)
        var usadas = Set<String>()
        var itens: [MXItemDaConversa] = []
        var ultimaData: Date?
        var anteriorFoiAgente = false

        for e in entries {
            let conteudo: MXConteudo?
            switch e.kind {
            case "user":
                guard e.data.status != "queued" else { continue }
                let nomes = (e.data.attachments ?? []).map(\.name)
                let texto = e.data.text?.nilSeVazio ?? nomes.joined(separator: ", ")
                let n = nota(e.data.status)
                conteudo = .usuario(MXMensagemDoUsuario(texto: texto, nota: n.texto, podeReenviar: n.reenviar))
            case "agent" where e.data.final == true:
                let minhas = (porTurno[e.turn] ?? []).filter { $0.seq < e.seq && !usadas.contains($0.id) }
                usadas.formUnion(minhas.map(\.id))
                conteudo = .agente(MXMensagemDoAgente(autor: bot.name, hora: MXHora.relogio(e.date, calendario: calendario),
                                                      cabecalho: !anteriorFoiAgente, texto: e.data.text ?? "",
                                                      passos: passos(minhas, pasta: bot.cwd), chave: "\(e.id)#\(e.rev)"))
            case "permission":
                conteudo = .aprovacao(aprovacao(e, autor: bot.name, computador: computador, respondendo: respondendo[e.id]))
            case "notice" where e.isChat:
                let texto = (e.data.heading ?? e.data.text)?.nilSeVazio
                let erro = e.data.style == "error"
                conteudo = texto.map { MXConteudo.aviso($0, erro: erro) }
            default:
                conteudo = nil
            }
            guard let conteudo else { continue }
            if ultimaData.map({ e.date.timeIntervalSince($0) > intervaloDoSeparador }) ?? true {
                itens.append(MXItemDaConversa(id: "sep-\(e.id)", .separador(MXHora.separador(e.date, agora: agora, calendario: calendario))))
                anteriorFoiAgente = false
            }
            ultimaData = e.date
            itens.append(MXItemDaConversa(id: e.id, conteudo))
            if case .agente = conteudo { anteriorFoiAgente = true } else { anteriorFoiAgente = false }
        }

        if mostraTrabalho, bot.isWorking(in: bot.id) {
            // Os passos do turno em curso: o mais recente com ferramentas ainda sem resposta final. (Não o maior
            // turno da conversa: mensagens na fila já ganham turno ao chegar.)
            let ultimaResposta = entries.filter { $0.kind == "agent" && $0.data.final == true }.map(\.turn).max() ?? Int64.min
            let soltas = ferramentas.filter { !usadas.contains($0.id) && $0.turn > ultimaResposta }
            let turno = soltas.map(\.turn).max()
            let emCurso = soltas.filter { $0.turn == turno }
            let esperando = temAprovacaoPendente(entries)
            let atividade = bot.activity.isEmpty ? String(localized: "Trabalhando…", bundle: .module) : bot.activity
            itens.append(MXItemDaConversa(id: "trabalhando-\(bot.id)", .trabalhando(MXTrabalhoEmCurso(
                autor: bot.name, atividade: atividade, passos: passosEmCurso(emCurso, pasta: bot.cwd), esperandoDecisao: esperando))))
        }
        return itens
    }

    /// A Conversa inteira de um bot.
    /// `sessao`: uma sessão extra (07b), com as entradas dela; o id da conversa vira o de `MXIdDaConversa`. O Hub não
    /// diz em que sessão o agente trabalha: numa extra, "trabalhando" fica falso (a mensagem entra na fila do
    /// mesmo jeito, se for o caso). `semEnvio`: por que a caixa fica desligada.
    public static func conversa(_ bot: Bot, entries: [Entry], computador: String, motor: String,
                                respondendo: [String: String], agora: Date, calendario: Calendar = .current,
                                sessao: String? = nil, semEnvio: String? = nil) -> MXConversaDados {
        let fila = ordenarFila(entries)
        let id = MXIdDaConversa(agente: bot.id, sessao: sessao)
        return MXConversaDados(
            id: id.texto, agenteId: bot.id, nome: bot.name, funcao: funcaoDoHub(bot), rosto: MXRosto(bot.avatar),
            estado: bot.avatarState(now: agora), autonomia: autonomia(bot), fixado: bot.pinned,
            itens: itens(entries, bot: bot, computador: computador, respondendo: respondendo, agora: agora, calendario: calendario,
                         mostraTrabalho: id.sessao == nil),
            fila: fila.itens, trabalhando: id.sessao == nil && bot.isWorking(in: bot.id), esperandoDecisao: temAprovacaoPendente(entries),
            modelo: modelo(motor: motor, modelo: bot.model), contexto: nil, ordemDaFilaConfiavel: fila.confiavel,
            semEnvio: semEnvio
        )
    }

    /// Por que a caixa de uma sessão extra não manda agora (nulo: manda). Só vale a leitura das sessões feita na MESMA
    /// ligação de agora (mesma geração do link, como na fila, no salvar e no fixar); numa ligação nova, espera outra.
    static func semEnvioNaSessao(_ leitura: MXLeituraDeSessoes?, geracaoAtual: AnyHashable?, storeConfirma: Bool) -> String? {
        guard let leitura, mesmoLink(leitura.geracao, geracaoAtual) else { return MXTextoDasSessoes.conferindoEnvio }
        switch leitura.resultado {
        case .anunciaEnvio: return storeConfirma ? nil : MXTextoDasSessoes.conferindoEnvio
        case .semEnvio: return MXTextoDasSessoes.envioNaoConfirmado
        case .falhou: return MXTextoDasSessoes.sessoesNaoVieram
        }
    }

    /// Quantas entradas o Hub manda por página de histórico (`HostClient.history`, `limit` 100): menos que isso, acabou.
    static let paginaDoHistorico = 100

    /// O estado da ligação a partir do da queda curta do store: `BotStore.estadoConexao` e
    /// `BotStore.temAutoridade`. Conectado só com autoridade (link de pé e Hub identificado nele: enviar e aprovar
    /// valem); `.conectado` sem autoridade ainda (o cliente é nulo até o `hello`) conta como reconectando.
    public static func conexao(_ estado: EstadoConexao, temAutoridade: Bool, agora: Date) -> MXEstadoConexao {
        if temAutoridade { return .conectado }
        switch estado {
        case .conectado: return .reconectando(desde: agora)
        case let .reconectando(desde): return .reconectando(desde: desde)
        case .desconectado: return .desconectado
        }
    }

    /// Fixar voltou: a resposta só entra se não for mais velha que o bot que já está no espelho (um evento mais novo
    /// pode ter chegado durante a espera).
    public static func aplicaRespostaDeFixar(atualRev: Int64?, respostaRev: Int64) -> Bool {
        guard let atualRev else { return false }
        return respostaRev >= atualRev
    }

    /// Fixar falhou: o valor a pôr de volta, ou nulo para deixar como está. Volta ao de antes só se nada mais novo
    /// chegou (mesma revisão do começo) e o valor ainda é o que pedimos.
    public static func desfazFixar(atual: Bool, atualRev: Int64, pedido: Bool, anterior: Bool, revInicial: Int64) -> Bool? {
        guard atualRev == revInicial, atual == pedido else { return nil }
        return anterior
    }

    /// A chamada voltou pelo mesmo link em que saiu (nenhuma queda e volta no meio).
    public static func mesmoLink(_ antes: AnyHashable?, _ depois: AnyHashable?) -> Bool {
        antes != nil && antes == depois
    }

    /// O aviso de uma chamada que falhou: se o link do toque caiu ou mudou, a causa é a ligação ("Sem conexão — tente
    /// de novo."), e não o Hub; só no mesmo link vale o aviso da própria ação.
    public static func avisoDaFalha(tocada: AnyHashable?, agora: AnyHashable?, daAcao: String) -> String {
        mesmoLink(tocada, agora) ? daAcao : MXTextoDeQueda.linkMudou
    }

    /// A fronteira do histórico carregado: o menor `seq` do Hub (o mesmo critério do `loadOlder` do store: sem as
    /// entradas locais, que têm `seq` 0 ou `Int64.max`).
    public static func fronteiraDoHistorico(_ entries: [Entry]) -> Int64? {
        entries.lazy.map(\.seq).filter { $0 > 0 && $0 != Int64.max }.min()
    }

    /// Contadores da barra: Conversas = conversas com não lidas; Mission Board = agentes que esperam você.
    public static func pendencias(_ bots: [Bot]) -> [MXAba: Int] {
        let agentes = bots.filter { !$0.isGroup }
        return [
            .conversas: bots.filter { $0.unread > 0 }.count,
            .missionBoard: agentes.filter { $0.needsInput || $0.hasPendingQuestions }.count,
        ]
    }
}

extension String {
    /// Nulo quando vazio.
    var nilSeVazio: String? { isEmpty ? nil : self }
}

/// A fonte real: lê e age pelo `BotStore` do computador (o mesmo do app atual), sem mudar ele.
///
/// O que o Hub ainda não manda fica oculto aqui: contagem de sessões rodando/esperando por agente (pílulas), a função
/// do agente (etiqueta), a pelúcia gerada sob medida (usa o conjunto de reserva, como o app atual), o % de contexto.
@MainActor
@Observable
public final class MXFonteReal: MXFonte {
    let store: BotStore
    /// O que existe de verdade hoje: o que só levaria a "Em breve" fica desligado nas telas (`MXCapacidades.real`).
    public let capacidades = MXCapacidades.real
    /// O relógio, trocável nos testes.
    @ObservationIgnored var agora: () -> Date = { Date() }
    #if DEBUG
    // Ganchos de teste (os testes rodam em Debug); em Release não existem.
    /// Força o estado da ligação.
    @ObservationIgnored var conexaoForcada: MXEstadoConexao?
    /// Troca a chamada da fila ao Hub e a do fixar (transporte falso).
    @ObservationIgnored var chamarFila: (@MainActor (String, MXPedidoDaFila) async throws -> MXRespostaDaFila)?
    @ObservationIgnored var atualizarBot: (@MainActor (MXPatchDoBot) async throws -> Bot)?
    /// A geração do link quando a conexão é forçada (mudar simula uma queda e volta no meio da chamada).
    @ObservationIgnored var geracaoForcada = 0
    /// Troca a leitura `sessions` do Hub (transporte falso).
    @ObservationIgnored var lerSessoes: (@MainActor (String) async throws -> MXRespostaDeSessoes)?
    /// Troca o `deleteBot` do Hub (o pedido que sairia; a resposta dele).
    @ObservationIgnored var excluirNoHub: (@MainActor (String) async throws -> Void)?
    /// Troca a leitura do histórico de uma sessão extra (agente, sessão, antes de): devolve a página. Sem o número bruto
    /// de linhas, vale a contagem das entradas devolvidas (o mesmo que `lerPaginaDaSessao` com `rawCount` igual a ela).
    @ObservationIgnored var lerHistoricoDaSessao: (@MainActor (String, String, Int64) async throws -> [Entry])?
    /// Como `lerHistoricoDaSessao`, mas devolve também o número BRUTO de linhas que o Hub mandou (antes de descartar as de
    /// `sessionId` inválido e filtrar as de outra sessão). Tem a prioridade sobre `lerHistoricoDaSessao`.
    @ObservationIgnored var lerPaginaDaSessao: (@MainActor (String, String, Int64) async throws -> BotStore.PaginaDeHistorico)?
    #endif
    /// As sessões que o Hub mandou (`sessions {botId}`), por agente; ausente = ainda não leu.
    private(set) var sessoesDoHub: [String: [MXSessaoDoHub]] = [:]
    /// Agentes cujo computador não respondeu `sessions` (Hub antigo ou falha): só a Principal aparece, com a nota.
    private(set) var sessoesFalharam: Set<String> = []
    /// A última leitura das sessões de cada agente: em que ligação (geração do link) e se anunciou o envio por sessão.
    /// O anúncio vale só na ligação em que foi lido; numa nova, a caixa das extras espera outra leitura.
    private(set) var leiturasDeSessoes: [String: MXLeituraDeSessoes] = [:]
    /// Rascunho da caixa de cada sessão extra (o do store é por agente, o da Principal).
    private var rascunhosDeSessao: [String: String] = [:]
    /// O que foi colado (e não digitado) em cada caixa de sessão extra, como o `composerPasted` do store faz na Principal:
    /// vai junto no envio, para a memória não tomar como palavras da pessoa.
    private var coladosDeSessao: [String: [String]] = [:]
    /// Sessões extras cujo histórico já chegou ao começo, com a marca do link em que isso foi visto. O fim só vale com
    /// evidência de continuidade: a marca tem de ser a do link de agora (reconectar ou trocar de Hub muda a marca, e o
    /// fim visto antes não vale; nada se presume pelo cache) e uma releitura com página bruta cheia (100) remove o fim.
    /// Quem reafirma o fim é uma leitura de página bruta curta (< 100) neste link (`abrindo` relê a página mais nova).
    private(set) var historicoCompletoDaSessao: [String: MarcaDoLink] = [:]
    /// A trava do Excluir por agente: só o resultado do próprio pedido mexe nela (`MXExclusoes`).
    private(set) var exclusoes = MXExclusoes()
    private(set) var avisoProprio: String?
    /// Salvar da fila a caminho, por agente e mensagem (observado: a tela desliga o ✓ e o Editar dela). Mora aqui, e
    /// não na tela, para sobreviver a sair e reabrir a conversa.
    private(set) var salvamentos = MXSalvamentosPendentes()
    /// Fixar em curso por bot (a identidade da operação).
    @ObservationIgnored private var fixando: [String: UUID] = [:]

    public init(store: BotStore) {
        self.store = store
    }

    /// A ligação, da queda curta do store (`estadoConexao` e `temAutoridade`; `MXMapeamento.conexao`).
    public var conexao: MXEstadoConexao {
        #if DEBUG
        if let conexaoForcada { return conexaoForcada }
        #endif
        return MXMapeamento.conexao(store.estadoConexao, temAutoridade: store.temAutoridade, agora: agora())
    }

    /// A faixa "Sem conexão — reconectando…" aparece depois da carência da queda (`BotStore.faixaDeQueda`).
    public var faixaDeQueda: Bool { store.faixaDeQueda }
    public var podeTentarAgora: Bool { store.podeTentarAgora }
    public func tentarAgora() { store.tentarAgora() }

    /// O cliente das chamadas diretas ao Hub (fila, fixar), pego do store NA HORA de cada ação, nunca guardado em
    /// estado de tela: o do canal em que o Hub se identificou, só com autoridade (como `AgentModelPicker`).
    /// Cada chamada dele sai só nesse canal: se o link cair, ela falha em vez de seguir para o próximo.
    func clienteComAutoridade() -> HostClient? { store.temAutoridade ? store.client : nil }

    /// A geração do link com autoridade: o `linkId` do store, que muda quando um link acaba ou o Hub se identifica
    /// num novo. Nula sem autoridade. Guardada no toque: dá o aviso certo ("o link mudou") antes de chamar e descarta
    /// a resposta que volta depois de uma troca de link (o próprio transporte já recusa a chamada em outro canal).
    func geracaoDoLink() -> AnyHashable? {
        #if DEBUG
        if let conexaoForcada { return conexaoForcada.podeAgir ? AnyHashable(geracaoForcada) : nil }
        #endif
        guard clienteComAutoridade() != nil else { return nil }
        return AnyHashable(store.linkId)
    }

    /// Revalida a ligação no instante de despachar e devolve a geração do link: sem link no ar, a intenção não fica
    /// guardada para depois de reconectar (a pessoa pede de novo) e o aviso diz por quê.
    private func despachar(_ motivo: String) -> AnyHashable? {
        guard conexao.podeAgir, let geracao = geracaoDoLink() else {
            avisoProprio = motivo
            return nil
        }
        avisoProprio = nil
        return geracao
    }

    /// O Space que este app enxerga: o pessoal (as chamadas antigas do app vão para ele).
    public var space: String { String(localized: "Pessoal", bundle: .module) }

    public var lista: MXConversasDados {
        let agora = agora()
        let bots = store.roster
        let pendente = { (id: String) in MXMapeamento.temAprovacaoPendente(self.store.chat(id)) }
        let berga = bots.first(where: MXMapeamento.ehBerga)
        return MXConversasDados(
            berga: berga.map { MXMapeamento.berga($0, space: space, aprovacaoPendente: pendente($0.id), agora: agora) },
            space: space,
            canais: bots.filter(\.isGroup).map(MXMapeamento.canal),
            agentes: bots.filter { !$0.isGroup && !MXMapeamento.ehBerga($0) }
                .map { MXMapeamento.agente($0, aprovacaoPendente: pendente($0.id), agora: agora) }
        )
    }

    public var pendencias: [MXAba: Int] {
        var valor = MXMapeamento.pendencias(store.roster)
        #if DEBUG
        // Mission Board v1: o contador é o do quadro (inclui o cartão de permissão pendente da conversa Principal e fica em
        // zero quando o quadro não mostra dado nenhum: sem acesso, ilegível…).
        valor[.missionBoard] = quadro.contador
        #endif
        return valor
    }

    public var aviso: String? { avisoProprio ?? store.lastError }

    /// A Principal: `store.chat(id)` (só a Principal). Uma sessão extra: `store.chat(id, sessionId:)`,
    /// com o mesmo mapeamento; a caixa desliga se o computador não confirmou o envio por sessão.
    public func conversa(_ id: String) -> MXConversaDados? {
        let c = MXIdDaConversa(id)
        guard let bot = store.bots[c.agente] else { return nil }
        return MXMapeamento.conversa(bot, entries: store.chat(c.agente, sessionId: c.sessao), computador: store.hostName,
                                     motor: store.backendName(bot.backend), respondendo: store.answering, agora: agora(),
                                     sessao: c.sessao, semEnvio: c.sessao == nil ? nil : semEnvioNaSessao(c.agente))
    }

    /// O envio a uma sessão extra vale só com o envio por sessão anunciado NESTA ligação (a leitura de `sessions` com
    /// "send", na mesma geração do link de agora) e guardado pelo store. Sem isso, a caixa fica desligada com o motivo:
    /// um Hub antigo ignora `send.sessionId` e a mensagem cairia na Principal.
    func semEnvioNaSessao(_ agente: String) -> String? {
        MXMapeamento.semEnvioNaSessao(leiturasDeSessoes[agente], geracaoAtual: geracaoDoLink(),
                                      storeConfirma: store.sessionSendSupported.contains(agente))
    }

    public func rascunho(_ id: String) -> String {
        let c = MXIdDaConversa(id)
        return c.sessao == nil ? store.composerDraft(for: id) : rascunhosDeSessao[id] ?? ""
    }

    public func guardarRascunho(_ texto: String, em id: String) {
        guard MXIdDaConversa(id).sessao == nil else {
            if texto.isEmpty {
                coladosDeSessao[id] = nil
            } else if let colado = pastedInsertion(from: rascunhosDeSessao[id] ?? "", to: texto) {
                coladosDeSessao[id, default: []].append(colado)
            }
            rascunhosDeSessao[id] = texto.isEmpty ? nil : texto
            return
        }
        guard texto != store.composerDraft(for: id) else { return }
        store.setComposerDraft(texto, for: id)
    }

    /// Numa sessão extra, com o link no ar: lista as sessões (o store guarda se o computador aceita mensagem em sessão;
    /// depois de reabrir o app, sem isso nada vai para a extra) e traz a página mais nova do histórico dela. A marca do
    /// link é lida aqui, antes de qualquer espera; uma resposta de outro link não entra.
    public func abrindo(_ id: String) async {
        let c = MXIdDaConversa(id)
        guard let sessao = c.sessao, conexao.podeAgir, let geracao = geracaoDoLink() else { return }
        let marca = store.marcaDoLink
        await carregarSessoes(c.agente, marca: marca, geracao: geracao)
        do {
            let pagina = try await lerHistorico(c.agente, sessao: sessao, antesDe: .max, marca: marca)
            registrarFimDoHistorico(id, pagina: pagina, marca: marca)
        } catch {
            guard marca == store.marcaDoLink else { return }
            avisoProprio = MXTextoDasSessoes.historicoNaoVeio
        }
    }

    /// O mesmo mecanismo da conversa atual (`ReadingConversation` → `setReading`): marca como lido ao ficar visível e
    /// de novo a cada mensagem que chega enquanto a tela está aberta e ativa (inclusive depois de reconectar). Só na
    /// Principal: o store não tem lido por sessão (uma extra aberta não marca a Principal como lida).
    public func lendo(_ id: String, token: UUID, ativo: Bool) {
        guard MXIdDaConversa(id).sessao == nil else { return }
        store.setReading(token, botId: id, active: ativo)
    }

    public func podeCarregarAnteriores(_ id: String) -> Bool {
        let c = MXIdDaConversa(id)
        guard let sessao = c.sessao else { return store.canLoadOlder(id) }
        return !historicoAcabou(id)
            && store.chat(c.agente, sessionId: sessao).contains { $0.seq > 0 && $0.seq != Int64.max }
    }

    /// Só com o link no ar (nada fica pendurado esperando reconectar). O `loadOlder` do store engole o erro: deu certo
    /// se a fronteira do histórico recuou ou se o histórico acabou (não pela contagem: o stream pode trazer mensagem
    /// nova enquanto isso). Numa sessão extra, `loadHistory(_:sessionId:beforeSeq:marca:)`, com a marca do toque.
    public func carregarAnteriores(_ id: String) async -> Bool {
        guard conexao.podeAgir else { return false }
        let c = MXIdDaConversa(id)
        if let sessao = c.sessao {
            let marca = store.marcaDoLink
            let conhecida = store.chat(c.agente, sessionId: sessao).first(where: { $0.seq > 0 && $0.seq != Int64.max })
            let primeira: Int64 = conhecida?.seq ?? Int64.max
            do {
                let pagina = try await lerHistorico(c.agente, sessao: sessao, antesDe: primeira, marca: marca)
                registrarFimDoHistorico(id, pagina: pagina, marca: marca)
                return true
            } catch {
                return false
            }
        }
        let antes = MXMapeamento.fronteiraDoHistorico(store.chat(id))
        await store.loadOlder(id)
        return MXConversaRegras.carregou(fronteiraAntes: antes, fronteiraDepois: MXMapeamento.fronteiraDoHistorico(store.chat(id)),
                                         aindaHaMais: store.canLoadOlder(id))
    }

    // Enviar, reenviar, parar e aprovar passam pelas ações públicas do store (`send`/`retry`/`stop`/`respond`),
    // chamadas de forma síncrona no toque: o store lê o link ali mesmo e só manda nele (`ready()` não espera;
    // `withLink(link:)` só manda e repete nesse link).

    public func enviar(_ texto: String, para id: String) {
        guard despachar(MXTextoDeQueda.semEnvio) != nil else { return }
        let c = MXIdDaConversa(id)
        guard let sessao = c.sessao else {
            // Pelo rascunho do store: leva junto o que foi colado (a memória não toma como palavras do usuário).
            store.setComposerDraft(texto, for: id)
            store.sendComposerDraft(to: id)
            return
        }
        // Sessão extra: só com o envio por sessão confirmado (nunca para a Principal achando que foi para a extra); a
        // marca do link é a do toque. O store confere de novo e marca "não enviada" se o Hub gravar em outra sessão.
        if let motivo = semEnvioNaSessao(c.agente) {
            avisoProprio = motivo
            return
        }
        // Como na Principal, o texto passa pelo rascunho da caixa (a marcação de colado vem dele) antes de sair.
        guardarRascunho(texto, em: id)
        store.send(texto, to: c.agente, sessionId: sessao, pasted: coladosDeSessao[id] ?? [], marca: store.marcaDoLink)
        rascunhosDeSessao[id] = nil
        coladosDeSessao[id] = nil
    }

    public func reenviar(_ mensagem: String, em id: String) {
        let c = MXIdDaConversa(id)
        guard despachar(MXTextoDeQueda.semEnvio) != nil,
              let e = store.chat(c.agente, sessionId: c.sessao).first(where: { $0.id == mensagem }) else { return }
        if c.sessao != nil, let motivo = semEnvioNaSessao(c.agente) {
            avisoProprio = motivo
            return
        }
        store.retry(e)
    }

    /// Para o agente (o Hub não tem parar por sessão: numa extra, para o que o agente estiver fazendo).
    public func parar(_ id: String) {
        guard despachar(MXTextoDeQueda.semAcao) != nil else { return }
        store.stop(MXIdDaConversa(id).agente)
    }

    public func mandarAgora(_ item: MXItemDaFila, em id: String) {
        guard let geracao = despachar(MXTextoDeQueda.semAcao) else { return }
        let agente = MXIdDaConversa(id).agente
        Task { _ = await acaoNaFila("queuedSendNow", MXPedidoDaFila(botId: agente, entryId: item.id), geracao: geracao) }
    }

    public func salvando(_ item: String, em id: String) -> Bool { salvamentos.pendente(item, em: id) }

    public func editar(_ item: MXItemDaFila, texto: String, revisao: Int64, em id: String) -> Task<Bool, Never>? {
        // No toque: um salvar por mensagem, a ligação conferida e a geração do link guardada para o despacho.
        guard !salvamentos.pendente(item.id, em: id), let geracao = despachar(MXTextoDeQueda.semAcao) else { return nil }
        _ = salvamentos.iniciar(item.id, em: id)
        let pedido = MXPedidoDaFila(botId: MXIdDaConversa(id).agente, entryId: item.id, text: texto, expectedRevision: revisao)
        return Task {
            defer { salvamentos.concluir(item.id, em: id) }
            return await acaoNaFila("queuedEdit", pedido, geracao: geracao)
        }
    }

    public func remover(_ item: MXItemDaFila, em id: String) {
        guard let geracao = despachar(MXTextoDeQueda.semAcao) else { return }
        let agente = MXIdDaConversa(id).agente
        Task { _ = await acaoNaFila("queuedRemove", MXPedidoDaFila(botId: agente, entryId: item.id), geracao: geracao) }
    }

    public func responder(_ pedido: MXPedidoDeAprovacao, opcao: String, em id: String) {
        guard despachar(MXTextoDeQueda.semAprovar) != nil else { return }
        let c = MXIdDaConversa(id)
        guard let e = store.chat(c.agente, sessionId: c.sessao).first(where: { $0.id == pedido.id }),
              MXMapeamento.respostaConfere(e, apresentado: pedido, opcao: opcao) else {
            avisoProprio = MXTextoDeQueda.pedidoMudou
            return
        }
        // `respond` manda `entryRev` = a revisão conferida acima (a apresentada).
        store.respond(e, option: opcao)
    }

    /// Fixar/desafixar: aparece na hora; uma operação por bot (outro toque espera a primeira voltar). A resposta só
    /// entra se não for mais velha que o bot que já está aqui; se o Hub recusar, volta ao valor de antes, a menos que
    /// um evento mais novo do bot tenha chegado no meio (ele é que vale). O `setPinned` do store não desfaz.
    public func alternarFixar(_ id: String) {
        let id = MXIdDaConversa(id).agente
        guard fixando[id] == nil else {
            avisoProprio = MXTextoDeQueda.fixarACaminho
            return
        }
        guard let geracao = despachar(MXTextoDeQueda.semAcao), let bot = store.bots[id] else { return }
        let operacao = UUID()
        fixando[id] = operacao
        let anterior = bot.pinned
        let pedido = !anterior
        let revInicial = bot.rev
        // Só o id e o campo mudado (o Hub aplica como patch): nada do espelho daqui volta para o Hub.
        let rascunho = MXPatchDoBot(id: id, pinned: pedido)
        store.bots[id]?.pinned = pedido
        Task {
            defer { if fixando[id] == operacao { fixando[id] = nil } }
            do {
                let novo = try await atualizarNoHub(rascunho, geracao: geracao)
                // O link mudou durante a espera: falha, sem aplicar a resposta (a sessão nova traz o estado do Hub).
                guard MXMapeamento.mesmoLink(geracao, geracaoDoLink()) else { throw MXFalhaDoLink.mudou }
                guard fixando[id] == operacao else { return }
                if MXMapeamento.aplicaRespostaDeFixar(atualRev: store.bots[id]?.rev, respostaRev: novo.rev) {
                    store.bots[novo.id] = novo
                }
            } catch {
                guard fixando[id] == operacao else { return }
                if let atual = store.bots[id],
                   let valor = MXMapeamento.desfazFixar(atual: atual.pinned, atualRev: atual.rev, pedido: pedido,
                                                        anterior: anterior, revInicial: revInicial) {
                    store.bots[id]?.pinned = valor
                }
                // O link caiu ou mudou: a causa é a ligação, não o Hub.
                avisoProprio = MXMapeamento.avisoDaFalha(
                    tocada: geracao, agora: geracaoDoLink(),
                    daAcao: pedido ? String(localized: "Não foi possível fixar \(bot.name). Tente de novo.", bundle: .module) : String(localized: "Não foi possível desafixar \(bot.name). Tente de novo.", bundle: .module))
            }
        }
    }

    /// A chamada da fila pelo link no ar, só se ainda for o da geração dada.
    private func chamarNoHub(_ metodo: String, _ pedido: MXPedidoDaFila, geracao: AnyHashable) async throws -> MXRespostaDaFila {
        #if DEBUG
        if let chamarFila { return try await chamarFila(metodo, pedido) }
        #endif
        guard geracaoDoLink() == geracao, let client = clienteComAutoridade() else { throw MXFalhaDoLink.mudou }
        return try await client.call(metodo, pedido)
    }

    /// O `updateBot` (fixar, autonomia, avisos) pelo link no ar, só se ainda for o da geração dada, com o pedido
    /// MÍNIMO: o `HostClient.updateBot` da base manda o `BotDraft` inteiro (nome, motor, conectores…), e o Hub aplica
    /// o que chega como patch (`hub.rs`, `update_bot_by_user_authorized`); um espelho velho aqui desfaria o que mudou
    /// no Mac. Por isso vai pelo `call` genérico do cliente, só com o id e o campo. Sai uma vez, sem repetir.
    private func atualizarNoHub(_ patch: MXPatchDoBot, geracao: AnyHashable) async throws -> Bot {
        #if DEBUG
        if let atualizarBot { return try await atualizarBot(patch) }
        #endif
        guard geracaoDoLink() == geracao, let client = clienteComAutoridade() else { throw MXFalhaDoLink.mudou }
        let resposta: MXRespostaDoBot = try await client.call("updateBot", patch)
        return resposta.bot
    }

    public func limparAviso() {
        avisoProprio = nil
        store.lastError = nil
    }

    // MARK: Sessões (07, 07b)

    public func sessoes(_ id: String) -> MXSessoesDados? {
        let id = MXIdDaConversa(id).agente
        guard let bot = store.bots[id] else { return nil }
        let agora = agora()
        let pendente = MXMapeamento.temAprovacaoPendente(store.chat(id))
        let principal = MXMapeamento.estadoDaPrincipal(bot, aprovacaoPendente: pendente)
        let doHub = sessoesDoHub[id]
        let nota: String? = doHub != nil ? nil
            : sessoesFalharam.contains(id) ? MXTextoDasSessoes.semSessoesNoHub : MXTextoDasSessoes.esperandoOComputador
        return MXSessoesDados(
            agenteId: id, nome: bot.name, funcao: MXMapeamento.funcaoDoHub(bot), rosto: MXRosto(bot.avatar),
            estado: bot.avatarState(now: agora), estadoTexto: MXMapeamento.previa(bot, aprovacaoPendente: pendente).texto,
            sessoes: MXMapeamento.arvore(doHub ?? [], botId: id, principal: principal,
                                         horaDaPrincipal: MXHora.naLista(Date(milliseconds: bot.lastAt), agora: agora), agora: agora,
                                         horas: horasDasSessoes(id, doHub ?? [], agora: agora)),
            // Arquivar e desarquivar não existem no Hub para o app; criar existe no Hub (`sessionCreate`), mas
            // a base não tem caminho público para ele: fica desligado ("Em breve").
            // A voz do topo (ligação) é de outra etapa: desligada.
            arquivadas: nil, podeArquivar: false, podeCriar: capacidades.criaSessao,
            podeLigar: capacidades.ligado(.vozDasSessoes), nota: nota)
    }

    /// A hora de cada sessão extra: a da última mensagem dela que este aparelho tem (o `BotSession` da base não traz
    /// `lastUsedAt`); sem nenhuma, fica sem hora.
    private func horasDasSessoes(_ id: String, _ sessoes: [MXSessaoDoHub], agora: Date) -> [String: String] {
        var horas: [String: String] = [:]
        for s in sessoes where s.lastUsedAt == nil && !s.principal && s.id != id {
            if let ultima = store.chat(id, sessionId: s.id).last(where: { $0.createdAt > 0 }) {
                horas[s.id] = MXHora.naLista(Date(milliseconds: ultima.createdAt), agora: agora)
            }
        }
        return horas
    }

    /// Lê as sessões pelo `BotStore.sessions(_:marca:)` (só leitura; o store guarda, para esta ligação, se o computador
    /// aceita mensagem em sessão), com a marca do link lida agora; a resposta que volta por outro link não entra. Sem
    /// ligação, não pede (a tela pede de novo ao reconectar).
    public func carregarSessoes(_ id: String) async {
        guard let geracao = geracaoDoLink() else { return }
        await carregarSessoes(MXIdDaConversa(id).agente, marca: store.marcaDoLink, geracao: geracao)
    }

    /// A leitura fica guardada com a geração do link em que saiu, e só entra se ainda for a de agora E se nenhuma leitura
    /// mais nova do mesmo agente já foi aplicada: a ordem é tirada antes de esperar (a tela Sessões e a abertura de uma
    /// extra leem o mesmo agente). Uma resposta velha, de sucesso ou de erro, não troca a vigente; cancelamento não é
    /// falha do computador e não muda nada.
    private func carregarSessoes(_ id: String, marca: MarcaDoLink, geracao: AnyHashable) async {
        let ordem = store.novaLeituraDeSessoes(id)
        do {
            let resposta = try await lerSessoesNoHub(id, marca: marca, ordem: ordem)
            guard marca == store.marcaDoLink, MXMapeamento.mesmoLink(geracao, geracaoDoLink()),
                  store.aplicarLeituraDeSessoes(id, ordem: ordem) else { return }
            sessoesDoHub[id] = resposta.sessions
            sessoesFalharam.remove(id)
            let anuncia = resposta.sessionActions?.contains("send") == true
            leiturasDeSessoes[id] = MXLeituraDeSessoes(geracao: geracao, resultado: anuncia ? .anunciaEnvio : .semEnvio)
        } catch {
            guard !Task.isCancelled, !(error is CancellationError), (error as? URLError)?.code != .cancelled,
                  marca == store.marcaDoLink, MXMapeamento.mesmoLink(geracao, geracaoDoLink()),
                  store.aplicarLeituraDeSessoes(id, ordem: ordem) else { return }
            sessoesFalharam.insert(id)
            leiturasDeSessoes[id] = MXLeituraDeSessoes(geracao: geracao, resultado: .falhou)
        }
    }

    private func lerSessoesNoHub(_ id: String, marca: MarcaDoLink, ordem: Int) async throws -> MXRespostaDeSessoes {
        #if DEBUG
        if let lerSessoes { return try await lerSessoes(id) }
        #endif
        return MXRespostaDeSessoes(try await store.sessions(id, marca: marca, ordem: ordem))
    }

    /// Uma página do histórico de uma sessão extra pelo `BotStore.loadHistoryPage(_:sessionId:beforeSeq:marca:)` (ele põe
    /// no espelho só as entradas desta sessão e falha se o link mudou). Devolve também o número bruto de linhas.
    private func lerHistorico(_ id: String, sessao: String, antesDe: Int64,
                              marca: MarcaDoLink) async throws -> BotStore.PaginaDeHistorico {
        #if DEBUG
        if let lerPaginaDaSessao { return try await lerPaginaDaSessao(id, sessao, antesDe) }
        if let lerHistoricoDaSessao {
            let entradas = try await lerHistoricoDaSessao(id, sessao, antesDe)
            return BotStore.PaginaDeHistorico(entries: entradas, rawCount: entradas.count)
        }
        #endif
        return try await store.loadHistoryPage(id, sessionId: sessao, beforeSeq: antesDe, marca: marca)
    }

    /// O fim do histórico se decide pelo número BRUTO de linhas da página (`rawCount < 100`), nunca pelas entradas que
    /// sobraram: o `BotStore` descarta as de `sessionId` inválido e filtra as de outra sessão, e uma página cheia que
    /// perde uma entrada ainda tem páginas anteriores. Página cheia remove o fim; curta o registra, sob a marca do link
    /// em que foi lida (e só se esse ainda é o link de agora).
    private func registrarFimDoHistorico(_ id: String, pagina: BotStore.PaginaDeHistorico, marca: MarcaDoLink) {
        guard marca == store.marcaDoLink else { return }
        if pagina.rawCount < MXMapeamento.paginaDoHistorico {
            historicoCompletoDaSessao[id] = marca
        } else {
            historicoCompletoDaSessao[id] = nil
        }
    }

    /// O fim visto vale só no link em que foi visto.
    func historicoAcabou(_ id: String) -> Bool { historicoCompletoDaSessao[id] == store.marcaDoLink }

    /// A Principal abre a Conversa de sempre; uma sessão extra abre a conversa dela (o `BotStore` separa as
    /// mensagens por sessão e manda com `sessionId`).
    public func conversaDaSessao(_ sessao: String, em id: String) -> String? {
        let id = MXIdDaConversa(id).agente
        guard store.bots[id] != nil else { return nil }
        // A Principal tem o id do próprio bot no Hub (`ensure_principal`).
        return MXIdDaConversa(agente: id, sessao: sessao).texto
    }

    public func novaSessao(em id: String) -> String? { nil }
    public func arquivarSessao(_ sessao: String, em id: String) {}
    public func desfazerArquivar(_ sessao: String, em id: String) {}

    // MARK: Ajustes do agente (08)

    public func ajustes(_ id: String) -> MXAjustesDoAgente? {
        let id = MXIdDaConversa(id).agente
        guard let bot = store.bots[id] else { return nil }
        // As telas de dentro são de outras etapas: os itens ficam à vista, com o valor real, desligados ("Em breve").
        let itens = [
            MXItemDeAjuste(.identidade, letra: "I", nome: String(localized: "Identidade", bundle: .module), valor: String(localized: "Nome, função e aparência", bundle: .module),
                           disponivel: capacidades.ligado(.ajuste(.identidade))),
            MXItemDeAjuste(.motor, letra: "M", nome: String(localized: "Motor", bundle: .module),
                           valor: MXMapeamento.modelo(motor: store.backendName(bot.backend), modelo: bot.model),
                           disponivel: capacidades.ligado(.ajuste(.motor))),
            MXItemDeAjuste(.comportamento, letra: "C", nome: String(localized: "Comportamento", bundle: .module), valor: MXAjustesRegras.comportamento(bot.description),
                           disponivel: capacidades.ligado(.ajuste(.comportamento))),
            MXItemDeAjuste(.ferramentas, letra: "F", nome: String(localized: "Ferramentas", bundle: .module),
                           valor: MXAjustesRegras.ferramentas(conectores: bot.connectors.count, skills: bot.skills.count),
                           disponivel: capacidades.ligado(.ajuste(.ferramentas))),
            // Memória e Rotinas: o app tem as telas de hoje, mas não a contagem; WhatsApp e Voz por agente o Hub não
            // manda (ficam ocultos).
            MXItemDeAjuste(.memoria, letra: "R", nome: String(localized: "Memória", bundle: .module), valor: nil, disponivel: capacidades.ligado(.ajuste(.memoria))),
            MXItemDeAjuste(.rotinas, letra: "T", nome: String(localized: "Rotinas", bundle: .module), valor: nil, disponivel: capacidades.ligado(.ajuste(.rotinas))),
        ]
        return MXAjustesDoAgente(
            id: id, nome: bot.name, funcao: MXMapeamento.funcaoDoHub(bot), rosto: MXRosto(bot.avatar),
            estado: bot.avatarState(now: agora()),
            sincronia: MXAjustesRegras.sincronia(computador: store.hostName, conectado: conexao.podeAgir),
            sincronizado: conexao.podeAgir, autonomia: MXAutonomia.doHub(bot.permission), itens: itens,
            avisos: bot.notify ?? true, podeExportar: false, podeExcluir: !MXMapeamento.ehBerga(bot),
            podeLigar: capacidades.ligado(.ligar),
            excluindo: exclusoes.excluindo(id))
    }

    public func mudarAutonomia(_ autonomia: MXAutonomia, em id: String) {
        guard autonomia.disponivel, let permissao = autonomia.paraOHub, store.bots[id]?.permission != permissao else { return }
        mudarBot(id, MXPatchDoBot(id: id, permission: permissao),
                 falha: { String(localized: "Não foi possível mudar a autonomia de \($0). Tente de novo.", bundle: .module) }) { $0.permission = permissao }
    }

    public func alternarAvisos(em id: String) {
        guard let atual = store.bots[id] else { return }
        let novo = !(atual.notify ?? true)
        mudarBot(id, MXPatchDoBot(id: id, notify: novo),
                 falha: { String(localized: "Não foi possível mudar os avisos de \($0). Tente de novo.", bundle: .module) }) { $0.notify = novo }
    }

    /// Excluir, com uma tarefa por agente que espera a resposta do PRÓPRIO pedido: o `deleteBot` do cliente
    /// (`HostClient.deleteBot(_:)`, público e aguardável), uma tentativa só, no link do toque e só com autoridade. O
    /// `BotStore.delete` não serve aqui: não devolve o resultado (a trava teria de adivinhar pelo `lastError`, que
    /// qualquer ação escreve) e não trava um segundo toque. A limpeza local não é feita aqui: no sucesso, o Hub publica
    /// `bot {deleted: true}` e o store tira agente, histórico e rascunho (`.botDeleted`); até lá a linha segue
    /// "Excluindo…". Se o computador recusar ou o link cair, a trava solta, o agente fica e o aviso diz por quê. O
    /// Berga não se exclui. A tela já confirmou.
    public func excluirAgente(_ id: String) -> Bool {
        let id = MXIdDaConversa(id).agente
        guard let bot = store.bots[id],
              exclusoes.podePedir(id, berga: MXMapeamento.ehBerga(bot), conectado: conexao.podeAgir),
              let geracao = despachar(MXTextoDeQueda.semAcao),
              exclusoes.pedir(id, berga: MXMapeamento.ehBerga(bot), conectado: true) else { return false }
        Task {
            do {
                try await excluirNoHub(id, geracao: geracao)
                // A resposta só vale na ligação do toque: se ela mudou no meio, o resultado não se sabe.
                if let aviso = exclusoes.concluir(id, ok: true, mesmoLink: MXMapeamento.mesmoLink(geracao, geracaoDoLink())) {
                    avisoProprio = aviso
                }
            } catch {
                let mesmoLink = MXMapeamento.mesmoLink(geracao, geracaoDoLink())
                var motivo: String?
                if case let HostError.http(_, m) = error { motivo = m }
                avisoProprio = exclusoes.concluir(id, ok: false, mesmoLink: mesmoLink)
                    ?? MXAjustesRegras.falhaAoExcluir(bot.name, motivo: motivo)
            }
        }
        return true
    }

    /// O `deleteBot` pelo link no ar, só se ainda for o da geração do toque. Sai uma vez, sem repetir.
    private func excluirNoHub(_ id: String, geracao: AnyHashable) async throws {
        #if DEBUG
        if let excluirNoHub { return try await excluirNoHub(id) }
        #endif
        guard geracaoDoLink() == geracao, let client = clienteComAutoridade() else { throw MXFalhaDoLink.mudou }
        try await client.deleteBot(id)
    }

    /// Muda um campo do bot pelo `updateBot` do Hub, como o fixar: aparece na hora; uma mudança por bot (divide a
    /// vez com o fixar); presa à geração do toque; a resposta só entra se não for mais velha que o bot no espelho; se
    /// o Hub recusar, volta ao bot de antes, a menos que algo mais novo tenha chegado no meio.
    private func mudarBot(_ id: String, _ patch: MXPatchDoBot, falha: @escaping (String) -> String,
                          _ mudanca: (inout Bot) -> Void) {
        guard fixando[id] == nil else {
            avisoProprio = MXTextoDeQueda.fixarACaminho
            return
        }
        guard let geracao = despachar(MXTextoDeQueda.semAcao), let anterior = store.bots[id] else { return }
        let operacao = UUID()
        fixando[id] = operacao
        var pedido = anterior
        mudanca(&pedido)
        store.bots[id] = pedido
        Task {
            defer { if fixando[id] == operacao { fixando[id] = nil } }
            do {
                let novo = try await atualizarNoHub(patch, geracao: geracao)
                guard MXMapeamento.mesmoLink(geracao, geracaoDoLink()) else { throw MXFalhaDoLink.mudou }
                guard fixando[id] == operacao else { return }
                if MXMapeamento.aplicaRespostaDeFixar(atualRev: store.bots[id]?.rev, respostaRev: novo.rev) {
                    store.bots[novo.id] = novo
                }
            } catch {
                guard fixando[id] == operacao else { return }
                if let atual = store.bots[id], MXMapeamento.desfazMudanca(atual: atual, pedido: pedido, revInicial: anterior.rev) {
                    store.bots[id] = anterior
                }
                avisoProprio = MXMapeamento.avisoDaFalha(tocada: geracao, agora: geracaoDoLink(), daAcao: falha(anterior.name))
            }
        }
    }

    /// `queuedRemove` / `queuedEdit` / `queuedSendNow` do Hub (`host/src/agent/bot/fork_fila.rs`), pelo cliente
    /// com autoridade do store (`clienteComAutoridade`): sem ele, falha na hora, sem esperar a reconexão nem repetir.
    /// As entradas que o Hub devolve entram no espelho na hora; o resto chega pelo stream.
    func acaoNaFila(_ metodo: String, _ pedido: MXPedidoDaFila, geracao tocada: AnyHashable) async -> Bool {
        // `tocada` é a geração capturada no toque (Mandar agora, Remover, Salvar). Na hora de despachar: sem link no ar
        // é "sem conexão"; com link, mas outro (caiu e voltou no meio), é "o link mudou". Nos dois casos, não sai.
        guard let geracao = despachar(MXTextoDeQueda.semAcao) else { return false }
        guard geracao == tocada else {
            avisoProprio = MXTextoDeQueda.linkMudou
            return false
        }
        do {
            let resposta = try await chamarNoHub(metodo, pedido, geracao: geracao)
            // O link mudou durante a espera: falha, sem aplicar a resposta no espelho.
            guard MXMapeamento.mesmoLink(geracao, geracaoDoLink()) else {
                avisoProprio = MXTextoDeQueda.linkMudou
                return false
            }
            for e in resposta.entries ?? [] { store.upsert(e) }
            return true
        } catch MXFalhaDoLink.mudou {
            avisoProprio = MXTextoDeQueda.linkMudou
            return false
        } catch {
            // O link caiu ou mudou durante a chamada: a causa é a ligação, não o Hub.
            avisoProprio = MXMapeamento.avisoDaFalha(tocada: geracao, agora: geracaoDoLink(),
                                                     daAcao: MXFilaRegras.falha(error.localizedDescription))
            return false
        }
    }
}

/// Uma sessão como o Hub manda em `sessions {botId}` (`host/src/fork/sessoes/store.rs`, `SessionRow`): só o que a
/// tela usa. Campos a mais são ignorados.
public struct MXSessaoDoHub: Decodable, Equatable, Sendable {
    public let id: String
    public let principal: Bool
    public let parentSession: String?
    public let title: String
    /// `open` ou `closed` (encerrada).
    public let state: String
    /// Nulo quando veio pelo `BotSession` da base (que não traz): a hora sai da última mensagem que o aparelho tem.
    public let lastUsedAt: Int64?

    public init(id: String, principal: Bool, parentSession: String?, title: String, state: String, lastUsedAt: Int64?) {
        self.id = id
        self.principal = principal
        self.parentSession = parentSession
        self.title = title
        self.state = state
        self.lastUsedAt = lastUsedAt
    }

    /// Da sessão como o `BotStore.sessions(_:marca:)` devolve.
    public init(_ s: BotSession) {
        self.init(id: s.id, principal: s.principal, parentSession: s.parentSession, title: s.title, state: s.state, lastUsedAt: nil)
    }
}

/// O pedido mínimo do `updateBot`: só o `id` e o campo mudado (as chaves nulas não vão no JSON). O Hub aplica como
/// patch e só aceita `permission` com `permissionChosenByUser: true` (`hub.rs`).
public struct MXPatchDoBot: Encodable, Equatable, Sendable {
    public let id: String
    public let pinned: Bool?
    public let notify: Bool?
    public let permission: String?
    public let permissionChosenByUser: Bool?

    public init(id: String, pinned: Bool? = nil, notify: Bool? = nil, permission: String? = nil) {
        self.id = id
        self.pinned = pinned
        self.notify = notify
        self.permission = permission
        permissionChosenByUser = permission == nil ? nil : true
    }

    private enum CodingKeys: String, CodingKey { case id, pinned, notify, permission, permissionChosenByUser }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encodeIfPresent(pinned, forKey: .pinned)
        try c.encodeIfPresent(notify, forKey: .notify)
        try c.encodeIfPresent(permission, forKey: .permission)
        try c.encodeIfPresent(permissionChosenByUser, forKey: .permissionChosenByUser)
    }
}

/// A resposta do `updateBot`: o bot como o Hub ficou.
struct MXRespostaDoBot: Decodable {
    let bot: Bot
}

/// A resposta de `sessions {botId}`.
public struct MXRespostaDeSessoes: Decodable, Sendable {
    public let sessions: [MXSessaoDoHub]
    /// O que o Hub anuncia para as sessões; sem "send" (ou ausente, Hub antigo), o envio por sessão não vale.
    public let sessionActions: [String]?

    public init(sessions: [MXSessaoDoHub], sessionActions: [String]? = nil) {
        self.sessions = sessions
        self.sessionActions = sessionActions
    }

    /// Do `BotSessions` da base.
    public init(_ r: BotSessions) {
        self.init(sessions: r.sessions.map(MXSessaoDoHub.init), sessionActions: r.sessionActions)
    }
}

/// Uma leitura das sessões de um agente: a geração do link em que saiu e o que o computador disse do envio por sessão.
public struct MXLeituraDeSessoes: Equatable {
    public enum Resultado: Equatable, Sendable {
        /// `sessionActions` com "send".
        case anunciaEnvio
        /// Listou sem anunciar (Hub antigo pode ignorar `send.sessionId`).
        case semEnvio
        /// A leitura falhou.
        case falhou
    }

    public let geracao: AnyHashable
    public let resultado: Resultado

    public init(geracao: AnyHashable, resultado: Resultado) {
        self.geracao = geracao
        self.resultado = resultado
    }
}

/// O link em que a ação saiu caiu ou foi trocado antes da resposta.
enum MXFalhaDoLink: Error { case mudou }

/// O corpo das ações da fila no Hub: `botId`, `entryId` e, para editar, `text` e `expectedRevision`.
struct MXPedidoDaFila: Encodable, Sendable {
    let botId: String
    let entryId: String
    var text: String?
    var expectedRevision: Int64?

    init(botId: String, entryId: String, text: String? = nil, expectedRevision: Int64? = nil) {
        self.botId = botId
        self.entryId = entryId
        self.text = text
        self.expectedRevision = expectedRevision
    }
}

/// A resposta do Hub: as entradas que mudaram (e se pediu para parar a resposta em andamento).
struct MXRespostaDaFila: Decodable, Sendable {
    let entries: [Entry]?
}

// MARK: - Sessões e ajustes (mapeamento puro, testado)

public extension MXMapeamento {
    /// O estado da Sessão Principal pelo que o app sabe do bot. O Hub não manda o estado por sessão: as outras
    /// aparecem livres, com a hora; e o trabalho de uma extra pode aparecer na Principal.
    static func estadoDaPrincipal(_ bot: Bot, aprovacaoPendente: Bool) -> MXEstadoDaSessao {
        if aprovacaoPendente || bot.needsInput || bot.hasPendingQuestions { return .precisaDeVoce }
        if bot.isWorking(in: bot.id) { return .trabalhando }
        return .livre
    }

    /// A árvore das sessões abertas do Hub (`state == "open"`; as encerradas ficam de fora), pelo `parentSession`.
    /// A Principal (id do bot) vem primeiro, com as filhas dela; as outras na ordem do Hub (a usada por último antes).
    /// Pai que não está na lista vira raiz; ciclo não trava (cada sessão aparece uma vez só).
    /// `horas`: a hora de cada sessão que veio sem `lastUsedAt` (pela última mensagem dela que o aparelho tem).
    static func arvore(_ linhas: [MXSessaoDoHub], botId: String, principal: MXEstadoDaSessao, horaDaPrincipal: String,
                       agora: Date, calendario: Calendar = .current, horas: [String: String] = [:]) -> [MXSessao] {
        let abertas = linhas.filter { $0.state == "open" && !$0.principal && $0.id != botId }
        let ids = Set(abertas.map(\.id))
        var filhas: [String: [MXSessaoDoHub]] = [:]
        var raizes: [MXSessaoDoHub] = []
        var daPrincipal: [MXSessaoDoHub] = []
        for l in abertas {
            if let pai = l.parentSession, pai == botId {
                daPrincipal.append(l)
            } else if let pai = l.parentSession, pai != l.id, ids.contains(pai) {
                filhas[pai, default: []].append(l)
            } else {
                raizes.append(l)
            }
        }
        var vistas = Set<String>()
        func montar(_ l: MXSessaoDoHub) -> MXSessao? {
            guard vistas.insert(l.id).inserted else { return nil }
            let dentro = (filhas[l.id] ?? []).compactMap(montar)
            return MXSessao(id: l.id, nome: nomeDaSessao(l.title),
                            estado: .livre,
                            hora: l.lastUsedAt.map { MXHora.naLista(Date(milliseconds: $0), agora: agora, calendario: calendario) }
                                ?? horas[l.id] ?? "",
                            filhas: dentro)
        }
        let principalSessao = MXSessao(id: botId, nome: String(localized: "Principal", bundle: .module), estado: principal, hora: horaDaPrincipal, principal: true,
                                       filhas: daPrincipal.compactMap(montar))
        var resto = raizes.compactMap(montar)
        // O que sobrou (preso num ciclo) entra como raiz, para nada sumir.
        resto += abertas.filter { !vistas.contains($0.id) }.compactMap(montar)
        return [principalSessao] + resto
    }

    /// O título da sessão; vazio (o Hub ainda não deu nome), "Sessão sem nome".
    static func nomeDaSessao(_ titulo: String) -> String {
        let t = titulo.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.isEmpty ? String(localized: "Sessão sem nome", bundle: .module) : t
    }

    /// Mudança do bot que falhou: volta ao de antes só se nada mais novo chegou (mesma revisão do começo) e o bot ainda
    /// é o que pedimos.
    static func desfazMudanca(atual: Bot, pedido: Bot, revInicial: Int64) -> Bool {
        atual.rev == revInicial && atual == pedido
    }
}
