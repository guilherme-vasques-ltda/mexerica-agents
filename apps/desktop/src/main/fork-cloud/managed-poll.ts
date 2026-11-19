/** Ritmo da leitura de fundo do computador oferecido pela nuvem (health + managed-offer).
 * Curto só enquanto o fluxo gerenciado avança; fora dele, espera crescente com teto e
 * um pouco de aleatoriedade. Escondido, minimizado, tela bloqueada ou suspenso: nenhuma
 * leitura periódica. Ocioso: só uma checagem local, sem rede. Voltar à janela, agir ou
 * entrar na conta lê na hora. Sair da conta não depende daqui: sem sessão, a leitura
 * não faz pedido nenhum. */
export type PollOutcome = 'active' | 'idle' | 'error'
export type PollClock = { now(): number; setTimeout(fn: () => void, ms: number): unknown; clearTimeout(handle: unknown): void; random(): number }

export const POLL = { activeMs: 2_000, baseMs: 60_000, maxMs: 300_000, awayCheckMs: 60_000, jitter: 0.2 } as const

const realClock: PollClock = {
  now: () => Date.now(),
  setTimeout: (fn, ms) => { const t = setTimeout(fn, ms); t.unref?.(); return t },
  clearTimeout: (t) => clearTimeout(t as ReturnType<typeof setTimeout>),
  random: Math.random,
}

export class ManagedPoll {
  private timer: unknown = null
  private delay: number = POLL.baseMs
  private lastRun = -Infinity
  private running = false
  private again = false
  private paused = false
  private stopped = false
  private wasAway = false

  private readonly tick: () => Promise<PollOutcome>
  private readonly away: () => boolean
  private readonly clock: PollClock
  constructor(tick: () => Promise<PollOutcome>, away: () => boolean = () => false, clock: PollClock = realClock) { this.tick = tick; this.away = away; this.clock = clock }

  /** Janela escondida/minimizada, tela bloqueada ou suspensão: para as leituras periódicas. */
  pause() { this.paused = true; this.clear() }
  /** Janela de volta à vista: lê na hora. Devolve false se já estava rodando à vista. */
  resume(): boolean { if (!this.paused) return false; this.paused = false; this.now(); return true }
  /** Ação do usuário, foco, nova sessão: lê na hora e recomeça do intervalo base. */
  now() {
    if (this.stopped) return
    this.delay = POLL.baseMs
    if (this.running) { this.again = true; return }
    this.clear()
    const wait = this.lastRun + POLL.activeMs - this.clock.now()
    if (wait > 0) this.schedule(wait)
    else void this.run()
  }
  stop() { this.stopped = true; this.clear() }

  private clear() { if (this.timer !== null) this.clock.clearTimeout(this.timer); this.timer = null }
  private schedule(ms: number) { this.clear(); this.timer = this.clock.setTimeout(() => { this.timer = null; void this.run() }, ms) }

  private async run() {
    if (this.stopped) return
    if (this.away()) { this.wasAway = true; if (!this.paused) this.schedule(POLL.awayCheckMs); return }
    if (this.wasAway) { this.wasAway = false; this.delay = POLL.baseMs }
    this.running = true; this.lastRun = this.clock.now()
    let outcome: PollOutcome = 'error'
    try { outcome = await this.tick() } catch { /* conta como erro */ }
    finally { this.running = false }
    if (this.stopped) return
    if (this.again) { this.again = false; this.schedule(POLL.activeMs); return }
    if (this.paused) return
    if (outcome === 'active') { this.delay = POLL.baseMs; this.schedule(POLL.activeMs); return }
    this.schedule(this.delay * (1 - POLL.jitter * this.clock.random()))
    this.delay = Math.min(this.delay * 2, POLL.maxMs)
  }
}
