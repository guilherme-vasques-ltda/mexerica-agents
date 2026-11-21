import {
  PROPOSALS_EVENT, decideBody, newUuid, parseProposalDecideAnswer, parseTaskProposals,
  type ProposalDecideBody, type ProposalDecision, type TaskProposal, type TaskProposalsSnapshot,
} from '../../../../shared/fork-propostas.ts'
import { Observable } from '../../../lib/observable.ts'
import type { TowerLink, TowerSource } from '../torre/torre-session.ts'

const REREAD_DELAY = 250
const RELISTEN_MIN = 2_000
const RELISTEN_MAX = 30_000
const INITIAL_RETRY_BASE = 2_000
const MAX_INITIAL_RETRIES = 2

export type ProposalsFailure = 'offline' | 'lostAccess' | 'authority' | 'invalid' | 'failed'

export type ProposalsPhase =
  | { kind: 'loading' }
  | { kind: 'ready' }
  | { kind: 'unavailable' }
  | { kind: 'error'; reason: ProposalsFailure }
  | { kind: 'moved' }

export type DecideStatus = 'sending' | 'uncertain' | 'conflict' | 'decided' | 'rejected'

export interface DecideState {
  proposalId: string
  decision: ProposalDecision
  clientRequestId: string
  body: ProposalDecideBody
  status: DecideStatus
  detail: string | null
}

export type DecideResult =
  | { ok: true; proposal: TaskProposal }
  | { ok: false; status: Exclude<DecideStatus, 'sending'> | 'blocked'; requestId?: string }

interface Origin { source: TowerSource; hostId: string; client: TowerLink; epoch: number; gen: number }
interface ProposalStreamEvent { type: string; spaceId?: string; hubId?: string }
interface WriteTicket { seq: number; source: TowerSource; hostId: string; spaceId: string }

function statusOf(e: unknown): number | null {
  return typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
}

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : typeof e === 'object' && e && 'message' in e ? String((e as { message: unknown }).message) : String(e)
}

function ambiguousAuthority(status: number | null, message: string) {
  return status !== 401 && status !== 403 && /^\s*not_allowed\b/i.test(message)
}

function isUnavailable(status: number | null, message: string) {
  return status === 404 || /unknown method/i.test(message)
}

/** Classifica falhas de leitura das propostas segundo os erros do Hub. */
export function readProposalFailure(e: unknown): ProposalsFailure | 'unavailable' {
  const status = statusOf(e)
  const message = messageOf(e)
  if (isUnavailable(status, message)) return 'unavailable'
  if (status === 403 || status === 401) return 'lostAccess'
  if (ambiguousAuthority(status, message)) return 'authority'
  if (/projects_invalid_answer/.test(message)) return 'invalid'
  if (status === 0) return 'offline'
  return 'failed'
}

/** Classifica falhas de decisão: só `uncertain` permite reenviar o mesmo pedido. */
export function decideProposalFailure(
  e: unknown,
): Exclude<DecideStatus, 'sending'> | 'lostAccess' | 'unavailable' | 'authority' {
  const status = statusOf(e)
  const message = messageOf(e)
  if (status === 403 || status === 401) return 'lostAccess'
  if (ambiguousAuthority(status, message)) return 'authority'
  if (isUnavailable(status, message)) return 'unavailable'
  if (/^\s*proposal_decided\b/i.test(message)) return 'decided'
  if (status === 409 || /^\s*proposal_conflict\b/i.test(message) || /\bconflict\b/i.test(message)) return 'conflict'
  if (status === 0 || status === null || /timed? ?out|timeout|projects_invalid_answer/i.test(message)) return 'uncertain'
  return 'rejected'
}

function hubDetail(e: unknown): string | null {
  const m = messageOf(e).trim()
  if (!m) return null
  return m.replace(/^[a-z][a-z_]*:\s*/i, '').slice(0, 300) || null
}

function mergeDecidedProposal(existing: TaskProposal | undefined, decided: TaskProposal): TaskProposal {
  if (!existing) return decided
  if (existing.version > decided.version) return existing
  if (existing.version === decided.version && existing.state !== 'pending') {
    const byId = new Map(existing.items.map((it) => [it.id, it]))
    return {
      ...decided,
      items: decided.items.map((it) => {
        const prev = byId.get(it.id)
        if (!prev) return it
        return {
          ...it,
          taskId: prev.taskId ?? it.taskId,
          taskState: prev.taskState ?? it.taskState,
          startState: prev.startState ?? it.startState,
        }
      }),
    }
  }
  return decided
}

/**
 * Sessão observável das propostas do Berga para um Hub + Space em memória.
 * Assina o stream antes de ler, relê em `donnaProjectsChanged`, aceita uma decisão por vez
 * e descarta silenciosamente quando o Hub antigo recusa o método.
 */
export class PropostasSession extends Observable {
  phase: ProposalsPhase = { kind: 'loading' }
  link: 'live' | 'offline' = 'live'
  events: 'live' | 'paused' = 'live'
  snapshot: TaskProposalsSnapshot | null = null
  offlineSince: number | null = null
  readAt: number | null = null
  rereadFailed = false
  retryingRead = false
  authority: 'ok' | 'pending' = 'ok'
  write: DecideState | null = null

  private sourceRef: TowerSource | null = null
  private sourceGen = 0
  private hostId: string | null = null
  private spaceId: string | null = null
  private origin: Origin | null = null
  private gen = 0
  private verified = false
  private reading: Origin | null = null
  private reads = 0
  private dirty = false
  private initialRetries = 0
  private rereadTimer: ReturnType<typeof setTimeout> | null = null
  private relistenTimer: ReturnType<typeof setTimeout> | null = null
  private initialRetryTimer: ReturnType<typeof setTimeout> | null = null
  private relistenDelay = RELISTEN_MIN
  private unwatch: (() => void) | null = null
  private stopEvents: (() => void) | null = null
  private writeSeq = 0
  private writeOp: WriteTicket | null = null
  private readonly now: () => number
  private readonly uuid: () => string

  constructor(now: () => number = Date.now, uuid: () => string = newUuid) {
    super()
    this.now = now
    this.uuid = uuid
  }

  get stale() {
    return this.link === 'offline' || this.events === 'paused' || this.authority === 'pending' || !this.verified
  }

  get identity() {
    return this.sourceGen
  }

  get space() {
    return this.spaceId
  }

  get hub() {
    return this.hostId
  }

  get canDecide() {
    return (
      this.verified
      && !this.stale
      && this.phase.kind === 'ready'
      && !!this.snapshot
      && this.write?.status !== 'sending'
      && this.write?.status !== 'uncertain'
    )
  }

  open(source: TowerSource, spaceId: string) {
    this.release()
    this.initialRetries = 0
    this.retryingRead = false
    if (this.sourceRef !== source) this.sourceGen++
    if (this.sourceRef && this.sourceRef !== source) {
      this.gen++
      this.origin = null
      this.forget()
      this.hostId = null
    }
    this.sourceRef = source
    if (this.spaceId !== spaceId) {
      this.gen++
      this.origin = null
      this.forget()
    }
    this.spaceId = spaceId
    this.unwatch = source.subscribe(() => this.watch(source))
    this.connect(source)
  }

  close() {
    this.release()
    this.gen++
    this.origin = null
    this.verified = false
    this.reading = null
    this.dirty = false
    this.retryingRead = false
    if (this.write?.status === 'sending') {
      this.writeSeq++
      this.writeOp = null
      this.write = { ...this.write, status: 'uncertain', detail: null }
    } else if (this.write && this.write.status !== 'uncertain') {
      this.dropWrite()
    }
    this.changed()
  }

  reread() {
    const origin = this.origin
    if (origin && this.current(origin)) void this.read(origin)
  }

  /** Tenta ler novamente após falha na carga inicial ou reconexão. */
  retryRead() {
    if (this.initialRetryTimer) {
      clearTimeout(this.initialRetryTimer)
      this.initialRetryTimer = null
    }
    this.initialRetries = 0
    const origin = this.origin
    if (origin && this.current(origin)) {
      if (!this.snapshot) {
        this.retryingRead = true
        this.changed()
      }
      void this.read(origin)
      return
    }
    if (this.sourceRef && this.spaceId) {
      this.connect(this.sourceRef)
    }
  }

  dismissWrite() {
    if (this.write?.status === 'sending') return
    this.dropWrite()
    this.changed()
    this.reread()
  }

  decide(
    proposal: Pick<TaskProposal, 'id' | 'version'>,
    decision: ProposalDecision,
    omitItemIds?: readonly string[],
  ): Promise<DecideResult> {
    const origin = this.origin
    if (!origin || !this.spaceId || !this.canDecide) return Promise.resolve({ ok: false, status: 'blocked' })
    const clientRequestId = this.uuid()
    const body = decideBody({ hubId: origin.hostId, spaceId: this.spaceId }, proposal, decision, clientRequestId, omitItemIds)
    return this.send({ proposalId: proposal.id, decision, clientRequestId, body, status: 'sending', detail: null })
  }

  /** Reenvia exatamente o mesmo pedido guardado após falha incerta. */
  retry(): Promise<DecideResult> {
    const w = this.write
    if (!w || w.status !== 'uncertain' || this.stale || !this.origin || !this.verified) {
      return Promise.resolve({ ok: false, status: 'blocked' })
    }
    return this.send({ ...w, status: 'sending', detail: null })
  }

  private async send(w: DecideState): Promise<DecideResult> {
    const origin = this.origin!
    const spaceId = this.spaceId!
    const op: WriteTicket = { seq: ++this.writeSeq, source: origin.source, hostId: origin.hostId, spaceId }
    this.writeOp = op
    this.write = w
    this.changed()
    try {
      const answer = await origin.client.call('donnaTaskProposalDecide', w.body)
      if (!this.sameAuthority(op)) return { ok: false, status: 'blocked' }
      if (!this.current(origin)) {
        this.writeOp = null
        this.write = { ...w, status: 'uncertain', detail: null }
        this.changed()
        return { ok: false, status: 'uncertain', requestId: w.clientRequestId }
      }
      const { proposal } = parseProposalDecideAnswer(answer, { hubId: origin.hostId, spaceId }, w.proposalId)
      this.reads++
      this.writeOp = null
      this.write = null
      if (this.snapshot) {
        const exists = this.snapshot.proposals.some((p) => p.id === proposal.id)
        const proposals = exists
          ? this.snapshot.proposals.map((p) => (p.id === proposal.id ? mergeDecidedProposal(p, proposal) : p))
          : [...this.snapshot.proposals, proposal]
        this.snapshot = { ...this.snapshot, proposals }
      }
      this.readAt = this.now()
      this.changed()
      this.scheduleRead(origin)
      return { ok: true, proposal }
    } catch (e) {
      if (!this.sameAuthority(op)) return { ok: false, status: 'blocked' }
      if (!this.current(origin)) {
        this.writeOp = null
        this.write = { ...w, status: 'uncertain', detail: null }
        this.changed()
        return { ok: false, status: 'uncertain', requestId: w.clientRequestId }
      }
      const kind = decideProposalFailure(e)
      if (kind === 'lostAccess') {
        this.lostAccess()
        return { ok: false, status: 'rejected' }
      }
      if (kind === 'authority') {
        this.dropWrite()
        this.authorityInDoubt()
        this.reread()
        return { ok: false, status: 'blocked' }
      }
      if (kind === 'unavailable') {
        this.dropWrite()
        this.snapshot = null
        this.verified = false
        this.phase = { kind: 'unavailable' }
        this.changed()
        return { ok: false, status: 'rejected' }
      }
      this.writeOp = null
      this.write = { ...w, status: kind, detail: kind === 'uncertain' ? null : hubDetail(e) }
      this.changed()
      if (kind === 'conflict' || kind === 'decided') this.reread()
      return kind === 'uncertain' ? { ok: false, status: kind, requestId: w.clientRequestId } : { ok: false, status: kind }
    }
  }

  private connect(source: TowerSource) {
    this.stopEvents?.()
    this.stopEvents = null
    if (this.initialRetryTimer) {
      clearTimeout(this.initialRetryTimer)
      this.initialRetryTimer = null
    }
    this.initialRetries = 0
    this.retryingRead = false
    const gen = ++this.gen
    const hostId = source.hello?.hostId
    const client = source.connectedClient
    this.verified = false
    if (hostId && this.hostId && this.hostId !== hostId) this.forget()
    if (hostId) this.hostId = hostId
    if (this.write && ((hostId && this.write.body.hubId !== hostId) || (this.spaceId && this.write.body.spaceId !== this.spaceId))) {
      this.dropWrite()
    }
    if (!hostId || !client || !this.spaceId) {
      this.origin = null
      if (this.link === 'live') this.offlineSince = this.now()
      this.link = 'offline'
      if (!this.snapshot && this.phase.kind !== 'unavailable') this.phase = { kind: 'error', reason: 'offline' }
      this.changed()
      return
    }
    const origin: Origin = { source, hostId, client, epoch: source.principalEpoch, gen }
    this.origin = origin
    this.link = 'live'
    this.offlineSince = null
    this.events = 'live'
    this.relistenDelay = RELISTEN_MIN
    if (!this.snapshot) this.phase = { kind: 'loading' }
    this.changed()
    this.listen(origin, this.spaceId)
    void this.read(origin)
  }

  private listen(origin: Origin, spaceId: string) {
    this.stopEvents = origin.client.inSpace(spaceId).events(origin.source.hello?.rev ?? 0, 'desktop', (e) => {
      if (!this.current(origin)) return
      this.relistenDelay = RELISTEN_MIN
      const ev = e as ProposalStreamEvent
      if ((ev.spaceId !== undefined && ev.spaceId !== spaceId) || (ev.hubId !== undefined && ev.hubId !== origin.hostId)) return
      if (ev.type === PROPOSALS_EVENT || ev.type === 'hello' || ev.type === 'resync' || ev.type === 'spaces') {
        this.scheduleRead(origin)
      }
    }, () => {
      if (!this.current(origin)) return
      this.verified = false
      this.reads++
      this.events = 'paused'
      if (this.offlineSince === null) this.offlineSince = this.now()
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

  private watch(source: TowerSource) {
    const hostId = source.hello?.hostId ?? null
    if (this.hostId && hostId && hostId !== this.hostId) {
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
    const origin = this.origin
    if (!origin || origin.client !== client || origin.epoch !== source.principalEpoch || this.link === 'offline') {
      this.connect(source)
    }
  }

  private scheduleRead(origin: Origin) {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = setTimeout(() => {
      this.rereadTimer = null
      if (this.current(origin)) void this.read(origin)
    }, REREAD_DELAY)
  }

  private current(origin: Origin) {
    const { source } = origin
    return (
      this.origin === origin
      && this.gen === origin.gen
      && source.hello?.hostId === origin.hostId
      && source.connectedClient === origin.client
      && source.principalEpoch === origin.epoch
    )
  }

  private sameAuthority(op: WriteTicket) {
    return (
      this.writeOp === op
      && op.seq === this.writeSeq
      && this.sourceRef === op.source
      && this.hostId === op.hostId
      && this.spaceId === op.spaceId
    )
  }

  private dropWrite() {
    this.writeSeq++
    this.writeOp = null
    this.write = null
  }

  private forget() {
    this.dropWrite()
    this.verified = false
    this.snapshot = null
    this.offlineSince = null
    this.readAt = null
    this.rereadFailed = false
    this.authority = 'ok'
    this.phase = { kind: 'loading' }
  }

  private authorityInDoubt() {
    this.verified = false
    if (this.authority === 'ok' && this.offlineSince === null) this.offlineSince = this.now()
    this.authority = 'pending'
    this.changed()
  }

  private lostAccess() {
    this.dropWrite()
    this.verified = false
    this.authority = 'ok'
    this.snapshot = null
    this.readAt = null
    this.phase = { kind: 'error', reason: 'lostAccess' }
    this.changed()
  }

  private authorized() {
    if (this.link === 'live' && this.events === 'live') {
      this.verified = true
      if (this.authority === 'pending') this.authority = 'ok'
      this.offlineSince = null
    }
  }

  private scheduleInitialRetry(origin: Origin) {
    if (this.initialRetries >= MAX_INITIAL_RETRIES) return
    if (this.initialRetryTimer) clearTimeout(this.initialRetryTimer)
    const delay = INITIAL_RETRY_BASE * 2 ** this.initialRetries
    this.initialRetries++
    this.initialRetryTimer = setTimeout(() => {
      this.initialRetryTimer = null
      if (this.current(origin) && !this.snapshot && this.phase.kind === 'error') {
        void this.read(origin)
      }
    }, delay)
  }

  private release() {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = null
    if (this.relistenTimer) clearTimeout(this.relistenTimer)
    this.relistenTimer = null
    if (this.initialRetryTimer) clearTimeout(this.initialRetryTimer)
    this.initialRetryTimer = null
    this.stopEvents?.()
    this.stopEvents = null
    this.unwatch?.()
    this.unwatch = null
  }

  private async read(origin: Origin) {
    if (this.reading === origin) {
      this.dirty = true
      return
    }
    this.reading = origin
    this.dirty = false
    const read = ++this.reads
    const spaceId = this.spaceId!
    const fresh = () => this.current(origin) && read === this.reads && this.spaceId === spaceId
    try {
      const answer = await origin.client.call('donnaTaskProposals', { hubId: origin.hostId, spaceId })
      if (!fresh()) return
      this.initialRetries = 0
      if (this.initialRetryTimer) {
        clearTimeout(this.initialRetryTimer)
        this.initialRetryTimer = null
      }
      this.snapshot = parseTaskProposals(answer, { hubId: origin.hostId, spaceId })
      this.phase = { kind: 'ready' }
      this.rereadFailed = false
      this.readAt = this.now()
      this.authorized()
    } catch (e) {
      if (!fresh()) return
      const reason = readProposalFailure(e)
      if (reason === 'unavailable') {
        this.dropWrite()
        this.verified = false
        this.snapshot = null
        this.phase = { kind: 'unavailable' }
        return
      }
      if (reason === 'lostAccess') return this.lostAccess()
      if (reason === 'authority') {
        this.authorityInDoubt()
        if (this.snapshot) return
      }
      if (!this.snapshot || reason === 'invalid') {
        this.verified = false
        this.snapshot = null
        this.phase = { kind: 'error', reason }
        if (reason === 'failed' || reason === 'offline') this.scheduleInitialRetry(origin)
      } else {
        this.rereadFailed = true
      }
    } finally {
      if (this.reading === origin) this.reading = null
      if (this.current(origin)) {
        this.retryingRead = false
        this.changed()
      }
      const latest = this.origin
      if (this.dirty && latest && this.current(latest)) this.scheduleRead(latest)
    }
  }
}
