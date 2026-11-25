interface Rc2Hello {
  computerId: string
  hostId: string
}

export interface Rc2ChildHandle {
  end(): void
  terminate(): void
  kill(): void
  exited: Promise<void>
  // Descendentes vivos no grupo do processo; consultado após cada sinal e antes de confirmar o encerramento.
  survivors?(): number | Promise<number>
}

export interface Rc2Connection {
  hello(): Promise<Rc2Hello>
  events(): AsyncIterable<string>
  close(): Promise<void>
  child?: Rc2ChildHandle
}

type Rc2Transport = (signal: AbortSignal) => Promise<Rc2Connection>

export function nativeRc2Transport(): Rc2Transport | null {
  return null
}

export interface Rc2Deadlines {
  open: number
  hello: number
  close: number
  eof: number
  sigterm: number
  sigkill: number
}

export type Rc2Schedule = (ms: number, fire: () => void) => () => void

const defaultDeadlines: Rc2Deadlines = { open: 15000, hello: 10000, close: 5000, eof: 2000, sigterm: 5000, sigkill: 5000 }

const deadlineKeys = ['open', 'hello', 'close', 'eof', 'sigterm', 'sigkill'] as const

const maxDeadline = 2 ** 31 - 1

const defaultSchedule: Rc2Schedule = (ms, fire) => {
  const timer = setTimeout(fire, ms)
  return () => clearTimeout(timer)
}

interface Generation {
  session: object
  abort: AbortController
  opened: Promise<Rc2Connection | null>
}

interface Invalidation {
  openedTimedOut?: boolean
  uncertain?: boolean
}

export class Rc2HubLifecycle {
  #generation: Generation | null = null
  #draining: Promise<boolean> | null = null
  #unsafeToRestart = false
  #stops = 0
  readonly #session: () => object | null
  readonly #transport: Rc2Transport | null
  readonly #deadlines: Rc2Deadlines
  readonly #schedule: Rc2Schedule

  constructor(session: () => object | null, transport: Rc2Transport | null,
    options: { deadlines?: Rc2Deadlines; schedule?: Rc2Schedule } = {}) {
    this.#session = session
    this.#transport = transport
    this.#deadlines = options.deadlines ?? defaultDeadlines
    if (options.deadlines) {
      const entries = Object.entries(options.deadlines)
      if (entries.length !== deadlineKeys.length || deadlineKeys.some(key => !Object.hasOwn(options.deadlines!, key)) ||
          entries.some(([, value]) => typeof value !== 'number' || !Number.isFinite(value) || value <= 0 || value > maxDeadline)) {
        throw new Error('invalid_rc2_deadlines')
      }
    }
    this.#schedule = options.schedule ?? defaultSchedule
  }

  #current(generation: Generation) {
    return this.#generation === generation && !generation.abort.signal.aborted && this.#session() === generation.session
  }

  #settled(promise: Promise<void>, ms: number): Promise<boolean> {
    return new Promise(resolve => {
      const cancel = this.#schedule(ms, () => resolve(false))
      promise.then(() => { cancel(); resolve(true) }, () => { cancel(); resolve(false) })
    })
  }

  #settledValue(promise: Promise<boolean>, ms: number): Promise<boolean> {
    return new Promise(resolve => {
      const cancel = this.#schedule(ms, () => resolve(false))
      promise.then(value => { cancel(); resolve(value) }, () => { cancel(); resolve(false) })
    })
  }

  #raceOpened(opened: Promise<Rc2Connection | null>): Promise<{ value: Rc2Connection | null; timedOut: boolean }> {
    return new Promise(resolve => {
      const cancel = this.#schedule(this.#deadlines.open, () => resolve({ value: null, timedOut: true }))
      opened.then(value => { cancel(); resolve({ value, timedOut: false }) })
    })
  }

  // Uma exceção em uma etapa do escalonamento não interrompe as seguintes: cada
  // sinal é tentado isoladamente e o resultado só é confirmado pelo close observado.
  // O encerramento alcança o filho e seus descendentes: confirmar exige o processo
  // morto e nenhum sobrevivente no grupo; sonda com erro conta como sobrevivente.
  async #groupDead(child: Rc2ChildHandle): Promise<boolean> {
    await child.exited
    try { return child.survivors ? (await Promise.resolve(child.survivors())) === 0 : true } catch { return false }
  }

  async #reap(child: Rc2ChildHandle | undefined): Promise<boolean> {
    if (!child) return true
    try { child.end() } catch { /* segue para o SIGTERM */ }
    if (await this.#settledValue(this.#groupDead(child), this.#deadlines.eof)) return true
    try { child.terminate() } catch { /* segue para o SIGKILL */ }
    if (await this.#settledValue(this.#groupDead(child), this.#deadlines.sigterm)) return true
    try { child.kill() } catch { /* a ausência de close decide o estado */ }
    return this.#settledValue(this.#groupDead(child), this.#deadlines.sigkill)
  }

  async #discard(connection: Rc2Connection): Promise<void> {
    try { await this.#settled(connection.close(), this.#deadlines.close) } catch { /* estado já marcado inseguro */ }
    await this.#reap(connection.child)
  }

  #invalidate(generation: Generation, invalidation: Invalidation = {}): Promise<boolean> {
    this.#generation = null
    if (invalidation.uncertain) this.#unsafeToRestart = true
    const draining = Promise.resolve().then(async () => {
      let closed = !invalidation.uncertain
      if (invalidation.openedTimedOut) {
        this.#unsafeToRestart = true
        void generation.opened.then(late => { if (late) void this.#discard(late) })
      } else {
        const raced = await this.#raceOpened(generation.opened)
        if (raced.timedOut) {
          this.#unsafeToRestart = true
          void generation.opened.then(late => { if (late) void this.#discard(late) })
        } else if (raced.value) {
          let settled = true
          try { settled = await this.#settled(raced.value.close(), this.#deadlines.close) } catch { settled = false }
          if (!settled) closed = false
          try { if (!(await this.#reap(raced.value.child))) closed = false } catch { closed = false }
        }
      }
      if (!closed) this.#unsafeToRestart = true
      return !this.#unsafeToRestart
    })
    this.#draining = draining
    void draining.then(() => { if (this.#draining === draining) this.#draining = null })
    generation.abort.abort()
    return draining
  }

  async start(): Promise<boolean> {
    const transport = this.#transport
    if (!transport || this.#unsafeToRestart) return false
    // A drenagem em curso já decidiu: nova partida espera o close confirmar e é recusada sem fila,
    // inclusive a disparada por listener de abort, handler de exited ou descarte da geração morta.
    if (this.#draining) return false
    const session = this.#session()
    if (this.#generation && this.#generation.session !== session) {
      const draining = this.stop()
      const stops = this.#stops
      if (!(await draining) || stops !== this.#stops) return false
    }
    if (!session || this.#session() !== session) return false
    if (!this.#generation) {
      const abort = new AbortController()
      const opened = Promise.resolve().then(() => {
        if (abort.signal.aborted || this.#session() !== session) return null
        return transport(abort.signal)
      }).catch(() => null)
      this.#generation = { session, abort, opened }
    }
    const generation = this.#generation
    const opened = await this.#raceOpened(generation.opened)
    if (opened.timedOut) {
      if (this.#generation === generation) this.#invalidate(generation, { openedTimedOut: true })
      else if (this.#draining) await this.#draining
      return false
    }
    const connection = opened.value
    if (!this.#current(generation)) {
      if (this.#generation === generation) await this.#invalidate(generation)
      else if (this.#draining) await this.#draining
      return false
    }
    if (!connection) {
      if (this.#generation === generation) await this.#invalidate(generation)
      else if (this.#draining) await this.#draining
      return false
    }
    if (connection.child) {
      void connection.child.exited.then(
        () => { if (this.#generation === generation && !generation.abort.signal.aborted) this.#invalidate(generation) },
        () => { if (this.#generation === generation && !generation.abort.signal.aborted) this.#invalidate(generation, { uncertain: true }) })
    }
    return true
  }

  stop(): Promise<boolean> {
    this.#stops++
    if (this.#draining) return this.#draining
    const generation = this.#generation
    if (!generation) return Promise.resolve(!this.#unsafeToRestart)
    return this.#invalidate(generation)
  }

  async hello(): Promise<Rc2Hello | null> {
    const generation = this.#generation
    if (!generation || !this.#current(generation)) return null
    const opened = await this.#raceOpened(generation.opened)
    const connection = opened.timedOut ? null : opened.value
    if (!connection || !this.#current(generation)) return null
    let cancelAbort = () => { }
    const aborted = new Promise<null>(resolve => {
      const done = () => resolve(null)
      if (generation.abort.signal.aborted) done()
      else generation.abort.signal.addEventListener('abort', done, { once: true })
      cancelAbort = () => generation.abort.signal.removeEventListener('abort', done)
    })
    // Nenhum hello sem prazo: expirado, o filho e a conexão são encerrados pela invalidação.
    const reply = await new Promise<Rc2Hello | null | false>(resolve => {
      const cancelTimer = this.#schedule(this.#deadlines.hello, () => resolve(false))
      Promise.race([connection.hello(), aborted]).then(
        value => { cancelTimer(); resolve(value) },
        () => { cancelTimer(); resolve(null) })
    })
    cancelAbort()
    if (reply === false) {
      if (this.#generation === generation) await this.#invalidate(generation)
      else if (this.#draining) await this.#draining
      return null
    }
    return reply && this.#current(generation) ? reply : null
  }

  async *events(): AsyncGenerator<string> {
    const generation = this.#generation
    if (!generation || !this.#current(generation)) return
    const opened = await this.#raceOpened(generation.opened)
    const connection = opened.timedOut ? null : opened.value
    if (!connection || !this.#current(generation)) return
    try {
      for await (const event of connection.events()) {
        if (!this.#current(generation)) return
        yield event
        if (!this.#current(generation)) return
      }
    } catch { return }
  }
}
