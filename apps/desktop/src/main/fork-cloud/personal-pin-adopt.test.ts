import assert from 'node:assert/strict'
import test from 'node:test'
import { promises as fs } from 'node:fs'
import * as nativeFs from 'node:fs'
import * as nativeCrypto from 'node:crypto'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import * as nobleEd25519 from '@noble/curves/ed25519.js'
import * as nobleUtils from '@noble/curves/utils.js'
import * as crypto from './crypto.ts'
import * as wire from './personal-wire.ts'
import * as protocol from './personal-protocol.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

// D70 focal: adoção pending nlink1 deve comparar os bytes do candidato RETIDO.
// FS Node real (FDs/pread/link/unlink/fsync) em scratch próprio. Seams declaradas:
// nomes por caminho (não openat), tryLock em memória (NÃO flock), sem C/NAPI/CAS/
// Electron/SafeStorage/Owner. Prova só ordem/comparação do adaptador JS.
const primitives={'node:crypto':nativeCrypto,'@noble/ciphers/chacha.js':{chacha20poly1305},'@noble/curves/ed25519.js':nobleEd25519,'@noble/curves/utils.js':nobleUtils,'./crypto':crypto,'./personal-wire':wire,'./personal-protocol':protocol}
const globals={Buffer,TextEncoder,TextDecoder,Uint8Array}
const key='b'.repeat(64),op='c'.repeat(32)
type H={fd:number;path:string;kind:'dir'|'file'|'lock';adopted?:boolean;locked?:boolean}
function stat(s:nativeFs.Stats){return{dev:String(s.dev),ino:String(s.ino),uid:String(s.uid),mode:String(s.mode),nlink:String(s.nlink),size:String(s.size),regular:s.isFile(),directory:s.isDirectory(),symlink:s.isSymbolicLink()}}
function scratchNative(root:string,beforeCandidateOpen:()=>void){
  const names={pending:`personal-pin-${key}.bin.pending`,final:`personal-pin-${key}.bin`,lock:`personal-pin-publication-${key}.lock`}
  const at=(n:string)=>join(root,n),trace:string[]=[]
  const same=(h:H,p:string)=>{const a=nativeFs.fstatSync(h.fd),b=nativeFs.lstatSync(p);assert(a.dev===b.dev && a.ino===b.ino && !b.isSymbolicLink());return stat(b)}
  const open=(p:string,flags:number,kind:H['kind'],mode?:number):H=>({fd:nativeFs.openSync(p,flags|nativeFs.constants.O_NOFOLLOW,mode),path:p,kind})
  const forbidden=(name:string)=>()=>{trace.push(name);throw new Error(`${name} forbidden in focal`)}
  const api={
    inspect(h:H){trace.push('inspect');return stat(nativeFs.fstatSync(h.fd))},
    inspectName(_d:H,_k:string,_o:string,role:'pending'|'final'){trace.push('inspectName');return stat(nativeFs.lstatSync(at(names[role])))},
    armOwnName(h:H){trace.push('armOwnName');return same(h,h.path)},
    armLinkedFinal(h:H){trace.push('armLinkedFinal');return same(h,at(names.final))},
    openRead(_d:H,_k:string,role:'pending'|'final'){trace.push(`openRead:${role}`);const h=open(at(names[role]),nativeFs.constants.O_RDONLY,'file');if(nativeFs.fstatSync(h.fd).nlink!==1){nativeFs.closeSync(h.fd);throw Object.assign(new Error('pair'),{code:'EPROOFPERM'})}return h},
    openLock(){trace.push('openLock');return open(at(names.lock),nativeFs.constants.O_RDWR,'lock')},
    createLock(){trace.push('createLock');return open(at(names.lock),nativeFs.constants.O_RDWR|nativeFs.constants.O_CREAT|nativeFs.constants.O_EXCL,'lock',0o600)},
    tryLock(h:H){trace.push('tryLock');assert(!h.locked);h.locked=true},
    unlock(h:H){trace.push('unlock');h.locked=false},
    sync(h:H){trace.push('sync');nativeFs.fsyncSync(h.fd)},
    readOne(h:H,offset:number,length:number){trace.push('readOne');const bytes=Buffer.alloc(length);return{count:nativeFs.readSync(h.fd,bytes,0,length,offset),bytes}},
    openPairCandidate(){beforeCandidateOpen();trace.push('openPairCandidate');return open(at(names.pending),nativeFs.constants.O_RDONLY,'file')},
    inspectPairCandidate(h:H){trace.push('inspectPairCandidate');const s=nativeFs.fstatSync(h.fd);assert(s.isFile() && (s.mode&0o7777)===0o600 && s.uid===process.geteuid!() && (s.nlink===1 || s.nlink===2));return stat(s)},
    adoptPair(h:H,lock:H){trace.push('adoptPair');assert(lock.locked);h.adopted=true},
    // Contado na fronteira ANTES do efeito real.
    linkFinal(h:H){trace.push('linkFinal');assert(h.adopted);nativeFs.linkSync(h.path,at(names.final))},
    unlinkOwnPending(h:H){trace.push('unlinkOwnPending');assert(h.adopted);nativeFs.unlinkSync(h.path)},
    close(h:H){trace.push('close');nativeFs.closeSync(h.fd)},
    createPending:forbidden('createPending'),createFirstJournal:forbidden('createFirstJournal'),createJournalTmp:forbidden('createJournalTmp'),writeOne:forbidden('writeOne'),publishJournal:forbidden('publishJournal'),
  }
  return{api,trace,names,at}
}
async function adoption(swap:boolean){
  const root=await fs.mkdtemp(join(tmpdir(),'personal-pin-adopt-'))
  const bytes=nativeCrypto.randomBytes(70000),decoy=Buffer.from(bytes);decoy[decoy.length-1]^=0xff
  let swapped=false
  const n=scratchNative(root,()=>{if(swap){nativeFs.renameSync(join(root,'decoy'),n.at(n.names.pending));swapped=true}})
  await fs.writeFile(n.at(n.names.pending),bytes,{mode:0o600});await fs.writeFile(join(root,'decoy'),decoy,{mode:0o600})
  const {RetainedPinDirectory}=loadMainModule<typeof import('./personal-pin-fs.ts')>('fork-cloud/personal-pin-fs',primitives,globals)
  const dir:H={fd:nativeFs.openSync(root,nativeFs.constants.O_RDONLY|nativeFs.constants.O_DIRECTORY),path:root,kind:'dir'}
  const d=new RetainedPinDirectory(n.api as never,dir as never,{current(){},close(){nativeFs.closeSync(dir.fd)}}),g={async check(){},current(){}}
  let oldRead:Buffer|undefined
  const outcome=await d.transaction(key,g,async tx=>{oldRead=Buffer.from((await tx.read('pending'))!);await tx.publish(bytes,op,true)}).then(()=>'ok',(e:Error)=>e)
  d.close()
  const read=(name:string)=>nativeFs.existsSync(n.at(name))?nativeFs.readFileSync(n.at(name)):null
  return{root,bytes,decoy,swapped,oldRead,outcome,trace:n.trace,final:read(n.names.final),pending:read(n.names.pending)}
}
test('PL1 PIN adoção pending1: candidato substituído após read antigo falha byteCompare do handle NOVO, ZERO link',async t=>{
  const r=await adoption(true)
  try{
    t.diagnostic(JSON.stringify({swapped:r.swapped,oldReadEqual:r.oldRead?.equals(r.bytes),decoyDiffers:!r.decoy.equals(r.bytes),links:r.trace.filter(x=>x==='linkFinal').length,trace:r.trace.join(',')}))
    assert.equal(r.swapped,true);assert(r.oldRead?.equals(r.bytes),'read antigo igual');assert(!r.decoy.equals(r.bytes) && r.decoy.length===r.bytes.length)
    assert.equal(r.trace.filter(x=>x==='linkFinal').length,0,'ZERO link do candidato divergente')
    assert.equal(r.trace.filter(x=>x==='adoptPair'||x==='unlinkOwnPending').length,0)
    assert(r.outcome instanceof Error && /personalSessionInvalid/.test(r.outcome.message))
    assert.equal(r.final,null);assert(r.pending?.equals(r.decoy),'candidato preservado intacto')
  }finally{await fs.rm(r.root,{recursive:true})}
})
test('PL1 PIN adoção pending1: controle candidato retido igual adota, link1 e readback final exato',async t=>{
  const r=await adoption(false)
  try{
    t.diagnostic(JSON.stringify({links:r.trace.filter(x=>x==='linkFinal').length,trace:r.trace.join(',')}))
    assert.equal(r.outcome,'ok');assert.equal(r.swapped,false)
    assert.equal(r.trace.filter(x=>x==='linkFinal').length,1);assert.equal(r.trace.filter(x=>x==='unlinkOwnPending').length,1)
    assert(r.final?.equals(r.bytes));assert.equal(r.pending,null)
  }finally{await fs.rm(r.root,{recursive:true})}
})
