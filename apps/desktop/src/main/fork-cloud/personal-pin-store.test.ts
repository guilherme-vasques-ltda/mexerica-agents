import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync,promises as fs } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import * as nativeCrypto from 'node:crypto'
import * as nativeFs from 'node:fs'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import * as nobleEd25519 from '@noble/curves/ed25519.js'
import * as nobleUtils from '@noble/curves/utils.js'
import * as crypto from './crypto.ts'
import * as wire from './personal-wire.ts'
import * as protocol from './personal-protocol.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
import type { Principal } from './principal.ts'
import type { DeviceBinding } from '../fork-account/service.ts'
const fixture=JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json',import.meta.url),'utf8')).personalBootstrap
const w=fixture.wireVectors,authority=protocol.inspectPersonalManifest(Buffer.from(fixture.manifestCanonical)),C=authority.context
const app=crypto.signerFromSeed(crypto.unb64url(w.seeds.app,32)!),namespace='c'.repeat(64)
const binding:DeviceBinding={version:1,sessionId:'fixture-session',origin:C[3],issuer:C[4],deploymentId:C[5],manifestHash:C[6],userId:w.tuple[5],deviceId:w.tuple[6],deviceKey:w.tuple[7]}
const primitives={'node:crypto':nativeCrypto,'@noble/ciphers/chacha.js':{chacha20poly1305},'@noble/curves/ed25519.js':nobleEd25519,'@noble/curves/utils.js':nobleUtils,'./crypto':crypto,'./personal-wire':wire,'./personal-protocol':protocol}
const globals={Buffer,TextEncoder,TextDecoder,Uint8Array}
const codec=loadMainModule<typeof import('./personal-pin-storage.ts')>('fork-cloud/personal-pin-storage',primitives,globals)
const history=loadMainModule<typeof import('./personal-pin-history.ts')>('fork-cloud/personal-pin-history',primitives,globals)
const session=loadMainModule<typeof import('./personal-session.ts')>('fork-cloud/personal-session',primitives,globals)
function pinFixture(){
  const T=w.tuple,o=Object.fromEntries(session.personalOriginalNames.map(n=>[n,w.packet[n]])),nonce=crypto.b64url(Buffer.alloc(32,79))
  const hello={hostId:'fixture',computerId:T[11],signKey:T[12],boxKey:T[13],name:'fixture',version:'0.8.0',minApp:'0.5.0'}
  const proof={nonce,profile:'personal-linux-v1',C,hT:wire.personalHash(T),computerId:T[11],hubGeneration:T[10],grantId:w.packet.receipt[0][6],hostStateRequest:o['host-state-request'],hostState:o['host-state'],publication:o.publish,activation:o.activation}
  const origin=['localHelper','local-linux','/run/donnabot/bootstrap-helper-v1.sock','d'.repeat(64)],P=[wire.personalDomain('local-pin'),1,'personal-linux-v1',namespace,crypto.b64url(Buffer.alloc(16,78)),'local-linux',origin,C,T,wire.personalHash(T),T[11],T[10],T[12],T[13],T[5],T[6],T[7],w.packet.receipt[0][6],wire.personalHash(w.packet.receipt[0]),o['local-ticket'],o.grant,o.receipt,o.publish,o['host-state-request'],o['host-state'],o.activation,nonce,hello,proof,o]
  const raw=Buffer.from(JSON.stringify([P,crypto.b64url(app.sign(Buffer.from(JSON.stringify(P))))]))
  return{pin:history.qualifyPersonalPinHistory(authority,binding,namespace,raw),raw,nonce,hello,proof,originals:o,T}
}
// Real authenticated crypto; this external OS boundary is NOT a SafeStorage proof.
function cipherFixture(afterEncrypt=()=>{}){
  const key=nativeCrypto.randomBytes(32)
  return{current(){},encryptString(text:string){const nonce=nativeCrypto.randomBytes(12),c=nativeCrypto.createCipheriv('aes-256-gcm',key,nonce),data=Buffer.concat([c.update(text,'utf8'),c.final()]);afterEncrypt();return Buffer.concat([nonce,c.getAuthTag(),data])},decryptString(data:Buffer){const d=nativeCrypto.createDecipheriv('aes-256-gcm',key,data.subarray(0,12));d.setAuthTag(data.subarray(12,28));return Buffer.concat([d.update(data.subarray(28)),d.final()]).toString('utf8')}}
}
test('PL1 PIN storage: frame/cifra reais mantêm P30/17 e record exatos; cópia/conta/tamper recusam',()=>{
  const f=pinFixture(),cipher=cipherFixture(),file=codec.sealPin(f.pin,cipher)
  assert.equal(file.subarray(0,8).toString(),'DNPLPIN1');assert.equal(file.readUInt32BE(8),file.length-12);assert.deepEqual(Buffer.from(codec.openPin(file,cipher)),f.raw)
  const record=codec.validatePinRecord({binding,operationId:'a'.repeat(32),revision:0,signedBytesB64:f.raw.toString('base64'),fileBytesB64:file.toString('base64'),sha256:codec.pinDigest(file),phase:'prepared'},binding)
  const sealed=codec.sealPinRecord(record,cipher),read=codec.openPinRecord(sealed,binding,cipher);assert.equal(JSON.stringify(read),JSON.stringify(record));assert.equal(Object.keys(f.pin.originals).length,17)
  const wrong=Buffer.from(file);wrong[12]^=1;assert.throws(()=>codec.openPin(wrong,cipher));assert.throws(()=>codec.openPin(Buffer.concat([file,Buffer.from([0])]),cipher));assert.throws(()=>codec.openPinRecord(sealed,{...binding,deviceId:'other'},cipher))
  assert.throws(()=>codec.validatePinRecord({...record,fileBytesB64:record.fileBytesB64+'\n'},binding));assert.throws(()=>codec.validatePinRecord({...record,extra:true},binding))
})
test('PL1 PIN prepare: EXATO probe da assinatura e pós-cifra; reserva durável sob lock antes do retorno; expiração antes da reserva não chega ao journal',async()=>{
  const f=pinFixture();let alive=true
  const {PersonalPinStore}=loadMainModule<typeof import('./personal-pin-store.ts')>('fork-cloud/personal-pin-store',primitives,globals)
  const {RetainedPinDirectory}=loadMainModule<typeof import('./personal-pin-fs.ts')>('fork-cloud/personal-pin-fs',primitives,globals)
  const p={namespace,current:()=>true} as Principal,probe={nonce:f.nonce,hello:f.hello,proof:f.proof,originals:f.originals,observation:{tuple:f.T},async check(){assert(alive)},current(){assert(alive)},close(){alive=false}} as unknown as import('./personal-probe').PersonalProbeEvidence
  const key=codec.pinKey(namespace,f.T[11]),journalName=`personal-pin-publication-${key}.journal`
  // Reserva exige transaction: FS real do scratch próprio pelo RetainedPinDirectory REAL. Seam nativa:
  // nomes por caminho (não openat) e lock sem flock; prova ordem JS, não NAPI/flock/SafeStorage.
  type Hd={fd:number;path:string}
  async function scratch(onDirSync=()=>{}){
    const root=await fs.mkdtemp(join(tmpdir(),'personal-pin-prepare-')),C=nativeFs.constants,D:Hd={fd:nativeFs.openSync(root,C.O_RDONLY|C.O_DIRECTORY),path:root}
    const name=(role:string)=>role==='lock'?`personal-pin-publication-${key}.lock`:role==='journal'?journalName:role==='pending'?`personal-pin-${key}.bin.pending`:`personal-pin-${key}.bin`
    const open=(path:string,flags:number,mode?:number):Hd=>({fd:nativeFs.openSync(path,flags|C.O_NOFOLLOW,mode),path})
    const st=(s:nativeFs.Stats)=>({dev:String(s.dev),ino:String(s.ino),uid:String(s.uid),mode:String(s.mode),nlink:String(s.nlink),size:String(s.size),regular:s.isFile(),directory:s.isDirectory(),symlink:s.isSymbolicLink()})
    let io=0,transactions=0
    const calls:Record<string,unknown>={
      inspect:(h:Hd)=>st(nativeFs.fstatSync(h.fd)),
      armOwnName:(h:Hd)=>{const a=nativeFs.fstatSync(h.fd),b=nativeFs.lstatSync(h.path);assert(a.dev===b.dev && a.ino===b.ino && !b.isSymbolicLink());return st(b)},
      openRead:(_d:Hd,_k:string,role:string)=>{const h=open(join(root,name(role)),C.O_RDONLY);if(nativeFs.fstatSync(h.fd).nlink!==1){nativeFs.closeSync(h.fd);throw Object.assign(new Error('pair'),{code:'EPROOFPERM'})}return h},
      openLock:()=>open(join(root,name('lock')),C.O_RDWR),createLock:()=>open(join(root,name('lock')),C.O_RDWR|C.O_CREAT|C.O_EXCL,0o600),tryLock(){},unlock(){},
      sync:(h:Hd)=>{nativeFs.fsyncSync(h.fd);if(h===D)onDirSync()},
      readOne:(h:Hd,at:number,length:number)=>{const b=Buffer.alloc(length);return{count:nativeFs.readSync(h.fd,b,0,length,at),bytes:b}},
      writeOne:(h:Hd,at:number,b:Buffer)=>nativeFs.writeSync(h.fd,b,0,b.length,at),
      createFirstJournal:()=>open(join(root,journalName),C.O_RDWR|C.O_CREAT|C.O_EXCL,0o600),close:(h:Hd)=>nativeFs.closeSync(h.fd),
    }
    const api=new Proxy({},{get(_t,n){const v=calls[String(n)];if(typeof v!=='function')throw new Error(`native ${String(n)} forbidden`);return(...a:unknown[])=>{io++;return(v as (...x:unknown[])=>unknown)(...a)}}})
    const directory=new RetainedPinDirectory(api as never,D as never,{current(){},close(){nativeFs.closeSync(D.fd)}})
    const counted={current:()=>directory.current(),read:directory.read.bind(directory),transaction:(k:string,g:never,a:never)=>{transactions++;return directory.transaction(k,g,a)},close:()=>directory.close()}
    const journal=()=>nativeFs.existsSync(join(root,journalName))?nativeFs.readFileSync(join(root,journalName)):null
    return{root,directory:counted as never,io:()=>io,transactions:()=>transactions,journal,files:()=>nativeFs.readdirSync(root).map(x=>x.replace(key,'K')).sort(),async done(){directory.close();await fs.rm(root,{recursive:true})}}
  }
  // Cifra PIN (P30) separada da cifra do record no MESMO cipher real de teste.
  const counting=(afterPin=()=>{})=>{const c=cipherFixture(),n={pin:0,all:0};return{n,cipher:{current:c.current,decryptString:c.decryptString,encryptString(text:string){n.all++;const out=c.encryptString(text);if(text.startsWith('[')){n.pin++;afterPin()}return out}}}}
  // (a) prepare bom: 1 transaction, 1 cifra PIN, record prepared rev0 durável com os MESMOS bytes antes do retorno.
  const A=await scratch(),ka=counting(),good=new PersonalPinStore(p,binding,authority,ka.cipher,A.directory),prepared=await good.prepare(f.pin,probe)
  assert.equal(A.transactions(),1);assert.equal(ka.n.pin,1);assert.equal(ka.n.all,2)
  const record=codec.openPinRecord(A.journal()!,binding,ka.cipher)
  assert.equal(record.phase,'prepared');assert.equal(record.revision,0);assert.equal(record.fileBytesB64,Buffer.from(prepared.fileBytes).toString('base64'));assert.equal(record.signedBytesB64,f.raw.toString('base64'))
  const a=prepared.fileBytes;a.fill(0);assert.notEqual(Buffer.from(prepared.fileBytes).subarray(0,8).toString(),'\0'.repeat(8))
  // (b) commit com outro probe: ZERO IO nativo e journal byte-idêntico.
  const ioBefore=A.io(),durable=A.journal()!
  await assert.rejects(good.commit(prepared,{...probe} as never));assert.equal(A.io(),ioBefore);assert(A.journal()!.equals(durable))
  // (c) nonce errado: ZERO transaction/cifra/IO.
  await assert.rejects(good.prepare(f.pin,{...probe,nonce:crypto.b64url(Buffer.alloc(32,1))} as never));assert.equal(A.transactions(),1);assert.equal(ka.n.all,2);assert.equal(A.io(),ioBefore)
  await A.done()
  // (d) expiração durante a cifra PIN (antes da reserva): rejeita e ZERO journal.
  const B=await scratch(),kb=counting(()=>{alive=false}),expires=new PersonalPinStore(p,binding,authority,kb.cipher,B.directory)
  await assert.rejects(expires.prepare(f.pin,probe));assert.equal(kb.n.pin,1);assert.equal(kb.n.all,1);assert.equal(B.journal(),null);assert.deepEqual(B.files().filter(x=>x.includes('.journal')),[])
  alive=true;await B.done()
  // (e) expiração APÓS a reserva durável: rejeita, record permanece; novo prepare ZERO cifra PIN (só reconcile/READ).
  let reserved=false
  const E=await scratch(()=>{const j=E.journal();if(j && j.length>0)reserved=true}),ke=counting()
  const late={...probe,async check(){if(reserved)alive=false;assert(alive)},current(){assert(alive)}} as never
  await assert.rejects(new PersonalPinStore(p,binding,authority,ke.cipher,E.directory).prepare(f.pin,late));assert.equal(reserved,true);assert.equal(ke.n.pin,1)
  const kept=E.journal()!;assert.equal(codec.openPinRecord(kept,binding,ke.cipher).phase,'prepared')
  alive=true;const ke2=counting()
  await assert.rejects(new PersonalPinStore(p,binding,authority,ke2.cipher,E.directory).prepare(f.pin,probe));assert.equal(ke2.n.all,0);assert(E.journal()!.equals(kept))
  await E.done()
  // (f) record parcial existente: rejeita sob lock, ZERO cifra, parcial byte-idêntico.
  const F=await scratch(),kf=counting(),blocked=new PersonalPinStore(p,binding,authority,kf.cipher,F.directory)
  await fs.writeFile(join(F.root,journalName),Buffer.from([0]),{mode:0o600})
  await assert.rejects(blocked.prepare(f.pin,probe));assert.equal(kf.n.all,0);assert.deepEqual([...F.journal()!],[0]);assert.deepEqual(F.files(),['personal-pin-publication-K.journal','personal-pin-publication-K.lock'])
  await F.done()
})
test('PL1 PIN native: sem provaElectron recusa antes qualquer carregamento/FS, sem pathfallback',async()=>{
  const native=loadMainModule<typeof import('./personal-pin-native.ts')>('fork-cloud/personal-pin-native',{},globals);let checks=0
  assert.equal(native.personalPinNativeAdmitted(),false);await assert.rejects(native.openPersonalPinDirectory(()=>{checks++}),/personalPinNativeQualificationPending/);assert.equal(checks,1)
})
test('PL1 PIN FS writer: guarda perdida na fila impede primeira chamada nativa',async()=>{
  const {RetainedPinDirectory}=loadMainModule<typeof import('./personal-pin-fs.ts')>('fork-cloud/personal-pin-fs',primitives,globals);let calls=0,active=true
  const api=new Proxy({},{get(){return()=>{calls++;throw new Error('native effect forbidden')}}})
  const d=new RetainedPinDirectory(api as never,{} as never,{current(){},close(){}}),g={async check(){active=false},current(){assert(active)}}
  await assert.rejects(d.transaction('a'.repeat(64),g,async()=>{throw new Error('action must not start')}));assert.equal(calls,0);d.close()
})
test('PL1 PIN FS writer: IO real de inspeção revoga guarda e ZERO open/create seguinte',async()=>{
  const root=await fs.mkdtemp(join(tmpdir(),'personal-pin-after-io-')),handle=await fs.open(root,'r')
  let active=true,inspections=0,effects=0
  const {RetainedPinDirectory}=loadMainModule<typeof import('./personal-pin-fs.ts')>('fork-cloud/personal-pin-fs',primitives,globals)
  try{
    const api=new Proxy({},{get(_t,name){if(name==='inspect')return()=>{const s=nativeFs.fstatSync(handle.fd);assert(s.isDirectory());inspections++;active=false;return{dev:String(s.dev),ino:String(s.ino),uid:String(s.uid),mode:String(s.mode),nlink:String(s.nlink),size:String(s.size),directory:true,regular:false,symlink:false}};return()=>{effects++;throw new Error('next effect forbidden')}}})
    const d=new RetainedPinDirectory(api as never,{} as never,{current(){},close(){}}),g={async check(){assert(active)},current(){assert(active)}}
    await assert.rejects(d.transaction('a'.repeat(64),g,async()=>{throw new Error('action forbidden')}));assert.equal(inspections,1);assert.equal(effects,0);d.close()
  }finally{await handle.close();await fs.rmdir(root)}
})
