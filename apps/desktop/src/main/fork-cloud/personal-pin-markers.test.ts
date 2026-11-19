import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as os from 'node:os'
import * as path from 'node:path'
import * as nodeCrypto from 'node:crypto'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import * as nobleEd25519 from '@noble/curves/ed25519.js'
import * as nobleUtils from '@noble/curves/utils.js'
import * as crypto from './crypto.ts'
import * as wire from './personal-wire.ts'
import * as protocol from './personal-protocol.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
import type { DeviceBinding } from '../fork-account/service.ts'
import type { Principal } from './principal.ts'

// D70 focal: PRIMEIRO marker revoke/fence usa nome FINAL O_EXCL; parcial bloqueia novo
// create/token/cifra/record após restart. RetainedPinDirectory e PersonalPinStore REAIS; FS
// Node real (open/pwrite/pread/fsync/rename, O_EXCL). Seams declaradas: nomes por caminho
// (não openat), tryLock em memória (NÃO flock), corte = short write na fronteira writeOne,
// cifra AES-GCM de teste (NÃO SafeStorage). P30/W/S assinados com seeds do vetor público.
// Prova só ordem/seleção JS de initial; não NAPI/flock/CAS/Electron/Owner.
const fixture=JSON.parse(fs.readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json',import.meta.url),'utf8')).personalBootstrap
const w=fixture.wireVectors,C=fixture.context,D=wire.personalDomain,bytes=(v:unknown)=>Buffer.from(JSON.stringify(v)),H=wire.personalHash
const signer=(role:'app'|'hub'|'issuer')=>crypto.signerFromSeed(crypto.unb64url(w.seeds[role],32)!)
const signed=(role:'app'|'hub'|'issuer',value:unknown[])=>[value,crypto.b64url(signer(role).sign(bytes(value)))]
const authority=protocol.inspectPersonalManifest(Buffer.from(fixture.manifestCanonical))
const primitives={'node:crypto':nodeCrypto,'@noble/ciphers/chacha.js':{chacha20poly1305},'@noble/curves/ed25519.js':nobleEd25519,'@noble/curves/utils.js':nobleUtils,'./crypto':crypto,'./personal-wire':wire,'./personal-protocol':protocol}
const globals={Buffer,Object,process,URL,TextEncoder,TextDecoder,Uint8Array,ArrayBuffer,DataView,performance,setTimeout,clearTimeout,queueMicrotask,Response}
const session=loadMainModule<typeof import('./personal-session.ts')>('fork-cloud/personal-session',primitives,globals)
const history=loadMainModule<typeof import('./personal-pin-history.ts')>('fork-cloud/personal-pin-history',primitives,globals)
const binding:DeviceBinding={version:1,sessionId:'fixture-session',origin:C[3],issuer:C[4],deploymentId:C[5],manifestHash:C[6],userId:w.tuple[5],deviceId:w.tuple[6],deviceKey:w.tuple[7]}
const namespace='c'.repeat(64),origin=['localHelper','local-linux','/run/donnabot/bootstrap-helper-v1.sock','d'.repeat(64)] as const
function publicPin(){
  const packet=JSON.parse(JSON.stringify(w.packet)) as Record<string,any>,T=JSON.parse(JSON.stringify(w.tuple)),R=packet.receipt
  const originals=Object.fromEntries(session.personalOriginalNames.map(name=>[name,packet[name]])),nonce=crypto.b64url(Buffer.alloc(32,62))
  const hello={hostId:'fixture-public-host',computerId:T[11],signKey:T[12],boxKey:T[13],name:'fixture',version:'0.8.0',minApp:'0.5.0'}
  const proof={nonce,profile:'personal-linux-v1',C,hT:H(T),computerId:T[11],hubGeneration:T[10],grantId:R[0][6],hostStateRequest:packet['host-state-request'],hostState:packet['host-state'],publication:packet.publish,activation:packet.activation}
  const P=[D('local-pin'),1,'personal-linux-v1',namespace,crypto.b64url(Buffer.alloc(16,61)),'local-linux',origin,C,T,H(T),T[11],T[10],T[12],T[13],T[5],T[6],T[7],R[0][6],H(R[0]),packet['local-ticket'],packet.grant,R,packet.publish,packet['host-state-request'],packet['host-state'],packet.activation,nonce,hello,proof,originals]
  const raw=bytes([P,crypto.b64url(signer('app').sign(bytes(P)))])
  return{packet,T,pin:history.qualifyPersonalPinHistory(authority,binding,namespace,raw)}
}
type H={fd:number;path:string;kind:'dir'|'file'|'lock';locked?:boolean;tmp?:string}
const stat=(s:fs.Stats)=>({dev:String(s.dev),ino:String(s.ino),uid:String(s.uid),mode:String(s.mode),nlink:String(s.nlink),size:String(s.size),regular:s.isFile(),directory:s.isDirectory(),symlink:s.isSymbolicLink()})
function scratchNative(root:string,cutFirstWrite:boolean){
  const calls:Record<string,number>={},count=(n:string)=>{calls[n]=(calls[n]??0)+1}
  const name=(key:string,role:string,op='')=>role==='lock'?`personal-pin-publication-${key}.lock`:role==='journal'?`personal-pin-publication-${key}.journal`:role==='journalTmp'?`personal-pin-publication-${key}.journal.${op}.tmp`:role==='pending'?`personal-pin-${key}.bin.pending`:`personal-pin-${key}.bin`
  const open=(p:string,flags:number,kind:H['kind'],mode?:number):H=>({fd:fs.openSync(p,flags|fs.constants.O_NOFOLLOW,mode),path:p,kind})
  const same=(h:H)=>{const a=fs.fstatSync(h.fd),b=fs.lstatSync(h.path);assert(a.dev===b.dev && a.ino===b.ino && !b.isSymbolicLink());return stat(b)}
  const C=fs.constants,forbidden=(n:string)=>()=>{count(n);throw new Error(`${n} forbidden in focal`)}
  let cut=cutFirstWrite
  const api={
    inspect(h:H){count('inspect');return stat(fs.fstatSync(h.fd))},
    armOwnName(h:H){count('armOwnName');return same(h)},
    openRead(_d:H,key:string,role:string){count(`openRead:${role}`);const h=open(path.join(root,name(key,role)),C.O_RDONLY,'file');if(fs.fstatSync(h.fd).nlink!==1){fs.closeSync(h.fd);throw Object.assign(new Error('pair'),{code:'EPROOFPERM'})}return h},
    openLock(_d:H,key:string){count('openLock');return open(path.join(root,name(key,'lock')),C.O_RDWR,'lock')},
    createLock(_d:H,key:string){count('createLock');return open(path.join(root,name(key,'lock')),C.O_RDWR|C.O_CREAT|C.O_EXCL,'lock',0o600)},
    tryLock(h:H){count('tryLock');assert(!h.locked);h.locked=true},
    unlock(h:H){count('unlock');h.locked=false},
    sync(h:H){count('sync');fs.fsyncSync(h.fd)},
    readOne(h:H,at:number,length:number){count('readOne');const b=Buffer.alloc(length);return{count:fs.readSync(h.fd,b,0,length,at),bytes:b}},
    createFirstJournal(_d:H,key:string,_op:string,lock:H){count('createFirstJournal');assert(lock.locked);return open(path.join(root,name(key,'journal')),C.O_RDWR|C.O_CREAT|C.O_EXCL,'file',0o600)},
    createJournalTmp(_d:H,key:string,op:string){count('createJournalTmp');const h=open(path.join(root,name(key,'journalTmp',op)),C.O_RDWR|C.O_CREAT|C.O_EXCL,'file',0o600);h.tmp=path.join(root,name(key,'journal'));return h},
    // Corte na fronteira: metade REAL gravada e count parcial (short IO ⇒ unknown).
    writeOne(h:H,at:number,b:Buffer){count('writeOne');if(cut){cut=false;return fs.writeSync(h.fd,b,0,Math.floor(b.length/2),at)}return fs.writeSync(h.fd,b,0,b.length,at)},
    publishJournal(h:H,lock:H){count('publishJournal');assert(lock.locked && h.tmp);fs.renameSync(h.path,h.tmp)},
    close(h:H){count('close');fs.closeSync(h.fd)},
    inspectName:forbidden('inspectName'),armLinkedFinal:forbidden('armLinkedFinal'),createPending:forbidden('createPending'),linkFinal:forbidden('linkFinal'),unlinkOwnPending:forbidden('unlinkOwnPending'),openPairCandidate:forbidden('openPairCandidate'),inspectPairCandidate:forbidden('inspectPairCandidate'),adoptPair:forbidden('adoptPair'),
  }
  return{api,calls}
}
// Cada "processo": módulos novos (fila/estado), diretório retido novo, cifra com a MESMA chave.
function proc(root:string,key:Buffer,cutFirstWrite=false){
  const n=scratchNative(root,cutFirstWrite),stats={encrypt:0}
  const {RetainedPinDirectory}=loadMainModule<typeof import('./personal-pin-fs.ts')>('fork-cloud/personal-pin-fs',primitives,globals)
  const {PersonalPinStore}=loadMainModule<typeof import('./personal-pin-store.ts')>('fork-cloud/personal-pin-store',primitives,globals)
  const dir:H={fd:fs.openSync(root,fs.constants.O_RDONLY|fs.constants.O_DIRECTORY),path:root,kind:'dir'}
  const directory=new RetainedPinDirectory(n.api as never,dir as never,{current(){},close(){fs.closeSync(dir.fd)}})
  const cipher={current(){},encryptString(text:string){stats.encrypt++;const nonce=nodeCrypto.randomBytes(12),c=nodeCrypto.createCipheriv('aes-256-gcm',key,nonce),data=Buffer.concat([c.update(text,'utf8'),c.final()]);return Buffer.concat([nonce,c.getAuthTag(),data])},decryptString(data:Buffer){const d=nodeCrypto.createDecipheriv('aes-256-gcm',key,data.subarray(0,12));d.setAuthTag(data.subarray(12,28));return Buffer.concat([d.update(data.subarray(28)),d.final()]).toString('utf8')}}
  const store=new PersonalPinStore({namespace,current:()=>true} as Principal,binding,authority,cipher as never,directory)
  return{store,calls:n.calls,stats}
}
const g={async check(){},current(){}}
const listing=(root:string)=>fs.readdirSync(root).filter(x=>x.includes('.journal')).map(x=>x.replace(/[a-f0-9]{64}/,'K').replace(/[a-f0-9]{32}/,'OP')).sort()
test('PL1 PIN marker revoke: primeiro marker cortado fica no nome final e novo processo NÃO cria token/send',async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'personal-pin-revoke-')),key=nodeCrypto.randomBytes(32),f=publicPin()
  try{
    let creates=0;const create=()=>{creates++;return{token:`t${creates}`}}
    const p1=proc(root,key,true)
    await assert.rejects(p1.store.beginRevoke(f.pin,g,create));const afterCut=listing(root),firstCreates=creates
    const p2=proc(root,key),second=await p2.store.beginRevoke(f.pin,g,create).then(v=>v,(e:Error)=>e)
    t.diagnostic(JSON.stringify({afterCut,firstCreates,secondCreates:creates-firstCreates,secondEncrypt:p2.stats.encrypt,p1:p1.calls,p2:p2.calls,second:second instanceof Error?second.message:second}))
    assert.equal(firstCreates,1);assert.equal(p1.stats.encrypt,1);assert.equal(p1.calls.writeOne,1)
    assert.equal(creates-firstCreates,0,'ZERO create/token/send após marker parcial')
    assert.equal(p2.stats.encrypt,0);assert.equal(second,null)
    assert.deepEqual(listing(root),['personal-pin-publication-K.journal'])
  }finally{fs.rmSync(root,{recursive:true})}
})
function fenceFixture(){
  const f=publicPin(),now=(f.packet.receipt[0][7] as number)+100,source={quantum:1 as const,generation:'fence-current-runtime',current(){},sample:()=>({lo:1000,hi:1001}),auxiliary:()=>1000,close(){}}
  const {PersonalClock}=loadMainModule<typeof import('./personal-clock.ts')>('fork-cloud/personal-clock',primitives,globals),clock=new PersonalClock(source),flight=clock.begin()
  clock.accept(authority,signed('issuer',[D('time'),C,crypto.b64url(crypto.random(32)),f.T[11],now,300000]),f.T[11],flight)
  const observe=(W:unknown)=>{
    const w0=clock.window(f.T[11]),q=[D('read'),C,f.T[1],f.T[9],H(f.T),crypto.b64url(crypto.random(32)),w0.lower,w0.lower+30000],read=[q,crypto.b64url(signer('app').sign(bytes(q)))] as const
    const originals=Object.fromEntries(Object.entries(session.personalRemoteNames).map(([remote,local])=>[remote,local==='fence'?W:['fence-ack','revocation'].includes(local)?null:f.packet[local]]))
    const state=signed('issuer',[D('state'),C,f.T[1],f.T[9],H(f.T),'revoking',8,f.packet.receipt[0][6],H(f.packet.grant[0]),H(f.packet.receipt[0]),null,null,H((W as unknown[][])[0]),null,q[5],now,now+30000])
    return session.verifyPersonalObservation(authority,binding,f.pin.originals,read,{state,originals,activationCurrent:null},clock.window(f.T[11]))
  }
  const wp=JSON.parse(JSON.stringify(f.packet.fence[0]));wp[4]=null;wp[5]=null
  const w2=JSON.parse(JSON.stringify(wp));w2[6]=2
  return{f,observe,W1:signed('issuer',wp),W2:signed('issuer',w2),close(){clock.cut();source.close()}}
}
const journalOf=(root:string)=>fs.readdirSync(root).find(x=>x.endsWith('.journal'))
const openFence=(root:string,key:Buffer)=>{const raw=fs.readFileSync(path.join(root,journalOf(root)!)),d=nodeCrypto.createDecipheriv('aes-256-gcm',key,raw.subarray(0,12));d.setAuthTag(raw.subarray(12,28));return JSON.parse(Buffer.concat([d.update(raw.subarray(28)),d.final()]).toString('utf8'))}
test('PL1 PIN marker fence: primeiro fence cortado bloqueia novo record no processo seguinte',async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'personal-pin-fence-')),key=nodeCrypto.randomBytes(32),x=fenceFixture()
  try{
    const p1=proc(root,key,true)
    await assert.rejects(p1.store.observeFence(x.f.pin,x.observe(x.W1),g));const afterCut=listing(root)
    const p2=proc(root,key),second=await p2.store.observeFence(x.f.pin,x.observe(x.W1),g).then(v=>v,(e:Error)=>e)
    t.diagnostic(JSON.stringify({afterCut,p1Encrypt:p1.stats.encrypt,p2Encrypt:p2.stats.encrypt,p2:p2.calls,second:second instanceof Error?second.message:'record',after:listing(root)}))
    assert.equal(p1.stats.encrypt,1);assert.equal(p1.calls.writeOne,1)
    assert.equal(p2.stats.encrypt,0,'ZERO novo record sobre fence parcial unknown')
    assert.equal((p2.calls.createFirstJournal??0)+(p2.calls.createJournalTmp??0)+(p2.calls.writeOne??0)+(p2.calls.publishJournal??0),0)
    assert(second instanceof Error)
    assert.deepEqual(listing(root),['personal-pin-publication-K.journal'])
  }finally{x.close();fs.rmSync(root,{recursive:true})}
})
test('PL1 PIN marker fence: controle append existente segue tmp/rename não inicial e preserva revisão anterior',async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'personal-pin-fence-append-')),key=nodeCrypto.randomBytes(32),x=fenceFixture()
  try{
    const p1=proc(root,key);assert(await p1.store.observeFence(x.f.pin,x.observe(x.W1),g));const first=openFence(root,key)
    const p2=proc(root,key);assert(await p2.store.observeFence(x.f.pin,x.observe(x.W2),g));const second=openFence(root,key)
    t.diagnostic(JSON.stringify({p2:p2.calls,revisions:[first.revision,second.revision],files:listing(root)}))
    assert.equal(p2.calls.createFirstJournal??0,0);assert.equal(p2.calls.createJournalTmp,1);assert.equal(p2.calls.publishJournal,1)
    assert.equal(first.revision,1);assert.equal(second.revision,2);assert.equal(JSON.stringify(second.entries[0]),JSON.stringify(first.entries[0]))
    assert.deepEqual(listing(root),['personal-pin-publication-K.journal'])
  }finally{x.close();fs.rmSync(root,{recursive:true})}
})
