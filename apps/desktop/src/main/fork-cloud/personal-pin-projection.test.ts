import assert from 'node:assert/strict'
import test from 'node:test'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
import type { DeviceBinding } from '../fork-account/service.ts'
import * as chacha20 from '@noble/ciphers/chacha.js'
import * as nobleEd25519 from '@noble/curves/ed25519.js'
import * as nobleUtils from '@noble/curves/utils.js'
import * as nodeCrypto from 'node:crypto'

// D70 focal white-box: PersonalService REAL (observePublished/snapshot/phase/guard/current).
// Substituídos e declarados: PinStore, assinatura P30, probe, PinnedSession, storage.advance,
// publishReceipt e checked. Prova SÓ a projeção pós-R do gancho local quando um record PIN pode
// existir; não prova M2/FS/nativo/SafeStorage/Owner nem o fluxo HTTP.
const binding:DeviceBinding={version:1,sessionId:'fixture',userId:'fixture',origin:'https://example.invalid',issuer:'https://clerk.invalid',deploymentId:'fixture',manifestHash:'a'.repeat(64),deviceId:'dev_fixture',deviceKey:'fixture-key'}
function harness(store:{prepare?:()=>Promise<unknown>;commit?:()=>Promise<unknown>;sign?:()=>Promise<unknown>;loadRecovery?:()=>Promise<unknown>}){
  const state={authority:true,advances:[] as string[],recovers:0,refreshes:0}
  class Pinned {constructor(){}async recover(){state.recovers++;return{kind:'stored'}}async refresh(){state.refreshes++}close(){}}
  const {PersonalService}=loadMainModule<typeof import('./personal-service.ts')>('fork-cloud/personal-service',{
    electron:{powerMonitor:{on(){},off(){}}},'./personal-authority':{personalAuthority:()=>({context:[],issuerPublicKey:'fixture'})},'./device-keys':{},
    './crypto':{b64url:()=>binding.deviceKey},'./personal-origin':{},'./personal-runtime':{},'./personal-clock':{},'./personal-journal':{},'./personal-api':{},'./personal-channel':{},'./personal-wire':{},'./personal-prepare':{},'./personal-prepare-wire':{},'./personal-initial':{},
    './personal-pin-main':{openPersonalPinStore:async()=>({loadRecovery:store.loadRecovery??(async()=>null),prepare:store.prepare??(async()=>({})),commit:store.commit??(async()=>({kind:'unknown'})),close(){}})},
    './personal-pin':{signPersonalPin:store.sign??(async()=>({}))},'./personal-pinned':{PersonalPinnedSession:Pinned},'@noble/ciphers/chacha.js':chacha20,'@noble/curves/ed25519.js':nobleEd25519,'@noble/curves/utils.js':nobleUtils,'node:crypto':nodeCrypto,
  },{Buffer,TextEncoder,TextDecoder})
  const s=Object.create(PersonalService.prototype) as Record<string,any>
  const capture={current:()=>true,close(){},originRecord:()=>({}),check:async()=>{}}
  Object.assign(s,{alive:true,busy:false,capture,computer:'hub',pinStore:null,pinned:null,probe:null,approvalAt:null,
    p:{current:()=>state.authority},binding,signer:{publicKey:new Uint8Array()},elapsed:{current(){}},clock:{cut(){}},
    journal:{attemptId:'a',mode:'command',phase:'receipt',originals:{receipt:'"R"'},sending:null,revision:7},
    storage:{async advance(j:Record<string,unknown>,phase:string,originals=j.originals,sending=j.sending){state.advances.push(phase);return{...j,phase,originals,sending,revision:(j.revision as number)+1}}},
  })
  s.checked=async()=>{s.guard()};s.publishReceipt=async(o:unknown)=>o;s.probePublished=async()=>({current(){}})
  const published={observation:{},activationCurrent:{},state:[0,0,0,0,0,'hubActive']}
  return{s,state,published}
}
test('PL1 PIN projeção pós-R: falha após reserva possível projeta unknown/reconcile e recover stored volta a receipt',async t=>{
  let prepares=0
  const h=harness({prepare:async()=>{prepares++;return{}},commit:async()=>({kind:'unknown'})})
  const failed=await h.s.observePublished(h.published).then(()=>'ok',(e:Error)=>e.message)
  const afterFail=h.s.snapshot(),advances=[...h.state.advances]
  t.diagnostic(JSON.stringify({failed,prepares,advances,afterFail}))
  assert.equal(failed,'personalPinOutcomeUnknown');assert.equal(prepares,1)
  assert.deepEqual(advances,['unknown'],'receipt→unknown local com Authority válida')
  assert.equal(afterFail.phase,'unknown');assert.equal(afterFail.canReconcile,true);assert.notEqual(afterFail.phase,'notSent')
  // Recuperação explícita: MESMO record via loadRecovery; só stored confirmado volta a receipt.
  h.s.pinStore.loadRecovery=async()=>({history:{},record:{}})
  await h.s.observePublished(h.published,true)
  const settled=h.s.snapshot()
  t.diagnostic(JSON.stringify({advances:h.state.advances,settled,recovers:h.state.recovers}))
  assert.equal(h.state.recovers,1);assert.deepEqual(h.state.advances,['unknown','receipt']);assert.equal(settled.phase,'receipt')
})
test('PL1 PIN projeção pós-R: Authority inválida ou falha antes de prepare não persiste fase',async t=>{
  const lost=harness({commit:async()=>{lost.state.authority=false;return{kind:'unknown'}}})
  const a=await lost.s.observePublished(lost.published).then(()=>'ok',(e:Error)=>e.message)
  const early=harness({sign:async()=>{throw new Error('signUnavailable')},prepare:async()=>{throw new Error('prepare forbidden')}})
  const b=await early.s.observePublished(early.published).then(()=>'ok',(e:Error)=>e.message)
  t.diagnostic(JSON.stringify({lost:{a,advances:lost.state.advances,alive:lost.s.alive},early:{b,advances:early.state.advances,phase:early.s.journal.phase}}))
  assert.notEqual(a,'ok');assert.deepEqual(lost.state.advances,[],'sem Authority nada é persistido');assert.equal(lost.s.journal.phase,'receipt')
  assert.equal(b,'signUnavailable');assert.deepEqual(early.state.advances,[]);assert.equal(early.s.journal.phase,'receipt')
})
