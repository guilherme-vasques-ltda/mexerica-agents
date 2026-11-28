// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { SpacesSnapshot, SpaceConversation } from '../../shared/fork-spaces'
import { AvatarBotMap, AvatarMoods } from '@shared/fork-avatar-mood'
import { cachedBots, mirrorCacheStamp, isCurrentMirrorSchema } from '@shared/fork-mirror-cache'
import { VersionGate, type Desatualizado } from './fork-version-gate'
import {
  type AccessRequest,
  type Bot,
  type CloudStatus,
  type Computer,
  type Entry,
  type Hello,
  type ScreenState,
  type Usage,
} from '@shared/models'
import type { HostClient, HostRoute, HostTransport } from '../client/host-client'
import type { Frase } from '../lib/fork-i18n/frase'
import { Observable } from '../lib/observable'
import { ComposerDrafts } from '@shared/composer-drafts'
import { canLoadHistory } from '@shared/mirror-window'

export type Connection =
  | { kind: 'unpaired' }
  | { kind: 'connecting' }
  | { kind: 'online' }
  | { kind: 'computerOffline'; lastSeen: number | null }
  | { kind: 'offline'; message: string }
  | { kind: 'unauthorized'; message: string }

/**
 * The state half of `BotStore`: one computer's mirror (bots, transcripts), the selectors views
 * read, entry merging and the on-device cache.
 */
export class BotMirror extends Observable {
  readonly avatarMoods = new AvatarMoods()
  computer: Computer
  connection: Connection = { kind: 'connecting' }
  hostRoute: HostRoute | null = null
  /** Cliente com transporte guardado; o hello de rechecagem usa um cliente interno separado. */
  protected link: HostClient | null = null
  /** O transporte entregue já é guardado; referências capturadas também são revogadas. */
  get client(): HostClient | null { return this.versionGate.allowed() ? this.link : null }
  set client(next: HostClient | null) { this.link = next }
  protected readonly versionGate: VersionGate
  acaoRecusada: Frase | null = null
  erroAcao: Desatualizado | null = null
  recusarAcao(error: Desatualizado) {
    this.erroAcao = error
    this.acaoRecusada = error.frase
    this.changed()
  }
  /** A prova é conferida também na fronteira de gravação, depois de qualquer await do consumidor. */
  protected guardProjection(client: HostClient) {
    this.versionGate.assert(this.link === client ? undefined : -1)
  }
  hello: Hello | null = null
  spaces: SpacesSnapshot | null = null
  conversations = new Map<string, SpaceConversation>()
  hostVersion: { version: string; minApp?: string | null } | null = null
  bots: Map<string, Bot> = new AvatarBotMap(this.avatarMoods)
  entries = new Map<string, Entry[]>()
  usage: Usage = { providers: [] }
  screen: ScreenState | null = null
  accessRequests: AccessRequest[] = []
  cloud: CloudStatus | null = null
  historyComplete = new Set<string>()
  routineDrafts = new Map<string, string>()
  composerDrafts: ComposerDrafts
  private cacheHub: string | null = null
  lastError: string | null = null
  /** A kit view selected a bot on this store (new bot or group, editor); the account picks it up. */
  selection: string | null = null
  /** Permission cards whose answer is on its way, with the chosen option. */
  answering = new Map<string, string>()
  /** MX-217 (store/fork-queda-curta.ts, BotSync.marcarQueda): desde quando o link deste computador remoto caiu. */
  linkCaidoDesde: number | null = null

  onRosterChanged: (() => void) | null = null
  onComputerChanged: ((c: Computer) => void) | null = null

  protected waiting = 0
  protected rev = 0
  protected hostId: string | null = null
  protected retired = false
  protected unreadable = false
  protected saveTimer: ReturnType<typeof setTimeout> | null = null
  protected cacheStamp: string | null = null

  constructor(
    computer: Computer,
    readonly isLoopback: boolean,
    protected makeTransport: () => HostTransport,
    protected appVersion = window.codync.appVersion,
    readonly contextId = 'local',
    readonly spaceId?: string,
    readonly expectedHostId?: string,
  ) {
    super()
    this.versionGate = new VersionGate(appVersion, error => this.recusarAcao(error))
    this.computer = computer
    this.hostId = expectedHostId ?? null
    this.cacheHub = expectedHostId ?? null
    if (!this.cacheHub) { try { this.cacheHub = localStorage.getItem(this.hubPointer) } catch {} }
    this.composerDrafts = this.drafts(this.cacheHub)
    this.loadCache()
  }

  private get hubPointer() { return `codync-mirror-hub-${JSON.stringify([this.contextId, this.computer.id])}` }
  private draftStorageKey(hub: string | null) {
    return `codync-drafts-v1-${JSON.stringify([this.contextId, this.computer.id, ...(this.spaceId ? [this.spaceId] : []), ...(hub ? [hub] : [])])}`
  }
  private drafts(hub: string | null) {
    return new ComposerDrafts(localStorage, this.draftStorageKey(hub),
      () => this.setError("Couldn't save the draft on this device. Your text is still here; keep this window open until you send or copy it."))
  }
  /** A identidade confirmada seleciona cache e rascunho. Migração só da chave sem Hub; a original fica preservada. */
  protected bindCacheHost(hub: string) {
    if (hub === this.cacheHub) return
    const previous = this.cacheHub
    if (!previous) {
      for (const prefix of ['codync-drafts-v1-', 'codync-draft-origins-v1-']) {
        const from = this.draftStorageKey(null).replace('codync-drafts-v1-', prefix)
        const to = this.draftStorageKey(hub).replace('codync-drafts-v1-', prefix)
        try { const raw = localStorage.getItem(from); if (raw && !localStorage.getItem(to)) localStorage.setItem(to, raw) } catch {}
      }
    }
    this.composerDrafts.retire()
    this.cacheHub = hub
    this.composerDrafts = this.drafts(hub)
    try { localStorage.setItem(this.hubPointer, hub) } catch {}
  }

  // MARK: derived

  get isOffline() {
    const k = this.connection.kind
    return k === 'computerOffline' || k === 'offline' || k === 'unauthorized'
  }

  /** Loopback has no relay mailbox. */
  get canQueue() {
    return false
  }

  /** What headers show: a reconnect the grace period keeps quiet reads "Connecting…" while an action waits. */
  get shownConnection(): Connection {
    return this.waiting > 0 && this.connection.kind === 'online' ? { kind: 'connecting' } : this.connection
  }

  get hostName() {
    return this.hello?.name ?? this.computer.name
  }

  get mismatch() { return this.versionGate.mismatch }

  /**
   * Estado limpo de "precisa atualizar" para a UI: quem atualiza, a versão encontrada e o mínimo
   * exigido, sem texto — a tela decide como mostrar (o aviso visual é o UpdateNeededCard).
   */
  get desatualizado(): { alvo: 'app' | 'hub'; encontrada: string | null; minimo: string } | null {
    const m = this.mismatch
    if (!m) return null
    if (m.kind === 'updateApp') return { alvo: 'app', encontrada: typeof this.appVersion === 'string' ? this.appVersion : null, minimo: m.minimum }
    return { alvo: 'hub', encontrada: m.version || null, minimo: m.minimum }
  }

  /** Roster order: pinned first, then where the user dragged them, then most recent activity. */
  get roster(): Bot[] {
    return [...this.bots.values()].filter((b) => !b.hidden).sort((a, b) => Number(b.pinned) - Number(a.pinned) || a.position - b.position || b.lastAt - a.lastAt)
  }

  get hiddenBots(): Bot[] {
    return [...this.bots.values()].filter((b) => b.hidden).sort((a, b) => a.name.localeCompare(b.name))
  }

  backendName(id: string) {
    return this.hello?.backends.find((b) => b.id === id)?.name ?? builtInBackendName(id)
  }

  /** A bot's or group's chat, trace entries included. */
  allEntries(botId: string) {
    return this.entries.get(botId) ?? []
  }

  canLoadOlder(botId: string) {
    return canLoadHistory(this.allEntries(botId), this.historyComplete.has(botId))
  }

  members(group: Bot) {
    return group.members.map((id) => this.bots.get(id)).filter((b): b is Bot => !!b)
  }

  authorName(id: string | null | undefined) {
    return (id && this.bots.get(id)?.name) || 'A deleted bot'
  }

  /** The latest thinking of the turn running in a chat. */
  currentThinking(botId: string) {
    const list = this.allEntries(botId)
    for (let i = list.length - 1; i >= 0; i--) {
      const e = list[i]!
      if (e.kind === 'user' || (e.kind === 'agent' && e.data.final === true)) return null
      if (e.kind === 'thought' && e.data.text) return e.data.text
    }
    return null
  }

  get connectionLabel() {
    switch (this.shownConnection.kind) {
      case 'online':
        return this.mismatch ? 'Needs update' : 'Connected'
      case 'connecting':
        return 'Connecting…'
      case 'computerOffline':
      case 'offline':
        return 'Offline'
      case 'unauthorized':
        return 'No access'
      case 'unpaired':
        return 'Not paired'
    }
  }

  updateComputer(change: (c: Computer) => Computer) {
    const next = change({ ...this.computer })
    if (JSON.stringify(next) === JSON.stringify(this.computer)) return
    this.computer = next
    this.onComputerChanged?.(next)
    this.changed()
  }

  protected upsert(e: Entry) {
    const nonce = e.kind === 'user' && !e.id.startsWith('local-') ? e.data.clientNonce : undefined
    const list = (this.entries.get(e.botId) ?? []).filter((x) => !nonce || x.id !== `local-${nonce}`)
    const i = list.findIndex((x) => x.id === e.id)
    if (i >= 0) {
      // A late API response mustn't undo a newer SSE update.
      if (e.rev >= list[i]!.rev) list[i] = e
    } else {
      const last = list[list.length - 1]
      if (last && last.seq > e.seq && e.seq > 0) {
        const at = list.findIndex((x) => x.seq > e.seq)
        list.splice(at < 0 ? list.length : at, 0, e)
      } else {
        list.push(e)
      }
    }
    this.entries.set(e.botId, list)
  }

  setError(message: string | null) {
    this.lastError = message
    this.changed()
  }

  // MARK: cache (instant launch, offline reading)

  private get cacheKey() {
    return `codync-mirror-${JSON.stringify([this.contextId, this.computer.id, this.cacheHub, this.spaceId ?? null])}`
  }

  private loadCache() {
    try {
      const raw = localStorage.getItem(this.cacheKey)
      if (!raw) return
      const cache = JSON.parse(raw) as { stamp?: string; hostId?: string; rev: number; bots: Bot[]; entries: Entry[]; spaces?: SpacesSnapshot; conversations?: SpaceConversation[] }
      if (!this.cacheHub || cache.hostId !== this.cacheHub || !isCurrentMirrorSchema(cache.stamp)) return
      this.spaces = cache.spaces ?? null
      this.conversations = new Map((cache.conversations ?? []).filter((c) => !this.spaceId || c.spaceId === this.spaceId).map((c) => [c.id, c]))
      this.cacheStamp = cache.stamp ?? ''
      this.hostId = cache.hostId ?? null
      this.rev = cache.rev
      this.bots = new AvatarBotMap(this.avatarMoods, cachedBots(cache.stamp, cache.bots.filter((b) => !this.spaceId || b.spaceId === this.spaceId)))
      const grouped = new Map<string, Entry[]>()
      for (const e of cache.entries.filter((e) => !this.spaceId || e.spaceId === this.spaceId)) grouped.set(e.botId, [...(grouped.get(e.botId) ?? []), e])
      this.entries = grouped
      this.avatarMoods.restoreEntries(grouped)
    } catch {}
  }

  protected scheduleSave() {
    // Sob mismatch CONHECIDO nada é gravado: o cache pré-existente não é sobrescrito, vale até um
    // hello compatível destravar. Sem prova ainda (hello pendente, Hub desligado) grava como na main:
    // só a prova de dados novos exige hello, e `apply` já a exige.
    if (this.retired || this.versionGate.mismatch) return
    if (this.saveTimer) clearTimeout(this.saveTimer)
    this.saveTimer = setTimeout(() => this.saveCache(), 2000)
  }

  saveCache() {
    if (this.retired || this.versionGate.mismatch) return
    // Keep the newest 200 entries per bot; older ones page in from the host.
    const kept: Entry[] = []
    for (const list of this.entries.values()) {
      kept.push(...list.filter((e) => !e.id.startsWith('local-')).slice(-200))
      kept.push(...list.filter((e) => e.id.startsWith('local-') && e.data.status === 'failed'))
    }
    const cache = { stamp: mirrorCacheStamp(this.appVersion, this.hello?.version ?? ''), hostId: this.hostId, rev: this.rev, bots: [...this.bots.values()], entries: kept, spaces: this.spaces, conversations: [...this.conversations.values()] }
    try {
      localStorage.setItem(this.cacheKey, JSON.stringify(cache))
    } catch {}
  }
}

export function builtInBackendName(id: string) {
  switch (id) {
    case 'claude':
      return 'Claude Code'
    case 'codex':
      return 'Codex'
    case 'opencode':
      return 'OpenCode'
    case 'grok':
      return 'Grok Build'
    case 'gemini':
      return 'Gemini CLI'
    default:
      return 'Custom agent'
  }
}
