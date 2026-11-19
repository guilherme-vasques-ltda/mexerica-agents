import test from 'node:test'
import assert from 'node:assert/strict'
import { browserStatusRequest, parseBrowserStatus } from './fork-browser.ts'

const origin = { hubId: 'fixture-hub', spaceId: 'fixture-space-a', botId: 'fixture-bot' }
// Resposta sintética do contrato PR1, sem navegador ou perfil real.
const fixture = () => ({
  version: 2, browserPreference: 'integrated', scope: { ...origin }, target: 'desktop', status: 'unavailable', reason: 'backend_absent',
  profiles: { status: 'unavailable' },
  capabilities: { persistentProfile: false, ephemeralProfile: false, observe: false, control: false, files: false, privateLogin: false },
  message: 'Navegador integrado indisponível.',
})

test('consulta fechada e leitura negativa de fixture preservam Space explícito', () => {
  assert.deepEqual(browserStatusRequest(origin, 'desktop'), { ...origin, version: 2, target: 'desktop' })
  assert.deepEqual(parseBrowserStatus(fixture(), origin, 'desktop'), fixture())
  assert.equal(parseBrowserStatus({ ...fixture(), target: 'host' }, origin, 'host').reason, 'backend_absent')
})

test('resposta tardia de outro Space, Hub, bot ou alvo não é adotada', () => {
  for (const key of ['hubId', 'spaceId', 'botId'] as const) {
    assert.throws(() => parseBrowserStatus(fixture(), { ...origin, [key]: 'fixture-outro' }, 'desktop'), /browser_context_changed/)
  }
  assert.throws(() => parseBrowserStatus(fixture(), origin, 'host'), /browser_unavailable/)
})

test('recurso ausente, cliente incompatível e backend ausente não viram perfil vazio', () => {
  for (const value of [null, {}, { ...fixture(), version: 1 }, { ...fixture(), profiles: [] },
    { ...fixture(), profiles: { status: 'ready', items: [] } },
    { ...fixture(), capabilities: { ...fixture().capabilities, privateLogin: true } },
    { ...fixture(), profileId: 'inventado' }]) {
    assert.throws(() => parseBrowserStatus(value, origin, 'desktop'), /browser_unavailable/)
  }
  const old = { ...fixture(), target: null, reason: 'client_unsupported' }
  assert.equal(parseBrowserStatus(old, origin, 'desktop').status, 'unavailable')
  assert.equal(parseBrowserStatus(fixture(), origin, 'desktop').profiles.status, 'unavailable')
})

test('v2 exige preferência no DTO e nunca promove DTO v1 ou incompleto', () => {
  const missing: Record<string, unknown> = { ...fixture() }
  delete missing.browserPreference
  for (const value of [missing, { ...fixture(), version: 1 }, { ...fixture(), browserPreference: null },
    { ...fixture(), browserPreference: 'chrome' }]) {
    assert.throws(() => parseBrowserStatus(value, origin, 'desktop'), /browser_unavailable/)
  }
  assert.equal(parseBrowserStatus({ ...fixture(), browserPreference: 'computer' }, origin, 'desktop').status, 'unavailable')
  assert.equal(parseBrowserStatus({ ...fixture(), browserPreference: 'off' }, origin, 'desktop').browserPreference, 'off')
  assert.equal(parseBrowserStatus({ ...fixture(), reason: 'client_unsupported' }, origin, 'desktop').reason, 'client_unsupported')
})
