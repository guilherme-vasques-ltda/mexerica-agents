import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs'
import * as path from 'node:path'
import { EventEmitter } from 'node:events'
import { tmpdir } from 'node:os'
import * as wire from './crypto.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

test('ACS4 conserva originais e unknown; só clique reconcilia o recibo e só ativação assinada libera o pin sem repetir cerimônia', async () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'bootstrap-consumer-'))
  let service: import('./bootstrap-service.ts').BootstrapService | null = null
  try {
    const root = wire.signerFromSeed(new Uint8Array(32).fill(1)), hub = wire.signerFromSeed(new Uint8Array(32).fill(2)), device = wire.signerFromSeed(new Uint8Array(32).fill(3))
    const kid = 'issuer_fixture', key = wire.b64url(device.publicKey), host = wire.b64url(hub.publicKey), computerId = wire.computerIdOf(hub.publicKey)
    const m = { schema: 2, deploymentId: 'fixture', env: 'main' as const, issuer: 'https://issuer.example', origin: 'https://cloud.example', accountId: 'a'.repeat(32), workerScript: 'fixture', d1DatabaseId: '00000000-0000-4000-8000-000000000001', relayNamespaceId: 'b'.repeat(32), relayClass: 'ComputerRelay', relayScript: 'fixture', relayEnvironment: 'main', relayWitnessName: 'donna/deployment/v4/fixture', relayWitnessId: 'c'.repeat(64), schemaVersion: 6, bootstrap: { issuerKeyId: kid, issuerPublicKey: wire.b64url(root.publicKey) } }
    const hash = (v: unknown) => crypto.createHash('sha256').update(JSON.stringify(v)).digest('hex')
    const deployment = { deploymentId: m.deploymentId, env: m.env, issuer: m.issuer, origin: m.origin, manifestHash: hash(m) }
    const p = { namespace: 'fixture', deployment, seal: { userId: 'user_1', sessionId: 'sess_1', epoch: 1 }, current: () => true, token: async () => 'fixture' }
    const binding = { version: 1 as const, userId: p.seal.userId, sessionId: p.seal.sessionId, origin: m.origin, issuer: m.issuer, deploymentId: m.deploymentId, manifestHash: deployment.manifestHash, deviceId: 'dev_1', deviceKey: key }
    const account = Object.assign(new EventEmitter(), { verifiedDeviceBinding: () => binding, isCloudSessionCurrent: () => true })
    type Envelope = readonly [readonly unknown[], string]
    const wrap = (payload: unknown[], signer: wire.DeviceSigner): Envelope => [payload, wire.b64url(signer.sign(Buffer.from(JSON.stringify(payload))))]
    let view: Record<string, unknown> = {}, originals: { grant: Envelope; challenge: Envelope; proof: Envelope; receipt: Envelope }, activation: Envelope
    let remote = 'attested', creates = 0, reconciles = 0, proofs = 0
    const setup = (id: string, nonce: string) => {
      const now = Date.now(), generation = wire.b64url(new Uint8Array(32).fill(5)), box = wire.b64url(new Uint8Array(32).fill(6))
      view = { bootstrapId: id, provisioningIntentId: 'pi_1', resourceDigest: 'd'.repeat(64), hubGeneration: generation, hubComputerId: computerId, hubSignKey: host, hubBoxKey: box, ...deployment, ownerUserId: p.seal.userId, deviceId: binding.deviceId, deviceKey: key, nonceApp: nonce, createdAt: now - 2000, expiresAt: now + 120000 }
      const grant = ['donna/bootstrap-grant/v1', id, view.provisioningIntentId, view.resourceDigest, generation, kid, m.issuer, m.deploymentId, m.env, deployment.manifestHash, p.seal.userId, binding.deviceId, key, nonce, computerId, host, box, nonce, now - 1000, now + 60000, ['control','owner.devices']]
      const challenge = ['donna/bootstrap-challenge/v1', id, generation, hash(grant), key, nonce, nonce, now, now + 30000]
      const proof = ['donna/bootstrap-proof/v1', id, generation, hash(grant), hash(challenge)]
      const grantId = `grt_${wire.b64url(new Uint8Array(16).fill(7))}`
      const pending = ['donna/bootstrap-pending/v1', grantId, id, generation, p.seal.userId, binding.deviceId, key, ['control','owner.devices'], hash(grant), hash(challenge), hash(proof), now, 1, 'pendingPublication']
      const receipt = ['donna/bootstrap-receipt/v1', id, generation, hash(grant), hash(challenge), hash(proof), p.seal.userId, binding.deviceId, key, computerId, host, grantId, ['control','owner.devices'], now, 1, hash(pending), 0, 'e'.repeat(64)]
      originals = { grant: wrap(grant, root), challenge: wrap(challenge, hub), proof: wrap(proof, device), receipt: wrap(receipt, hub) }
      activation = wrap(['donna/bootstrap-activation/v1', id, generation, grantId, p.seal.userId, binding.deviceId, key, computerId, hash(receipt), 2, 1, 'f'.repeat(64), now, now + 120000, ['control','owner.devices']], hub)
    }
    class Api {
      async execute(intent: { kind: string; bootstrapId?: string; nonceApp?: string }) {
        if (intent.kind === 'health') return { kind: 'ok', value: { ok: true, deployment } }
        if (intent.kind === 'bootstrapCreate') { creates++; setup(intent.bootstrapId!, intent.nonceApp!); return { kind: 'ok', value: { ...view, state: remote } } }
        assert.equal(intent.bootstrapId, view.bootstrapId)
        if (intent.kind === 'bootstrapReconcile') {
          reconciles++
          if (reconciles === 1) return { kind:'notSent' }
          if (reconciles === 2) return { kind:'refused',code:'unauthenticated',status:401 }
          return { kind: 'unknown' }
        }
        assert.equal(intent.kind, 'bootstrapRead')
        return { kind: 'ok', value: { ...view, state: remote, grant: originals.grant, receipt: remote === 'attested' ? null : { ...originals, hashHubReceipt: hash(originals.receipt[0]) }, activation: remote === 'hubActive' ? activation : null } }
      }
    }
    class Channel {
      static async create(_a: unknown, _p: unknown, _e: unknown, _root: string, _grant: unknown, persist: (v: unknown) => Promise<void>) { return new Channel(persist) }
      private persist: (v: unknown) => Promise<void>
      constructor(persist: (v: unknown) => Promise<void>) { this.persist = persist }
      async start() { return { kind: 'ok' } }
      async challenge() { return { kind: 'ok' } }
      async prove() { proofs++; await this.persist({ grant: originals.grant, challenge: originals.challenge, proof: originals.proof }); remote = 'hubPending'; return { kind: 'ok', value: { originals } } }
      close() {}
    }
    const deps = { 'node:crypto': crypto, 'node:fs': fs, 'node:path': path, './crypto': wire,
      electron: { app: { getPath: () => dir }, safeStorage: { isEncryptionAvailable: () => true, getSelectedStorageBackend: () => 'fixture', encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() } },
      '../environment': { buildConfig: { environment: 'main', deploymentManifest: m } }, './device-keys': { deviceSigner: async () => device }, './account-api': { AccountApi: Api }, './bootstrap-channel': { BootstrapChannel: Channel } }
    const { BootstrapService } = loadMainModule<typeof import('./bootstrap-service.ts')>('fork-cloud/bootstrap-service', deps, { Buffer, process, URL })
    const a = account as unknown as Parameters<typeof BootstrapService.restore>[0]
    service = await BootstrapService.restore(a, p)
    assert.ok(service)
    await service.invoke('startBootstrap'); const id = service.snapshot()!.bootstrapId
    await service.invoke('checkBootstrap', id); assert.equal(service.snapshot()?.canProve, true)
    await service.invoke('proveBootstrap', id); assert.equal(service.snapshot()?.phase, 'receipt')
    await service.invoke('reconcileBootstrap', id); assert.equal(service.snapshot()?.phase, 'receipt', 'notSent permits an explicit first publication')
    await service.invoke('reconcileBootstrap', id); assert.equal(service.snapshot()?.phase, 'receipt', '401 before the publication keeps the receipt')
    await service.invoke('reconcileBootstrap', id); assert.equal(service.snapshot()?.phase, 'reconcileUnknown')
    await service.invoke('checkBootstrap', id); assert.equal(reconciles, 3, 'a read never publishes')
    await service.invoke('reconcileBootstrap', id); assert.equal(reconciles, 4, 'explicit reconciliation addresses the same immutable receipt')
    service.stop(); service = await BootstrapService.restore(a, p)
    assert.equal(service?.snapshot()?.phase, 'reconcileUnknown')
    await service!.invoke('checkBootstrap', id)
    assert.equal(service?.snapshot()?.phase, 'reconcileUnknown'); assert.equal(service?.snapshot()?.canReconcile, true)
    assert.equal(reconciles, 4, 'restart and read do not repeat publication')
    remote = 'hubActive'
    await service!.invoke('checkBootstrap', id); assert.equal(service?.snapshot()?.phase, 'published')
    await service!.invoke('reconcileBootstrap', id); assert.equal(service?.snapshot()?.phase, 'complete')
    const pins = loadMainModule<typeof import('./pins.ts')>('fork-cloud/pins', deps, { Buffer, process, URL })
    const pin = await pins.readPin(p, computerId)
    assert.equal(pin?.source, 'ACS4'); assert.equal(pin?.signKey, host)
    assert.equal(creates, 1); assert.equal(proofs, 1); assert.equal(reconciles, 4)
    assert.equal(fs.readdirSync(dir).some(n => n.endsWith('.pending')), false)
  } finally { service?.stop(); fs.rmSync(dir, { recursive: true, force: true }) }
})

test('prazo libera abertura e desafio sem prova; proofUnknown continua cercado após reinício', async () => {
  for (const failure of ['openingUnknown','challengeUnknown','challenged','proofUnknown']) {
    const dir=fs.mkdtempSync(path.join(tmpdir(),'bootstrap-before-proof-'))
    let service: import('./bootstrap-service.ts').BootstrapService | null=null
    try {
      let now=Date.now(), channels=0
      const Clock=class extends Date { static now() { return now } }
      const root=wire.signerFromSeed(new Uint8Array(32).fill(21)), hub=wire.signerFromSeed(new Uint8Array(32).fill(22)), device=wire.signerFromSeed(new Uint8Array(32).fill(23))
      const m={schema:2,deploymentId:'fixture',env:'main' as const,issuer:'https://issuer.example',origin:'https://cloud.example',accountId:'a'.repeat(32),workerScript:'fixture',d1DatabaseId:'00000000-0000-4000-8000-000000000001',relayNamespaceId:'b'.repeat(32),relayClass:'ComputerRelay',relayScript:'fixture',relayEnvironment:'main',relayWitnessName:'donna/deployment/v4/fixture',relayWitnessId:'c'.repeat(64),schemaVersion:6,bootstrap:{issuerKeyId:'root_fixture',issuerPublicKey:wire.b64url(root.publicKey)}}
      const digest=(v:unknown) => crypto.createHash('sha256').update(JSON.stringify(v)).digest('hex')
      const deployment={deploymentId:m.deploymentId,env:m.env,issuer:m.issuer,origin:m.origin,manifestHash:digest(m)}
      const p={namespace:'fixture',deployment,seal:{userId:'user_1',sessionId:'sess_1',epoch:1},current:() => true,token:async () => 'fixture'}
      const binding={version:1 as const,userId:p.seal.userId,sessionId:p.seal.sessionId,origin:m.origin,issuer:m.issuer,deploymentId:m.deploymentId,manifestHash:deployment.manifestHash,deviceId:'dev_1',deviceKey:wire.b64url(device.publicKey)}
      const account=Object.assign(new EventEmitter(),{verifiedDeviceBinding:() => binding,isCloudSessionCurrent:() => true})
      type Envelope=readonly [readonly unknown[],string]
      const wrap=(payload:unknown[],signer:wire.DeviceSigner):Envelope => [payload,wire.b64url(signer.sign(Buffer.from(JSON.stringify(payload))))]
      let view:Record<string,unknown>, grant:Envelope, challenge:Envelope, proof:Envelope
      class Api {
        async execute(intent:{kind:string;bootstrapId:string;nonceApp:string}) {
          if(intent.kind==='health') return {kind:'ok',value:{ok:true,deployment}}
          if(intent.kind==='bootstrapCreate') {
            const gen=wire.b64url(new Uint8Array(32).fill(24)), box=wire.b64url(new Uint8Array(32).fill(25)), computerId=wire.computerIdOf(hub.publicKey)
            view={...deployment,bootstrapId:intent.bootstrapId,nonceApp:intent.nonceApp,provisioningIntentId:'pi_1',resourceDigest:'d'.repeat(64),hubGeneration:gen,hubComputerId:computerId,hubSignKey:wire.b64url(hub.publicKey),hubBoxKey:box,ownerUserId:p.seal.userId,deviceId:binding.deviceId,deviceKey:binding.deviceKey,createdAt:now-1000,expiresAt:now+120000,state:'attested'}
            grant=wrap(['donna/bootstrap-grant/v1',intent.bootstrapId,'pi_1',view.resourceDigest,gen,'root_fixture',m.issuer,m.deploymentId,m.env,deployment.manifestHash,p.seal.userId,binding.deviceId,binding.deviceKey,intent.nonceApp,computerId,view.hubSignKey,box,intent.nonceApp,now-500,now+60000,['control','owner.devices']],root)
            challenge=wrap(['donna/bootstrap-challenge/v1',intent.bootstrapId,gen,digest(grant[0]),binding.deviceKey,intent.nonceApp,intent.nonceApp,now,now+30000],hub)
            proof=wrap(['donna/bootstrap-proof/v1',intent.bootstrapId,gen,digest(grant[0]),digest(challenge[0])],device)
            return {kind:'ok',value:view}
          }
          assert.equal(intent.kind,'bootstrapRead');return {kind:'ok',value:{...view,grant}}
        }
      }
      class Channel {
        static async create(_a:unknown,_p:unknown,_e:unknown,_r:string,_g:unknown,persist:(v:unknown) => Promise<void>) { channels++;return new Channel(persist) }
        private persist:(v:unknown) => Promise<void>
        constructor(persist:(v:unknown) => Promise<void>) { this.persist=persist }
        async start(){return {kind:failure==='openingUnknown' ? 'unknown' : 'ok'}}
        async challenge(){return {kind:failure==='challengeUnknown' ? 'unknown' : 'ok'}}
        async prove(){if(failure==='proofUnknown') await this.persist({grant,challenge,proof});return {kind:'unknown'}}
        close(){}
      }
      const deps={'node:crypto':crypto,'node:fs':fs,'node:path':path,'./crypto':wire,electron:{app:{getPath:() => dir},safeStorage:{isEncryptionAvailable:() => true,getSelectedStorageBackend:() => 'fixture',encryptString:(s:string) => Buffer.from(s),decryptString:(b:Buffer) => b.toString()}},'../environment':{buildConfig:{environment:'main',deploymentManifest:m}},'./device-keys':{deviceSigner:async () => device},'./account-api':{AccountApi:Api},'./bootstrap-channel':{BootstrapChannel:Channel}}
      const {BootstrapService}=loadMainModule<typeof import('./bootstrap-service.ts')>('fork-cloud/bootstrap-service',deps,{Buffer,process,URL,Date:Clock})
      const a=account as unknown as Parameters<typeof BootstrapService.restore>[0]
      service=await BootstrapService.restore(a,p);assert.ok(service)
      await service.invoke('startBootstrap');const id=service.snapshot()!.bootstrapId
      await service.invoke('checkBootstrap',id);await service.invoke('proveBootstrap',id)
      assert.equal(service.snapshot()?.phase,failure);assert.equal(service.blocksOtherOperations,true)
      now+=120001
      assert.equal(service.blocksOtherOperations,failure==='proofUnknown')
      await service.invoke('proveBootstrap',id);assert.equal(channels,1,'expired or uncertain channel never reopens')
      service.stop();service=await BootstrapService.restore(a,p)
      assert.equal(service?.snapshot()?.phase,failure);assert.equal(service?.blocksOtherOperations,failure==='proofUnknown')
    } finally {service?.stop();fs.rmSync(dir,{recursive:true,force:true})}
  }
})

test('ACS4 só reenvia o mesmo pedido sem efeito; 409 fica incerto e prazo sem abertura libera outros Hubs', async () => {
  for (const uncertain of [false, true]) {
    const dir = fs.mkdtempSync(path.join(tmpdir(), 'bootstrap-no-effect-'))
    let service: import('./bootstrap-service.ts').BootstrapService | null = null
    try {
      const root = wire.signerFromSeed(new Uint8Array(32).fill(11)), hub = wire.signerFromSeed(new Uint8Array(32).fill(12)), device = wire.signerFromSeed(new Uint8Array(32).fill(13))
      const m = { schema: 2, deploymentId: 'fixture', env: 'main' as const, issuer: 'https://issuer.example', origin: 'https://cloud.example', accountId: 'a'.repeat(32), workerScript: 'fixture', d1DatabaseId: '00000000-0000-4000-8000-000000000001', relayNamespaceId: 'b'.repeat(32), relayClass: 'ComputerRelay', relayScript: 'fixture', relayEnvironment: 'main', relayWitnessName: 'donna/deployment/v4/fixture', relayWitnessId: 'c'.repeat(64), schemaVersion: 6, bootstrap: { issuerKeyId: 'issuer_fixture', issuerPublicKey: wire.b64url(root.publicKey) } }
      const deployment = { deploymentId: m.deploymentId, env: m.env, issuer: m.issuer, origin: m.origin, manifestHash: crypto.createHash('sha256').update(JSON.stringify(m)).digest('hex') }
      const p = { namespace: 'fixture', deployment, seal: { userId: 'user_1', sessionId: 'sess_1', epoch: 1 }, current: () => true, token: async () => 'fixture' }
      const binding = { version: 1 as const, userId: p.seal.userId, sessionId: p.seal.sessionId, origin: m.origin, issuer: m.issuer, deploymentId: m.deploymentId, manifestHash: deployment.manifestHash, deviceId: 'dev_1', deviceKey: wire.b64url(device.publicKey) }
      const account = Object.assign(new EventEmitter(), { verifiedDeviceBinding: () => binding, isCloudSessionCurrent: () => true })
      const creates: { bootstrapId: string; nonceApp: string }[] = []
      class Api {
        async execute(intent: { kind: string; bootstrapId: string; nonceApp: string }) {
          if (intent.kind === 'health') return { kind: 'ok', value: { ok: true, deployment } }
          assert.equal(intent.kind, 'bootstrapCreate')
          creates.push({ bootstrapId: intent.bootstrapId, nonceApp: intent.nonceApp })
          if (uncertain) return { kind: 'refused', code: 'conflict' }
          if (creates.length === 1) return { kind: 'notSent' }
          if (creates.length === 2) return { kind: 'refused', code: 'bootstrapUnavailable' }
          const now = Date.now()
          return { kind: 'ok', value: { ...deployment, ...creates[0], provisioningIntentId: 'pi_fixture', resourceDigest: 'd'.repeat(64),
            hubGeneration: wire.b64url(new Uint8Array(32).fill(14)), hubComputerId: wire.computerIdOf(hub.publicKey), hubSignKey: wire.b64url(hub.publicKey),
            hubBoxKey: wire.b64url(new Uint8Array(32).fill(15)), ownerUserId: p.seal.userId, deviceId: binding.deviceId, deviceKey: binding.deviceKey,
            state: 'attested', createdAt: now - 5000, expiresAt: now - 1000 } }
        }
      }
      const deps = { 'node:crypto': crypto, 'node:fs': fs, 'node:path': path, './crypto': wire,
        electron: { app: { getPath: () => dir }, safeStorage: { isEncryptionAvailable: () => true, getSelectedStorageBackend: () => 'fixture', encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() } },
        '../environment': { buildConfig: { environment: 'main', deploymentManifest: m } }, './device-keys': { deviceSigner: async () => device }, './account-api': { AccountApi: Api },
        './bootstrap-channel': { BootstrapChannel: { create: () => { throw new Error('No channel may open in this test.') } } } }
      const { BootstrapService } = loadMainModule<typeof import('./bootstrap-service.ts')>('fork-cloud/bootstrap-service', deps, { Buffer, process, URL })
      const a = account as unknown as Parameters<typeof BootstrapService.restore>[0]
      service = await BootstrapService.restore(a, p); assert.ok(service)
      await service.invoke('startBootstrap')
      assert.equal(service.snapshot()?.phase, uncertain ? 'createUnknown' : 'prepared')
      assert.equal(service.blocksOtherOperations, uncertain)
      service.stop(); service = await BootstrapService.restore(a, p); assert.ok(service)
      await service.invoke('startBootstrap')
      if (uncertain) { assert.equal(creates.length, 1); assert.equal(service.snapshot()?.phase, 'createUnknown'); continue }
      assert.equal(service.snapshot()?.phase, 'prepared'); assert.equal(service.blocksOtherOperations, false)
      await service.invoke('startBootstrap')
      assert.deepEqual(creates, [creates[0], creates[0], creates[0]])
      assert.equal(service.snapshot()?.phase, 'requested'); assert.equal(service.snapshot()?.blocksOtherOperations, false)
      assert.equal(service.snapshot()?.canProve, false); assert.equal(service.snapshot()?.canStart, false)
      service.stop(); service = await BootstrapService.restore(a, p)
      assert.equal(service?.blocksOtherOperations, false); assert.equal(service?.snapshot()?.bootstrapId, creates[0]!.bootstrapId)
    } finally { service?.stop(); fs.rmSync(dir, { recursive: true, force: true }) }
  }
})

test('A4 só encerra o mesmo recibo com cancelamento confirmado ou estado terminal do Worker; conserva o histórico no reinício', async () => {
  for (const terminal of ['canceled', 'expired', 'pinPendingCanceled'] as const) {
    const dir = fs.mkdtempSync(path.join(tmpdir(), 'bootstrap-terminal-'))
    let service: import('./bootstrap-service.ts').BootstrapService | null = null
    let now = Date.now(), failPinLink = terminal === 'pinPendingCanceled', pinLinkFailures = 0
    const Clock = class extends Date { static now() { return now } }
    try {
    const root = wire.signerFromSeed(new Uint8Array(32).fill(1)), hub = wire.signerFromSeed(new Uint8Array(32).fill(2)), device = wire.signerFromSeed(new Uint8Array(32).fill(3))
    const kid = 'issuer_fixture', key = wire.b64url(device.publicKey), host = wire.b64url(hub.publicKey), computerId = wire.computerIdOf(hub.publicKey)
    const m = { schema: 2, deploymentId: 'fixture', env: 'main' as const, issuer: 'https://issuer.example', origin: 'https://cloud.example', accountId: 'a'.repeat(32), workerScript: 'fixture', d1DatabaseId: '00000000-0000-4000-8000-000000000001', relayNamespaceId: 'b'.repeat(32), relayClass: 'ComputerRelay', relayScript: 'fixture', relayEnvironment: 'main', relayWitnessName: 'donna/deployment/v4/fixture', relayWitnessId: 'c'.repeat(64), schemaVersion: 6, bootstrap: { issuerKeyId: kid, issuerPublicKey: wire.b64url(root.publicKey) } }
    const hash = (v: unknown) => crypto.createHash('sha256').update(JSON.stringify(v)).digest('hex')
    const deployment = { deploymentId: m.deploymentId, env: m.env, issuer: m.issuer, origin: m.origin, manifestHash: hash(m) }
    const p = { namespace: 'fixture', deployment, seal: { userId: 'user_1', sessionId: 'sess_1', epoch: 1 }, current: () => true, token: async () => 'fixture' }
    const binding = { version: 1 as const, userId: p.seal.userId, sessionId: p.seal.sessionId, origin: m.origin, issuer: m.issuer, deploymentId: m.deploymentId, manifestHash: deployment.manifestHash, deviceId: 'dev_1', deviceKey: key }
    const account = Object.assign(new EventEmitter(), { verifiedDeviceBinding: () => binding, isCloudSessionCurrent: () => true })
    type Envelope = readonly [readonly unknown[], string]
    const wrap = (payload: unknown[], signer: wire.DeviceSigner): Envelope => [payload, wire.b64url(signer.sign(Buffer.from(JSON.stringify(payload))))]
    let view: Record<string, unknown> = {}, originals = {} as { grant: Envelope; challenge: Envelope; proof: Envelope; receipt: Envelope }, activation: Envelope
    let remote = 'attested', creates = 0, reconciles = 0, proofs = 0, reads = 0
    let revocation: unknown = null
    const setup = (id: string, nonce: string) => {
      const now = Date.now(), generation = wire.b64url(new Uint8Array(32).fill(5)), box = wire.b64url(new Uint8Array(32).fill(6))
      view = { bootstrapId: id, provisioningIntentId: 'pi_1', resourceDigest: 'd'.repeat(64), hubGeneration: generation, hubComputerId: computerId, hubSignKey: host, hubBoxKey: box, ...deployment, ownerUserId: p.seal.userId, deviceId: binding.deviceId, deviceKey: key, nonceApp: nonce, createdAt: now - 2000, expiresAt: now + 120000 }
      const grant = ['donna/bootstrap-grant/v1', id, view.provisioningIntentId, view.resourceDigest, generation, kid, m.issuer, m.deploymentId, m.env, deployment.manifestHash, p.seal.userId, binding.deviceId, key, nonce, computerId, host, box, nonce, now - 1000, now + 60000, ['control','owner.devices']]
      const challenge = ['donna/bootstrap-challenge/v1', id, generation, hash(grant), key, nonce, nonce, now, now + 30000]
      const proof = ['donna/bootstrap-proof/v1', id, generation, hash(grant), hash(challenge)]
      const grantId = `grt_${wire.b64url(new Uint8Array(16).fill(7))}`
      const pending = ['donna/bootstrap-pending/v1', grantId, id, generation, p.seal.userId, binding.deviceId, key, ['control','owner.devices'], hash(grant), hash(challenge), hash(proof), now, 1, 'pendingPublication']
      const receipt = ['donna/bootstrap-receipt/v1', id, generation, hash(grant), hash(challenge), hash(proof), p.seal.userId, binding.deviceId, key, computerId, host, grantId, ['control','owner.devices'], now, 1, hash(pending), 0, 'e'.repeat(64)]
      originals = { grant: wrap(grant, root), challenge: wrap(challenge, hub), proof: wrap(proof, device), receipt: wrap(receipt, hub) }
      activation = wrap(['donna/bootstrap-activation/v1', id, generation, grantId, p.seal.userId, binding.deviceId, key, computerId, hash(receipt), 2, 1, 'f'.repeat(64), now, now + 120000, ['control','owner.devices']], hub)
    }
    class Api {
      async execute(intent: { kind: string; bootstrapId?: string; nonceApp?: string }) {
        if (intent.kind === 'health') return { kind: 'ok', value: { ok: true, deployment } }
        if (intent.kind === 'bootstrapCreate') { creates++; setup(intent.bootstrapId!, intent.nonceApp!); return { kind: 'ok', value: { ...view, state: remote } } }
        assert.equal(intent.bootstrapId, view.bootstrapId)
        if (intent.kind === 'bootstrapReconcile') {
          reconciles++
          if (reconciles === 1) return { kind:'notSent' }
          if (reconciles === 2) return { kind:'refused',code:'unauthenticated',status:401 }
          return { kind: 'unknown' }
        }
        assert.equal(intent.kind, 'bootstrapRead'); reads++
        return { kind: 'ok', value: { ...view, state: remote, revision: 3, revocation, grant: originals.grant, receipt: remote === 'attested' ? null : { ...originals, hashHubReceipt: hash(originals.receipt[0]) }, activation: remote === 'hubActive' ? activation : null } }
      }
    }
    class Channel {
      static async create(_a: unknown, _p: unknown, _e: unknown, _root: string, _grant: unknown, persist: (v: unknown) => Promise<void>) { return new Channel(persist) }
      private persist: (v: unknown) => Promise<void>
      constructor(persist: (v: unknown) => Promise<void>) { this.persist = persist }
      async start() { return { kind: 'ok' } }
      async challenge() { return { kind: 'ok' } }
      async prove() { proofs++; await this.persist({ grant: originals.grant, challenge: originals.challenge, proof: originals.proof }); remote = 'hubPending'; return { kind: 'ok', value: { originals } } }
      close() {}
    }
    const deps = { 'node:crypto': crypto, 'node:fs': { ...fs, promises: { ...fs.promises,
      link: async (pending: fs.PathLike, final: fs.PathLike) => {
        if (failPinLink && String(pending).endsWith('.pending')) {
          failPinLink = false; pinLinkFailures++
          throw Object.assign(new Error('fixture interruption after pending sync'), { code: 'EIO' })
        }
        return fs.promises.link(pending, final)
      },
    } }, 'node:path': path, './crypto': wire,
      electron: { app: { getPath: () => dir }, safeStorage: { isEncryptionAvailable: () => true, getSelectedStorageBackend: () => 'fixture', encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() } },
      '../environment': { buildConfig: { environment: 'main', deploymentManifest: m } }, './device-keys': { deviceSigner: async () => device }, './account-api': { AccountApi: Api }, './bootstrap-channel': { BootstrapChannel: Channel } }
    const { BootstrapService } = loadMainModule<typeof import('./bootstrap-service.ts')>('fork-cloud/bootstrap-service', deps, { Buffer, process, URL, Date: Clock })
    const a = account as unknown as Parameters<typeof BootstrapService.restore>[0]
    service = await BootstrapService.restore(a, p); assert.ok(service)
    await service.invoke('startBootstrap'); const id = service.snapshot()!.bootstrapId
    await service.invoke('checkBootstrap', id); await service.invoke('proveBootstrap', id)
    assert.equal(service.snapshot()?.phase, 'receipt'); assert.equal(service.blocksOtherOperations, true)
    if (terminal === 'pinPendingCanceled') {
      remote = 'hubActive'
      await service.invoke('checkBootstrap', id)
      assert.equal(service.snapshot()?.phase, 'published')
      await assert.rejects(service.invoke('reconcileBootstrap', id), { code: 'EIO' })
      assert.equal(pinLinkFailures, 1)
      assert.equal(service.snapshot()?.phase, 'pinUnknown')
      const pending = fs.readdirSync(dir).find(n => n.endsWith('.pending'))
      assert.ok(pending)
      const pendingBytes = fs.readFileSync(path.join(dir, pending))
      remote = 'canceled'
      const r = originals.receipt[0]
      now = Math.max(now, Number(r[13]))
      const payload = ['donna/bootstrap-revocation/v1', id, view.hubGeneration, r[11], hash(r), Number(r[14]) + 1, now, 1, 'a'.repeat(64)]
      revocation = { bootstrapId: id, grantId: r[11], hashHubReceipt: hash(r), revocation: wrap(payload, hub), status: 'confirmed' }
      await service.invoke('checkBootstrap', id)
      assert.equal(service.snapshot()?.phase, 'pinUnknown', 'the same pending pin must retain its existing local recovery')
      assert.equal(service.blocksOtherOperations, true)
      assert.equal(service.snapshot()?.canReconcile, true)
      assert.deepEqual(fs.readFileSync(path.join(dir, pending)), pendingBytes)
      service.stop(); service = await BootstrapService.restore(a, p); assert.ok(service)
      assert.equal(service.snapshot()?.phase, 'pinUnknown')
      const before = reads
      await service.invoke('reconcileBootstrap', id)
      assert.equal(service.snapshot()?.phase, 'complete')
      assert.equal(reads, before, 'local recovery does not repeat cloud publication or proof')
      assert.equal(fs.readdirSync(dir).some(n => n.endsWith('.pending')), false)
      assert.deepEqual(fs.readFileSync(path.join(dir, pending.slice(0, -'.pending'.length))), pendingBytes)
      assert.equal(creates, 1); assert.equal(proofs, 1); assert.equal(reconciles, 0)
      continue
    }
    if (terminal === 'canceled') {
      remote = 'canceled'
      const r = originals.receipt[0]
      now = Math.max(now, Number(r[13]))
      const payload = ['donna/bootstrap-revocation/v1', id, view.hubGeneration, r[11], hash(r), Number(r[14]) + 1, now, 1, 'a'.repeat(64)]
      revocation = { bootstrapId: id, grantId: r[11], hashHubReceipt: hash(r), revocation: wrap(payload, hub), status: 'pending' }
      await service.invoke('checkBootstrap', id)
      assert.equal(service.blocksOtherOperations, true, 'an unconfirmed relay fence cannot release the attempt')
      const good = { ...(revocation as Record<string, unknown>), status: 'confirmed' }
      revocation = { ...good, hashHubReceipt: 'b'.repeat(64) }
      await service.invoke('checkBootstrap', id).catch(() => {})
      assert.equal(service.blocksOtherOperations, true, 'a different receipt never releases the attempt')
      revocation = good
    } else {
      now = Number(view.expiresAt) + 1
      await service.invoke('checkBootstrap', id)
      assert.equal(service.blocksOtherOperations, true, 'local expiry is insufficient after proof')
      remote = 'expired'
    }
    await service.invoke('checkBootstrap', id)
    assert.equal(service.snapshot()?.phase, 'closed')
    assert.equal(service.snapshot()?.blocksOtherOperations, false)
    assert.equal((service.snapshot() as { terminalReason?: string } | null)?.terminalReason, terminal)
    assert.equal(service.snapshot()?.canStart, false); assert.equal(service.snapshot()?.canReconcile, false)
    const before = reads
    service.stop(); service = await BootstrapService.restore(a, p); assert.ok(service)
    assert.equal(service.snapshot()?.phase, 'closed'); assert.equal(service.blocksOtherOperations, false)
    remote = 'hubActive'
    for (const action of ['startBootstrap','checkBootstrap','proveBootstrap','reconcileBootstrap'] as const) await service.invoke(action, id)
    assert.equal(reads, before, 'the closed generation cannot be reopened by late replies or stale actions')
    assert.equal(creates, 1); assert.equal(proofs, 1); assert.equal(reconciles, 0)
    const pins = loadMainModule<typeof import('./pins.ts')>('fork-cloud/pins', deps, { Buffer, process, URL })
    assert.equal(await pins.readPin(p, computerId), null)
    const journal = loadMainModule<typeof import('./bootstrap-journal.ts')>('fork-cloud/bootstrap-journal', deps, { Buffer, process, URL })
    const stored = await journal.readBootstrapJournal(p, binding)
    assert.equal(JSON.stringify(stored?.data.originals), JSON.stringify(originals))
    } finally { service?.stop(); fs.rmSync(dir, { recursive: true, force: true }) }
  }
})
