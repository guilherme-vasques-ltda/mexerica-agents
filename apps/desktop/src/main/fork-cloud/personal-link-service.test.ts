import assert from 'node:assert/strict'
import test, { before } from 'node:test'
import * as crypto from 'node:crypto'
import { EventEmitter } from 'node:events'
import { deferred, loadMainModule, settle } from '../../shared/test-support/main-module.ts'
import type { Principal } from './principal.ts'
import type { PersonalLinkOwnStatus } from '../../shared/fork-cloud.ts'

/** Serviço REAL; somente canal/conta são fronteiras simuladas. Não qualifica transporte/cofre. */
function fixture() {
  const entered = deferred(), remote = deferred()
  const state = { constructed: 0, queries: 0, stops: 0, publications: 0, imports: 0 }
  const principal = { namespace: 'c'.repeat(64), seal: { userId: 'fixture', sessionId: 'original', epoch: 1 }, current: () => true } as Principal
  let captured = principal
  class Channel {
    private active = false
    private result = deferred<PersonalLinkOwnStatus>()
    constructor() { state.constructed++ }
    snapshot() { return null }
    query() {
      state.queries++; this.active = true; entered.resolve()
      void remote.promise.then(() => {
        if (!this.active) return
        state.publications++
        this.result.resolve({ version: 1, kind: 'ownStatus', context: 'fixture', hub: 'public-H', device: 'public-T', rights: ['devices.manage', 'link.connect'], state: 'active', validForMs: 1000 })
      })
      return this.result.promise
    }
    stop() {
      state.stops++; this.active = false
      this.result.resolve({ version: 1, kind: 'unavailable', context: 'fixture', reason: 'refused' })
    }
    dispose() { this.stop() }
  }
  const channelModule = Object.defineProperty({}, 'PersonalLinkChannel', { enumerable: true, get() { state.imports++; return Channel } })
  const { ForkCloudService } = loadMainModule<typeof import('./service.ts')>('fork-cloud/service', {
    'node:crypto': crypto, electron: {}, './personal-runtime': {}, './principal': { capturePrincipal: () => captured },
    './account-api': {}, './device-keys': {}, './crypto': {}, './enroll': {}, './connection': {}, './pins': {},
    './managed-connection': {}, './managed-poll': {}, './personal-link-channel': channelModule,
  }, { Buffer, Object, process })
  const account = Object.assign(new EventEmitter(), { sessionEpoch: 1, verifiedDeviceBinding: () => null })
  const service = new ForkCloudService(account as never, {} as never, { captureLocal: () => null, personalProfile: () => null })
  return { service, state, entered, remote, principal, use(p: Principal) { captured = p } }
}

// MEX-1431: a primeira carga do service.ts real transpila a árvore a frio (0,7 s sozinha; 3,4 s com a máquina carregada
// no `npm test` completo) e estourava o `timeout: 2000` do primeiro teste, que saía cancelado. O cache de
// `loadMainModule` guarda o JS; aquecer aqui paga o custo fora do prazo de cada teste (o `before` não tem prazo).
before(() => { fixture().service.dispose() })

test('PRB-PRE-01: invoke disconnect cancela status em await sem abrir segunda query ou publicar retorno tardio', { timeout: 2000 }, async () => {
  const f = fixture()
  try {
    const pending = f.service.invoke({ kind: 'personalLinkStatus', sourceId: 'fixture' })
    await f.entered.promise
    const second = await f.service.invoke({ kind: 'personalLinkStatus', sourceId: 'fixture' })
    assert.equal(second.state.kind, 'notSent')
    assert.equal(f.state.queries, 1)
    assert.equal(f.state.constructed, 1)
    const closed = await f.service.invoke({ kind: 'personalLinkDisconnect', sourceId: 'fixture' })
    assert.equal(f.state.stops, 1, 'RED esperado em v1: busy recusava disconnect antes de stop')
    assert.equal(closed.personalLinkOwn, undefined)
    const result = await pending
    assert.equal(result.personalLinkOwn?.kind, 'unavailable')
    assert.ok(result.personalLinkOwn?.kind === 'unavailable' && result.personalLinkOwn.reason === 'refused')
    f.remote.resolve(); await settle()
    assert.equal(f.state.publications, 0)
    assert.equal((await f.service.invoke({ kind: 'state' })).personalLinkOwn, undefined)
    assert.equal(f.state.constructed, 1)
    assert.equal(f.state.queries, 1)
  } finally { f.service.dispose() }
})

test('PRB-PRE-01: disconnect exige captura/source originais e chamador atual, sem afrouxar busy', { timeout: 2000 }, async () => {
  const f = fixture()
  try {
    const pending = f.service.invoke({ kind: 'personalLinkStatus', sourceId: 'fixture' })
    await f.entered.promise
    const p = f.principal
    const changed = [
      { ...p, namespace: 'd'.repeat(64) },
      { ...p, seal: { ...p.seal, userId: 'other' } },
      { ...p, seal: { ...p.seal, sessionId: 'other' } },
      { ...p, seal: { ...p.seal, epoch: 2 } },
    ]
    for (const other of changed) {
      f.use(other)
      assert.equal((await f.service.invoke({ kind: 'personalLinkDisconnect', sourceId: 'fixture' })).state.kind, 'notSent')
      assert.equal(f.state.stops, 0)
    }
    f.use(p)
    assert.equal((await f.service.invoke({ kind: 'personalLinkDisconnect', sourceId: 'other' })).state.kind, 'notSent')
    assert.equal((await f.service.invoke({ kind: 'personalLinkDisconnect', sourceId: 'fixture' }, () => false)).state.kind, 'notSent')
    assert.equal(f.state.stops, 0)
    await f.service.invoke({ kind: 'personalLinkDisconnect', sourceId: 'fixture' })
    assert.equal((await pending).personalLinkOwn?.kind, 'unavailable')
    assert.equal(f.state.stops, 1)
    assert.equal(f.state.queries, 1)
  } finally { f.service.dispose() }
})

test('PRB-PRE-01: disconnect sem canal é no-op readonly sem import ou construção', { timeout: 2000 }, async () => {
  const f = fixture()
  try {
    const before = await f.service.invoke({ kind: 'state' })
    const after = await f.service.invoke({ kind: 'personalLinkDisconnect', sourceId: 'fixture' })
    assert.deepEqual(after, before)
    assert.equal(f.state.imports, 0)
    assert.equal(f.state.constructed, 0)
    assert.equal(f.state.queries, 0)
    assert.equal(f.state.stops, 0)
  } finally { f.service.dispose() }
})
