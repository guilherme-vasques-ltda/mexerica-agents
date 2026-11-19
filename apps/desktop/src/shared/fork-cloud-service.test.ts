import assert from 'node:assert/strict'
import test from 'node:test'
import { randomBytes } from 'node:crypto'
import { loadMainModule, deferred } from './test-support/main-module.ts'

// Fakes só exercitam o fechamento do serviço; não produzem autoridade real.
test('forma estrita, exclusão de escrita concorrente e descarte após troca de época', async () => {
  let epoch = 1, listener = () => {}, sent = 0
  const gate = deferred<{ kind: 'registered' }>()
  const seal = { epoch: 1, sessionId: 'session', userId: 'user_fixture' }
  const account = { get sessionEpoch() { return epoch }, on: (_: string, fn: () => void) => { listener = fn }, off: () => {}, verifiedDeviceBinding: () => null }
  const principal = { seal, deployment: {}, namespace: 'fixture', current: () => epoch === 1, token: async () => 'fixture' }
  const { ForkCloudService } = loadMainModule<typeof import('../main/fork-cloud/service.ts')>('fork-cloud/service', {
    // Fronteira de efeito do Main: captura nativa do runtime pessoal (import só de efeito em service.ts).
    './personal-runtime': {},
    'node:crypto': { randomBytes }, electron: { app: {}, ipcMain: {} }, '../fork-account/service': {},
    './principal': { capturePrincipal: () => principal }, './account-api': {}, './device-keys': {},
    './crypto': { unb64url: () => null }, './enroll': {}, './connection': {}, './pins': {},
  }, { Buffer, Object, process, TextEncoder, URL })
  const service = new ForkCloudService(account as never, { registerDevice: async () => { sent++; return gate.promise }, reconcileRegistration: async () => ({ kind: 'unavailable' }) }, { captureLocal: () => null })
  assert.equal((await service.invoke({ kind: 'registerDevice', name: 'x', extra: true })).state.kind, 'notSent')
  assert.equal(sent, 0)
  const pending = service.invoke({ kind: 'registerDevice', name: 'x' })
  assert.equal(sent, 1)
  assert.equal((await service.invoke({ kind: 'registerDevice', name: 'y' })).state.kind, 'notSent')
  epoch = 2; listener(); gate.resolve({ kind: 'registered' })
  assert.equal((await pending).state.kind, 'signedOut')
  assert.equal((await service.invoke({ kind: 'registerDevice', name: 'x' })).state.kind, 'notSent')
  assert.equal(sent, 1)
})

test('registro incerto antigo não bloqueia outra conta e só consulta libera o mesmo contexto', async () => {
  let epoch = 1, namespace = 'A', listener = () => {}, registrations = 0, reconciliations = 0
  const first = deferred<{ kind: 'unknown' }>()
  const account = { get sessionEpoch() { return epoch }, on: (_: string, cb: () => void) => { listener = cb }, off() {}, verifiedDeviceBinding: () => null }
  const principal = () => {
    const capturedEpoch = epoch, capturedNamespace = namespace
    return { seal: { epoch, sessionId: `session-${epoch}`, userId: namespace }, namespace, deployment: {}, current: () => epoch === capturedEpoch && namespace === capturedNamespace, token: async () => 'fixture' }
  }
  const { ForkCloudService } = loadMainModule<typeof import('../main/fork-cloud/service.ts')>('fork-cloud/service', {
    // Fronteira de efeito do Main: captura nativa do runtime pessoal (import só de efeito em service.ts).
    './personal-runtime': {},
    'node:crypto': { randomBytes }, electron: { app: {}, ipcMain: {} }, './principal': { capturePrincipal: principal },
    './account-api': {}, './device-keys': {}, './crypto': { unb64url: () => null }, './enroll': {}, './connection': {}, './pins': {},
  }, { Buffer, Object, process, TextEncoder, URL })
  const service = new ForkCloudService(account as never, {
    registerDevice: async () => { registrations++; return registrations === 1 ? first.promise : { kind: 'registered' } },
    reconcileRegistration: async () => { reconciliations++; return { kind: 'registered' } },
  }, { captureLocal: () => null })
  const pending = service.invoke({ kind: 'registerDevice', name: 'A' })
  namespace = 'B'; epoch++; listener(); first.resolve({ kind: 'unknown' })
  assert.equal((await pending).state.kind, 'signedOut')
  assert.equal((await service.invoke({ kind: 'registerDevice', name: 'B' })).state.kind, 'needsLink')
  namespace = 'A'; epoch++; listener()
  const retained = await service.invoke({ kind: 'state' })
  assert.equal(retained.state.kind, 'unknown'); assert.equal(retained.recovery, 'registration')
  assert.equal((await service.invoke({ kind: 'registerDevice', name: 'A' })).state.kind, 'notSent')
  assert.equal(registrations, 2)
  assert.equal((await service.invoke({ kind: 'reconcileRegistration' })).state.kind, 'needsLink')
  assert.equal(reconciliations, 1); assert.equal(registrations, 2)
})

test('managed: oferta vencida retoma complete e consulta unknown, sem cadastro ou cerimônia nova', async () => {
  // Real main + real policy; only account/network/OS/Connection edges are replaced.
  const { managedAction } = await import('../main/fork-cloud/managed-connection.ts')
  for (const scenario of [
    {phase:'complete',available:true,registered:true,connects:1,checks:0},
    {phase:'proofUnknown',available:true,registered:true,connects:0,checks:1},
    {phase:'available',available:true,registered:true,connects:0,checks:0},
    {phase:'prepared',available:true,registered:true,connects:0,checks:0},
    {phase:'available',available:true,registered:false,connects:0,checks:0},
    {phase:'complete',available:false,registered:true,connects:0,checks:0},
  ]) {
    let connects=0,starts=0,checks=0,registrations=0
    const computerId=Buffer.alloc(16,1).toString('base64url'),key=Buffer.alloc(32,2)
    const binding={deviceId:'dev_fixture',deviceKey:key.toString('base64url')}
    const principal={seal:{epoch:1,userId:'owner'},namespace:'fixture',current:()=>true,bootstrapAvailable:true,deployment:{origin:'https://fixture.invalid'}}
    const account={sessionEpoch:1,on(){},off(){},verifiedDeviceBinding:()=>scenario.registered?binding:null}
    const {ForkCloudService}=loadMainModule<any>('fork-cloud/service',{
    // Fronteira de efeito do Main: captura nativa do runtime pessoal (import só de efeito em service.ts).
    './personal-runtime': {},
      'node:crypto':{},electron:{},'./principal':{capturePrincipal:()=>principal},
      './account-api':{AccountApi:class {async execute(i:{kind:string}) {return {kind:'ok',value:i.kind==='managedOffer'?{available:scenario.available,computerId:scenario.available?computerId:null,canStart:false}:{}}}}},
      './device-keys':{deviceSigner:async()=>({publicKey:key})},
      './crypto':{unb64url:()=>Buffer.alloc(16),b64url:(b:Buffer)=>b.toString('base64url')},
      './connection':{Connection:class {constructor(_account:unknown,_p:unknown,id:string){assert.equal(id,computerId);connects++} getAttachment(){return null} async start(){starts++;return true} stop(){}}},
      './enroll':{},'./pins':{},'./managed-connection':{managedAction},
    },{Buffer,Object,process})
    const service=new ForkCloudService(account,{registerDevice:async()=>{registrations++;return {kind:'registered'}}},{})
    service.bootstrap={current:()=>true,stop(){},snapshot:()=>({bootstrapId:'b',phase:scenario.phase,computerId,blocksOtherOperations:scenario.phase==='proofUnknown',canCheck:true,canProve:false,canReconcile:false}),
      async invoke(kind:string,id:string){assert.equal(kind,'checkBootstrap');assert.equal(id,'b');checks++}}
    await service.invoke({kind:'state'})
    assert.equal(connects,scenario.connects);assert.equal(starts,scenario.connects);assert.equal(checks,scenario.checks);assert.equal(registrations,0)
    service.dispose()
  }
})
