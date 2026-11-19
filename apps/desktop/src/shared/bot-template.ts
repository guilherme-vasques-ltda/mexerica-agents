import type { BotDraft } from './models.ts'

/** Portable settings exclude identity, accounts and engine/model choices (D42/D91). */
export function encodeTemplate(draft: BotDraft): string {
  const {
    id: _id, pinned: _pinned, hidden: _hidden, connectors: _connectors, logins: _logins,
    backend: _backend, command: _command, model: _model, modelOptions: _modelOptions, fastMode: _fastMode,
    ...rest
  } = draft
  const sorted = Object.fromEntries(
    Object.entries(rest)
      .filter(([, v]) => v !== null && v !== undefined)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)),
  )
  return JSON.stringify(sorted, null, 2).replace(/^(\s*"(?:[^"\\]|\\.)*"): /gm, '$1 : ')
}
