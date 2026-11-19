import type { BotAvatarConfig } from './fork-avatar-config.ts'

export interface AvatarSaveState { value: BotAvatarConfig; dirty: boolean; saving: boolean; error: string | null }

/** Fila de intenção por bot, independente do ciclo de vida do editor React. */
export class AvatarSaveQueue {
  private state: AvatarSaveState
  private pending: BotAvatarConfig | null = null
  private timer: ReturnType<typeof setTimeout> | undefined
  private deadline = 0
  private inFlight = false
  private online: boolean
  private disposed = false
  private listeners = new Set<() => void>()
  private readonly write: (config: BotAvatarConfig) => Promise<void>

  constructor(value: BotAvatarConfig, write: (config: BotAvatarConfig) => Promise<void>, online: boolean) {
    this.state = { value: structuredClone(value), dirty: false, saving: false, error: null }
    this.write = write
    this.online = online
  }
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  getSnapshot = () => this.state
  private update(patch: Partial<AvatarSaveState>) {
    this.state = { ...this.state, ...patch }
    for (const listener of this.listeners) listener()
  }
  receive(value: BotAvatarConfig) {
    if (!this.pending && !this.inFlight && !this.disposed) this.update({ value: structuredClone(value) })
  }
  change(value: BotAvatarConfig) {
    if (this.disposed) return
    this.pending = structuredClone(value)
    this.deadline = Date.now() + 400
    this.update({ value: structuredClone(value), dirty: true, error: null })
    this.schedule()
  }
  private schedule() {
    clearTimeout(this.timer)
    if (this.pending && !this.disposed) this.timer = setTimeout(() => { void this.flush() }, Math.max(0, this.deadline - Date.now()))
  }
  setOnline(online: boolean) {
    const reconnecting = online && !this.online
    this.online = online
    if (reconnecting) { this.update({ error: null }); void this.flush() }
  }
  async retry() { this.update({ error: null }); await this.flush(true) }
  async flush(force = false): Promise<void> {
    if (this.disposed || this.inFlight || !this.online || !this.pending || this.state.error) return
    if (!force && Date.now() < this.deadline) { this.schedule(); return }
    clearTimeout(this.timer)
    const next = this.pending
    this.pending = null
    this.inFlight = true
    this.update({ saving: true })
    try {
      await this.write(structuredClone(next))
    } catch (error) {
      this.pending ??= next
      this.update({ error: `Não foi possível salvar o avatar: ${error instanceof Error ? error.message : String(error)}` })
    } finally {
      this.inFlight = false
      if (!this.disposed) {
        this.update({ saving: false, dirty: this.pending !== null })
        if (this.pending && !this.state.error) this.schedule()
      }
    }
  }
  dispose() { this.disposed = true; clearTimeout(this.timer); this.listeners.clear(); this.pending = null }
}
