import assert from 'node:assert/strict'
import { test } from 'node:test'
import { defaultNewBotBackend, fillNewBotDefaults, NEW_BOT_BACKEND } from './new-bot-defaults.ts'
import type { Backend, BotDraft } from './models.ts'

const backend = (id: string, extra: Partial<Backend> = {}): Backend => ({ id, name: id, available: true, curated: true, signedIn: true, command: '', installHint: '', ...extra })
const draft: BotDraft = { name: '', description: '', avatarColor: 'red', avatarShape: 'blob', backend: NEW_BOT_BACKEND, cwd: '', permission: 'ask' }

test('new bots prefer Codex and only fall back when unavailable or signed out on the Hub', () => {
  const claude = backend('claude')
  assert.equal(defaultNewBotBackend([claude, backend('codex')]), 'codex')
  assert.equal(defaultNewBotBackend([claude, backend('codex', { signedIn: undefined })]), 'codex')
  assert.equal(defaultNewBotBackend([claude, backend('codex', { signedIn: false })]), 'claude')
  assert.equal(defaultNewBotBackend([claude, backend('codex', { available: false })]), 'claude')
  assert.equal(defaultNewBotBackend([backend('registry', { curated: false }), backend('cursor')]), 'cursor')
  assert.equal(defaultNewBotBackend([]), 'codex')
})

test('new drafts start without accounts and preserve the user selections', () => {
  const backends = [backend('claude'), backend('codex')]
  const fresh = fillNewBotDefaults(draft, backends)
  assert.equal(fresh.backend, 'codex')
  assert.deepEqual(fresh.connectors, [])
  assert.deepEqual(fresh.logins, [])
  const selected = { ...draft, backend: 'claude', connectors: ['mcp', 'ferramenta-mail', 'conta-externa-x'], logins: ['mail-account'] }
  assert.deepEqual(fillNewBotDefaults(selected, backends), selected)
  assert.equal(fillNewBotDefaults({ ...draft, backend: 'custom' }, backends).backend, 'custom')
  assert.equal(fillNewBotDefaults(draft, [backend('codex', { signedIn: false }), backend('claude')]).backend, 'claude')
})
