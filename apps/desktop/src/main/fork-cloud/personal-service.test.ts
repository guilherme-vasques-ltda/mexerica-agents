import assert from 'node:assert/strict'
import test from 'node:test'
import { deferred,loadMainModule } from '../../shared/test-support/main-module.ts'
import type { Principal } from './principal.ts'
import type { DeviceBinding } from '../fork-account/service.ts'
import * as chacha20 from '@noble/ciphers/chacha.js'
import * as nobleEd25519 from '@noble/curves/ed25519.js'
import * as nobleUtils from '@noble/curves/utils.js'
import * as nodeCrypto from 'node:crypto'
test('PL1 C3b.1: Authority original e métodos sobrevivem await; troca do aparelho impede captura/efeito',async()=>{
  const gate=deferred(),binding:DeviceBinding={version:1,sessionId:'fixture',userId:'fixture',origin:'https://example.invalid',issuer:'https://clerk.invalid',deploymentId:'fixture',manifestHash:'a'.repeat(64),deviceId:'dev_fixture',deviceKey:'fixture-key'}
  let actual=binding,sends=0,tokens=0
  const principal={current:()=>true,token:async()=>{tokens++;return 'discardable'},seal:{userId:binding.userId,sessionId:binding.sessionId},deployment:{},namespace:'fixture'} as Principal
  const {PersonalService}=loadMainModule<typeof import('./personal-service.ts')>('fork-cloud/personal-service',{
    electron:{powerMonitor:{on(){},off(){}}},'./personal-authority':{personalAuthority:()=>({context:[],issuerPublicKey:'fixture'})},
    './device-keys':{deviceSigner:async(p:Principal)=>{assert.equal(typeof p.current,'function');assert.equal(typeof p.token,'function');await gate.promise;return{publicKey:new Uint8Array(),sign(){sends++;return new Uint8Array()}}}},
    './crypto':{b64url:()=>binding.deviceKey},'./personal-origin':{},'./personal-runtime':{},'./personal-clock':{},'./personal-journal':{},'./personal-api':{},'./personal-channel':{},'./personal-wire':{},'./personal-prepare':{},'./personal-prepare-wire':{},'./personal-initial':{},'@noble/ciphers/chacha.js':chacha20,'@noble/curves/ed25519.js':nobleEd25519,'@noble/curves/utils.js':nobleUtils,'node:crypto':nodeCrypto,
  },{Buffer,TextEncoder,TextDecoder})
  const pending=PersonalService.create({verifiedDeviceBinding:()=>actual} as never,principal,()=>null)
  actual={...binding,deviceKey:'changed'};gate.resolve();await assert.rejects(pending,/personalBindingChanged/);assert.equal(sends,0);assert.equal(tokens,0)
})
