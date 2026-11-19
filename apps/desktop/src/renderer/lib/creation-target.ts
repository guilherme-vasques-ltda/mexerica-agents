import { FALLBACK_LANGUAGE, isAppLanguage } from '../../shared/fork-idioma.ts'
import { traduzir } from './fork-i18n/index.ts'
/** Only a currently usable link can receive a new bot/channel, including during drop grace. */
interface CreationStore {
  computer: { id: string }
  connectedClient: object | null
}

export interface CreationAvailability {
  error: string | null
  /** Re-read the target at confirmation time, before any request or wait for reconnection. */
  check: () => void
}

/** O texto no idioma do <html> (fork-i18n/estado o mantém; sem DOM, o padrão), como em lib/fork-donna-erros. */
function creationUnavailable(): string {
  const lang = globalThis.document?.documentElement?.lang
  return traduzir(lang && isAppLanguage(lang) ? lang : FALLBACK_LANGUAGE, 'navegacao.janela.criacaoHubIndisponivel')
}

export function canCreateOn(store: CreationStore | null | undefined): boolean {
  return store?.connectedClient != null
}

/** Contextual creation has an explicit destination; the global menu may choose a fallback. */
export function resolveCreationStore<T extends CreationStore>(stores: T[], preferred: (T | null | undefined)[], computerId?: string): T | undefined {
  const available = stores.filter(canCreateOn)
  if (computerId !== undefined) return available.find((s) => s.computer.id === computerId)
  return preferred.find((s): s is T => !!s && available.includes(s)) ?? available[0]
}

export function requireCreationStore<T extends CreationStore>(current: T | null | undefined, expected: T): void {
  if (current !== expected || !canCreateOn(current)) throw new Error(creationUnavailable())
}
