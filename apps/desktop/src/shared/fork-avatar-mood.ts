import type { BaseMood, MomentAction } from '../renderer/lib/donnabot-avatars/src/core/types.ts'
import type { Bot, BotStatus, Entry } from './models.ts'

export type AvatarEvent = 'typing' | 'listening' | 'idle' | 'quiet' | 'generating' | 'tool' | 'completed' | 'error' | 'message' | 'disabled' | 'enabled'
export interface AvatarMoodState {
  base: BaseMood; action: MomentAction | null; sequence: number; at: number
  /** Fatos projetados também para a ilha. Conexão e digitação não contam como atividade do bot. */
  runtimeStatus?: BotStatus
  ultimaAtividade?: number
  indisponivel?: boolean
  precisaResposta?: boolean
}
export const IDLE_MOOD: AvatarMoodState = { base: 'idle', action: null, sequence: 0, at: 0 }

/** Política pura dos eventos do runtime → base contínua e reação breve da biblioteca. */
export function avatarMoodEvent(state: AvatarMoodState, event: AvatarEvent, now = Date.now()): AvatarMoodState {
  const base = (base: BaseMood): AvatarMoodState => ({ ...state, base, action: null, sequence: state.sequence + 1, at: now })
  const play = (action: MomentAction): AvatarMoodState => ({ ...state, action, sequence: state.sequence + 1, at: now })
  switch (event) {
    case 'typing': return state.base === 'idle' ? base('listen') : state
    case 'listening': return state.base === 'listen' ? state : base('listen')
    case 'idle': return base('idle')
    case 'quiet': return state.base === 'listen' ? base('idle') : state
    case 'generating': return base('think')
    case 'tool': return base('work')
    case 'completed': return { ...base('idle'), action: 'happy' }
    case 'error': return { ...base('idle'), action: 'surprise' }
    case 'message': return state.base === 'sleep' ? state : play('blink')
    case 'disabled': return base('sleep')
    case 'enabled': return { ...base('idle'), action: 'wake' }
  }
}

const owners = new WeakMap<Bot, AvatarMoods>()
export const avatarMoodsFor = (bot: Bot) => owners.get(bot)

/** Um store por Hub; IDs de bots de Hubs diferentes não compartilham reações. */
export class AvatarMoods {
  private states = new Map<string, AvatarMoodState>()
  private bots = new Map<string, Bot>()
  private tools = new Map<string, Set<string>>()
  private listeners = new Set<() => void>()
  private typingTimers = new Map<string, ReturnType<typeof setTimeout>>()
  private seen = new Map<string, string>()
  private offline = false
  activity = (id: string) => this.get(id).ultimaAtividade ?? 0
  available = () => !this.offline
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  get = (id: string) => this.states.get(id) ?? IDLE_MOOD
  private emit(id: string, event: AvatarEvent) {
    const old = this.get(id)
    const next = avatarMoodEvent(old, event)
    if (next === old) return
    this.states.set(id, next)
    for (const listener of this.listeners) listener()
  }
  bot(bot: Bot) {
    const previous = this.bots.get(bot.id)
    owners.set(bot, this)
    this.bots.set(bot.id, bot)
    const old = this.get(bot.id)
    const activity = Math.max(0, ...[old.ultimaAtividade, bot.lastAt, bot.createdAt, bot.startedAt,
      previous && previous.status !== bot.status && bot.status !== 'unavailable' && previous.status !== 'unavailable' && !this.offline ? Date.now() : 0]
      .filter((at): at is number => typeof at === 'number' && Number.isFinite(at)))
    const precisaResposta = bot.status === 'needsInput' || (bot.pendingQuestions?.length ?? 0) > 0
    if (old.runtimeStatus !== bot.status || old.ultimaAtividade !== activity || old.indisponivel !== this.offline || old.precisaResposta !== precisaResposta) {
      this.states.set(bot.id, { ...old, runtimeStatus: bot.status, ultimaAtividade: activity, indisponivel: this.offline, precisaResposta })
      for (const listener of this.listeners) listener()
    }
    // O runtime assumiu o estado: uma pausa de digitação anterior não o apaga.
    if (bot.status !== 'idle') {
      clearTimeout(this.typingTimers.get(bot.id))
      this.typingTimers.delete(bot.id)
    }
    if (this.offline) return this.emit(bot.id, 'disabled')
    if (['disabled', 'stopped', 'offline', 'sleep'].includes(bot.status)) return this.emit(bot.id, 'disabled')
    if (this.get(bot.id).base === 'sleep') this.emit(bot.id, 'enabled')
    if (bot.status === 'working') {
      const target = this.tools.get(bot.id)?.size ? 'tool' : 'generating'
      if (this.get(bot.id).base !== (target === 'tool' ? 'work' : 'think')) this.emit(bot.id, target)
    } else if (bot.status === 'needsInput') {
      if (this.get(bot.id).base !== 'listen') this.emit(bot.id, 'listening')
    } else if (bot.status === 'error' && previous?.status !== 'error') {
      this.tools.delete(bot.id)
      this.emit(bot.id, 'error')
    } else if (previous && ['working', 'needsInput'].includes(previous.status) && bot.status === 'idle') {
      this.tools.delete(bot.id)
      this.emit(bot.id, 'completed')
    } else if (bot.status === 'waiting' && ['think', 'work'].includes(this.get(bot.id).base)) this.emit(bot.id, 'idle')
    else if (bot.status === 'unavailable') {
      // runtime-audiencia-01: sem informação não é turno encerrado nem falha; o rosto só para, sem reação.
      this.tools.delete(bot.id)
      if (this.get(bot.id).base !== 'idle') this.emit(bot.id, 'idle')
    }
  }
  entry(entry: Entry, reactions = true) {
    const id = entry.data.author ?? entry.botId
    // Também reconstrói a atividade do histórico; uma repetição não reinicia o relógio.
    if (Number.isFinite(entry.updatedAt) && entry.updatedAt > this.activity(id)) {
      this.states.set(id, { ...this.get(id), ultimaAtividade: entry.updatedAt })
      for (const listener of this.listeners) listener()
    }
    const signature = `${entry.kind}/${entry.data.status}/${entry.data.final}`
    const key = entry.id
    if (this.seen.get(key) === signature) return
    const first = !this.seen.has(key)
    this.seen.set(key, signature)
    if (this.seen.size > 2000) this.seen.delete(this.seen.keys().next().value!)
    // Reconstrói ferramentas longas no catch-up; apenas reações têm prazo de frescor.
    const recent = reactions && Date.now() - entry.updatedAt <= 15_000
    if (entry.kind === 'tool') {
      const tools = this.tools.get(id) ?? new Set<string>()
      const currentTurn = entry.createdAt >= (this.bots.get(id)?.startedAt ?? 0)
      if (currentTurn && ['pending', 'in_progress'].includes(entry.data.status ?? '')) tools.add(entry.id)
      else tools.delete(entry.id)
      this.tools.set(id, tools)
      if (this.bots.get(id)?.status === 'working') this.emit(id, tools.size ? 'tool' : 'generating')
      if (recent && entry.data.status === 'failed') this.emit(id, 'error')
    } else if (recent && entry.kind === 'agent' && first) this.emit(id, 'message')
    else if (recent && entry.kind === 'notice' && entry.data.style === 'error') this.emit(id, 'error')
  }
  restoreEntries(entries: Map<string, Entry[]>) {
    for (const list of entries.values()) for (const entry of list) this.entry(entry, false)
  }
  typing(id: string) {
    if (this.offline || this.bots.get(id)?.status !== 'idle') return
    // Borda de entrada uma vez; as teclas seguintes só adiam a borda de saída.
    this.emit(id, 'typing')
    clearTimeout(this.typingTimers.get(id))
    this.typingTimers.set(id, setTimeout(() => { this.typingTimers.delete(id); this.emit(id, 'quiet') }, 1800))
  }
  connection(online: boolean) {
    if (this.offline === !online) return
    this.offline = !online
    for (const bot of this.bots.values()) {
      this.states.set(bot.id, { ...this.get(bot.id), indisponivel: !online })
      if (online) { this.emit(bot.id, 'enabled'); this.bot(bot) }
      else this.emit(bot.id, 'disabled')
    }
  }
  remove(id: string) {
    clearTimeout(this.typingTimers.get(id)); this.typingTimers.delete(id)
    this.states.delete(id); this.bots.delete(id); this.tools.delete(id)
  }
  retire() {
    for (const timer of this.typingTimers.values()) clearTimeout(timer)
    this.typingTimers.clear(); this.states.clear(); this.bots.clear(); this.tools.clear(); this.seen.clear(); this.listeners.clear()
  }
}

/** Liga também cópias otimistas e respostas de API ao mesmo estado do bot. */
export class AvatarBotMap extends Map<string, Bot> {
  private readonly moods: AvatarMoods
  constructor(moods: AvatarMoods, bots: Bot[] = []) {
    super()
    this.moods = moods
    for (const bot of bots) this.set(bot.id, bot)
  }
  override set(id: string, bot: Bot): this {
    this.moods.bot(bot)
    return super.set(id, bot)
  }
  override delete(id: string): boolean { this.moods.remove(id); return super.delete(id) }
}
