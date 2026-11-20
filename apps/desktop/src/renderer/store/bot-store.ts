// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { acompanharVinculoEnvio, type AcompanhamentoEnvioVoz } from '../views/fork/voz/operador-voz'
import { registrarEstadoEmMemoria } from '@shared/estado-so-em-memoria'
import { originDevice, originDraft, originPasted, originReport, type OriginReport } from '@shared/guard-hub'
import { retireAvatarEditors } from '../fork/avatars/editor-store'
import { avatarDoBot, camposAvatarDoBot, confirmAvatar, requireAvatarHub, requireAvatarMexeriquinhaHub, validAvatar, type BotAvatarConfig } from '@shared/fork-avatar-config'
import {
  isWorking,
  needsInput,
  normalizeBot,
  normalizeScreen,
  type Bot,
  type BotDraft,
  type DirListing,
  type Entry,
  type GroupDraft,
  type Hello,
  type ReplyQuote,
  type ScreenState,
} from '@shared/models'
import { FileDownloads } from './file-downloads'
import { ErroFrase } from '../lib/fork-i18n/erro-frase'
import { traduzir } from '../lib/fork-i18n/index.ts'
import { idiomaAtual } from '../lib/fork-i18n/estado.ts'
import { BotSync, sleep } from './bot-sync'
import type { SessionIntent } from './fork-sessions'
import { botEditorUpdate } from '@shared/bot-autonomy'
import { bloqueioSenha, destinoSenha, type ResultadoEnvio } from './fork-senha-bloqueio'

export { builtInBackendName, type Connection } from './bot-mirror'

/** A file picked in the composer, sent with the next message. */
export interface OutgoingFile {
  id: string
  name: string
  data: Uint8Array
}

/** What a message carries besides its text; `replyTo` shows on the bubble until the computer's own quote replaces it. */
export interface SendOptions {
  voz?: AcompanhamentoEnvioVoz
  files?: OutgoingFile[]
  nonce?: string
  pasted?: string[]
  contentOrigin?: OriginReport
  sourceKind?: 'voice' | 'generated'
  replyTo?: ReplyQuote | null
  /** Fork (UI-01): a extra escolhida no instante do envio (store/fork-sessions.ts `intent`); sem ela, a principal. */
  session?: SessionIntent
}

export interface TeamSettings {
  primaryBotId: string | null
  permission: Bot['permission']
  workspaceRoot: string | null
}

/** Anything larger is refused by the computer. */
export const MAX_FILE_SIZE = 100 * 1024 * 1024

export const QUICK_REACTIONS = ['👍', '❤️', '😂', '🎉', '👀', '✅']

/**
 * One computer's mirror (bots, transcripts), kept by one events stream (catch-up since
 * `rev`, then live). Port of kit's `BotStore`.
 */
export class BotStore extends BotSync {
  /** E2: mesma rota capturada, com espelho e rascunhos próprios do Space. */
  mirrorInSpace(spaceId: string) {
    return new BotStore(this.computer, this.isLoopback, this.makeTransport, this.appVersion, this.contextId, spaceId, this.hello?.hostId)
  }

  /** Recibo confirmado da criação: a seleção não espera o stream nem volta para o Berga por uma corrida de rede. */
  receiveCreatedBot(raw: Bot, hubId: string) {
    if (this.retired || !this.spaceId || raw.spaceId !== this.spaceId || this.hello?.hostId !== hubId) return false
    const bot = normalizeBot(raw)
    const before = this.bots.get(bot.id)
    if (!before || before.rev <= bot.rev) this.bots.set(bot.id, bot)
    this.onRosterChanged?.()
    this.changed()
    return true
  }

  readonly fileDownloads = new FileDownloads(() => this.changed())

  override retire() { this.pararConsultaMemoria(); retireAvatarEditors(this); this.fileDownloads.retire(); super.retire() }

  private outgoingFiles = new Map<string, OutgoingFile[]>()

  /** #753: uploads de anexo em curso (dentro de um envio). */
  private uploadsEmCurso = 0

  /**
   * #753: o que só vive em memória nesta loja: upload em curso, arquivos de envios ainda guardados para Reenviar e
   * mensagens em envio (a gravação do cache só leva as locais que falharam). A recarga da web pergunta antes.
   */
  private readonly pararConsultaMemoria = registrarEstadoEmMemoria(() => this.retired ? {} : {
    soEmMemoria: this.uploadsEmCurso > 0 || this.outgoingFiles.size > 0 ||
      [...this.entries.values()].some(lista => lista.some(e => e.id.startsWith('local-') && e.data.status !== 'failed')),
  })

  /** MX-214: o aviso de senha colada deste bot está aberto, então nenhuma mensagem sai por entrada nenhuma. */
  envioBloqueado(botId: string): boolean {
    return bloqueioSenha.bloqueado(destinoSenha(this.computer.id, botId))
  }

  /**
   * O motivo aparece no próprio cartão do aviso de senha (um aviso, não um erro: nada deu errado, a proteção funcionou).
   * Quem recebe `bloqueado` mantém texto, anexos e a entrada falha.
   */
  avisarEnvioBloqueado(botId: string) {
    bloqueioSenha.barrar(destinoSenha(this.computer.id, botId))
  }

  /** Devolve `bloqueado` (nada foi criado nem enviado, e o motivo foi avisado) ou `aceito`. */
  send(text: string, botId: string, { files = [], nonce = crypto.randomUUID(), pasted = [], replyTo = null, contentOrigin, sourceKind, voz, session }: SendOptions = {}): ResultadoEnvio {
    const trimmed = text.trim()
    if ((!trimmed && !files.length) || this.retired) { voz?.concluir(false); return 'aceito' }
    if (this.envioBloqueado(botId)) {
      this.avisarEnvioBloqueado(botId)
      voz?.concluir(false)
      return 'bloqueado'
    }
    const report = contentOrigin ?? (sourceKind ? originReport(text, originDraft(text, sourceKind), originDevice(), 'desktopVoice') : undefined)
    pasted = originPasted(trimmed, report, pasted)
    const data: Entry['data'] = { text: trimmed, status: 'sending', clientNonce: nonce }
    if (report) data.contentOrigin = { schema: 1, mode: 'observe', status: 'appReported', report, segments: report.segments, attachments: [], reply: null }
    if (pasted.length) data.pasted = pasted
    if (replyTo) data.replyTo = replyTo
    if (files.length) {
      data.attachments = files.map((f) => ({ id: f.id, name: f.name, size: f.data.length }))
      this.outgoingFiles.set(nonce, files)
    }
    this.upsert({
      id: `local-${nonce}`, seq: Number.MAX_SAFE_INTEGER, botId, rev: 0, kind: 'user',
      turn: 0, data, createdAt: Date.now(), updatedAt: 0, ...(session ? { sessionId: session.id } : {}),
    })
    this.changed()
    const vinculo = acompanharVinculoEnvio(this, botId, nonce)
    void (async () => {
      try {
        // Safe to repeat: the computer skips a nonce it already has.
        const e = await this.withLink(async (client) => {
          vinculo.despachar(client)
          // Fork (UI-01): cada tentativa (inclusive o replay depois de status 0) só segue no link da intenção; um link
          // novo recusa antes de qualquer upload ou envio.
          if (session && !this.sessions.current(session, client)) throw this.sessions.error('linkChanged')
          let ids: string[] | null = null
          const pending = this.outgoingFiles.get(nonce)
          if (pending) {
            ids = []
            for (const file of pending) {
              const id = crypto.randomUUID()
              this.uploadsEmCurso++
              try { await client.upload(botId, id, file.name, file.data) } finally { this.uploadsEmCurso-- }
              cacheAttachment(id, file.data)
              ids.push(id)
            }
          }
          voz?.validar()
          // Fork (UI-01): depois do último upload, relê o anúncio e a sessão neste link e confere a época; entre o fim
          // da releitura e o `send` não há outro await. Recusa aqui não envia nada, nem à principal.
          if (session) await this.sessions.guardSend(session, client)
          const sent = await client.send(botId, trimmed, nonce, ids, pasted, replyTo?.id ?? null, report, session?.id)
          // Depois do `send` o efeito já ocorreu: as conferências abaixo só decidem o que mostrar, nunca desfazem.
          if (session && !this.sessions.current(session, client)) throw this.sessions.error('linkChanged')
          return sent
        }, !voz)
        this.outgoingFiles.delete(nonce)
        if (session && e.sessionId !== session.id) {
          // O Hub gravou fora da extra (trocado por um que ignora `sessionId` depois da releitura): mostra a entrada
          // onde ela está e diz isso, sem marcar como não enviada nem tentar de novo.
          this.upsert(e)
          this.sessions.fail(botId, session.id, 'misrouted')
          return
        }
        if (!vinculo.aceitar(e)) return voz?.concluir(false)
        voz?.concluir(true)
        this.upsert(e)
        this.changed()
      } catch (error) {
        voz?.concluir(false)
        if (session) {
          // Fork (UI-01): a mensagem da extra fica na bolha da extra, com Tentar de novo (que revalida), mesmo se o
          // link mudou; o texto não se perde e nada vai para a principal.
          const client = this.connectedClient
          const failure = !client || !this.sessions.current(session, client) ? 'linkChanged' : this.sessions.failureOf(error)
          this.sessions.fail(botId, session.id, failure)
          return this.markLocal(nonce, botId, 'failed')
        }
        if (!vinculo.aceitar()) return
        // Recusa tipada (ex.: mismatch) fica registrada como motivo, além da bolha "não enviada".
        if (error instanceof ErroFrase) this.acaoRecusada = error.frase
        if (this.outgoingFiles.has(nonce)) this.lastError = `Couldn't send the files: ${error instanceof Error ? error.message : error}`
        // A fila cheia não cria nada no Hub: a bolha fica "não enviada", com os anexos guardados para tentar de novo.
        if (error instanceof Error && error.message.startsWith('queue_full')) this.lastError = traduzir(idiomaAtual(), 'conversa.fila.erro.cheia')
        this.markLocal(nonce, botId, 'failed')
      }
    })()
    return 'aceito'
  }

  retry(entry: Entry): ResultadoEnvio {
    const files = (entry.data.clientNonce && this.outgoingFiles.get(entry.data.clientNonce)) || []
    const text = entry.data.text ?? ''
    if (!text && !files.length) return 'aceito'
    // MX-214: o bloqueio é conferido ANTES de tirar a entrada falha da conversa; barrado, ela continua lá com Reenviar.
    if (this.envioBloqueado(entry.botId)) {
      this.avisarEnvioBloqueado(entry.botId)
      return 'bloqueado'
    }
    // Fork (UI-01): a falha de uma extra tenta de novo na mesma extra, ou fica como está; nunca vai à principal.
    const extra = entry.sessionId && entry.sessionId !== entry.botId ? entry.sessionId : null
    const session = extra ? this.sessions.intent(entry.botId, extra) : undefined
    if (session === null) { this.sessions.fail(entry.botId, extra, 'unsupported'); return 'aceito' }
    this.entries.set(entry.botId, this.allEntries(entry.botId).filter((e) => e.id !== entry.id))
    return this.send(text, entry.botId, { files, nonce: entry.data.clientNonce, replyTo: entry.data.replyTo, pasted: entry.data.pasted, contentOrigin: entry.data.contentOrigin?.report ?? undefined, session })
  }

  retrySubscription(entry: Entry) {
    return this.perform((c) => c.call('retrySubscription', { botId: entry.data.author ?? entry.botId, entryId: entry.id }), true)
  }

  discard(entry: Entry) {
    if (entry.data.clientNonce) this.outgoingFiles.delete(entry.data.clientNonce)
    this.entries.set(entry.botId, this.allEntries(entry.botId).filter((e) => e.id !== entry.id))
    this.changed()
    this.scheduleSave()
  }

  private markLocal(nonce: string, botId: string, status: string) {
    const list = this.entries.get(botId)
    const i = list?.findIndex((e) => e.id === `local-${nonce}`) ?? -1
    if (!list || i < 0) return
    const next = [...list]
    next[i] = { ...next[i]!, data: { ...next[i]!.data, status } }
    this.entries.set(botId, next)
    this.changed()
    this.scheduleSave()
  }

  /** A sent file's bytes: cached after the first fetch (sent files never change). */
  async attachmentData(fileId: string, botId: string): Promise<Uint8Array | null> {
    const cached = attachmentCache.get(fileId)
    if (cached) return cached
    const client = this.connectedClient
    if (!client) return null
    try {
      const data = await client.readUpload(botId, fileId)
      cacheAttachment(fileId, data)
      return data
    } catch {
      return null
    }
  }

  /** DON-907 (D431): uma ação por mensagem na fila (views/fork/conversa/fila.ts). Devolve o erro, sem mostrá-lo. */
  async queuedAction(method: import('../views/fork/conversa/fila.ts').MetodoFila, body: Record<string, unknown>): Promise<unknown> {
    try {
      await this.withLink((c) => c.call(method, body))
      return null
    } catch (error) {
      return error ?? 'failed'
    }
  }

  stop(botId: string) {
    return this.perform((c) => c.call('stop', { botId }), true)
  }

  newSession(botId: string) {
    return this.perform((c) => c.call('newSession', { botId }))
  }

  logCall(botId: string, seconds: number) {
    return this.perform((c) => c.call('logCall', { botId, seconds }))
  }

  screenTakeover(on: boolean) {
    return this.perform(async (c) => {
      const epoch = this.screenEpoch
      this.adoptScreen(normalizeScreen(await c.call<ScreenState>('screenTakeover', { on })), c, epoch)
      this.changed()
    }, true)
  }

  /** Only the computer itself may turn remote screen on. */
  async setScreenEnabled(on: boolean) {
    const client = await this.ready()
    const epoch = this.screenEpoch
    this.adoptScreen(normalizeScreen(await client.call<ScreenState>('setScreenEnabled', { enabled: on })), client, epoch)
    this.changed()
  }

  /** Product analytics on this computer; undefined before hello. */
  get analytics() {
    return this.hello?.analytics
  }

  /** Only the computer itself may change analytics (the host accepts it from loopback). */
  async setAnalytics(enabled: boolean) {
    const client = await this.ready()
    const res = await client.call<{ enabled: boolean }>('setAnalytics', { enabled })
    this.guardProjection(client)
    if (this.hello) this.hello = { ...this.hello, analytics: res.enabled }
    this.changed()
  }

  /** Toggles the user's reaction; shown at once, then replaced by the host's copy. */
  react(entry: Entry, emoji: string) {
    const list = this.entries.get(entry.botId)
    const i = list?.findIndex((e) => e.id === entry.id) ?? -1
    if (!list || i < 0) return
    const reactions = [...(list[i]!.data.reactions ?? [])]
    const at = reactions.indexOf(emoji)
    if (at >= 0) reactions.splice(at, 1)
    else reactions.push(emoji)
    const next = [...list]
    next[i] = { ...next[i]!, data: { ...next[i]!.data, reactions } }
    return this.perform(async (c) => {
      this.entries.set(entry.botId, next)
      this.changed()
      const res = await c.call<{ entry: Entry }>('react', { entryId: entry.id, emoji })
      this.upsert(res.entry)
      this.changed()
    })
  }

  respond(entry: Entry, option: string | null, source?: 'voice', voz?: AcompanhamentoEnvioVoz) {
    if (this.answering.has(entry.id)) return voz?.concluir(false)
    this.answering.set(entry.id, option ?? '')
    this.changed()
    void (async () => {
      try {
        await this.withLink((c) => {
          voz?.validar()
          return c.call('respondPermission', { botId: entry.botId, entryId: entry.id, entryRev: entry.rev, optionId: option, ...(source ? { source } : {}) })
        }, !voz)
        voz?.concluir(true)
        // The card's own update follows on the events stream; hold the spinner until then.
        await sleep(2000)
      } catch (error) {
        voz?.concluir(false)
        if (voz?.erro) { this.lastError = voz.erro; this.changed() }
        else this.noteActionError(error)
      }
      this.answering.delete(entry.id)
      this.changed()
    })()
  }

  async undoTeamChange(entry: Entry) {
    if (this.answering.has(entry.id)) return
    this.answering.set(entry.id, 'team')
    this.changed()
    try {
      const result = await this.withLink((c) => c.call<{ entry: Entry }>('undoTeamChange', { entryId: entry.id }), true)
      this.upsert(result.entry)
    } catch (error) {
      this.noteActionError(error)
    } finally {
      this.answering.delete(entry.id)
      this.changed()
    }
  }

  async answerMemory(entry: Entry, accept: boolean) {
    if (this.answering.has(entry.id)) return
    this.answering.set(entry.id, accept ? 'save' : 'decline')
    this.changed()
    try {
      const result = await this.withLink((c) => c.call<{ entry: Entry }>('answerMemory', {
        entryId: entry.id, botId: entry.botId, entryRev: entry.rev, accept,
      }), true)
      this.upsert(result.entry)
    } finally {
      this.answering.delete(entry.id)
      this.changed()
    }
  }

  /** Takes back a bot's memory write from its "Lembrei" line; the line's update follows on the stream. */
  undoMemory(entry: Entry) {
    if (this.answering.has(entry.id)) return
    this.answering.set(entry.id, 'undo')
    this.changed()
    void (async () => {
      try {
        await this.withLink((c) => c.call('undoMemory', { entryId: entry.id }), true)
      } catch (error) {
        this.noteActionError(error)
      }
      this.answering.delete(entry.id)
      this.changed()
    })()
  }

  /** Saves the user's version of a handoff that hasn't been sent; whether it did. */
  async editHandoff(entry: Entry, text: string) {
    try {
      await this.withLink((c) => c.call('editHandoff', { entryId: entry.id, text }), true)
      return true
    } catch (error) {
      this.noteActionError(error)
      return false
    }
  }

  /** Carries on with the bot's current conversation instead of the handoff. */
  keepConversation(entry: Entry) {
    if (this.answering.has(entry.id)) return
    this.answering.set(entry.id, 'handoff')
    this.changed()
    void (async () => {
      try {
        await this.withLink((c) => c.call('keepConversation', { entryId: entry.id }), true)
      } catch (error) {
        this.noteActionError(error)
      }
      this.answering.delete(entry.id)
      this.changed()
    })()
  }

  /** The roster's "Mark as read". */
  markAllRead(botId: string) {
    const bot = this.bots.get(botId)
    if (!bot || bot.unread <= 0) return
    this.bots.set(botId, { ...bot, unread: 0 })
    this.changed()
    return this.perform((c) => c.call('markRead', { botId }), true)
  }

  private patchBot(bot: Bot, change: Partial<BotDraft>) {
    return this.perform(c => {
      const current = this.bots.get(bot.id)
      if (current) this.bots.set(bot.id, { ...current, ...change } as Bot)
      this.changed()
      return c.call('updateBot', { id: bot.id, ...change })
    }, true)
  }

  setPinned(bot: Bot, pinned: boolean) {
    if (bot.id === this.principal.protectedId) return
    return this.patchBot(bot, { pinned })
  }

  /** Whether the bot may see and use the computer's screen. */
  setComputer(bot: Bot, computer: boolean) {
    return this.patchBot(bot, { computer })
  }

  /** Whether the bot uses the memory all bots share. */
  setSharedMemory(bot: Bot, sharedMemory: boolean) {
    return this.patchBot(bot, { sharedMemory })
  }

  /** Whether the bot's harness keeps its own memory on beside Codync's. */
  setHarnessMemory(bot: Bot, harnessMemory: boolean) {
    return this.patchBot(bot, { harnessMemory })
  }

  setHidden(bot: Bot, hidden: boolean) {
    if (hidden && bot.id === this.principal.protectedId) return
    return this.patchBot(bot, { hidden })
  }

  setFastMode(bot: Bot, fastMode: boolean) {
    return this.patchBot(bot, { fastMode })
  }

  /** The roster as the user dragged it, top first: shown at once, then saved on the host. */
  reorder(ids: string[]) {
    return this.perform(c => {
      ids.forEach((id, position) => {
        const bot = this.bots.get(id)
        if (bot) this.bots.set(id, { ...bot, position })
      })
      this.changed()
      return c.call('reorderBots', { ids })
    }, true)
  }

  delete(bot: Bot) {
    if (bot.id === this.principal.protectedId) return
    return this.perform(c => {
      this.composerDrafts.removeBot(bot.id)
      this.bots.delete(bot.id)
      this.entries.delete(bot.id)
      this.onRosterChanged?.()
      this.changed()
      return c.call('deleteBot', { botId: bot.id })
    }, true)
  }

  /** Creates a group chat (or opens the one these bots already share) and selects it. */
  async createGroup(name: string, description: string, members: string[]) {
    const draft: GroupDraft = { kind: 'group', name, description, members }
    const client = await this.ready()
    const res = await client.call<{ bot: Bot }>('createBot', { ...draft, ...(this.spaceId ? { spaceId: this.spaceId, hubId: this.hello?.hostId } : {}) })
    this.guardProjection(client)
    const bot = normalizeBot(res.bot)
    this.bots.set(bot.id, bot)
    this.selection = bot.id
    this.onRosterChanged?.()
    this.changed()
    return bot
  }

  async updateGroup(draft: GroupDraft) {
    const client = await this.ready()
    const res = await client.call<{ bot: Bot }>('updateBot', draft)
    this.guardProjection(client)
    this.bots.set(res.bot.id, normalizeBot(res.bot))
    this.changed()
  }

  /** Takes one bot out of a group; an automatic "Alice, Bob" name follows the new roster. */
  removeMember(group: Bot, memberId: string) {
    const members = group.members.filter((m) => m !== memberId)
    const autoName = (ids: string[]) => ids.map((id) => this.bots.get(id)?.name).join(', ')
    const name = group.name === autoName(group.members) ? autoName(members) : group.name
    const shown = { ...group, name, members }
    this.bots.set(group.id, shown)
    this.changed()
    const draft: GroupDraft = { id: group.id, kind: 'group', name, description: group.description, members, pinned: group.pinned }
    // Recusado (ex.: é o líder do canal), o canal volta a ser o que era, salvo se o Hub já mandou outro.
    void this.withLink((c) => c.call('updateBot', draft), true).catch((error: unknown) => {
      if (this.bots.get(group.id) === shown) this.bots.set(group.id, group)
      this.noteActionError(error)
    })
  }

  /** The history between a bot and one peer. Never upserted: old notices would break `loadOlder`'s paging. */
  async botConversation(botId: string, peerId: string): Promise<Entry[]> {
    return (await this.ready()).botConversation(botId, peerId)
  }

  async save(draft: BotDraft) {
    const client = await this.ready()
    const escolha = avatarDoBot(draft)
    if (!draft.id && escolha) {
      if (draft.mexeriquinha) requireAvatarMexeriquinhaHub(this.hello)
      else requireAvatarHub(this.hello)
    }
    const res = draft.id
      ? await client.call<{ bot: Bot }>('updateBot', botEditorUpdate(draft))
      : await client.call<{ bot: Bot }>('createBot', { ...draft, ...(this.spaceId ? { spaceId: this.spaceId, hubId: this.hello?.hostId } : {}) })
    this.guardProjection(client)
    if (!draft.id && escolha) confirmAvatar(avatarDoBot(res.bot), escolha)
    const bot = normalizeBot(res.bot)
    this.bots.set(bot.id, bot)
    this.onRosterChanged?.()
    this.changed()
    return bot
  }

  /** D115: intenção humana parcial, independente do autosave do restante do perfil. */
  async saveAvatar(id: string, avatarConfig: BotAvatarConfig) {
    const client = await this.ready()
    if (validAvatar(avatarConfig)) requireAvatarHub(this.hello)
    else requireAvatarMexeriquinhaHub(this.hello)
    const res = await client.call<{ bot: Bot }>('updateBot', { id, ...camposAvatarDoBot(avatarConfig) })
    this.guardProjection(client)
    confirmAvatar(avatarDoBot(res.bot), avatarConfig)
    if (!this.bots.has(id)) return
    const bot = normalizeBot(res.bot)
    if (bot.rev >= (this.bots.get(id)?.rev ?? 0)) this.bots.set(id, bot)
    this.onRosterChanged?.()
    this.changed()
  }

  /** Escolha explícita, inclusive quando o valor é igual ao herdado. */
  async setPermission(id: string, permission: string) {
    const client = await this.ready()
    const res = await client.call<{ bot: Bot }>('updateBot', { id, permission, permissionChosenByUser: true })
    this.guardProjection(client)
    const bot = normalizeBot(res.bot)
    this.bots.set(id, bot)
    this.changed()
    return bot
  }

  async teamSettings() {
    return (await this.ready()).call<TeamSettings>('teamSettings', {})
  }

  async setPrimaryBot(primaryBotId: string | null) {
    return this.setTeamSettings({ primaryBotId })
  }

  async setTeamSettings(patch: Partial<TeamSettings>) {
    const client = await this.ready()
    const result = await client.call<TeamSettings>('setTeamSettings', patch)
    this.guardProjection(client)
    this.principal.saved(result)
    this.changed()
    return result
  }

  async refreshBackends() {
    const client = this.connectedClient
    if (!client || !this.hello) return false
    const hostId = this.hostId, epoch = this.principalEpoch
    try {
      const res = await client.call<{ backends: Hello['backends'] }>('refreshBackends', {}, 60_000)
      if (this.connectedClient !== client || this.hostId !== hostId || this.principalEpoch !== epoch || !this.hello) return false
      this.hello = { ...this.hello, backends: res.backends }
      this.changed()
      return true
    } catch { return false }
  }

  async listDirs(path: string | null) {
    return (await this.ready()).call<DirListing>('listDirs', { path })
  }

  async loadOlder(botId: string) {
    const client = this.connectedClient
    if (!client || this.historyComplete.has(botId)) return
    const first = this.allEntries(botId).find((e) => e.seq > 0 && e.seq !== Number.MAX_SAFE_INTEGER)?.seq ?? Number.MAX_SAFE_INTEGER
    try {
      const epoch = this.sessions.epoch
      const older = await client.history(botId, first, 100)
      if (this.connectedClient !== client || this.sessions.epoch !== epoch) return
      if (older.length < 100) this.historyComplete.add(botId)
      for (const e of older) this.upsert(e)
      this.changed()
    } catch {}
  }

  /** Fork (Barra05, passo 12): seleciona o bot e abre o painel direito na aba Sessões. */
  openSessionsPanel(botId: string) {
    this.setSelection(botId)
    this.sessions.requestPanel(botId)
  }

  setSelection(id: string | null) {
    this.selection = id
    this.changed()
  }

  consumeRoutineDraft(botId: string) {
    const value = this.routineDrafts.get(botId)
    if (value !== undefined) {
      this.routineDrafts.delete(botId)
      this.changed()
    }
    return value
  }

  setRoutineDraft(botId: string, text: string) {
    this.routineDrafts.set(botId, text)
    this.changed()
  }
}

// ponytail: in-memory attachment cache; a disk cache (main process) if relaunches refetching hurts.
const attachmentCache = new Map<string, Uint8Array>()
function cacheAttachment(id: string, data: Uint8Array) {
  attachmentCache.set(id, data)
}

export { isWorking, needsInput }
