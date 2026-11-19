import { parseTimesPresentation, type TimesPresentation } from './times-roster.ts'

export const TIMES_PRESENTATION_KEY = 'donnabotTimesRosterPresentationV1'
export const TIMES_FILTER_KEY = 'donnabotTimesRosterFilterV1'

/** Armazenamento de apresentação isolado: nunca lê/escreve chaves de seções ou cache do Hub. */
export class TimesRosterPreferences {
  private readonly storage: Pick<Storage, 'getItem' | 'setItem'>
  private values: Record<string, unknown> = {}
  private listeners = new Set<() => void>()

  constructor(storage: Pick<Storage, 'getItem' | 'setItem'>) { this.storage = storage; this.reload() }

  get = () => this.values
  set = (next: Record<string, unknown>) => {
    this.values = next
    try { this.storage.setItem(TIMES_PRESENTATION_KEY, JSON.stringify(next)) } catch { /* RAM ainda funciona. */ }
    this.notify()
  }
  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => void this.listeners.delete(listener)
  }
  reload() {
    try {
      const value: unknown = JSON.parse(this.storage.getItem(TIMES_PRESENTATION_KEY) ?? '{}')
      this.values = value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
    } catch { this.values = {} }
    this.notify()
  }
  change(domain: string, change: (p: TimesPresentation) => TimesPresentation) {
    this.set({ ...this.values, [domain]: change(parseTimesPresentation(this.values[domain])) })
  }
  private notify() { for (const listener of this.listeners) listener() }
}
