import {
  graphOf, parseTimesSnapshot, reconcileRoster, singleLeader, timesErrorText, timesFailure,
  type TimesChange, type TimesGraph, type TimesSaveAnswer, type TimesSnapshot,
} from '../../../../shared/fork-times.ts'
import { Observable } from '../../../lib/observable.ts'
import { cru, ErroFrase, frase, juntas, type Frase } from '../../../lib/fork-i18n/frase.ts'
import { ExecucaoMarketing } from './modelo-marketing.ts'

// Times (components/TimesView): o que a tela lê e pede ao Hub de um space.
// Toda chamada parte de uma origem fixa: o Hub (hello.hostId), o link, a geração e o space. Resposta de outra origem
// (outro Hub, link novo, leitura mais antiga, A→B→A) é descartada antes de pintar. Sem link, só leitura com o último
// estado; nunca troca para outro Hub. Prévia não grava; só Salvar cria revisão nova. Um Salvar sem resposta fica
// guardado com a mesma chave: Tentar de novo reenvia a mesma intenção, e o Hub devolve o mesmo recibo se já gravou.

/** Um evento do stream do space: só o tipo, o space e a revisão importam aqui. */
export interface TimesEvent { type: string; spaceId?: string; revision?: number }

/** O que a sessão usa do link de um computador (HostClient). */
export interface TimesLink {
  call(method: string, body?: unknown, timeoutMs?: number): Promise<unknown>
  inSpace(spaceId: string): { events(since: number, client: string, onEvent: (e: TimesEvent) => void, onEnd: (error: unknown) => void): () => void }
}

/** O que a sessão lê da store do computador (BotStore); nunca escreve nela. */
export interface TimesSource {
  readonly hello: { hostId: string; rev: number } | null
  readonly connectedClient: TimesLink | null
  readonly principalEpoch: number
  readonly spaces: { personalSpaceId: string } | null
  /** Os bots que a store espelha: mudou o roster (nome, rosto, apagado), o catálogo do Hub pode ter mudado. */
  readonly bots?: ReadonlyMap<string, RosterBot>
  subscribe(listener: () => void): () => void
}

/** O que o roster da store diz de um bot, só para perceber mudança de catálogo (nunca vai para a tela). */
export interface RosterBot { id: string; name: string; kind?: string; title?: string; hidden?: boolean; spaceId?: string; avatarColor?: string; avatarShape?: string; avatarConfig?: unknown; mexeriquinha?: unknown }

/** O hello confirma a inscrição: reler fecha a janela entre a leitura inicial e o início dos avisos ao vivo. */
const REREAD_EVENTS = new Set(['hello', 'times', 'bot', 'botDeleted', 'resync'])
const REREAD_DELAY = 250
const RELISTEN_MIN = 2_000
const RELISTEN_MAX = 30_000

export function rosterKey(bots: ReadonlyMap<string, RosterBot> | undefined): string {
  if (!bots) return ''
  return JSON.stringify([...bots.values()].map((b) => [b.id, b.name, b.kind ?? '', b.title ?? '', b.hidden ?? false, b.spaceId ?? '', b.avatarColor ?? '', b.avatarShape ?? '', b.avatarConfig ?? null, b.mexeriquinha ?? null]).sort((a, b) => String(a[0]).localeCompare(String(b[0]))))
}

interface Origin { source: TimesSource; hostId: string; client: TimesLink; epoch: number; gen: number }

/** Uma gravação pedida: a chave e o corpo não mudam até o Hub responder ou a pessoa descartar. */
export interface SaveIntent { clientRequestId: string; body: TimesChange & { clientRequestId: string } }

export type TimesPhase =
  | { kind: 'loading' }
  | { kind: 'ready' }
  | { kind: 'error'; failure: ReturnType<typeof timesFailure>; message: Frase }
  /** O Hub deste computador mudou: a sessão para, sem cair em outro Hub. */
  | { kind: 'moved' }

export type TimesCheck =
  | { kind: 'idle' }
  | { kind: 'checking'; label: Frase }
  | { kind: 'refused'; label: Frase; message: Frase }

export type TimesSave =
  | { kind: 'idle' }
  | { kind: 'saving'; intent: SaveIntent }
  /** Sem resposta: não se sabe se gravou. O rascunho fica travado até Tentar de novo ou Descartar. */
  | { kind: 'unknown'; intent: SaveIntent }
  | { kind: 'conflict'; message: Frase }
  | { kind: 'error'; message: Frase }

const PREVIEW_TIMEOUT = 20_000
const SAVE_TIMEOUT = 30_000

export class TimesSession extends Observable {
  phase: TimesPhase = { kind: 'loading' }
  link: 'live' | 'offline' = 'live'
  /** O estado do Hub, a única fonte canônica. */
  snapshot: TimesSnapshot | null = null
  /** Mudanças conferidas pelo Hub e ainda não salvas; null sem mudança. */
  draft: TimesGraph | null = null
  /** A revisão e o catálogo sobre os quais o rascunho foi conferido. */
  base: { revision: number; catalogVersion: string } | null = null
  /** O Hub tem uma revisão mais nova que a base do rascunho (outro aparelho salvou). */
  stale = false
  check: TimesCheck = { kind: 'idle' }
  save: TimesSave = { kind: 'idle' }
  /** Aviso curto do último resultado (Salvo, relido…), para `role="status"`; frase traduzida na tela (D507). */
  notice: Frase | null = null
  /** Mudanças conferidas no rascunho, para a faixa de edição. */
  changes = 0
  /** Os avisos de mudança deste space pararam (só o stream do space); a sessão tenta de novo sozinha. */
  events: 'live' | 'paused' = 'live'
  /** O modelo Equipe Marketing: uma execução por sessão, que sobrevive a fechar a janela (D353, R1/R2). */
  readonly marketing = new ExecucaoMarketing(() => this.changed())

  private origin: Origin | null = null
  /** O Hub de onde vieram o snapshot e o rascunho; outro Hub nunca os herda. */
  private hostId: string | null = null
  private spaceId: string | null = null
  private gen = 0
  private reads = 0
  private unwatch: (() => void) | null = null
  private stopEvents: (() => void) | null = null
  private now: () => string
  private roster = ''
  private rereadTimer: ReturnType<typeof setTimeout> | null = null
  private relistenTimer: ReturnType<typeof setTimeout> | null = null
  private relistenDelay = RELISTEN_MIN
  /**
   * A operação (prévia ou gravação) que ainda pode mudar a tela. Fechar, recusa de dono ou outra operação a trocam:
   * a resposta de uma operação antiga nunca sobrescreve o estado de uma mais nova (por exemplo, um Tentar de novo).
   */
  private op = 0

  constructor(uuid: () => string = () => crypto.randomUUID()) {
    super()
    this.now = uuid
  }

  /** O space desta sessão: o pedido, ou o Pessoal canônico que o Hub devolveu. */
  get space() { return this.spaceId }

  /** Editar exige snapshot lido, link vivo, rascunho sobre a versão atual e nada em curso ou sem resposta. */
  get editable() {
    return this.phase.kind === 'ready' && this.link === 'live' && !!this.snapshot && !this.stale && this.check.kind !== 'checking'
      && this.save.kind !== 'saving' && this.save.kind !== 'unknown'
  }

  /** O grafo que a tela desenha: o rascunho, se houver, senão o do Hub. Recusa de dono não mostra nada. */
  get graph(): TimesGraph | null {
    if (this.phase.kind === 'error' && (this.phase.failure === 'owner' || this.phase.failure === 'old_hub')) return null
    return this.draft ?? this.snapshot
  }

  /** Abre o space `spaceId` (null: o Pessoal canônico da store) do Hub desta store. */
  open(source: TimesSource, spaceId: string | null) {
    this.release()
    this.spaceId = spaceId ?? source.spaces?.personalSpaceId ?? null
    this.unwatch = source.subscribe(() => this.watch(source))
    this.connect(source)
  }

  /**
   * Fechar a tela não perde nada: uma gravação em curso vira “sem resposta”, com a mesma intenção, para Tentar de
   * novo ao reabrir; uma conferência em curso termina sem mudar o rascunho já aceito. Respostas antigas são ignoradas.
   */
  close() {
    this.release()
    this.gen++
    this.op++
    this.origin = null
    if (this.save.kind === 'saving') this.save = { kind: 'unknown', intent: this.save.intent }
    if (this.check.kind === 'checking') this.check = { kind: 'idle' }
    this.changed()
  }

  /** Lê de novo pelo mesmo link (depois de evento, conflito ou pedido da pessoa). */
  reread() {
    const origin = this.origin
    if (origin && this.current(origin)) void this.read(origin)
  }

  /** Lê o Hub agora e espera (o modelo Marketing usa para ver os bots que acabou de criar). Null sem leitura. */
  async sync(): Promise<TimesSnapshot | null> {
    const origin = this.origin
    if (!origin || !this.current(origin)) return null
    await this.read(origin)
    return this.current(origin) && this.phase.kind === 'ready' ? this.snapshot : null
  }

  /**
   * Uma mudança da tela: o Hub confere o grafo inteiro resultante, sem gravar. Aceita, vira o rascunho; recusada,
   * só ela volta e o rascunho anterior continua. Retorna se entrou no rascunho.
   */
  async propose(rotulo: Frase | string, change: (g: TimesGraph) => TimesGraph): Promise<boolean> {
    // D507: o rótulo vira frase; quem ainda manda texto (criar-bot-equipe) entra como dado.
    const label = typeof rotulo === 'string' ? cru(rotulo) : rotulo
    const origin = this.origin
    const base = this.draft ? this.base : this.snapshot && { revision: this.snapshot.revision, catalogVersion: this.snapshot.catalogVersion }
    const from = this.graph
    if (!this.editable || !origin || !base || !from) return false
    let candidate: TimesGraph
    try {
      // D340: a prévia local já mostra o membro único como chefe, como o Hub vai devolver.
      candidate = singleLeader(change(graphOf(from)), from, this.snapshot?.primaryBotId ?? null)
    } catch (e) {
      this.check = { kind: 'refused', label, message: e instanceof ErroFrase ? e.frase : cru(e instanceof Error ? e.message : String(e)) }
      this.changed()
      return false
    }
    return this.preview(origin, label, candidate, base)
  }

  /** Depois de um conflito: confere o mesmo rascunho sobre a versão atual do Hub. Nada é gravado. */
  async reconfirm(): Promise<boolean> {
    const origin = this.origin
    if (!this.draft || !this.snapshot || !origin || this.phase.kind !== 'ready' || this.link !== 'live' || this.check.kind === 'checking' || this.save.kind === 'saving' || this.save.kind === 'unknown') return false
    // O elenco do Hub pode ter mudado: bot novo entra como o Hub o projetou; saída ou principal trocada voltam como motivo.
    const fitted = reconcileRoster(this.draft, this.snapshot)
    if ('problem' in fitted) {
      this.check = { kind: 'refused', label: frase('times.quadro.conferirDeNovo'), message: fitted.problem }
      this.changed()
      return false
    }
    return this.preview(origin, frase('times.quadro.conferirDeNovo'), fitted.graph, { revision: this.snapshot.revision, catalogVersion: this.snapshot.catalogVersion }, false)
  }

  /** Grava o rascunho conferido. A chave nasce aqui e acompanha a intenção até o fim. */
  async commit(): Promise<boolean> {
    if (!this.draft || !this.base || !this.editable || this.stale) return false
    const clientRequestId = this.now()
    const intent: SaveIntent = { clientRequestId, body: { ...this.change(this.draft, this.base), clientRequestId } }
    return this.send(intent)
  }

  /** Sem resposta da última gravação: reenvia a mesma intenção, com a mesma chave. Nunca é outro Salvar. */
  async retrySave(): Promise<boolean> {
    if (this.save.kind !== 'unknown' || this.link !== 'live' || !this.origin) return false
    return this.send(this.save.intent)
  }

  /** Descarta o rascunho (e uma gravação sem resposta) e relê o Hub. */
  discard() {
    if (this.save.kind === 'saving' || this.check.kind === 'checking') return
    this.draft = null
    this.base = null
    this.stale = false
    this.changes = 0
    this.check = { kind: 'idle' }
    this.save = { kind: 'idle' }
    this.notice = null
    this.changed()
    this.reread()
  }

  dismissCheck() {
    if (this.check.kind !== 'refused') return
    this.check = { kind: 'idle' }
    this.changed()
  }

  // MARK: origem

  private connect(source: TimesSource) {
    this.stopEvents?.()
    this.stopEvents = null
    const gen = ++this.gen
    const hostId = source.hello?.hostId
    const client = source.connectedClient
    // Outro Hub neste computador (mesmo sem link agora): nada do anterior fica à vista.
    if (hostId && this.hostId && this.hostId !== hostId) this.forget()
    if (hostId) this.hostId = hostId
    if (!hostId || !client) {
      this.origin = this.origin ? { ...this.origin, gen } : null
      this.link = 'offline'
      if (!this.snapshot) this.phase = { kind: 'error', failure: 'unknown', message: frase('times.quadro.sessao.semConexao') }
      this.changed()
      return
    }
    const origin: Origin = { source, hostId, client, epoch: source.principalEpoch, gen }
    this.origin = origin
    this.roster = rosterKey(source.bots)
    this.link = 'live'
    this.events = 'live'
    this.relistenDelay = RELISTEN_MIN
    if (!this.snapshot) this.phase = { kind: 'loading' }
    this.changed()
    void this.read(origin)
    if (this.spaceId) this.listen(origin, this.spaceId)
  }

  private listen(origin: Origin, spaceId: string) {
    this.stopEvents = origin.client.inSpace(spaceId).events(origin.source.hello?.rev ?? 0, 'desktop', (e) => {
      if (!this.current(origin)) return
      this.relistenDelay = RELISTEN_MIN
      if (!REREAD_EVENTS.has(e.type)) return
      if (e.type === 'times') {
        if (e.spaceId !== spaceId) return
        if (typeof e.revision === 'number' && this.snapshot && e.revision <= this.snapshot.revision) return
      }
      this.scheduleRead(origin)
    }, () => {
      // Só o stream deste space acabou: a tela diz isso e, na mesma origem, relê e se inscreve de novo, com espera
      // crescente. Outra origem (link novo, outro Hub, tela fechada) nunca é reaberta daqui.
      if (!this.current(origin)) return
      this.events = 'paused'
      this.changed()
      if (this.relistenTimer) clearTimeout(this.relistenTimer)
      this.relistenTimer = setTimeout(() => {
        this.relistenTimer = null
        if (!this.current(origin)) return
        this.events = 'live'
        this.stopEvents?.()
        this.listen(origin, spaceId)
        void this.read(origin)
      }, this.relistenDelay)
      this.relistenDelay = Math.min(RELISTEN_MAX, this.relistenDelay * 2)
    })
  }

  /** A store mudou: outro Hub encerra; link novo do mesmo Hub relê; sem link, só leitura. */
  private watch(source: TimesSource) {
    const origin = this.origin
    const hostId = source.hello?.hostId ?? null
    if (origin && hostId && hostId !== origin.hostId) {
      this.release()
      this.gen++
      this.origin = null
      this.forget()
      this.phase = { kind: 'moved' }
      this.changed()
      return
    }
    const client = source.connectedClient
    if (!client) {
      if (this.link === 'live') this.connect(source)
      return
    }
    if (!origin || origin.client !== client || origin.epoch !== source.principalEpoch || this.link === 'offline') return this.connect(source)
    // Mesmo link: um bot novo, apagado, renomeado ou com outro rosto muda o catálogo; relê (um pouco depois).
    const roster = rosterKey(source.bots)
    if (roster !== this.roster) {
      this.roster = roster
      this.scheduleRead(origin)
    }
  }

  /** Junta vários avisos seguidos numa leitura só. */
  private scheduleRead(origin: Origin) {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = setTimeout(() => {
      this.rereadTimer = null
      if (this.current(origin)) void this.read(origin)
    }, REREAD_DELAY)
  }

  private current(origin: Origin) {
    const { source } = origin
    return this.origin === origin && this.gen === origin.gen && source.hello?.hostId === origin.hostId
      && source.connectedClient === origin.client && source.principalEpoch === origin.epoch
  }

  /** Tudo o que veio de um Hub: some quando o computador passa a ser outro Hub. */
  private forget() {
    this.snapshot = null
    this.draft = null
    this.base = null
    this.stale = false
    this.changes = 0
    this.save = { kind: 'idle' }
    this.check = { kind: 'idle' }
    this.notice = null
    this.phase = { kind: 'loading' }
  }

  private release() {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = null
    if (this.relistenTimer) clearTimeout(this.relistenTimer)
    this.relistenTimer = null
    this.stopEvents?.()
    this.stopEvents = null
    this.unwatch?.()
    this.unwatch = null
  }

  private change(graph: TimesGraph, base: { revision: number; catalogVersion: string }): TimesChange {
    return { ...(this.spaceId ? { spaceId: this.spaceId } : {}), expectedRevision: base.revision, catalogVersion: base.catalogVersion, ...graphOf(graph) }
  }

  // MARK: chamadas

  private async read(origin: Origin) {
    const read = ++this.reads
    try {
      const answer = await origin.client.call('donnaTimes', this.spaceId ? { spaceId: this.spaceId } : {})
      if (!this.current(origin) || read !== this.reads) return
      this.accept(parseTimesSnapshot(answer, this.spaceId))
      this.phase = { kind: 'ready' }
    } catch (e) {
      if (!this.current(origin) || read !== this.reads) return
      const failure = timesFailure(e)
      if (failure === 'owner') return this.denied(e)
      if (!this.snapshot || failure === 'old_hub') {
        this.phase = { kind: 'error', failure, message: timesErrorText(e, 'read') }
        if (failure === 'old_hub') this.snapshot = null
      } else {
        this.notice = frase('times.quadro.sessao.releituraFalhou')
      }
    }
    this.changed()
  }

  /** Aceita um snapshot do Hub, nunca um mais antigo que o atual. */
  private accept(next: TimesSnapshot) {
    if (this.snapshot && next.revision < this.snapshot.revision) return
    if (!this.spaceId) this.spaceId = next.spaceId
    this.snapshot = next
    this.stale = !!this.draft && !!this.base && (next.revision !== this.base.revision || next.catalogVersion !== this.base.catalogVersion)
  }

  private async preview(origin: Origin, label: Frase, candidate: TimesGraph, base: { revision: number; catalogVersion: string }, counts = true) {
    const op = ++this.op
    this.check = { kind: 'checking', label }
    this.notice = null
    this.changed()
    try {
      const answer = await origin.client.call('donnaTimesPreview', this.change(candidate, base), PREVIEW_TIMEOUT)
      if (op !== this.op) return false
      if (!this.current(origin)) return this.dropped(origin, label)
      const checked = parseTimesSnapshot(answer, this.spaceId)
      if ((answer as { preview?: unknown }).preview !== true) throw new Error('times_invalid_answer')
      this.draft = graphOf(checked)
      this.base = { revision: checked.revision, catalogVersion: checked.catalogVersion }
      // A prévia passou pelo CAS: a base é a versão atual do Hub. Um snapshot local mais antigo só é relido.
      this.stale = false
      if (this.snapshot && (this.snapshot.revision !== checked.revision || this.snapshot.catalogVersion !== checked.catalogVersion)) this.reread()
      if (counts) this.changes++
      this.check = { kind: 'idle' }
      // Conferido sobre a versão atual: um conflito ou erro anterior de Salvar deixa de valer.
      if (this.save.kind === 'conflict' || this.save.kind === 'error') this.save = { kind: 'idle' }
      this.notice = frase('times.quadro.sessao.conferido', { mudanca: label })
      this.changed()
      return true
    } catch (e) {
      if (op !== this.op) return false
      if (!this.current(origin)) return this.dropped(origin, label)
      const failure = timesFailure(e)
      if (failure === 'owner') { this.denied(e); return false }
      this.check = { kind: 'refused', label, message: failure === 'unknown' ? frase('times.quadro.sessao.semResposta') : timesErrorText(e) }
      this.changed()
      if (failure === 'conflict' || failure === 'catalog') this.reread()
      return false
    }
  }

  /**
   * O Hub negou a autoridade de dono: nada de Times fica à vista (snapshot, rascunho, painel ou folha) e nenhuma
   * resposta atrasada pode recolocá-los. Só uma nova leitura autorizada mostra o quadro de novo.
   */
  private denied(e: unknown) {
    this.op++
    this.reads++
    this.forget()
    this.phase = { kind: 'error', failure: 'owner', message: timesErrorText(e) }
    this.changed()
  }

  /** A resposta chegou para uma origem que já não vale: nada muda na tela. */
  private dropped(origin: Origin, label: Frase) {
    if (this.origin === null || this.origin.hostId !== origin.hostId) return false
    this.check = { kind: 'refused', label, message: frase('times.quadro.sessao.conexaoMudou') }
    this.changed()
    return false
  }

  private async send(intent: SaveIntent): Promise<boolean> {
    const origin = this.origin
    if (!origin || !this.current(origin)) return false
    const op = ++this.op
    this.save = { kind: 'saving', intent }
    this.notice = null
    this.changed()
    try {
      const answer = await origin.client.call('donnaTimesSave', intent.body, SAVE_TIMEOUT) as Partial<TimesSaveAnswer> | null
      if (op !== this.op) return false
      if (!this.current(origin)) return this.lost(origin, intent)
      const receipt = answer?.receipt
      // O recibo é da intenção enviada; outro recibo não confirma esta gravação.
      if (!answer || !receipt || receipt.clientRequestId !== intent.clientRequestId || typeof receipt.revision !== 'number') throw new Error('times_invalid_answer')
      const snapshot = parseTimesSnapshot(answer.snapshot, this.spaceId)
      this.draft = null
      this.base = null
      this.stale = false
      this.changes = 0
      this.accept(snapshot)
      this.save = { kind: 'idle' }
      this.notice = frase('times.quadro.sessao.salvo')
      this.changed()
      return true
    } catch (e) {
      if (op !== this.op) return false
      if (!this.current(origin)) return this.lost(origin, intent)
      const failure = timesFailure(e)
      if (failure === 'owner') { this.denied(e); return false }
      if (failure === 'unknown') this.save = { kind: 'unknown', intent }
      else if (failure === 'conflict' || failure === 'catalog') {
        this.save = { kind: 'conflict', message: juntas(timesErrorText(e), frase('times.quadro.sessao.rascunhoContinua')) }
        this.reread()
      } else this.save = { kind: 'error', message: timesErrorText(e) }
      this.changed()
      return false
    }
  }

  /** O link mudou durante a gravação: o resultado é desconhecido, e a mesma intenção espera Tentar de novo. */
  private lost(origin: Origin, intent: SaveIntent) {
    if (this.origin === null || this.origin.hostId !== origin.hostId) return false
    this.save = { kind: 'unknown', intent }
    this.changed()
    return false
  }
}
