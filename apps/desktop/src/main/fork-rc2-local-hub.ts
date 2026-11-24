interface Rc2Hello {
  computerId: string
  hostId: string
}

export interface Rc2Connection {
  hello(): Promise<Rc2Hello>
  events(): AsyncIterable<string>
  close(): Promise<void>
}

type Rc2Transport = (signal: AbortSignal) => Promise<Rc2Connection>

export function nativeRc2Transport(): Rc2Transport | null {
  return null
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

  constructor(session: () => object | null, transport: Rc2Transport | null) {
    this.#session = session
    this.#transport = transport
  }

  #current(generation: Generation) {
    return this.#generation === generation && !generation.abort.signal.aborted && this.#session() === generation.session
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
    const connection = await generation.opened
    if (!this.#current(generation)) {
      if (this.#generation === generation) await this.stop()
      else if (this.#draining) await this.#draining
      return false
    }
    if (connection) return true
    await this.stop()
    return false
  }

  stop(): Promise<boolean> {
    this.#stops++
    if (this.#draining) return this.#draining
    const generation = this.#generation
    if (!generation) return Promise.resolve(!this.#unsafeToRestart)
    this.#generation = null
    const draining = generation.opened.then(async connection => {
      try { await connection?.close() } catch { this.#unsafeToRestart = true }
      this.#draining = null
      return !this.#unsafeToRestart
    })
    this.#draining = draining
    generation.abort.abort()
    return draining
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
