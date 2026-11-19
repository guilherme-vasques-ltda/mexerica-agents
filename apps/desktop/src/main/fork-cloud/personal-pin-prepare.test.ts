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

// D70 focal: prepare precisa ler o journal e reservar record durável SOB o lock antes de
// cifra/ID. RetainedPinDirectory e PersonalPinStore REAIS por "processo" (módulos próprios);
// FS Node real (O_EXCL/pwrite/pread/fsync/rename/link/unlink) em scratch. Seams declaradas:
// nomes por caminho (não openat); lock = registro em memória COMPARTILHADO por caminho entre
// cooperantes (NÃO flock; não cobre quem ignora o lock); cifra AES-GCM de teste (NÃO
// SafeStorage); P30 assinado com seeds do vetor público. Prova ordem JS, não NAPI/CAS/Electron/Owner.
const fixture=JSON.parse(fs.readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json',import.meta.url),'utf8')).personalBootstrap
const w=fixture.wireVectors,authority=protocol.inspectPersonalManifest(Buffer.from(fixture.manifestCanonical)),C=authority.context
const app=crypto.signerFromSeed(crypto.unb64url(w.seeds.app,32)!),namespace='c'.repeat(64)
const binding:DeviceBinding={version:1,sessionId:'fixture-session',origin:C[3],issuer:C[4],deploymentId:C[5],manifestHash:C[6],userId:w.tuple[5],deviceId:w.tuple[6],deviceKey:w.tuple[7]}
const primitives={'node:crypto':nodeCrypto,'@noble/ciphers/chacha.js':{chacha20poly1305},'@noble/curves/ed25519.js':nobleEd25519,'@noble/curves/utils.js':nobleUtils,'./crypto':crypto,'./personal-wire':wire,'./personal-protocol':protocol}
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
  return{pin:history.qualifyPersonalPinHistory(authority,binding,namespace,raw),nonce,hello,proof,originals:o,T}
}
type H={fd:number;path:string;tmp?:string}
const stat=(s:fs.Stats)=>({dev:String(s.dev),ino:String(s.ino),uid:String(s.uid),mode:String(s.mode),nlink:String(s.nlink),size:String(s.size),regular:s.isFile(),directory:s.isDirectory(),symlink:s.isSymbolicLink()})
const names=(key:string,op='')=>({lock:`personal-pin-publication-${key}.lock`,journal:`personal-pin-publication-${key}.journal`,journalTmp:`personal-pin-publication-${key}.journal.${op}.tmp`,pending:`personal-pin-${key}.bin.pending`,final:`personal-pin-${key}.bin`})
function scratchNative(root:string,locks:Map<string,H>,onJournalMissing:()=>void){
  const calls:Record<string,number>={},count=(n:string)=>{calls[n]=(calls[n]??0)+1}
  const at=(key:string,role:keyof ReturnType<typeof names>,op='')=>path.join(root,names(key,op)[role])
  const open=(p:string,flags:number,mode?:number):H=>({fd:fs.openSync(p,flags|fs.constants.O_NOFOLLOW,mode),path:p})
  const same=(h:H,p=h.path)=>{const a=fs.fstatSync(h.fd),b=fs.lstatSync(p);assert(a.dev===b.dev && a.ino===b.ino && !b.isSymbolicLink());return stat(b)}
  const K=fs.constants,X=K.O_RDWR|K.O_CREAT|K.O_EXCL
  const finals=new WeakMap<H,string>()
  const api={
    inspect(h:H){count('inspect');return stat(fs.fstatSync(h.fd))},
    armOwnName(h:H){count('armOwnName');return same(h)},
    armLinkedFinal(h:H){count('armLinkedFinal');return same(h,finals.get(h)!)},
    openRead(_d:H,key:string,role:'pending'|'final'|'journal'){
      count(`openRead:${role}`);let h:H
      try{h=open(at(key,role),K.O_RDONLY)}catch(e){if(role==='journal' && (e as NodeJS.ErrnoException).code==='ENOENT')onJournalMissing();throw e}
      if(fs.fstatSync(h.fd).nlink!==1){fs.closeSync(h.fd);throw Object.assign(new Error('pair'),{code:'EPROOFPERM'})}return h
    },
    openLock(_d:H,key:string){count('openLock');return open(at(key,'lock'),K.O_RDWR)},
    createLock(_d:H,key:string){count('createLock');return open(at(key,'lock'),X,0o600)},
    tryLock(h:H){count('tryLock');const owner=locks.get(h.path);if(owner && owner!==h)throw Object.assign(new Error('busy'),{code:'EWOULDBLOCK'});locks.set(h.path,h)},
    unlock(h:H){count('unlock');if(locks.get(h.path)===h)locks.delete(h.path)},
    sync(h:H){count('sync');fs.fsyncSync(h.fd)},
    readOne(h:H,offset:number,length:number){count('readOne');const b=Buffer.alloc(length);return{count:fs.readSync(h.fd,b,0,length,offset),bytes:b}},
    writeOne(h:H,offset:number,b:Buffer){count('writeOne');return fs.writeSync(h.fd,b,0,b.length,offset)},
    createFirstJournal(_d:H,key:string,_op:string,lock:H){count('createFirstJournal');assert(locks.get(lock.path)===lock);return open(at(key,'journal'),X,0o600)},
    createJournalTmp(_d:H,key:string,op:string){count('createJournalTmp');const h=open(at(key,'journalTmp',op),X,0o600);h.tmp=at(key,'journal');return h},
    publishJournal(h:H,lock:H){count('publishJournal');assert(locks.get(lock.path)===lock && h.tmp);fs.renameSync(h.path,h.tmp)},
    createPending(_d:H,key:string){count('createPending');const h=open(at(key,'pending'),X,0o600);finals.set(h,at(key,'final'));return h},
    linkFinal(h:H){count('linkFinal');fs.linkSync(h.path,finals.get(h)!)},
    unlinkOwnPending(h:H){count('unlinkOwnPending');fs.unlinkSync(h.path)},
    close(h:H){count('close');if(locks.get(h.path)===h)locks.delete(h.path);fs.closeSync(h.fd)},
    inspectName(){throw new Error('inspectName forbidden')},openPairCandidate(){throw new Error('candidate forbidden')},inspectPairCandidate(){throw new Error('candidate forbidden')},adoptPair(){throw new Error('adopt forbidden')},
  }
  return{api,calls}
}
const journalPath=(root:string)=>fs.readdirSync(root).filter(x=>x.endsWith('.journal')).map(x=>path.join(root,x))[0]??null
// Um "processo": módulos/fila próprios, diretório retido próprio, MESMA chave de cifra e MESMO registro de lock.
function proc(root:string,key:Buffer,locks:Map<string,H>,hooks:{onJournalMissing?:()=>void;onPinCipher?:()=>void}={}){
  const n=scratchNative(root,locks,()=>hooks.onJournalMissing?.()),stats={pinCiphers:0,recordSeals:0,journalAtPinCipher:[] as boolean[]}
  const {RetainedPinDirectory}=loadMainModule<typeof import('./personal-pin-fs.ts')>('fork-cloud/personal-pin-fs',primitives,globals)
  const {PersonalPinStore}=loadMainModule<typeof import('./personal-pin-store.ts')>('fork-cloud/personal-pin-store',primitives,globals)
  const dir:H={fd:fs.openSync(root,fs.constants.O_RDONLY|fs.constants.O_DIRECTORY),path:root}
  const directory=new RetainedPinDirectory(n.api as never,dir as never,{current(){},close(){fs.closeSync(dir.fd)}})
  const cipher={current(){},encryptString(text:string){
    // Fronteira da cifra observada ANTES do efeito: PIN (P30 array) ou record (objeto).
    if(text.startsWith('[')){stats.pinCiphers++;stats.journalAtPinCipher.push(journalPath(root)!==null);hooks.onPinCipher?.()}else stats.recordSeals++
    const nonce=nodeCrypto.randomBytes(12),c=nodeCrypto.createCipheriv('aes-256-gcm',key,nonce),data=Buffer.concat([c.update(text,'utf8'),c.final()]);return Buffer.concat([nonce,c.getAuthTag(),data])
  },decryptString(data:Buffer){const d=nodeCrypto.createDecipheriv('aes-256-gcm',key,data.subarray(0,12));d.setAuthTag(data.subarray(12,28));return Buffer.concat([d.update(data.subarray(28)),d.final()]).toString('utf8')}}
  const store=new PersonalPinStore({namespace,current:()=>true} as Principal,binding,authority,cipher as never,directory)
  return{store,calls:n.calls,stats,cipher}
}
function probeOf(f:ReturnType<typeof pinFixture>,check:()=>Promise<void>=async()=>{}){
  return{nonce:f.nonce,hello:f.hello,proof:f.proof,originals:f.originals,observation:{tuple:f.T},check,current(){},close(){}} as unknown as import('./personal-probe').PersonalProbeEvidence
}
test('PL1 PIN prepare: cooperante torna record durável após read null; MESMA operação não cifra/gera ID sobre ele',async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'personal-pin-prepare-race-')),key=nodeCrypto.randomBytes(32),locks=new Map<string,H>(),f=pinFixture()
  try{
    let armed=false,fired=false,contender:unknown=null
    const B=proc(root,key,locks)
    const A=proc(root,key,locks,{onJournalMissing:()=>{if(!fired)armed=true}})
    // Interleaving determinístico: o PRÓXIMO check de A após A observar journal ENOENT roda B uma vez.
    const probeA=probeOf(f,async()=>{if(armed && !fired){fired=true;const pB=probeOf(f);contender=await B.store.prepare(f.pin,pB).then(p=>B.store.commit(p,pB)).then(r=>r.kind,(e:Error)=>`rejected:${e.message}`)}})
    const outcome=await A.store.prepare(f.pin,probeA).then(()=>'prepared',(e:Error)=>`rejected:${e.message}`)
    const durable=journalPath(root)
    t.diagnostic(JSON.stringify({fired,contender,outcome,A:A.stats,Bstats:B.stats,durable:durable!==null,files:fs.readdirSync(root).map(x=>x.replace(/[a-f0-9]{64}/,'K')).sort()}))
    assert.equal(fired,true,'interleaving executado')
    assert(A.stats.pinCiphers<=1,'operação legítima cifra no máximo uma vez')
    assert.deepEqual(A.stats.journalAtPinCipher.filter(Boolean),[],'ZERO cifra PIN de A com record durável de outro cooperante')
    if(contender==='stored')assert.notEqual(outcome,'prepared','A não devolve PreparedPin sobre record durável alheio')
  }finally{fs.rmSync(root,{recursive:true})}
})
test('PL1 PIN prepare: PreparedPin só retorna com record prepared durável dos MESMOS bytes no nome final',async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'personal-pin-prepare-durable-')),key=nodeCrypto.randomBytes(32),f=pinFixture()
  try{
    const A=proc(root,key,new Map()),prepared=await A.store.prepare(f.pin,probeOf(f))
    const durable=journalPath(root),record=durable?codec.openPinRecord(fs.readFileSync(durable),binding,A.cipher as never):null
    t.diagnostic(JSON.stringify({stats:A.stats,calls:A.calls,durable:durable!==null,phase:record?.phase??null,revision:record?.revision??null}))
    assert.equal(A.stats.pinCiphers,1,'cifra real de origem ocorreu uma vez')
    assert.notEqual(durable,null,'record durável antes do retorno de prepare')
    assert.equal(record!.phase,'prepared');assert.equal(record!.revision,0)
    assert.equal(record!.fileBytesB64,Buffer.from(prepared.fileBytes).toString('base64'));assert.equal(record!.signedBytesB64,Buffer.from(prepared.signedBytes).toString('base64'))
  }finally{fs.rmSync(root,{recursive:true})}
})
test('PL1 PIN prepare: controle sem concorrência prepare+commit armazena final exato com uma cifra PIN',async t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'personal-pin-prepare-control-')),key=nodeCrypto.randomBytes(32),f=pinFixture()
  try{
    const A=proc(root,key,new Map()),probe=probeOf(f),prepared=await A.store.prepare(f.pin,probe),result=await A.store.commit(prepared,probe)
    const final=fs.readdirSync(root).find(x=>x.endsWith('.bin'))
    t.diagnostic(JSON.stringify({kind:result.kind,stats:A.stats,files:fs.readdirSync(root).map(x=>x.replace(/[a-f0-9]{64}/,'K')).sort()}))
    assert.equal(result.kind,'stored');assert.equal(A.stats.pinCiphers,1)
    assert(final && fs.readFileSync(path.join(root,final)).equals(Buffer.from(prepared.fileBytes)))
    assert.equal(result.record.phase,'committed')
  }finally{fs.rmSync(root,{recursive:true})}
})
