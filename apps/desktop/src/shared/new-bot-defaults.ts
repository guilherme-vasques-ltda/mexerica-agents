import type { Backend, BotDraft } from './models.ts'

export const NEW_BOT_BACKEND = 'codex'

/** D42: the Hub's catalog decides availability; an unknown login doesn't trigger a switch. */
export function defaultNewBotBackend(backends: Backend[]): string {
  const ready = backends.filter((b) => b.available && b.signedIn !== false)
  return ready.find((b) => b.id === 'codex')?.id
    ?? ready.find((b) => b.id === 'claude')?.id
    ?? ready.find((b) => b.curated === true)?.id
    ?? NEW_BOT_BACKEND
}

/** Only new drafts use this fallback; explicit ready engines and account selections survive. */
export function fillNewBotDefaults(draft: BotDraft, backends: Backend[]): BotDraft {
  const selected = backends.find((b) => b.id === draft.backend)
  const backend = draft.backend === 'custom' || (selected?.available && selected.signedIn !== false)
    ? draft.backend
    : defaultNewBotBackend(backends)
  return { ...draft, backend, connectors: draft.connectors ?? [], logins: draft.logins ?? [] }
}
