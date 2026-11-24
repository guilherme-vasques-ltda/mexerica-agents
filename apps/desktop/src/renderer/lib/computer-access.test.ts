// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { registerHooks } from 'node:module'
import { test } from 'node:test'
import { CATALOGOS } from './fork-i18n/index.ts'
import type { AppModel } from '../store/app-model'

registerHooks({ resolve(specifier, context, nextResolve) {
  if (specifier.startsWith('@shared/')) return nextResolve(new URL(`../../shared/${specifier.slice(8)}.ts`, import.meta.url).href, context)
  if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
    const url = new URL(`${specifier}.ts`, context.parentURL)
    if (existsSync(url)) return nextResolve(url.href, context)
  }
  return nextResolve(specifier, context)
} })

test('permission setup uses the local host identity even when a remote computer is selected', async () => {
  Object.assign(globalThis, { window: { codync: { computerName: 'OS computer name' } } })
  const local = { hostName: 'Local host' }
  const state = { local: local as typeof local | null, selectedStore: { hostName: 'Remote host' } }
  const { ComputerAccess } = await import('../store/computer-access.ts')
  const access = new ComputerAccess(state as unknown as AppModel)
  assert.equal(access.computerName, 'Local host')
  local.hostName = 'Renamed local host'
  assert.equal(access.computerName, 'Renamed local host')
  state.local = null
  assert.equal(access.computerName, 'OS computer name')
  window.codync.computerName = ''
  assert.equal(access.computerName, 'Your computer')
})

test('repeated checks never request permissions; explicit actions and failures stay reviewable', async () => {
  const calls: string[] = []
  let fail = false
  let capture = false
  let settings = ''
  Object.assign(globalThis, { window: { codync: { platform: 'darwin', app: {
    screenAgentState: async () => ({ available: true, needsApproval: false, registered: true }),
    openSettings: (url: string) => { settings = url },
  } } } })
  // A store do Pessoal com o stream já observado (R5: só então uma resposta de tela pode dizer quem usa).
  const app = { local: { connection: { kind: 'online' }, screenEpoch: 0, screenObserved: () => true, client: { call: async (method: string) => {
    calls.push(method)
    if (method === 'requestScreenPermission' && fail) throw new Error('Permission denied')
    return { enabled: true, connected: true, capture, input: false, permissionApp: 'DonnabotDevScreen' }
  } } }, setRemoteScreen: async () => { calls.push('enable') }, screenError: null } as unknown as AppModel
  const { ComputerAccess } = await import('../store/computer-access.ts')
  const access = new ComputerAccess(app)
  await access.refresh()
  await access.refresh()
  assert.deepEqual(calls, ['screenStatus', 'screenStatus'])
  assert.equal(access.helperName, 'DonnabotDevScreen')
  assert.equal(access.step, 'capture')
  access.openSettings('input')
  assert.match(settings, /Privacy_Accessibility/)
  assert.equal(access.step, 'capture')
  fail = true
  await access.request('capture')
  assert.equal(access.busy, null)
  assert.equal(access.error, 'Permission denied')
  assert.equal(access.requested.has('capture'), true)
  fail = false
  capture = true
  await access.request('capture')
  assert.equal(access.error, null)
  assert.equal(access.step, 'input')
  await access.enable()
  assert.ok(calls.includes('enable'))
})

test('the portal notice does not change permission outcomes or survive a screen epoch', async () => {
  Object.assign(globalThis, { window: { codync: { platform: 'linux', app: {
    screenAgentState: async () => ({ available: true, needsApproval: false, registered: true }),
  } } } })
  const { ComputerAccess } = await import('../store/computer-access.ts')
  const calls: string[] = []
  let value: unknown
  let denied = false
  let epoch = 0
  const screen = { platform: 'linux', enabled: true, connected: true, capture: true, input: true }
  let read = async (): Promise<unknown> => ({ ...screen, capture: !denied, input: !denied, portalRestoreNotice: value })
  const client = { call: async (method: string) => {
    calls.push(method)
    if (method === 'screenStatus') return read()
    if (denied) throw new Error('fixture denied')
    return {}
  } }
  const local = { connection: { kind: 'online' }, client, get screenEpoch() { return epoch },
    screenObserved: (observed: number) => observed === epoch }
  const access = new ComputerAccess({ local } as unknown as AppModel)
  for (value of [undefined, 'stateDirectoryNotPrivate']) {
    await access.refresh()
    const granted = await access.request('portal')
    assert.equal(granted?.ok, true)
    assert.equal(access.step, 'ready')
    assert.equal(access.error, null)
    assert.equal(access.portalRestoreUnavailable, value !== undefined)
    assert.equal(Object.hasOwn(access.screen ?? {}, 'portalRestoreNotice'), false)
    denied = true
    const refused = await access.request('portal')
    assert.equal(refused?.ok, false)
    assert.equal(access.error, 'fixture denied')
    denied = false
  }
  assert.equal(calls.filter(method => method === 'requestScreenPermission').length, 4)
  for (value of [null, 'unknown-fixture', { kind: 'stateDirectoryNotPrivate' }]) {
    await access.refresh()
    assert.equal(access.portalRestoreUnavailable, false)
  }
  value = 'stateDirectoryNotPrivate'
  await access.refresh()
  assert.equal(access.portalRestoreUnavailable, true)
  let finishRead!: (value: unknown) => void
  let markCalled!: () => void
  const late = new Promise<unknown>(resolve => { finishRead = resolve })
  const called = new Promise<void>(resolve => { markCalled = resolve })
  read = () => { markCalled(); return late }
  const refresh = access.refresh()
  await called
  epoch++
  assert.equal(access.portalRestoreUnavailable, false)
  finishRead({ ...screen, portalRestoreNotice: value })
  await refresh
  assert.equal(access.portalRestoreUnavailable, false)
})

test('the portal notice has fixed catalogue text in every language', () => {
  const keys = ['ajustes.acesso.restauracaoPortalIndisponivelTitulo', 'ajustes.acesso.restauracaoPortalIndisponivelDetalhe'] as const
  assert.equal(Object.keys(CATALOGOS).length, 6)
  for (const catalogue of Object.values(CATALOGOS)) {
    for (const key of keys) {
      assert.ok(Object.hasOwn(catalogue, key))
      assert.ok(catalogue[key].trim())
      assert.doesNotMatch(catalogue[key], /[{}]/)
    }
  }
})
