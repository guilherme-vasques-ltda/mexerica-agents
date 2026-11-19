import { pref } from '../../../lib/prefs'
import { TimesRosterPreferences, TIMES_PRESENTATION_KEY, TIMES_FILTER_KEY } from '../../../lib/times-roster-preferences'
import { emptyTimesPresentation, parseTimesPresentation, type TimesPresentation } from '../../../lib/times-roster'

// Domínio completo serializado pelo readmodel; nomes não são identidade. Sem tocar nas chaves D114.
export const timesPresentations = new TimesRosterPreferences(localStorage)
window.addEventListener('storage', (event) => { if (event.key === TIMES_PRESENTATION_KEY) timesPresentations.reload() })
export const timesFilter = pref<string>(TIMES_FILTER_KEY, '', (v): v is string => typeof v === 'string')
export function presentationFor(domain: string | null, saved = timesPresentations.get()): TimesPresentation {
  return domain ? parseTimesPresentation(saved[domain]) : emptyTimesPresentation()
}
export function updatePresentation(domain: string | null, change: (p: TimesPresentation) => TimesPresentation) {
  if (!domain) return
  timesPresentations.change(domain, change)
}
