import type { Entry } from '../../../../shared/models'

/** Transporte incerto nunca modifica a memória nem presume o resultado do Hub. */
export class MemoryUndoState {
  private requests = new Map<string, { busy: boolean; unknown: boolean; confirmed?: Entry }>()
  private readonly send: (entryId: string) => Promise<{ entry?: Entry }>
  private readonly changed: () => void

  constructor(send: (entryId: string) => Promise<{ entry?: Entry }>, changed: () => void) {
    this.send = send
    this.changed = changed
  }

  state(entry: Entry) {
    const request = this.requests.get(entry.id)
    const confirmed = request?.confirmed
    const latest = confirmed && confirmed.rev > entry.rev ? confirmed : entry
    const terminal = latest.data.memory?.status === 'undone' || latest.data.memory?.status === 'changed'
    return { entry: latest, busy: request?.busy ?? false, unknown: !terminal && (request?.unknown ?? false) }
  }

  async undo(entry: Entry) {
    if (this.requests.get(entry.id)?.busy) return
    const request = { busy: true, unknown: false, confirmed: this.requests.get(entry.id)?.confirmed }
    this.requests.set(entry.id, request)
    this.changed()
    try {
      const result = await this.send(entry.id)
      if (result.entry?.id === entry.id && result.entry.rev >= entry.rev && ['undone', 'changed'].includes(result.entry.data.memory?.status ?? '')) request.confirmed = result.entry
      else request.unknown = true
    } catch {
      request.unknown = true
    } finally {
      request.busy = false
      this.changed()
    }
  }
}
