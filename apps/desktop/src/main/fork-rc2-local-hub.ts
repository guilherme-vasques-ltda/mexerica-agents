interface Rc2Hello {
  computerId: string
  hostId: string
}

export interface Rc2ChildHandle {
  end(): void
  terminate(): void
  kill(): void
  exited: Promise<void>
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
  eof: number
  sigterm: number
  sigkill: number
}

export type Rc2Schedule = (ms: number, fire: () => void) => () => void

const defaultDeadlines: Rc2Deadlines = { open: 15000, eof: 2000, sigterm: 5000, sigkill: 5000 }

const defaultSchedule: Rc2Schedule = (ms, fire) => {
  const timer = setTimeout(fire, ms)
  return () => clearTimeout(timer)
}

interface Generation {
  session: object
  abort: AbortController
  opened: Promise<Rc2Connection | null>
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
    this.#schedule = options.schedule ?? defaultSchedule
  }

  #current(generation: Generation) {
    return this.#generation === generation && !generation.abort.signal.aborted && this.#session() === generation.session
  }

  #settled(promise: Promise<void>, ms: number): Promise<boolean> {
    return new Promise(resolve => {
      const cancel = this.#schedule(ms, () => resolve(false))
      promise.then(() => { cancel(); resolve(true) }, () => { cancel(); resolve(true) })
    })
  }

  async #reap(child: Rc2ChildHandle | undefined): Promise<boolean> {
    if (!child) return true
    child.end()
    if (await this.#settled(child.exited, this.#deadlines.eof)) return true
    child.terminate()
    if (await this.#settled(child.exited, this.#deadlines.sigterm)) return true
    child.kill()
    return this.#settled(child.exited, this.#deadlines.sigkill)
  }

  #invalidate(generation: Generation): Promise<boolean> {
    this.#generation = null
    generation.abort.abort()
    const draining = generation.opened.then(async connection => {
      let closed = true
      try { await connection?.close() } catch { closed = false }
      try { if (!(await this.#reap(connection?.child))) closed = false } catch { closed = false }
      if (!closed) this.#unsafeToRestart = true
      return !this.#unsafeToRestart
    })
    this.#draining = draining
    void draining.then(() => { if (this.#draining === draining) this.#draining = null })
    return draining
  }

  async start(): Promise<boolean> {
    const transport = this.#transport
    if (!transport || this.#unsafeToRestart) return false
    const session = this.#session()
    const draining = this.#generation && this.#generation.session !== session ? this.stop() : this.#draining
    if (draining) {
      const stopped = this.#stops
      if (!(await draining) || stopped !== this.#stops) return false
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
    const opened = await new Promise<{ connection: Rc2Connection | null; timedOut: boolean }>(resolve => {
      const cancel = this.#schedule(this.#deadlines.open, () => { generation.abort.abort(); resolve({ connection: null, timedOut: true }) })
      generation.opened.then(connection => { cancel(); resolve({ connection, timedOut: false }) })
    })
    if (opened.timedOut) {
      if (this.#generation === generation) this.#invalidate(generation)
      else if (this.#draining) await this.#draining
      return false
    }
    const connection = opened.connection
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
      void connection.child.exited.then(() => {
        if (this.#generation === generation && !generation.abort.signal.aborted) this.#invalidate(generation)
      })
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
    const connection = await generation.opened
    if (!connection || !this.#current(generation)) return null
    try {
      const { computerId, hostId } = await connection.hello()
      return this.#current(generation) ? { computerId, hostId } : null
    } catch { return null }
  }

  async *events(): AsyncGenerator<string> {
    const generation = this.#generation
    if (!generation || !this.#current(generation)) return
    const connection = await generation.opened
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
