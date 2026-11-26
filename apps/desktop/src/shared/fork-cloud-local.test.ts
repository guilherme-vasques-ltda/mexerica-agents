import assert from 'node:assert/strict'
import test, { type TestContext } from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'
import * as crypto from 'node:crypto'
import * as path from 'node:path'
import type { LocalCloudIntent } from './fork-cloud-local.ts'
import { hubProva, TOKEN_PROVA } from './test-support/hub-prova.ts'

const { createLocalCloudCapture, LocalCloudCallError } = loadMainModule<typeof import('../main/ssh.ts')>('ssh', {
  './fork-desktop-profile': {},
  './environment': { identity: {} }, 'node:child_process': {}, 'node:crypto': crypto,
  'node:fs': { existsSync: () => false, promises: {}, readFileSync: () => '', statSync: () => ({ isFile: () => false }) },
  'node:net': {}, 'node:os': { homedir: () => '/fixture', platform: () => 'darwin' }, 'node:path': path,
  electron: {}, '../shared/ssh': {}, './host-controller': { dataDir: '/fixture' }, './ssh-command': {},
}, { Buffer, Object, process, URL })

const setup = async (t: TestContext) => {
  let alive = true
  let owns = true
  let body = ''
  let sent!: () => void
  const started = new Promise<void>((resolve) => { sent = resolve })
  let release!: () => void
  const blocked = new Promise<void>((resolve) => { release = resolve })
  let wait = false
  const hub = await hubProva((req, res) => {
    req.setEncoding('utf8').on('data', (parte) => { body += parte })
    req.on('end', async () => {
      sent()
      if (wait) await blocked
      res.end(JSON.stringify({ result: 'approved' }))
    })
    return true
  })
  t.after(() => hub.close())
  const capture = createLocalCloudCapture({ profileId: 'fixture', computerId: 'cid', signKey: 'key',
    baseURL: hub.url(), token: TOKEN_PROVA, current: () => alive, listener: async () => owns,
  })
  return { capture, release, started, count: () => hub.seen.filter((r) => r.url.startsWith('/api/')).length, seen: hub.seen, body: () => body,
    stop: () => { alive = false }, loseListener: () => { owns = false }, block: () => { wait = true } }
}
const expected = () => ({ deviceKey: 'A'.repeat(43), code: '123456', transcript: 'A'.repeat(43) })

test('rejeita formas forjadas e listener indisponível antes do envio', async (t) => {
  const x = await setup(t)
  for (const bad of [
    { kind: 'hello', extra: true }, Object.assign(Object.create({ forged: true }), { kind: 'hello' }),
    { kind: 'decideAccessRequest', requestId: 'req_1', expect: { ...expected(), code: '12345' } },
    { kind: 'decideAccessRequest', requestId: 'req_1', expect: { ...expected(), approve: true } },
  ]) await assert.rejects(x.capture.execute(bad as LocalCloudIntent), (e: unknown) => e instanceof LocalCloudCallError && e.kind === 'notSent')
  x.loseListener()
  await assert.rejects(x.capture.execute({ kind: 'hello' }), (e: unknown) => e instanceof LocalCloudCallError && e.kind === 'notSent')
  assert.deepEqual(x.seen, [])
})

test('captura body antes da espera e descarta resposta de geração invalidada', { timeout: 2000 }, async (t) => {
  const x = await setup(t)
  x.block()
  const intent: LocalCloudIntent = { kind: 'decideAccessRequest', requestId: 'req_1', expect: expected() }
  const pending = x.capture.execute(intent)
  intent.expect.code = '654321'
  // Aguarda o envio fake; a implementação capturou os campos antes do primeiro await.
  await x.started
  assert.equal(x.count(), 1)
  assert.equal(JSON.parse(x.body()).expect.code, '123456')
  x.stop(); x.release()
  await assert.rejects(pending, (e: unknown) => e instanceof LocalCloudCallError && e.kind === 'unknown')
  assert.equal(x.count(), 1)
})

test('claim é enumerado, valida nonce e não envia depois da troca durante a verificação', async (t) => {
  let current = true
  let release!: (value: boolean) => void
  const waiting = new Promise<boolean>((resolve) => { release = resolve })
  const hub = await hubProva(() => false)
  t.after(() => hub.close())
  const capture = createLocalCloudCapture({ profileId: 'fixture', computerId: 'cid', signKey: 'key',
    baseURL: hub.url(), token: TOKEN_PROVA, current: () => current, listener: () => waiting })
  await assert.rejects(capture.execute({ kind: 'claimSign', claimId: 'clm_1', nonce: 'invalid', userId: 'user_1' }),
    (e: unknown) => e instanceof LocalCloudCallError && e.kind === 'notSent')
  const pending = capture.execute({ kind: 'claimSign', claimId: 'clm_1', nonce: 'A'.repeat(22), userId: 'user_1' })
  current = false; release(true)
  await assert.rejects(pending, (e: unknown) => e instanceof LocalCloudCallError && e.kind === 'notSent')
  assert.deepEqual(hub.seen, [])
})
