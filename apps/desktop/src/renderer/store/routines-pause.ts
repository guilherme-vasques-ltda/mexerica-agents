import { Observable } from '../lib/observable.ts'

export interface PauseClient {
  call<T>(method: string, body: Record<string, unknown>): Promise<T>
}

export interface PauseSource {
  readonly hello: { hostId: string } | null
  readonly connectedClient: PauseClient | null
  readonly principalEpoch: number
  ready(): Promise<PauseClient>
  subscribe(listener: () => void): () => void
}

type Origin = { hostId: string; client: PauseClient; epoch: number }

/** A confirmação e as intenções pertencem ao mesmo Hub, cliente e geração do stream. */
export class RoutinesPauseModel extends Observable {
  private value: boolean | null = null
  private working = false
  private waiting = true
  private failure: string | null = null
  private active = false
  private generation = 0
  private origin: Origin | null = null
  private timer: ReturnType<typeof setTimeout> | undefined
  private unsubscribe: (() => void) | null = null
  private readonly source: PauseSource

  constructor(source: PauseSource) { super(); this.source = source }

  private current(origin: Origin | null): origin is Origin {
    return !!origin && this.active && this.source.hello?.hostId === origin.hostId &&
      this.source.connectedClient === origin.client && this.source.principalEpoch === origin.epoch
  }

  get paused() { return this.current(this.origin) ? this.value : null }
  get busy() { return this.current(this.origin) && this.working }
  get loading() { return !this.current(this.origin) || this.waiting }
  get error() { return this.current(this.origin) ? this.failure : null }

  start() {
    if (this.active) return
    this.active = true
    this.unsubscribe = this.source.subscribe(this.sync)
    this.sync()
  }

  stop() {
    this.active = false
    this.unsubscribe?.()
    this.unsubscribe = null
    this.reset(null)
  }

  private reset(origin: Origin | null) {
    this.generation++
    clearTimeout(this.timer)
    this.origin = origin
    this.value = null
    this.working = false
    this.waiting = true
    this.failure = null
    this.changed()
  }

  private sync = () => {
    if (!this.active || this.current(this.origin)) return
    const hostId = this.source.hello?.hostId
    const client = this.source.connectedClient
    const origin = hostId && client ? { hostId, client, epoch: this.source.principalEpoch } : null
    this.reset(origin)
    if (origin) void this.refresh()
  }

  /** Não inicia leitura durante mutação; resultados mais antigos nunca a sobrescrevem. */
  async refresh() {
    const origin = this.origin
    if (!this.current(origin) || this.working) return
    const generation = this.generation
    clearTimeout(this.timer)
    try {
      const client = await this.source.ready()
      if (!this.current(origin) || client !== origin.client || generation !== this.generation || this.working) return
      const result = await client.call<{ paused: boolean }>('routinesPause', { hubId: origin.hostId })
      if (this.current(origin) && generation === this.generation && !this.working) {
        this.value = result.paused
        this.failure = null
        this.waiting = false
        this.changed()
      }
    } catch (error) {
      if (this.current(origin) && generation === this.generation && !this.working) {
        this.value = null
        this.failure = error instanceof Error ? error.message : String(error)
        this.waiting = false
        this.changed()
      }
    } finally {
      if (this.current(origin) && generation === this.generation && !this.working) this.schedule()
    }
  }

  private schedule() {
    clearTimeout(this.timer)
    this.timer = setTimeout(() => void this.refresh(), 5000)
  }

  async setPaused(paused: boolean) {
    const origin = this.origin
    if (!this.current(origin) || this.working || this.value === null) return
    clearTimeout(this.timer)
    this.working = true
    const generation = ++this.generation
    this.changed()
    try {
      const client = await this.source.ready()
      if (!this.current(origin) || client !== origin.client || generation !== this.generation) return
      const result = await client.call<{ paused: boolean }>('setRoutinesPaused', { hubId: origin.hostId, paused })
      if (this.current(origin) && generation === this.generation) {
        this.value = result.paused
        this.failure = null
      }
    } catch (error) {
      if (this.current(origin) && generation === this.generation) {
        this.value = null
        this.failure = error instanceof Error ? error.message : String(error)
      }
    } finally {
      if (this.current(origin) && generation === this.generation) {
        this.generation++
        this.working = false
        this.changed()
        this.schedule()
      }
    }
  }
}
