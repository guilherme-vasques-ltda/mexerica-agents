import { sessionDeletedOf, type SessionDeleted } from '../../shared/fork-session-delete.ts'
// Barra05 (passos 12 e 13): o catálogo de sessões por conversa, do RPC `sessions {botId}` e do evento
// `session` do Hub (host/src/fork/sessoes/api.rs). Módulo do fork, ligado ao BotSync por um gancho (D71).
// UI-01: criar (`sessionCreate`) e enviar numa extra (`send.sessionId`), só quando o Hub anuncia as duas ações.
// UI01b/D372: reconhece o anúncio `engine` (`sessionEngineSet`) e a projeção `session.engine`; o seletor é da C9.
import type { HostClient, HostEvent } from '../client/host-client'
import type { SessionEngine } from '../../shared/models'
import type { HubSession } from '../views/fork/sessoes/estado'
import { Observable } from '../lib/observable.ts'
import { falhaDeControle, type ControleResultado } from '../views/fork/conversa/arquivar/controle.ts'

/**
 * O que o app faz com extras, lido de `sessions.sessionActions` (host/src/fork/sessoes/api.rs `SESSION_ACTIONS`).
 * O `sessionCreate` existe desde o catálogo (#162), antes de o `send` respeitar `sessionId`: criar funcionar não prova
 * que o envio cai na extra. Por isso cada ação só vale anunciada, e um nome desconhecido nunca libera uma conhecida.
 * `close`/`continue` (Sessões PR5, DON-907 C5) = `sessionClose`/`sessionContinue`, só para extras; Hub antigo sem eles
 * não oferece encerrar nem continuar, sem substituto por `newSession` ou pela principal (views/fork/conversa/arquivar).
 * `engine` (D372) = `sessionEngineSet`: só com o anúncio positivo deste link/época; sem ele não há troca por conversa
 * nem substituto por `updateBot`.
 * `choice` (G1b, `sessionChoiceAnswer` v1) = responder cartão de escolha numa extra; não depende de `send`.
 */
export const SESSION_ACTIONS = ['create', 'send', 'engine', 'close', 'continue', 'choice', 'delete'] as const
export type SessionAction = (typeof SESSION_ACTIONS)[number]

/** O anúncio do Hub; ausente, malformado ou de outro tipo = nenhuma ação (o Hub antigo fica só com a principal). */
export function parseSessionActions(raw: unknown): ReadonlySet<SessionAction> {
  if (!Array.isArray(raw) || !raw.every((a) => typeof a === 'string')) return new Set()
  return new Set(SESSION_ACTIONS.filter((a) => raw.includes(a)))
}

/** Por que criar ou enviar numa extra não deu certo. A view traduz o código; nenhum texto do Hub ou do motor vai à tela. */
export type SessionFailure =
  | 'unsupported' // o Hub não anuncia a ação (antigo, anúncio ilegível ou fora do Pessoal)
  | 'linkChanged' // reconectou, trocou de Hub ou de computador durante a espera
  | 'closed' // `session_closed`
  | 'unknownSession' // `unknown_session` / `invalid_session`, ou a sessão sumiu do catálogo
  | 'selectionUnavailable' // `session_selection_unavailable`: falta conta gerenciada com modelo
  | 'offline' // sem link com o computador
  | 'misrouted' // o Hub respondeu com a entrada fora da extra pedida (trocado depois da releitura): ela já existe
  | 'failed' // qualquer outro erro

/** Os códigos que o Hub põe no começo da mensagem de erro (api.rs); o resto do texto nunca é exibido. */
export function failureOf(error: unknown): SessionFailure {
  if (error instanceof SessionError) return error.failure
  const status = (error as { status?: unknown } | null)?.status
  if (status === 0) return 'offline'
  const message = (error as { message?: unknown } | null)?.message
  if (typeof message !== 'string') return 'failed'
  const code = /^[a-z_]+/.exec(message)?.[0]
  switch (code) {
    case 'session_closed': return 'closed'
    case 'unknown_session':
    case 'invalid_session': return 'unknownSession'
    case 'session_selection_unavailable': return 'selectionUnavailable'
    default: return 'failed'
  }
}

export class SessionError extends Error {
  readonly failure: SessionFailure
  constructor(failure: SessionFailure) { super(failure); this.failure = failure }
}

/**
 * Um envio para uma extra, capturado no instante da intenção: a sessão, a época e o próprio link (`HostClient`) de
 * então. "Reconectar" troca o link sem sair de online e antes do `hello` novo; só a identidade do link pega isso.
 */
export interface SessionIntent { readonly chatId: string; readonly id: string; readonly gen: number; readonly client: HostClient }

/** A última falha a mostrar; `sessionId` null = ao criar. `seq` sobe a cada falha nova. */
export interface SessionFailureNotice { chatId: string; sessionId: string | null; failure: SessionFailure; seq: number }

export type CreateResult = { ok: true; session: HubSession } | { ok: false; failure: SessionFailure }

export type SidePanelTab = 'sessions' | 'routines' | 'members' | 'settings' | 'whatsapp' | 'time'

/** O que o Hub disse a uma conversa, e em que link e época: vale só enquanto os dois forem os mesmos. */
interface Learned { client: HostClient; gen: number; actions: ReadonlySet<SessionAction> }

/**
 * A seleção da conversa como o Hub a projeta (D372). Forma inválida, revisão diferente da sessão ou campo fora do
 * contrato = `null` (indisponível): nada é completado nem copiado do bot.
 */
export function engineOf(raw: unknown, revision: unknown): SessionEngine | null {
  const e = raw as Partial<SessionEngine> | null
  if (!e || typeof e !== 'object' || Array.isArray(e)) return null
  const options = e.modelOptions as unknown
  const validOptions = !!options && typeof options === 'object' && !Array.isArray(options)
    && Object.values(options as Record<string, unknown>).every((v) => typeof v === 'string')
  const ok = (e.accountId === null || typeof e.accountId === 'string') && typeof e.backend === 'string'
    && (e.model === null || typeof e.model === 'string') && validOptions && typeof e.fastMode === 'boolean'
    && (e.source === 'botDefault' || e.source === 'own') && typeof e.revision === 'number' && e.revision === revision
  return ok ? { accountId: e.accountId!, backend: e.backend!, model: e.model!, modelOptions: { ...(options as Record<string, string>) }, fastMode: e.fastMode!, source: e.source!, revision: e.revision! } : null
}

/** Uma sessão do catálogo como o app a usa; uma linha sem id/chatId/estado legíveis não entra. */
function sessionOf(raw: unknown, chatId: string): HubSession | null {
  const s = raw as Partial<HubSession> | null
  if (!(s && typeof s === 'object' && typeof s.id === 'string' && s.chatId === chatId && typeof s.principal === 'boolean'
    && (s.state === 'open' || s.state === 'closed'))) return null
  // Hub anterior não manda `engine` (fica ausente); presente, só entra validado.
  return 'engine' in s ? { ...(s as HubSession), engine: engineOf(s.engine, s.revision) } : (s as HubSession)
}

export class ChatSessionsModel extends Observable {
  private deleted = new Map<string, string>()
  private deleting = new Map<string, Promise<{ ok: boolean; falha?: import('../views/fork/conversa/arquivar/controle').ControleFalha }>>()
  private lists = new Map<string, HubSession[]>()
  private loading = new Map<string, Promise<void>>()
  private learned = new Map<string, Learned>()
  private creating = new Map<string, Promise<CreateResult>>()
  /** DON-907 C5: encerrar/continuar em andamento, por sessão (o botão fica desabilitado até a resposta). */
  private controlling = new Map<string, Promise<ControleResultado>>()
  private hostId: string | null = null
  private gen = 0
  /** O pedido de abrir o painel direito numa aba (dois cliques na barra, ⌃↩, menu "Sessões de …"). */
  panelRequest: { chatId: string; tab: SidePanelTab; seq: number } | null = null
  failure: SessionFailureNotice | null = null
  /** O "Nova sessão" do menu do bot: a conversa abre e cria por `useNewSession` (views/fork/sessoes/ganchos.tsx). */
  newRequest: { chatId: string; seq: number; at: number } | null = null
  private newSeq = 0
  private newConsumed = 0
  private link: () => Promise<HostClient>
  /** O link atual (null sem conexão ou com app/Hub incompatíveis): uma resposta de outro link não vale. */
  private live: () => HostClient | null

  private readonly onDeleted: (event: SessionDeleted) => void

  constructor(link: () => Promise<HostClient>, live: () => HostClient | null = () => null, onDeleted: (event: SessionDeleted) => void = () => {}) {
    super()
    this.link = link
    this.live = live
    this.onDeleted = onDeleted
  }

  receive(event: HostEvent) {
    if (event.type === 'hello') {
      // Outro Hub ou uma reconexão: relê o que estiver aberto, sem guardar listas de outro Hub nem o anúncio antigo.
      if (this.hostId !== event.hostId) { this.lists.clear(); this.deleted.clear() }
      // A primeira leitura pode estar esperando ready()/RPC: ainda não existe em lists.
      // Ela também precisa ser refeita, pois sua resposta pertence à geração anterior.
      const reload = new Set([...this.lists.keys(), ...this.loading.keys()])
      this.hostId = event.hostId
      this.gen++
      this.loading.clear()
      this.learned.clear()
      // Um "Nova sessão" do menu pedido antes da reconexão não roda depois dela.
      this.newRequest = null
      for (const chatId of reload) void this.load(chatId)
      this.changed()
    } else if (event.type === 'sessionDeleted') {
      this.deleted.set(event.sessionId, event.chatId)
      const rows = this.lists.get(event.chatId)
      if (rows) this.lists.set(event.chatId, rows.filter((s) => s.id !== event.sessionId))
      this.changed()
    } else if (event.type === 'session') {
      if (this.isDeleted(event.session.chatId, event.session.id)) return
      const list = this.lists.get(event.session.chatId)
      if (!list) return
      const session = sessionOf(event.session, event.session.chatId)
      if (!session) return
      const at = list.findIndex((s) => s.id === session.id)
      if (at >= 0 && list[at]!.revision > session.revision) return
      this.lists.set(session.chatId, at >= 0 ? list.map((s, i) => (i === at ? session : s)) : [...list, session])
      this.changed()
    }
  }

  /** D545: invalida menus e confirmações abertos antes de um novo hello. */
  get epoch() { return this.gen }

  /** As sessões da conversa; null até a primeira resposta (a view pede com `load`). */
  of(chatId: string): readonly HubSession[] | null { return this.lists.get(chatId) ?? null }

  load(chatId: string): Promise<void> {
    const running = this.loading.get(chatId)
    if (running) return running
    const gen = this.gen
    const request = (async () => {
      try {
        const client = await this.link()
        const res = await client.call<{ sessions?: unknown; sessionActions?: unknown }>('sessions', { botId: chatId })
        if (gen !== this.gen) return
        const rows = Array.isArray(res.sessions) ? res.sessions : []
        this.lists.set(chatId, rows.map((s) => sessionOf(s, chatId)).filter((s): s is HubSession => s !== null && !this.isDeleted(chatId, s.id)))
        this.learned.set(chatId, { client, gen, actions: parseSessionActions(res.sessionActions) })
      } catch {
        // Hub sem o catálogo (anterior ao 2a) ou fora do ar: a conversa fica só com a aba principal.
        if (gen === this.gen) {
          this.learned.delete(chatId)
          if (!this.lists.has(chatId)) this.lists.set(chatId, [])
        }
      } finally {
        this.changed()
      }
    })()
    this.loading.set(chatId, request)
    void request.finally(() => { if (this.loading.get(chatId) === request) this.loading.delete(chatId) })
    return request
  }

  /** O Hub deste link, nesta época, anunciou a ação para esta conversa? A view só mostra o controle quando sim. */
  can(chatId: string, action: SessionAction): boolean {
    const learned = this.learned.get(chatId)
    const client = this.live()
    return !!learned && !!client && learned.gen === this.gen && learned.client === client && learned.actions.has(action)
  }

  /** Criando uma sessão nesta conversa agora (o "+" e "Nova sessão" ficam ocupados). */
  busy(chatId: string) { return this.creating.has(chatId) }

  /**
   * Cria uma sessão vazia na conversa, sem mandar nada. Dois cliques seguidos devolvem o mesmo pedido. A resposta só
   * vale no mesmo link e época do pedido: depois de reconectar, a sessão (se o Hub a criou) volta pelo catálogo, sem
   * abrir sozinha.
   */
  create(chatId: string): Promise<CreateResult> {
    const running = this.creating.get(chatId)
    if (running) return running
    const gen = this.gen
    const client = this.live()
    const request = (async (): Promise<CreateResult> => {
      if (!client || !this.can(chatId, 'create')) return this.fail(chatId, null, 'unsupported')
      try {
        const res = await client.call<{ session?: unknown }>('sessionCreate', { botId: chatId })
        if (gen !== this.gen || this.live() !== client) return this.fail(chatId, null, 'linkChanged')
        const session = sessionOf(res.session, chatId)
        if (!session || session.principal) return this.fail(chatId, null, 'failed')
        this.receive({ type: 'session', session })
        this.dismiss(chatId)
        return { ok: true, session }
      } catch (error) {
        return this.fail(chatId, null, failureOf(error))
      }
    })()
    this.creating.set(chatId, request)
    this.changed()
    void request.finally(() => {
      if (this.creating.get(chatId) === request) this.creating.delete(chatId)
      this.changed()
    })
    return request
  }

  /** Encerrando ou continuando esta sessão agora. */
  controlBusy(sessionId: string) { return this.controlling.has(sessionId) || this.deleting.has(sessionId) }

  /**
   * DON-907 C5: `sessionClose` (extra aberta) ou `sessionContinue` (extra encerrada), com a revisão que o app viu.
   * Só a resposta confirmada muda o catálogo; erro ou resposta incerta (`session_control_pending`) relê `sessions` e
   * nunca mostra encerrada por otimismo. Continuar devolve a sessão nova para a view abrir.
   */
  control(chatId: string, sessionId: string, action: 'close' | 'continue', context?: { hubId: string; spaceId: string; client: object; epoch: number }): Promise<ControleResultado> {
    if (context && (!context.hubId.trim() || !context.spaceId.trim() || context.client !== this.live() || context.epoch !== this.gen)) {
      return Promise.resolve({ ok: false, falha: 'linkChanged' })
    }
    const running = this.controlling.get(sessionId)
    if (running) return running
    const gen = this.gen
    const client = this.live()
    const row = this.lists.get(chatId)?.find((s) => s.id === sessionId)
    const request = (async (): Promise<ControleResultado> => {
      if (!client || !this.can(chatId, action)) return { ok: false, falha: 'unsupported' }
      if (!row || row.principal || row.state !== (action === 'close' ? 'open' : 'closed')) return { ok: false, falha: 'failed' }
      try {
        const res = await client.call<{ session?: unknown; source?: unknown }>(action === 'close' ? 'sessionClose' : 'sessionContinue', { botId: chatId, sessionId, expectedRevision: row.revision, ...(context ? { hubId: context.hubId, spaceId: context.spaceId } : {}) })
        if (gen !== this.gen || this.live() !== client) return { ok: false, falha: 'linkChanged' }
        const session = sessionOf(res.session, chatId)
        const valid = action === 'close' ? session?.id === sessionId && session.state === 'closed' : !!session && !session.principal && session.id !== sessionId && session.state === 'open'
        if (!session || !valid) {
          await this.reload(chatId)
          return { ok: false, falha: 'failed' }
        }
        const source = action === 'continue' ? sessionOf(res.source, chatId) : null
        const confirmed = source?.id === sessionId ? [session, source] : [session]
        // Uma leitura já em andamento começou antes desta confirmação: quando ela chegar, a confirmação vale de novo.
        const stale = this.loading.get(chatId)
        for (const s of confirmed) this.receive({ type: 'session', session: s })
        if (stale) {
          await stale
          // Reconectou durante a espera: a resposta é da época anterior; a sessão volta pelo catálogo novo.
          if (gen !== this.gen || this.live() !== client) return { ok: false, falha: 'linkChanged' }
          for (const s of confirmed) this.receive({ type: 'session', session: s })
        }
        return { ok: true, session }
      } catch (error) {
        const falha = falhaDeControle(error)
        if (falha !== 'offline' && gen === this.gen) await this.reload(chatId)
        return { ok: false, falha }
      }
    })()
    this.controlling.set(sessionId, request)
    this.changed()
    void request.finally(() => {
      if (this.controlling.get(sessionId) === request) this.controlling.delete(sessionId)
      this.changed()
    })
    return request
  }

  /** Tombstone confirmado: inclusive respostas de histórico anteriores à exclusão são descartadas. */
  isDeleted(chatId: string, sessionId: string) { return this.deleted.get(sessionId) === chatId }

  remove(chatId: string, sessionId: string, spaceId: string, expectedRevision?: number, context?: { hubId: string; client: object; epoch: number }): Promise<{ ok: boolean; falha?: import('../views/fork/conversa/arquivar/controle').ControleFalha }> {
    if (context && (context.client !== this.live() || context.epoch !== this.gen || context.hubId !== this.hostId)) {
      return Promise.resolve({ ok: false, falha: 'linkChanged' })
    }
    const running = this.deleting.get(sessionId)
    if (running) return running
    const client = this.live(), gen = this.gen, hostId = this.hostId
    const row = this.of(chatId)?.find((s) => s.id === sessionId)
    const request = (async () => {
      if (!client || !hostId || !row || row.principal || !spaceId || !this.can(chatId, 'delete') || this.controlling.has(sessionId)) return { ok: false, falha: 'unsupported' as const }
      if (expectedRevision !== undefined && row.revision !== expectedRevision) return { ok: false, falha: 'conflict' as const }
      try {
        const raw = await client.call('sessionDelete', { hubId: hostId, spaceId, botId: chatId, sessionId, expectedRevision: row.revision })
        if (gen !== this.gen || this.live() !== client) return { ok: false, falha: 'linkChanged' as const }
        const event = sessionDeletedOf(raw)
        if (!event || event.chatId !== chatId || event.sessionId !== sessionId || event.spaceId !== spaceId) return { ok: false, falha: 'failed' as const }
        this.receive(event)
        this.onDeleted(event)
        return { ok: true }
      } catch (error) {
        if (gen !== this.gen || this.live() !== client) return { ok: false, falha: 'linkChanged' as const }
        await this.reload(chatId)
        return { ok: false, falha: falhaDeControle(error) }
      }
    })()
    this.deleting.set(sessionId, request)
    this.changed()
    void request.finally(() => { if (this.deleting.get(sessionId) === request) this.deleting.delete(sessionId); this.changed() })
    return request
  }

  /** Relê o catálogo depois de uma leitura que já estivesse em andamento (ela pode ser de antes da mudança). */
  private async reload(chatId: string) {
    await this.loading.get(chatId)
    await this.load(chatId)
  }

  /** A intenção de enviar numa extra (ou `choice`: responder o cartão), capturada agora; null se este Hub/link não recebe. */
  intent(chatId: string, sessionId: string, action: SessionAction = 'send'): SessionIntent | null {
    const client = this.live()
    if (sessionId === chatId || !client || !this.can(chatId, action)) return null
    const session = this.lists.get(chatId)?.find((s) => s.id === sessionId)
    return session && !session.principal && session.state === 'open' ? { chatId, id: sessionId, gen: this.gen, client } : null
  }

  /**
   * A intenção ainda vale neste link? Mesma época, o mesmo link da intenção e ele ainda é o atual. Um link novo nunca
   * herda a intenção (o replay do BotStore recusa aqui), e uma resposta de antes de reconectar não é adotada.
   */
  current(intent: SessionIntent, client: HostClient) { return intent.gen === this.gen && intent.client === client && this.live() === client }

  /**
   * Logo antes de enviar (depois dos uploads), no mesmo link que vai enviar: relê o catálogo e exige `send` anunciado,
   * a sessão aberta e a mesma época antes e depois da releitura. Um Hub antigo ou um link novo recusam aqui, e nada é
   * enviado; nunca cai na principal. Não cobre um Hub trocado no mesmo link entre esta resposta e a chegada do `send`.
   */
  async guardSend(intent: SessionIntent, client: HostClient): Promise<void> {
    if (!this.current(intent, client)) throw new SessionError('linkChanged')
    const res = await client.call<{ sessions?: unknown; sessionActions?: unknown }>('sessions', { botId: intent.chatId })
    if (!this.current(intent, client)) throw new SessionError('linkChanged')
    if (!parseSessionActions(res.sessionActions).has('send')) {
      this.learned.delete(intent.chatId)
      throw new SessionError('unsupported')
    }
    const rows = Array.isArray(res.sessions) ? res.sessions : []
    const session = rows.map((s) => sessionOf(s, intent.chatId)).find((s) => s?.id === intent.id)
    if (!session || session.principal) throw new SessionError('unknownSession')
    if (session.state !== 'open') throw new SessionError('closed')
  }

  /** Para o BotStore, sem importar este módulo em tempo de execução (os testes de voz carregam o BotStore isolado). */
  error(failure: SessionFailure) { return new SessionError(failure) }
  failureOf(error: unknown) { return failureOf(error) }

  /** Guarda a falha para a view mostrar (código, não texto). */
  fail(chatId: string, sessionId: string | null, failure: SessionFailure): { ok: false; failure: SessionFailure } {
    this.failure = { chatId, sessionId, failure, seq: (this.failure?.seq ?? 0) + 1 }
    this.changed()
    return { ok: false, failure }
  }

  dismiss(chatId: string) {
    if (this.failure?.chatId !== chatId) return
    this.failure = null
    this.changed()
  }

  requestNew(chatId: string) {
    this.newRequest = { chatId, seq: ++this.newSeq, at: Date.now() }
    this.changed()
  }

  /**
   * Consome o pedido do menu, uma vez, nesta origem (este modelo = um computador/Hub). Vale para qualquer consumidor
   * desta conversa e sobrevive a desmontar e montar de novo: voltar à conversa não cria de novo.
   */
  takeNew(chatId: string, ttl: number): boolean {
    const request = this.newRequest
    if (!request || request.chatId !== chatId || request.seq <= this.newConsumed) return false
    this.newConsumed = request.seq
    return Date.now() - request.at < ttl
  }

  requestPanel(chatId: string, tab: SidePanelTab = 'sessions') {
    this.panelRequest = { chatId, tab, seq: (this.panelRequest?.seq ?? 0) + 1 }
    this.changed()
  }
}
