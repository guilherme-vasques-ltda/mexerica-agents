import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as path from 'node:path'
import * as os from 'node:os'
import * as crypto from 'node:crypto'
import * as wire from './personal-wire.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
import type { Principal } from './principal.ts'
import type { DeviceBinding } from '../fork-account/service.ts'
test('PL1 C3b.1: journal real FS cifrado/CAS congela originais e binding; sem backend seguro recusa',async()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'personal-journal-')), key=crypto.randomBytes(32)
  let secure=true,current=true,backend='gnome_libsecret'
  const safeStorage={isEncryptionAvailable:()=>secure,getSelectedStorageBackend:()=>backend,encryptString(value:string){const nonce=crypto.randomBytes(12),cipher=crypto.createCipheriv('aes-256-gcm',key,nonce);const data=Buffer.concat([cipher.update(value),cipher.final()]);return Buffer.concat([nonce,cipher.getAuthTag(),data])},decryptString(data:Buffer){const cipher=crypto.createDecipheriv('aes-256-gcm',key,data.subarray(0,12));cipher.setAuthTag(data.subarray(12,28));return Buffer.concat([cipher.update(data.subarray(28)),cipher.final()]).toString()}}
  const boundary={'node:fs':fs,'node:path':path,'node:crypto':crypto,electron:{app:{getPath:()=>dir},safeStorage},'./crypto':{},'./device-keys':{}}
  const pins=loadMainModule<typeof import('./pins.ts')>('fork-cloud/pins',boundary,{Buffer,process})
  const linuxProcess={...process,platform:'linux'}
  const {PersonalJournalStore}=loadMainModule<typeof import('./personal-journal.ts')>('fork-cloud/personal-journal',{...boundary,'./pins':pins,'./personal-wire':wire},{Buffer,process:linuxProcess})
  const b:DeviceBinding={version:1,sessionId:'synthetic-session',userId:'synthetic-user',origin:'https://example.invalid',issuer:'https://clerk.invalid',deploymentId:'fixture',manifestHash:'a'.repeat(64),deviceId:'dev_fixture',deviceKey:Buffer.alloc(32,7).toString('base64url')}
  const p={namespace:'fixture',current:()=>current,seal:{userId:b.userId},deployment:{}} as Principal
  try {
    const store=new PersonalJournalStore(p,b), first=await store.create('command','local-linux')
    const next=await store.advance(first,'unknown',{intent:'synthetic-original'},{operationId:'synthetic-operation',route:'command',body:'synthetic-body'})
    await assert.rejects(store.advance(first,'running'))
    await assert.rejects(store.advance(next,'running',{intent:'changed'}))
    assert.deepEqual(JSON.parse(JSON.stringify(await store.load())),JSON.parse(JSON.stringify(next)))
    const bytes=fs.readFileSync(path.join(dir,fs.readdirSync(dir).find(n=>n.endsWith('.bin'))!));assert(!bytes.includes(Buffer.from('synthetic-original')))
    const switched=new PersonalJournalStore(p,{...b,sessionId:'other'});await assert.rejects(switched.load())
    const secret={...next.secret,commandToken:'discardable-synthetic-token',candidateBody:'discardable-original-body',candidateRequest:'discardable-original-request'}
    const held=await store.advance(next,'unknown',next.originals,next.sending,{secret})
    await assert.rejects(store.advance(held,'unknown',held.originals,held.sending,{secret:{...secret,commandToken:'divergent'}}))
    await assert.rejects(store.advance(held,'unknown',held.originals,held.sending,{secret:{...secret,commandToken:null}}))
    const cleared=await store.advance(held,'unknown',held.originals,held.sending,{secret:{...secret,commandToken:null},clearProof:'b'.repeat(64)})
    await assert.rejects(store.advance(cleared,'unknown',cleared.originals,cleared.sending,{secret}))
    assert.equal(cleared.originals.intent,next.originals.intent);assert.equal(cleared.sending?.operationId,next.sending?.operationId)
    current=false;await assert.rejects(store.advance(cleared,'running'));current=true;backend='basic_text';await assert.rejects(store.load());backend='unknown';await assert.rejects(store.load());backend='gnome_libsecret';secure=false;await assert.rejects(store.load())
  } finally {key.fill(0);fs.rmSync(dir,{recursive:true,force:true})}
})
