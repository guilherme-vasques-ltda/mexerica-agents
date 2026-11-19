import assert from 'node:assert/strict'
import test from 'node:test'
import { randomBytes } from 'node:crypto'
import { deferred, loadMainModule, settle } from './test-support/main-module.ts'

test('hello interno qualifica ID; reconexão revoga ID, subs e resposta antiga', async () => {
  const firstHello = deferred<{ kind: 'ok'; value: unknown }>()
  const pendingCall = deferred<{ kind: 'ok'; value: unknown }>()
  let oldEvent: (value: unknown) => void = () => {}, events = 0, cancelled = 0
  const channel = { isOpen: true, close() { this.isOpen = false }, call: (method: string) => method === 'hello' ? firstHello.promise : pendingCall.promise,
    subscribe: (_kind: string, _body: unknown, onEvent: (value: unknown) => void) => { oldEvent = onEvent; return { id: 1, cancel: () => { cancelled++ } } } }
  let callbacks: { onChannel: (c: unknown) => void; onState: (s: unknown) => void } | null = null
  const signer = { publicKey: Buffer.alloc(32), sign: () => Buffer.alloc(64) }
  const { Connection } = loadMainModule<typeof import('../main/fork-cloud/connection.ts')>('fork-cloud/connection', {
    'node:crypto': { randomBytes }, './account-api': { AccountApi: class { async execute() { return { kind: 'ok' } } } },
    './device-keys': { deviceSigner: async () => signer }, './crypto': { unb64url: () => Buffer.alloc(32) },
    './pins': { readPin: async () => ({ computerId: 'cid', signKey: 'pinned' }) },
    './relay-socket': { RelayLink: class { constructor(o: typeof callbacks) { callbacks = o } start() {} stop() {} } },
  }, { Buffer })
  let current = true
  const account = { on() {}, off() {}, isCloudSessionCurrent: () => current, verifiedDeviceBinding: () => ({ deviceKey: Buffer.alloc(32).toString('base64url') }) }
  const principal = { seal: { epoch: 1 }, namespace: 'scope', current: () => current, deployment: { origin: 'https://fixture.invalid' } }
  const changes: string[] = []
  const connection = new Connection(account as never, principal as never, 'cid', state => changes.push(state.kind))
  assert.equal(await connection.start(), true)
  const relay = callbacks as unknown as { onChannel: (c: unknown) => void; onState: (s: unknown) => void }
  relay.onState({ kind: 'ready' }); relay.onChannel(channel)
  assert.equal(connection.getAttachment(), null); assert.equal(changes.includes('ready'), false)
  firstHello.resolve({ kind: 'ok', value: { computerId: 'cid', signKey: 'pinned', hostId: 'host', name: 'Hub' } })
  await settle()
  const first = connection.getAttachment(); assert.ok(first); assert.equal(first.contextId, 'scope')
  connection.subscribe({ since: 0, client: 'test' }, () => { events++ }, () => {})
  const pending = connection.call('send', {}, 1000)
  relay.onChannel(null); oldEvent({ type: 'bot' }); assert.equal(events, 0); assert.equal(cancelled, 1)
  pendingCall.resolve({ kind: 'ok', value: {} })
  assert.equal((await pending).kind, 'unknown')
  const second = { ...channel, isOpen: true, call: async () => ({ kind: 'ok', value: { computerId: 'cid', signKey: 'pinned', hostId: 'host', name: 'Hub' } }) }
  relay.onChannel(second); await settle()
  assert.notEqual(connection.getAttachment()?.connectionId, first.connectionId)
  current = false; assert.equal(connection.getAttachment(), null)
})

test('IPC recusa subframe e cancelamento de outro emissor', () => {
  const handles = new Map<string, (...args: any[]) => any>(), listeners = new Map<string, (...args: any[]) => void>()
  const ipcMain = { handle: (name: string, cb: (...args: any[]) => any) => handles.set(name, cb), on: (name: string, cb: (...args: any[]) => void) => listeners.set(name, cb) }
  const url = 'file:///fixture/index.html?window=chat'
  const frame = { url }, otherFrame = { url }
  const events: unknown[] = []
  const sender = { id: 1, mainFrame: frame, isDestroyed: () => false, send: (...args: unknown[]) => events.push(args), on() {}, once() {}, removeListener() {} }
  const other = { ...sender, id: 2, mainFrame: otherFrame }
  const account = { sessionEpoch: 1, on() {}, off() {} }
  const { registerForkCloudService } = loadMainModule<typeof import('../main/fork-cloud/service.ts')>('fork-cloud/service', {
    // Fronteira de efeito do Main: captura nativa do runtime pessoal (import só de efeito em service.ts).
    './personal-runtime': {},
    'node:crypto': { randomBytes }, electron: { app: { once() {}, on() {} }, ipcMain, BrowserWindow: { getAllWindows: () => [] }, powerMonitor: { on() {} } },
    './principal': {}, './account-api': {}, './device-keys': {}, './crypto': { unb64url: () => Buffer.alloc(16) }, './enroll': {}, './connection': {}, './pins': {},
  }, { Buffer, Object, URL, process })
  const service = registerForkCloudService(account as never, {} as never, {} as never,
    () => [{ isDestroyed: () => false, webContents: sender }, { isDestroyed: () => false, webContents: other }] as never, [url], () => false)
  let cancelled = 0
  const fake = { getAttachment: () => ({ connectionId: 'A'.repeat(22) }), subscribe: () => ({ id: 1, cancel: () => { cancelled++ } }) }
  ;(service as unknown as { connection: unknown }).connection = fake
  const open = listeners.get('forkCloud:events:open')!, cancel = listeners.get('forkCloud:events:cancel')!
  const id = 'b'.repeat(32)
  open({ sender, senderFrame: otherFrame }, id, 'A'.repeat(22), { since: 0, client: 'fixture' })
  assert.equal(cancelled, 0)
  open({ sender, senderFrame: frame }, id, 'A'.repeat(22), { since: 0, client: 'fixture' })
  cancel({ sender: other, senderFrame: otherFrame }, id)
  assert.equal(cancelled, 0)
  cancel({ sender, senderFrame: frame }, id)
  assert.equal(cancelled, 1)
  assert.equal(events.length, 0)
  assert.equal(handles.has('forkCloud:call'), true)
})
