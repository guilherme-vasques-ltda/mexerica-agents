// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { spaceAllows } from '../lib/fork-space-capabilities'
import { draftKey } from '../../shared/composer-drafts'
import { AvatarBotMap } from '@shared/fork-avatar-mood'
import { mirrorCacheStamp } from '@shared/fork-mirror-cache'
import { isChat, screenWithoutAgent, type AccessRequest, type CloudStatus, type Entry, type ScreenState } from '@shared/models'
import type { SpacesSnapshot } from '@shared/fork-spaces'
import { isOutsideLoadedWindow, snapshotFloors } from '@shared/mirror-window'
import { HostClient, HostError, type HostEvent } from '../client/host-client'
import { ErroFrase } from '../lib/fork-i18n/erro-frase'
import { Desatualizado } from './fork-version-gate'
import { BotMirror, type Connection } from './bot-mirror'
import { BotGroupsModel } from './bot-groups'
import { PrincipalModel } from './principal'
import { EmptyRosterWatch } from './fork-empty-roster'
import { TimesRoster } from './fork-times-roster'
import { ChatSessionsModel } from './fork-sessions'
import { acompanharIdioma, enviarIdioma } from './fork-idioma-hub'

const DROP_GRACE = 5000
const INITIAL_GRACE = 1000
const ACTION_PATIENCE = 20_000
const MISMATCH_RECHECK = 30_000
const MISMATCH_RECHECK_MAX = 4 * 60_000

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const same = (a: Connection, b: Connection) => a.kind === b.kind && JSON.stringify(a) === JSON.stringify(b)

/** Each chat's entries that `keep` accepts; chats left empty go. */
function keptEntries(entries: Map<string, Entry[]>, keep: (e: Entry) => boolean) {
  const kept = new Map<string, Entry[]>()
  for (const [id, list] of entries) {
    const rest = list.filter(keep)
    if (rest.length) kept.set(id, rest)
  }
  return kept
}

/**
 * The link half of `BotStore`: the events stream (catch-up since `rev`, then live), connection
 * graces, waiting for the link before an action, and read acknowledgements.
 */
export class BotSync extends BotMirror {
  /** D121: muda só no hello do stream (inclui reconexões dentro da tolerância offline). */
  principalEpoch = 0
  /** D545: prova transitória do hello RPC, vinculada ao link; nunca restaurada do mirror. */
  private menuHello: { client: HostClient; hello: NonNullable<BotMirror['hello']> } | null = null
  get sessionMenuHello() {
    return this.menuHello?.client === this.connectedClient && this.menuHello.hello.hostId === this.hostId
      ? this.menuHello.hello : null
  }
  /** Só invalidação para releitura administrativa; nenhum catálogo no mirror. */
  backgroundEpoch = 0
  /** whatsapp R5b: sobe a cada `whatsappChanged`; a tela do WhatsApp relê. R5c: `whatsappBot` é o bot do último. */
  whatsappEpoch = 0
  whatsappBot: string | null = null
  /** Capacidade transitória SAAS-04; nunca gravada no mirror/cache. */
  canManageEngineAccounts = false
  readonly principal = new PrincipalModel(this, {
    read: async () => (await this.ready()).call<{ primaryBotId: string | null }>('teamSettings', {}),
    hostId: () => this.hostId,
    storage: localStorage,
  })
  readonly botGroups = new BotGroupsModel(() => this.ready(), () => this.spaceId ?? this.spaces?.personalSpaceId, () => this.hello?.hostId)
  /** Fork (Barra05): catálogo de sessões por conversa (store/fork-sessions.ts). */
  readonly sessions = new ChatSessionsModel(() => this.ready(), () => this.connectedClient, (event) => this.apply(event))
  private readonly pararIdioma = acompanharIdioma(() => this.connectedClient) // D395
  /** Fork (abertura no Berga): quando a lista vazia vale como ausência de bots (store/fork-empty-roster.ts). */
  readonly emptyWatch = new EmptyRosterWatch()
  readonly timesRoster = new TimesRoster(() => {
    const client = this.connectedClient
    const spaceId = this.spaceId ?? this.spaces?.personalSpaceId
    return spaceAllows(this.spaceId, 'teams') && client && this.hostId && spaceId ? { contextId: this.contextId, computerId: this.computer.id,
      hostId: this.hostId, spaceId, client, epoch: this.principalEpoch } : null
  })
  private heldDrop: Connection | null = null
  private dropTimer: ReturnType<typeof setTimeout> | null = null
  private mismatchTimer: ReturnType<typeof setTimeout> | null = null
  private retryBackoff = 1000
  private mismatchWake: (() => void) | null = null
  private closeEvents: (() => void) | null = null
  private eventsLoop = 0
  private screenEra = 0
  /**
   * D432 (R2-01 do #511): o `rev` do Hub no hello do stream atual; entrada com rev até ele é carga (cache ou
   * catch-up), acima é ao vivo. null antes do 1º hello. Sem relógio (views/fork/novas).
   */
  streamRev: number | null = null
  /** R5: the epoch this store's own stream last observed (its hello or a `screen` event); -1 before any. */
  private screenSeen = -1
  private rewound = false
  /** The `since` the current events connection asked for, and the per-bot floors snapshotted at its `hello`. */
  private requestedSince = 0
  private floors = new Map<string, number>()
  private readingViews = new Map<string, string>()

  // MARK: lifecycle

  retire() {
    this.pararIdioma()
    this.timesRoster.retire()
    this.principal.retire()
    this.retired = true
    this.avatarMoods.retire()
    this.composerDrafts.retire()
    this.stopTransport()
    if (this.saveTimer) clearTimeout(this.saveTimer)
    this.onRosterChanged = null
    this.onEventApplied = null
    this.onComputerChanged = null
    this.client = null
    this.selection = null
    this.changed()
  }

  start() {
    this.restartStream()
  }

  restartStream(preserveDrop = false) {
    if (this.retired) return
    this.stopTransport(preserveDrop)
    if (!preserveDrop && this.connection.kind !== 'online') this.setConnection({ kind: 'connecting' })
    const transport = this.makeTransport()
    const generation = this.versionGate.currentGeneration
    this.client = new HostClient(this.versionGate.transport(transport, generation), this.spaceId, this.expectedHostId)
    this.changed()
    this.hostRoute = this.isLoopback ? 'loopback' : null
    this.runEvents(this.link!, new HostClient(transport, this.spaceId, this.expectedHostId), generation)
  }

  private stopTransport(preserveDrop = false) {
    this.versionGate.invalidate()
    this.menuHello = null
    this.canManageEngineAccounts = false
    this.timesRoster.suspend()
    this.endStreamCycle()
    if (!preserveDrop) {
      if (this.dropTimer) clearTimeout(this.dropTimer)
      this.dropTimer = null
      this.heldDrop = null
    }
  }

  /** Give initial connection failures a second to recover and online drops five seconds. */
  private setConnection(next: Connection) {
    if (next.kind === 'online' && this.linkCaidoDesde !== null) this.linkCaidoDesde = null
    if (next.kind !== 'online') {
      this.canManageEngineAccounts = false
      if (next.kind === 'unauthorized') this.timesRoster.clear('owner')
      else this.timesRoster.suspend()
      // Sem o stream, a identidade e toda resposta de tela da época anterior deixam de valer.
      this.forgetScreenAgent()
    }
    const transient = next.kind === 'connecting' || next.kind === 'offline' || next.kind === 'computerOffline'
    const recovering = next.kind === 'online' && (this.connection.kind !== 'online' || this.heldDrop !== null)
    const initialFailure = this.connection.kind === 'connecting' && transient && next.kind !== 'connecting'
    // Fork (abertura no Berga): toda queda zera o relógio da lista vazia, inclusive a segurada pela tolerância.
    this.emptyWatch.connection(next.kind === 'online', Date.now())
    if (transient && (this.connection.kind === 'online' || initialFailure)) {
      const grace = this.connection.kind === 'online' ? DROP_GRACE : INITIAL_GRACE
      this.heldDrop = next
      this.changed()
      if (!this.dropTimer) {
        this.dropTimer = setTimeout(() => {
          this.dropTimer = null
          const held = this.heldDrop
          this.heldDrop = null
          if (held && !this.retired) {
            this.connection = held
            this.emptyWatch.connection(held.kind === 'online', Date.now())
            this.avatarMoods.connection(held.kind === 'online')
            this.changed()
          }
        }, grace)
      }
      return
    }
    if (this.dropTimer) clearTimeout(this.dropTimer)
    this.dropTimer = null
    this.heldDrop = null
    if (!same(next, this.connection)) {
      this.connection = next
      this.avatarMoods.connection(next.kind === 'online')
      this.changed()
    }
    if (recovering) for (const botId of new Set(this.readingViews.values())) this.markRead(botId)
  }

  /**
   * MX-217 (store/fork-queda-curta.ts): o Main confirmou que o link deste computador caiu (`desde`) ou abriu um novo
   * (`null`). Sem a tolerância visual de uma queda do stream: caído, nenhuma ação usa o link já; retomado, a loja fica
   * "conectando" e ainda em queda, sem autoridade de ação, até o hello do stream novo conferir o Hub (outro Hub zera o
   * espelho antes; setConnection limpa o marcador ao ficar online).
   */
  marcarQueda(desde: number | null) {
    this.stopTransport()
    if (this.dropTimer) clearTimeout(this.dropTimer)
    this.dropTimer = null
    this.heldDrop = null
    this.menuHello = null
    this.canManageEngineAccounts = false
    this.timesRoster.suspend()
    this.forgetScreenAgent()
    // Retomado, o marcador da queda (faixa, Enviar e Permitir desligados) só sai quando o stream novo fica online.
    if (desde !== null) this.linkCaidoDesde = desde
    this.connection = desde !== null ? { kind: 'offline', message: HostError.unreachable().message } : { kind: 'connecting' }
    this.emptyWatch.connection(false, Date.now())
    this.avatarMoods.connection(false)
    this.changed()
  }

  /** Events (catch-up since `rev`, then live) for as long as the store lives. */
  private runEvents(client: HostClient, helloClient: HostClient, generation: number) {
    const loop = ++this.eventsLoop
    let mismatchBackoff = MISMATCH_RECHECK
    // O laço fala com o Hub pelo `link`: o `client` público já esconde o transporte sob mismatch,
    // e este hello é justamente a rechecagem permitida.
    const alive = () => loop === this.eventsLoop && client === this.link && !this.retired
    const attempt = async () => {
      if (!alive()) return
      try {
        await this.refreshHello(client, helloClient, generation, alive)
        if (!alive()) return
        // Versão incompatível deste computador: só ele fica "precisa atualizar" — o cache segue
        // visível, nada é gravado e o stream não abre. Reconfere o hello em backoff crescente até
        // um teto; um hello bom destrava sozinho na próxima tentativa, sem reabrir o app.
        if (this.mismatch) {
          this.setConnection({ kind: 'online' })
          await this.sleepMismatch(mismatchBackoff)
          if (!alive()) return
          mismatchBackoff = Math.min(mismatchBackoff * 2, MISMATCH_RECHECK_MAX)
          return void attempt()
        }
        mismatchBackoff = MISMATCH_RECHECK
        try {
          const access = await client.call<{ capability: string; canManage: boolean }>('donnaEngineAccess', {})
          if (!alive()) return
          this.canManageEngineAccounts = access.capability === 'engine.accounts' && access.canManage === true
        } catch {
          if (!alive()) return
          this.canManageEngineAccounts = false
        }
        this.changed()
        if (this.isLoopback) {
          try {
            const response = await client.call<{ requests: AccessRequest[] }>('accessRequests')
            if (!alive()) return
            this.accessRequests = response.requests
          } catch {}
          try {
            const response = await client.call<CloudStatus>('cloudStatus')
            if (!alive()) return
            this.cloud = response
          } catch {}
          if (!alive()) return
          this.changed()
        }
        if (!alive()) return
        await new Promise<void>((resolve, reject) => {
          this.requestedSince = this.rev
          this.floors = new Map()
          // R3/R4: each stream counts only until its own end (HostClient.events drops what comes after it).
          this.closeEvents = client.events(
            this.requestedSince,
            'desktop',
            (event) => {
              if (!alive()) return
              this.setConnection({ kind: 'online' })
              this.retryBackoff = 1000
              this.apply(event)
            },
            (error) => {
              if (!alive()) return
              this.versionGate.invalidate()
              error ? reject(error) : resolve()
            },
          )
        })
        if (!alive()) return
        this.setConnection({ kind: 'connecting' })
      } catch (error) {
        if (!alive()) return
        const message = error instanceof Error ? error.message : String(error)
        this.setConnection({ kind: 'offline', message })
        if (error instanceof HostError && (error.status === 401 || error.status === 403)) { this.timesRoster.clear('owner'); return }
      }
      if (!alive()) return
      this.versionGate.invalidate()
      await sleep(this.retryBackoff)
      this.retryBackoff = Math.min(this.retryBackoff * 2, 20_000)
      if (alive()) this.restartStream(true)
    }
    void attempt()
  }

  /** Resubscreve de rev com outra geração e exige um novo hello. */
  private restartEvents() {
    this.restartStream()
  }

  /** Sleep de rechecagem do mismatch (ativo em todo computador, sem depender da seleção): cancelável —
   *  descartar a store, reiniciar ou destravar encerra a espera na hora, sem timer nem closure vazando. */
  private sleepMismatch(ms: number) {
    return new Promise<void>((resolve) => {
      this.mismatchWake = resolve
      this.mismatchTimer = setTimeout(() => {
        this.mismatchTimer = null
        this.mismatchWake = null
        resolve()
      }, ms)
    })
  }

  private cancelMismatch() {
    if (this.mismatchTimer) clearTimeout(this.mismatchTimer)
    this.mismatchTimer = null
    const wake = this.mismatchWake
    this.mismatchWake = null
    wake?.()
  }

  /**
   * R4: the one place a stream cycle ends on purpose (Reconectar/⌘R, resync, undecodable, retire): the old stream
   * is closed and its loop dropped, and who uses the computer stops being current before any new transport answers.
   * A natural end goes through `setConnection` and does the same to the screen.
   */
  private endStreamCycle() {
    this.eventsLoop++
    this.cancelMismatch()
    this.closeEvents?.()
    this.closeEvents = null
    this.forgetScreenAgent()
  }

  /** A new epoch nothing has observed yet: who uses the computer is unknown until this store's stream says. */
  private forgetScreenAgent() {
    this.screenEra++
    if (this.screen) this.screen = screenWithoutAgent(this.screen)
  }

  /**
   * R5: only the store's own stream (its SSE hello, `screen`) opens an observed epoch, through `apply`. The `hello` RPC
   * that `runEvents` awaits before opening `/events` (`refreshHello`) never does, nor does any screen RPC.
   */
  private observeScreen() {
    this.screenSeen = ++this.screenEra
  }

  private async refreshHello(client: HostClient, helloClient: HostClient, generation: number, alive: () => boolean) {
    const h = await helloClient.hello()
    if (!alive()) return
    this.versionGate.prove(generation, { version: h.version, minApp: h.minApp }, this.unreadable)
    this.noteHostVersion({ version: h.version, minApp: h.minApp })
    if (!this.versionGate.allowed(generation)) { this.changed(); return }
    if (h.hostId && this.hostId && this.hostId !== h.hostId) this.resetMirror()
    if (!alive()) return
    if (h.hostId) { this.bindCacheHost(h.hostId); this.hostId = h.hostId }
    this.erroAcao = null
    client.acceptHello(h)
    this.hello = h
    this.menuHello = { client, hello: h }
    // Aqui o hello já passou pela prova (retorno antecipado acima), então a projeção é segura.
    if (h.spaces) this.spaces = h.spaces
    if (h.conversations) this.conversations = new Map(h.conversations.map((c) => [c.id, c]))
    enviarIdioma(client) // D395
    if (this.isLoopback) this.updateComputer((c) => ({ ...c, name: h.name, device: h.device ?? c.device }))
    this.changed()
  }

  private pendingRefetch = false
  private noteHostVersion(next: { version: string; minApp?: string | null }) {
    const cached = this.cacheStamp !== null && this.cacheStamp !== mirrorCacheStamp(this.appVersion, next.version)
    const changed = !!(this.hostVersion && this.hostVersion.version !== next.version)
    this.cacheStamp = null
    this.hostVersion = next
    if (!this.mismatch) this.acaoRecusada = null
    // A different host version or app build: data may carry new fields, so fetch it all again.
    // Em mismatch a releitura fica PENDENTE (não se perde): um hello compatível — mesmo só com um
    // minApp novo — a executa antes de liberar qualquer dado desse Hub.
    if (cached || changed) this.pendingRefetch = true
    if (this.pendingRefetch && !this.mismatch) {
      this.pendingRefetch = false
      this.refetchAll()
      this.rewound = false
      this.unreadable = false
    }
  }

  /**
   * Observação por evento (MX-251): chamada a cada evento do stream logo após ser aplicado, antes
   * do `changed()` que agrega notificações num único microtask. Quem não pode perder uma transição
   * escondida num lote (working+idle na mesma entrega) observa por aqui; a assinatura continua
   * servindo ao resto. Uso opcional — perder um "Terminou" é aceitável e fail-safe.
   */
  onEventApplied: (() => void) | null = null

  private apply(event: HostEvent) {
    if (!this.versionGate.allowed()) return
    if (event.type === 'sessionDeleted' && event.spaceId !== (this.spaceId ?? this.spaces?.personalSpaceId)) return
    if (event.type === 'hello' && event.spaces) this.spaces = event.spaces
    this.botGroups.receive(event)
    this.sessions.receive(event)
    switch (event.type) {
      case 'hello':
        if (this.hostId && this.hostId !== event.hostId) this.resetMirror()
        if (event.spaces) this.spaces = event.spaces
        if (event.conversations) this.conversations = new Map(event.conversations.map((c) => [c.id, c]))
        this.principalEpoch++
        this.bindCacheHost(event.hostId)
        this.hostId = event.hostId
        this.usage = event.usage
        this.observeScreen()
        this.screen = event.screen
        if (event.rev < this.rev) this.rev = 0
        this.streamRev = event.rev
        this.floors = snapshotFloors(this.entries, this.rev === 0 ? 0 : this.requestedSince)
        this.emptyWatch.hello(event.conversations)
        break
      case 'spaces':
        // D169: a late event never puts back an older list over a newer one (an action's answer may land first).
        if (event.snapshot && event.snapshot.revision >= (this.spaces?.revision ?? -1)) this.spaces = event.snapshot
        break
      case 'conversation':
        if (event.conversation && event.conversation.stateRevision >= (this.conversations.get(event.conversation.id)?.stateRevision ?? 0)) this.conversations.set(event.conversation.id, event.conversation)
        break
      case 'bot':
        this.bots.set(event.bot.id, event.bot)
        this.bump(event.bot.rev)
        this.onRosterChanged?.()
        if (event.bot.unread > 0) this.acknowledgeVisible(event.bot.id)
        break
      case 'botDeleted':
        this.composerDrafts.removeBot(event.id)
        this.bots.delete(event.id)
        this.entries.delete(event.id)
        this.floors.delete(event.id)
        this.bump(event.rev)
        if (this.selection === event.id) this.selection = null
        this.onRosterChanged?.()
        break
      case 'sessionDeleted': {
        const space = this.spaceId ?? this.spaces?.personalSpaceId
        if (space && event.spaceId !== space) break
        const cached = this.allEntries(event.chatId)
        const kept = cached.filter((e) => e.sessionId !== event.sessionId)
        this.entries.set(event.chatId, kept)
        const key = draftKey(event.chatId, event.sessionId)
        this.composerDrafts.removeBot(key)
        // Se toda a janela carregada pertencia à extra, trazer o histórico restante deste bot.
        if (cached.length && !kept.length) void this.refillDeletedWindow(event.chatId)
        this.bump(event.rev)
        break
      }
      case 'entry':
        this.avatarMoods.entry(event.entry)
        // Old entries re-stamped by the host (new rev) belong to history paging, not the live window.
        if (!this.isBelowFloor(event.entry)) this.upsert(event.entry)
        this.bump(event.entry.rev)
        this.acknowledgeVisible(event.entry.botId, event.entry)
        break
      case 'usage':
        this.usage = event.usage
        break
      case 'screen':
        this.observeScreen()
        this.screen = event.screen
        break
      case 'accessRequests':
        this.accessRequests = event.requests
        break
      case 'cloud':
        this.cloud = event.cloud
        break
      case 'functionModelsChanged':
      case 'backgroundSpaceSelectionsChanged':
      case 'backgroundHistoryChanged':
        this.backgroundEpoch++
        break
      case 'whatsappChanged':
        this.whatsappEpoch++
        this.whatsappBot = event.botId ?? null
        break
      case 'resync':
        this.restartEvents()
        break
      case 'undecodable':
        // Never skip past data we couldn't read: rewind once and fetch everything again.
        if (!this.rewound) {
          this.rewound = true
          this.refetchAll()
          this.restartEvents()
        } else if (!this.unreadable) {
          this.unreadable = true
          if (this.hostVersion) this.versionGate.prove(this.versionGate.currentGeneration, this.hostVersion, true)
          if (this.mismatch) this.restartEvents()
        }
        break
    }
    // D280: origem/epoch e catálogo já foram aplicados; reutiliza o stream, sem avançar rev em times.
    if (['hello', 'times', 'bot', 'botDeleted', 'resync', 'spaces'].includes(event.type)) this.timesRoster.sync(event)
    try {
      // O observador é opcional (o "Terminou" da ilha): uma exceção dele nunca pode sair do
      // apply — interromperia o loop do preload e as linhas seguintes do lote, incluindo
      // falha e aprovação. Engolida aqui, `changed()` e o save continuam normalmente.
      this.onEventApplied?.()
    } catch (error) {
      console.warn('bot-sync: observador de evento falhou', error)
    }
    this.changed()
    this.scheduleSave()
  }

  private async refillDeletedWindow(chatId: string) {
    const client = this.connectedClient, epoch = this.sessions.epoch
    if (!client) return
    this.historyComplete.delete(chatId)
    this.floors.delete(chatId)
    try {
      const entries = await client.history(chatId, Number.MAX_SAFE_INTEGER, 200)
      if (this.connectedClient !== client || this.sessions.epoch !== epoch) return
      for (const entry of entries) this.upsert(entry)
      if (entries.length < 200) this.historyComplete.add(chatId)
      this.changed()
      this.scheduleSave()
    } catch { /* Falha de leitura não desfaz a exclusão confirmada. */ }
  }

  protected override upsert(entry: Entry) {
    if (this.sessions.isDeleted(entry.botId, entry.sessionId ?? entry.botId)) return
    super.upsert(entry)
  }

  private resetMirror() {
    this.menuHello = null
    // MX-158: o `usage` não carrega o Hub de origem e só é trocado no hello do stream; trocar de Hub o esquece já aqui,
    // antes da identidade nova ser publicada (nenhum limite do Hub anterior sob o nome do novo). Offline no mesmo Hub
    // não passa por aqui e mantém o último conhecido.
    this.usage = { providers: [] }
    this.timesRoster.clear()
    // O rascunho do Hub anterior permanece no seu namespace; bindCacheHost troca o ativo.
    this.spaces = null
    this.conversations.clear()
    this.bots = new AvatarBotMap(this.avatarMoods)
    this.entries = new Map()
    this.selection = null
    this.onRosterChanged?.()
    this.rev = 0
    this.hostId = null
    this.historyComplete = new Set()
    this.floors = new Map()
    this.forgetScreenAgent()
    this.screen = null
    // O cache do Hub anterior permanece íntegro; o novo hello grava só no novo namespace.
  }

  /**
   * Fetches everything again from rev 0. That catch-up brings only each chat's newest entries, so the
   * main-chat ones held now go too: an older one kept would hide the gap from `loadOlder`, which pages
   * from the oldest one held. Messages still sending stay.
   */
  private refetchAll() {
    this.rev = 0
    this.entries = keptEntries(this.entries, (e) => e.id.startsWith('local-'))
    this.historyComplete = new Set()
  }

  private isBelowFloor(e: Entry) {
    const held = this.allEntries(e.botId).some((x) => x.id === e.id)
    return isOutsideLoadedWindow(this.floors.get(e.botId), held, e)
  }

  private bump(r: number) {
    this.rev = Math.max(this.rev, r)
  }

  // MARK: link

  /**
   * R3: what the screen state says now. It moves on every drop, end of a stream cycle, Hub change, hello and `screen`
   * event of this store; the same `HostClient` reconnecting does not make an older answer current again. Local to
   * the app, nothing on the wire.
   */
  get screenEpoch() { return this.screenEra }

  /**
   * R5: an answer that left in `epoch` may say who uses the computer only if that epoch is still the current one AND
   * was opened by this store's own stream (hello or `screen`). After a connect, restart, resync, undecodable line,
   * drop or Hub change, a new epoch exists before the new stream has observed anything: not ready, whatever the client.
   */
  screenObserved(epoch: number) {
    return epoch === this.screenEra && this.screenSeen === epoch
  }

  /**
   * R2/R3/R5: a screen answer brings who uses the computer only if it left from the live link that asked, in an epoch
   * that is still current and that this store's stream had observed.
   * Otherwise the rest of it (on, connected, permissions, `computerUse`) is taken and who uses it stays what the
   * stream says now: an old answer never puts an identity back.
   */
  protected adoptScreen(screen: ScreenState | null, asked: HostClient, epoch: number) {
    this.guardProjection(asked)
    if (!screen || (this.live === asked && this.screenObserved(epoch))) return void (this.screen = screen)
    this.screen = { ...screen, agentUse: this.screen?.agentUse ?? 'unavailable', agentBot: this.screen?.agentBot ?? null }
  }

  protected get live(): HostClient | null {
    return this.connection.kind === 'online' && !this.heldDrop && !this.retired ? this.client : null
  }

  /** O link utilizável, inclusive durante a tolerância visual de uma queda curta. */
  get connectedClient(): HostClient | null { return this.mismatch ? null : this.live }

  /**
   * D169 (store/fork-spaces.ts): the list a Spaces action got back from the Hub. Kept only while the Hub, the
   * link and the generation are the ones that asked, and never over a newer one; `false` otherwise.
   */
  acceptSpaces(snapshot: SpacesSnapshot, origin: { hostId: string; client: HostClient; epoch: number }) {
    if (this.retired || this.hostId !== origin.hostId || this.connectedClient !== origin.client || this.principalEpoch !== origin.epoch) return false
    if (this.spaces && snapshot.revision < this.spaces.revision) return false
    this.spaces = snapshot
    this.changed()
    this.scheduleSave()
    return true
  }

  /** The client once the link is up. A reconnect in progress is waited out (headers show it). */
  async ready(deadline = Date.now() + ACTION_PATIENCE): Promise<HostClient> {
    let counted = false
    let retried = false
    try {
      while (!this.retired) {
        // Versão incompatível JÁ CONHECIDA deste computador: ação que exige o Hub falha na hora, com
        // motivo. Sem prova ainda (hello pendente), espera o link como sempre: `live` é null até lá.
        this.versionGate.refuseIfMismatch()
        if (this.live) return this.live
        switch (this.connection.kind) {
          case 'online':
          case 'connecting':
            break
          case 'offline':
            if (!retried) {
              retried = true
              this.restartStream()
              break
            }
            throw HostError.unreachable()
          case 'unpaired':
            throw HostError.unreachable()
          case 'computerOffline':
            throw new HostError(0, 'Your computer is offline.')
          case 'unauthorized':
            throw new HostError(401, this.connection.message)
        }
        if (Date.now() >= deadline) break
        if (!counted) {
          counted = true
          this.waiting++
          this.changed()
        }
        await sleep(100)
      }
      throw HostError.unreachable()
    } finally {
      if (counted) {
        this.waiting--
        this.changed()
      }
    }
  }

  /** Runs `op` once the link is up; with `replay`, a call the link dropped under is tried again. */
  protected async withLink<T>(op: (c: HostClient) => Promise<T>, replay = false): Promise<T> {
    const deadline = Date.now() + ACTION_PATIENCE
    // MX-217: a intenção pertence ao Hub em que nasceu; se a espera pelo link termina noutro Hub, não segue.
    const origem = this.hostId
    for (;;) {
      const client = await this.ready(deadline)
      if (origem !== null && this.hostId !== origem) throw new Error('stale_space_link')
      try {
        this.versionGate.assert()
        const host = this.hostId
        const epoch = this.principalEpoch
        const result = await op(client)
        this.versionGate.assert()
        if (this.retired || this.client !== client || this.hostId !== host || this.principalEpoch !== epoch) throw new Error('stale_space_link')
        return result
      } catch (error) {
        if (replay && error instanceof HostError && error.isTransient && Date.now() < deadline) {
          await sleep(500)
          continue
        }
        throw error
      }
    }
  }

  /**
   * Motivo observável de toda ação que falha: recusa tipada (`ErroFrase`, ex.: o bloqueio por
   * mismatch em `ready`) vai para `acaoRecusada` junto do estado `desatualizado`; o resto segue em
   * `lastError`. Nenhuma ação falha calada — e sem diálogo global por uma loja.
   */
  protected noteActionError(error: unknown) {
    if (error instanceof Desatualizado) this.recusarAcao(error)
    else if (error instanceof ErroFrase) this.acaoRecusada = error.frase
    else this.lastError = error instanceof Error ? error.message : String(error)
    this.changed()
  }

  protected async perform(op: (c: HostClient) => Promise<unknown>, replay = false) {
    try {
      await this.withLink(op, replay)
      return { ok: true as const }
    } catch (error) {
      this.noteActionError(error)
      return { ok: false as const, error }
    }
  }

  /** Views register while visible. Multiple windows may read the same conversation. */
  setReading(token: string, botId: string, active: boolean) {
    if (active) {
      this.readingViews.set(token, botId)
      if (this.connection.kind === 'online') this.markRead(botId)
    } else {
      this.readingViews.delete(token)
    }
  }

  private acknowledgeVisible(botId: string, entry?: Entry) {
    if (this.connection.kind !== 'online') return
    if (![...this.readingViews.values()].includes(botId) || (entry && !isChat(entry))) return
    this.markRead(botId)
  }

  markRead(botId: string) {
    void (async () => {
      if (!this.connectedClient) await sleep(INITIAL_GRACE)
      const client = this.connectedClient
      if (!client) return
      try {
        await client.call('markRead', { botId })
      } catch {
        // The visible scope is acknowledged again when the link recovers.
      }
    })()
  }

}
