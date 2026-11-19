// voz-16a (D71): política, intenção e origem comuns a todos os adaptadores de voz.
import type { Entry } from '../../../../shared/models'
import type { PermissaoEncerramento } from './encerrar.ts'
import type { BotStore } from '../../../store/bot-store'
import type { OperatorTool } from '../../../../shared/fork-voz-operador.ts'
import { voiceApprovalOption } from '../../call/voice-approval.ts'
import { PedidoCancelado } from './falha-segura.ts'

export const INDISPONIVEL_NESTA_CONVERSA = 'Isso não está disponível nesta conversa'
// textos-ok: resultado de ferramenta só ao modelo de voz (nunca exibido), que fala no idioma da pessoa (D416).
export const HUB_DESCONECTADO = 'Estou sem conexão com o Hub agora, então não consigo enviar nem confirmar nada. Continuo ouvindo; quando a conexão voltar, confira no app antes de repetir um pedido.'
export const ORIGEM_RECUSADA = 'Preciso de um pedido na fala da pessoa neste turno.'
export const APROVACAO_FALHOU = 'Não foi possível confirmar a resposta ao cartão.'
export const ENVIO_FALHOU = 'Não foi possível confirmar o envio. Confira a mensagem no app antes de tentar novamente.'

/** Gancho opcional do envio normal: o composer continua retornando void e usando seu replay habitual. */
export interface AcompanhamentoEnvioVoz {
  erro: string
  validar: () => void
  concluir: (aceito: boolean) => void
}

type StoreVoz = Pick<BotStore, 'bots' | 'send' | 'allEntries'> & Partial<Pick<BotStore,
  'client' | 'computer' | 'contextId' | 'spaceId' | 'connection' | 'isLoopback' | 'hostRoute' | 'hello' | 'conversations' | 'subscribe'>>

/** Identidade do destino; o cliente é vinculado separadamente a cada despacho. */
function contextoEnvio(store: StoreVoz, botId: string): readonly unknown[] {
  const conversa = [...(store.conversations?.values() ?? [])].find((c) => c.botId === botId
    && c.primary && !c.closed && (!store.spaceId || c.spaceId === store.spaceId))
  return [store.computer?.id, store.contextId, store.spaceId, store.hostRoute, store.hello?.hostId, conversa?.id]
}

/** Gancho do send herdado: só o retorno do vínculo que despachou pode alterar o espelho. */
export function acompanharVinculoEnvio(store: StoreVoz & Pick<BotStore, 'discard'>, botId: string, nonce: string) {
  let contexto = contextoEnvio(store, botId)
  const otimista = store.allEntries(botId).find((e) => e.id === `local-${nonce}`)
  let cliente = store.client
  return {
    despachar(client: StoreVoz['client']) { cliente = client; contexto = contextoEnvio(store, botId) },
    aceitar(entry?: Entry): boolean {
      const atual = store.client === cliente && contextoEnvio(store, botId).every((parte, i) => parte === contexto[i])
        && (!entry || entry.botId === botId)
      if (!atual && otimista && store.allEntries(botId).includes(otimista)) store.discard(otimista)
      return atual
    },
  }
}

export interface ContextoElegibilidade {
  local: boolean
  conectado: boolean
  grupo: boolean
  capacidade: boolean
}

export interface RequisitosVoz { somenteLocal?: boolean; unicoBot?: boolean }

/** A oferta e a invocação consultam a mesma política; capacidade ausente falha fechada. */
export function elegivel(contexto: ContextoElegibilidade, requisitos: RequisitosVoz = {}): boolean {
  return contexto.conectado && contexto.capacidade && (!requisitos.somenteLocal || contexto.local)
    && (!requisitos.unicoBot || !contexto.grupo)
}

/** Uma entrada vale para uma única geração; resultado de ferramenta revoga também a entrada pendente. */
export class OrigemEncerramento {
  private geracao = 0
  get versao() { return this.geracao }
  private entradaPendente = false
  private resposta: string | null = null
  private autorizada = false

  get emResposta() { return this.resposta !== null }

  entrada() { this.geracao++; this.entradaPendente = true }

  iniciar(id: string, ferramenta = false) {
    this.geracao++
    this.resposta = id
    this.autorizada = this.entradaPendente && !ferramenta
    this.entradaPendente = false
  }

  permite(id: unknown, _intencao?: string): boolean {
    return typeof id === 'string' && id === this.resposta && this.autorizada
  }

  invalidar() {
    this.geracao++
    this.entradaPendente = false
    this.autorizada = false
  }

  terminar(id: unknown) {
    if (id !== this.resposta) return
    this.invalidar()
    this.resposta = null
  }
}

/** Gemini não identifica a entrada: fronteiras mantêm a ambiguidade até a confirmação. */
export class OrigemGemini {
  private geracao = 0
  get versao() { return this.geracao }
  private entrada: { geracao: number; confirmada: string | null } | null = null
  private resposta: { id: string; geracao: number; autorizada: boolean; conversa: boolean; audio: boolean } | null = null
  private ambigua = false
  private inicial = false
  private iniciada = false
  private pergunta: 'aguardando' | 'em-curso' | null = null
  private janela: { geracao: number; intencao: string } | null = null
  private intencaoPergunta = 'end_call'
  private confirmada: string | null = null
  /** O teto sobrevive às invalidações; só uma nova conversa concluída o renova. */
  private perguntou = false
  private entradaConversa = false

  get emResposta() { return this.resposta !== null }
  get confirmacaoAberta() { return !!this.entrada?.confirmada }

  iniciarSessao() {
    this.invalidar()
    // Antes da primeira captura não existe transcrição anterior desta sessão.
    this.inicial = !this.iniciada
    this.ambigua = !this.inicial
    this.iniciada = true
  }

  transcricao(entreTurnos: boolean) {
    // Sem fronteira do modelo/protocolo, outro fragmento pertence à mesma entrada.
    if (this.entrada?.geracao === this.geracao) return
    const confirmada = this.janela?.geracao === this.geracao ? this.janela.intencao : null
    this.geracao++ // Só uma nova entrada muda a versão; fragmentos da mesma fala não a renovam.
    this.entradaConversa = entreTurnos && !confirmada
    this.pergunta = null
    this.entrada = null
    if (this.resposta) this.resposta.autorizada = false
    if (this.inicial || !entreTurnos || (this.ambigua && !confirmada)) return
    this.entrada = { geracao: this.geracao, confirmada }
  }

  modeloFalou() {
    if (this.resposta) this.resposta.audio = true
    if (this.pergunta === 'aguardando') this.pergunta = 'em-curso'
  }

  captura() {
    // Áudio do microfone só revoga a espera; jamais autoriza encerramento.
    if (this.pergunta === 'aguardando') this.invalidar()
  }

  iniciar(id: string) {
    // modelTurn ou a ferramenta (inclusive end_call) consome a entrada aberta.
    this.confirmada = this.entrada?.confirmada ?? null
    this.janela = null
    this.resposta = {
      id, geracao: this.geracao,
      autorizada: this.entrada?.geracao === this.geracao && (!this.ambigua || !!this.entrada.confirmada),
      conversa: this.entradaConversa, audio: false,
    }
    this.entrada = null
    this.entradaConversa = false
  }

  permite(id: unknown, intencao = 'end_call') {
    return (!this.confirmada || this.confirmada === intencao) && this.resposta !== null && this.resposta.id === id && this.resposta.geracao === this.geracao && this.resposta.autorizada
  }

  solicitar(id: unknown, intencao = 'end_call'): PermissaoEncerramento {
    if (this.resposta) this.resposta.conversa = false
    if (this.permite(id, intencao)) return true
    if (this.perguntou) return 'botao'
    this.perguntou = true
    return 'confirmar'
  }

  aguardarPergunta(intencao = 'end_call') { this.pergunta = 'aguardando'; this.intencaoPergunta = intencao }

  invalidar() {
    this.geracao++
    this.entrada = null
    this.entradaConversa = false
    this.resposta = null
    this.pergunta = null
    this.janela = null
    this.ambigua = true
  }

  terminar() {
    const abrir = this.pergunta === 'em-curso'
    if (this.resposta?.conversa && this.resposta.audio) this.perguntou = false
    this.geracao++
    this.entrada = null
    this.entradaConversa = false
    this.resposta = null
    this.janela = abrir ? { geracao: this.geracao, intencao: this.intencaoPergunta } : null
    // Gemini pode terminar um turno vazio antes da fala assíncrona da pergunta.
    if (abrir) this.pergunta = null
    if (!this.inicial) this.ambigua = true
    this.inicial = false
  }
}

type OrigemVoz = OrigemEncerramento | OrigemGemini
type InvocacaoVoz = { resposta?: unknown; cancelada: boolean; confirmacao?: string }
const LEITURAS = new Set(['bot_status', 'recent_messages', 'bot_updates'])
const EFEITOS = new Set(['send_to_bot', 'answer_approval', 'end_call'])
const json = (result: Record<string, unknown>) => JSON.stringify(result)

/** Uma instância por chamada. Nenhum texto recebido é interpretado como comando nesta porta. */
export class OperadorVoz {
  private contexto: readonly unknown[]
  // Um ID pode existir em outra geração; cancelamento e confirmação não migram com ele.
  private readonly invocacoes = new Map<number, Map<string, InvocacaoVoz>>()
  private geracaoEntrega = 0

  private store: StoreVoz
  private botId: string
  private sinal?: AbortSignal

  constructor(store: StoreVoz, botId: string, sinal?: AbortSignal) {
    this.store = store
    this.botId = botId
    this.sinal = sinal
    this.contexto = contextoEnvio(store, botId)
  }

  cancelar(ids: string[]) {
    for (const id of ids) this.cancelarInvocacao(this.invocacao(id))
  }

  cancelarResposta(id: unknown) {
    for (const chamadas of this.invocacoes.values()) for (const chamada of chamadas.values()) {
      if (chamada.resposta !== undefined && chamada.resposta === id) this.cancelarInvocacao(chamada)
    }
  }

  private invocacao(callId: string, geracao = this.geracaoEntrega): InvocacaoVoz {
    let chamadas = this.invocacoes.get(geracao)
    if (!chamadas) { chamadas = new Map(); this.invocacoes.set(geracao, chamadas) }
    let chamada = chamadas.get(callId)
    if (!chamada) { chamada = { cancelada: false }; chamadas.set(callId, chamada) }
    return chamada
  }

  private cancelarInvocacao(chamada: InvocacaoVoz) { chamada.cancelada = true; delete chamada.confirmacao }

  podeResponder(callId: string, geracao = this.geracaoEntrega) {
    return geracao === this.geracaoEntrega && !this.sinal?.aborted && !this.invocacao(callId, geracao).cancelada
  }

  /** Done normal não abandona a RPC; interrupção/turno novo revoga só sua entrega à voz. */
  vincularEntrega(callId: string): () => boolean {
    const entrega = this.geracaoEntrega
    return () => this.podeResponder(callId, entrega)
  }

  abandonarEntregas() { this.geracaoEntrega++ }

  private get atual() {
    if (this.sinal?.aborted || !this.store.client || this.store.connection?.kind !== 'online' || !this.store.bots.has(this.botId)) return false
    const contexto = contextoEnvio(this.store, this.botId)
    // Um setup offline pode ainda não conhecer Host/conversa; fixe-os na primeira conexão.
    if (!contexto.every((parte, i) => (i >= 3 && this.contexto[i] == null) || parte === this.contexto[i])) return false
    this.contexto = contexto
    return true
  }

  indisponibilidade(): string | undefined {
    if (!this.store.client || this.store.connection?.kind !== 'online') return HUB_DESCONECTADO
    if (!this.atual) return INDISPONIVEL_NESTA_CONVERSA
    return undefined
  }

  /** Uma notificação de retomada por transição; o adaptador conserva sua fila e seus limites de áudio. */
  aoReconectar(retomar: () => void): () => void {
    let disponivel = !this.indisponibilidade()
    return this.store.subscribe?.(() => {
      const antes = disponivel
      disponivel = !this.indisponibilidade()
      if (!antes && disponivel) retomar()
    }) ?? (() => {})
  }

  ferramentas(catalogo: OperatorTool[]): OperatorTool[] {
    // Só ferramentas já existentes: o catálogo permanece estável inclusive após setup offline.
    return catalogo.filter((f) => LEITURAS.has(f.name) || EFEITOS.has(f.name))
  }

  private chave(name: string, args: Record<string, unknown>) {
    if (name === 'send_to_bot') return JSON.stringify([name, args.text])
    if (name === 'answer_approval') {
      const card = this.store.allEntries(this.botId).findLast((e) => e.kind === 'permission' && e.data.status === 'pending')
      return JSON.stringify([name, args.option, card?.id, card?.rev])
    }
    return name
  }

  executar(name: string, args: Record<string, unknown>, origem: OrigemVoz, id: unknown,
    callId: string, executarLegado: (acompanhamento: AcompanhamentoEnvioVoz | undefined, permitirFim: PermissaoEncerramento) => string): string | Promise<string> {
    if (!LEITURAS.has(name) && !EFEITOS.has(name)) return json({ error: INDISPONIVEL_NESTA_CONVERSA })
    const indisponivel = this.indisponibilidade()
    if (name !== 'end_call' && indisponivel) return json({ error: indisponivel })
    if (LEITURAS.has(name)) return executarLegado(undefined, false)
    if (name === 'answer_approval') {
      const card = this.store.allEntries(this.botId).findLast((e) => e.kind === 'permission' && e.data.status === 'pending')
      // A recusa D98 é leitura: um cartão inelegível nunca ganha pergunta de confirmação pela voz.
      if (!card || !voiceApprovalOption(card, typeof args.option === 'string' ? args.option : '')) return executarLegado(undefined, false)
    }
    const entrega = this.geracaoEntrega
    const invocacao = this.invocacao(callId, entrega)
    invocacao.resposta = id
    const chave = this.chave(name, args)
    const cliente = this.store.client
    const geracao = origem.versao
    const permite = () => this.podeResponder(callId, entrega) && origem.versao === geracao && origem.permite(id, chave)
    const validar = () => {
      if (!this.atual || this.store.client !== cliente || !permite() || chave !== this.chave(name, args)) throw new PedidoCancelado()
    }
    if (name === 'end_call' && origem instanceof OrigemGemini) {
      const permissao = origem.solicitar(id, chave)
      if (permissao === 'confirmar') invocacao.confirmacao = chave
      return executarLegado(undefined, permissao === true && !permite() ? false : permissao)
    }
    if (!permite()) {
      if (origem instanceof OrigemGemini) {
        invocacao.confirmacao = chave
        return json({ confirmar: name === 'send_to_bot'
          ? 'Quer que eu envie esse pedido?' : 'Quer permitir uma vez este cartão?' })
      }
      return name === 'end_call' ? executarLegado(undefined, false) : json({ error: ORIGEM_RECUSADA })
    }
    if (name === 'send_to_bot') return this.enviar(typeof args.text === 'string' ? args.text : '', validar)
    if (name === 'answer_approval') return this.acompanhar(validar, (voz) => executarLegado(voz, true), APROVACAO_FALHOU)
    return executarLegado(undefined, true)
  }

  private enviar(text: string, validar: () => void): Promise<string> {
    const trimmed = text.trim()
    if (!trimmed) return Promise.resolve(json({ error: 'Não há mensagem para enviar.' }))
    return this.acompanhar(validar, (voz) => { this.store.send(trimmed, this.botId, { sourceKind: 'generated', voz }) })
  }

  private acompanhar(validar: () => void, fazer: (voz: AcompanhamentoEnvioVoz) => string | void, erro = ENVIO_FALHOU): Promise<string> {
    return new Promise((resolve) => {
      const cliente = this.store.client
      let concluido = false
      let saida: string | void
      const terminar = (resultado: string) => {
        if (concluido) return
        concluido = true
        this.sinal?.removeEventListener('abort', cancelar)
        resolve(resultado)
      }
      const concluir = (aceito: boolean) => {
        // O envio e a aprovação reais são assíncronos; também tolera uma porta fake síncrona.
        queueMicrotask(() => terminar(this.atual && this.store.client === cliente ? aceito
          ? saida || json({ sent: true, note: 'O Hub aceitou a mensagem. A resposta do bot chega separadamente.' })
          : json({ sent: false, error: erro }) : json({ sent: false, error: this.indisponibilidade() ?? INDISPONIVEL_NESTA_CONVERSA })))
      }
      const cancelar = () => terminar(json({ error: INDISPONIVEL_NESTA_CONVERSA }))
      this.sinal?.addEventListener('abort', cancelar, { once: true })
      try {
        validar()
        saida = fazer({ validar, concluir, erro })
        if (saida && !JSON.parse(saida).answered) terminar(saida)
      } catch { concluir(false) }
    })
  }

  /** Depois do resultado, a única guarda Gemini pode aguardar áudio da pergunta daquela intenção. */
  resultadoGemini(message: Record<string, unknown>, origem: OrigemGemini) {
    const respostas = (message.toolResponse as { functionResponses?: { id?: string; response?: { result?: { confirmar?: string } } }[] } | undefined)?.functionResponses
    if (respostas?.length === 1) {
      const r = respostas[0]!
      const chave = r.id ? this.invocacoes.get(this.geracaoEntrega)?.get(r.id)?.confirmacao : undefined
      if (chave && r.response?.result?.confirmar) origem.aguardarPergunta(chave)
    }
    for (const chamada of this.invocacoes.get(this.geracaoEntrega)?.values() ?? []) delete chamada.confirmacao
  }
}

/** O adaptador só serializa. Leituras continuam imediatas; a RPC de envio resolve mais tarde. */
export function resultadoVoz<T>(resultado: string | Promise<string>, serializar: (output: string) => T,
  entregar: (item: T) => void): T[] {
  if (typeof resultado === 'string') return [serializar(resultado)]
  void resultado.then((output) => entregar(serializar(output)))
  return []
}
