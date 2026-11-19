/** Journal PL1 Main. Originais públicos e segredos temporários em slots distintos. */
import { constants,promises as fs } from 'node:fs'
import { join } from 'node:path'
import { randomBytes } from 'node:crypto'
import { app,safeStorage } from 'electron'
import { securePinsAvailable,readPinBytes } from './pins'
import { personalHash } from './personal-wire'
import type { Principal } from './principal'
import type { DeviceBinding } from '../fork-account/service'
export type SecretSlot='commandToken'|'candidateBody'|'candidateRequest'|'userCode'
export interface PersonalJournal {
  readonly schema:2;readonly binding:Readonly<DeviceBinding>;readonly namespace:string;readonly attemptId:string
  readonly mode:'command'|'device';readonly sourceId:string;readonly revision:number
  readonly phase:'prepared'|'awaitingApproval'|'running'|'unknown'|'receipt'|'stopped'|'closed'
  readonly originals:Readonly<Record<string,string>>
  readonly sending:{readonly operationId:string;readonly route:string;readonly body:string}|null
  readonly secret:Readonly<Record<SecretSlot,string|null>>
  readonly cleared:readonly SecretSlot[]
  readonly facts:Readonly<{origin:string|null;enrollment:string|null;candidate:string|null;closure:string|null}>
  readonly sent:Readonly<Record<string,string>>
  readonly cleanupProofs:readonly string[]
}
export interface PersonalJournalGuard {check():Promise<void>;current():void}
interface Update {sent?:PersonalJournal['sent'];secret?:PersonalJournal['secret'];facts?:PersonalJournal['facts'];clearProof?:string}
const queues=new Map<string,Promise<unknown>>()
const slots:readonly SecretSlot[]=['commandToken','candidateBody','candidateRequest','userCode']
const allowed=['create','attach','intent','local-ticket','app-consent','local-consent','box-challenge','box-proof','approve','local-confirm','consume','grant','challenge','proof','receipt','cancel','publish','activation','host-state-request','host-state','revoke']
const check=(p:Principal)=>{if(!p.current() || !securePinsAvailable() || process.platform==='linux' && !['gnome_libsecret','kwallet','kwallet5','kwallet6'].includes(safeStorage.getSelectedStorageBackend()))throw new Error('personalStorageUnavailable')}
const same=(a:unknown,b:unknown)=>JSON.stringify(a)===JSON.stringify(b)
const pathFor=(p:Principal,b:Readonly<DeviceBinding>)=>join(app.getPath('userData'),`personal-journal-${personalHash(['personal-linux-v1',p.namespace,b.deviceId,b.deviceKey])}.bin`)
function immutable<T>(v:T):T{if(v && typeof v==='object'){for(const item of Object.values(v))immutable(item);Object.freeze(v)}return v}
async function queue<T>(path:string,action:()=>Promise<T>):Promise<T>{const old=queues.get(path)??Promise.resolve(),next=old.catch(()=>{}).then(action);queues.set(path,next);try{return await next}finally{if(queues.get(path)===next)queues.delete(path)}}
async function directory(){const d=await fs.open(app.getPath('userData'),constants.O_RDONLY|constants.O_DIRECTORY|constants.O_NOFOLLOW);try{await d.sync()}finally{await d.close()}}
async function read(p:Principal,b:Readonly<DeviceBinding>,path:string):Promise<PersonalJournal|null>{
  check(p);let encrypted:Uint8Array
  try{encrypted=await readPinBytes(path,320*1024)}catch(e){if((e as NodeJS.ErrnoException).code==='ENOENT'){check(p);return null}throw e}
  check(p);const j=JSON.parse(safeStorage.decryptString(Buffer.from(encrypted))) as PersonalJournal
  if(!j || Object.keys(j).sort().join(',')!=='attemptId,binding,cleanupProofs,cleared,facts,mode,namespace,originals,phase,revision,schema,secret,sending,sent,sourceId' || j.schema!==2 || j.namespace!==p.namespace || !same(j.binding,b) || !Number.isSafeInteger(j.revision) || j.revision<0 || !/^[A-Za-z0-9_-]{22}$/.test(j.attemptId) || !['command','device'].includes(j.mode) || typeof j.sourceId!=='string' || !['prepared','awaitingApproval','running','unknown','receipt','stopped','closed'].includes(j.phase) || !j.originals || !j.secret || !j.facts || !j.sent || !Array.isArray(j.cleared) || !Array.isArray(j.cleanupProofs))throw new Error('personalJournalMismatch')
  if(Object.keys(j.secret).sort().join(',')!=='candidateBody,candidateRequest,commandToken,userCode' || Object.keys(j.facts).sort().join(',')!=='candidate,closure,enrollment,origin')throw new Error('personalJournalShape')
  for(const[name,raw]of Object.entries(j.originals))if(!allowed.includes(name) || typeof raw!=='string' || Buffer.byteLength(raw)>16384)throw new Error('personalJournalOriginal')
  for(const key of slots)if(j.secret[key]!==null && (typeof j.secret[key]!=='string' || j.secret[key]!.length>32768) || j.cleared.includes(key) && j.secret[key]!==null)throw new Error('personalJournalSecret')
  if(j.cleared.some(k=>!slots.includes(k)) || j.cleanupProofs.some(p=>typeof p!=='string' || !/^[a-f0-9]{64}$/.test(p)))throw new Error('personalJournalCleanup')
  check(p);return immutable(j)
}
async function write(p:Principal,path:string,value:PersonalJournal,exclusive:boolean,guard?:PersonalJournalGuard){
  check(p);if(guard){await guard.check();guard.current()}const plain=JSON.stringify(value);if(Buffer.byteLength(plain)>256*1024)throw new Error('personalJournalTooLarge')
  const sealed=safeStorage.encryptString(plain);check(p);const tmp=exclusive?path:`${path}.${randomBytes(16).toString('hex')}.tmp`
  const f=await fs.open(tmp,constants.O_WRONLY|constants.O_CREAT|constants.O_EXCL|constants.O_NOFOLLOW,0o600)
  try{check(p);if(guard){await guard.check();guard.current()}await f.writeFile(sealed);if(guard){await guard.check();guard.current()}await f.sync();if(guard){await guard.check();guard.current()}}finally{await f.close()}
  // Depois de todas as esperas, a amostra síncrona precede a publicação.
  // Aborto conserva o temporário cifrado exclusivo, sem promover/apagar/reusar.
  check(p);if(guard){await guard.check();guard.current()}check(p)
  if(!exclusive){await fs.rename(tmp,path);check(p);if(guard){await guard.check();guard.current()}}await directory();check(p);if(guard){await guard.check();guard.current()}
}
export class PersonalJournalStore {
  constructor(private readonly principal:Principal,private readonly binding:Readonly<DeviceBinding>){}
  load(){const p=this.principal,path=pathFor(p,this.binding);return queue(path,()=>read(p,this.binding,path))}
  create(mode:'command'|'device',sourceId:string):Promise<PersonalJournal>{const p=this.principal,path=pathFor(p,this.binding);return queue(path,async()=>{
    check(p);if(await read(p,this.binding,path))throw new Error('personalJournalExists')
    const value:PersonalJournal={schema:2,binding:{...this.binding},namespace:p.namespace,attemptId:randomBytes(16).toString('base64url'),mode,sourceId,revision:0,phase:'prepared',originals:{},sending:null,secret:{commandToken:null,candidateBody:null,candidateRequest:null,userCode:null},cleared:[],facts:{origin:null,enrollment:null,candidate:null,closure:null},sent:{},cleanupProofs:[]}
    await write(p,path,value,true);return immutable(value)
  })}
  advance(expected:PersonalJournal,phase:PersonalJournal['phase'],originals=expected.originals,sending=expected.sending,update:Update={},guard?:PersonalJournalGuard):Promise<PersonalJournal>{const p=this.principal,path=pathFor(p,this.binding);return queue(path,async()=>{
    check(p);const old=await read(p,this.binding,path)
    if(!old || !same(old,expected) || old.phase==='closed' || Object.entries(old.originals).some(([k,v])=>originals[k]!==v) || old.sending && sending && !same(old.sending,sending) || Object.keys(originals).some(k=>!allowed.includes(k)))throw new Error('personalJournalConflict')
    if(guard){await guard.check();guard.current()}
    const secret=update.secret??old.secret,facts=update.facts??old.facts,sent=update.sent??old.sent,cleared=[...old.cleared],proofs=[...old.cleanupProofs]
    for(const key of slots){if(old.cleared.includes(key) && secret[key]!==null)throw new Error('personalSecretClosed');if(old.secret[key]!==null && old.secret[key]!==secret[key]){if(secret[key]!==null || !update.clearProof || !/^[a-f0-9]{64}$/.test(update.clearProof))throw new Error('personalSecretChanged');if(!cleared.includes(key))cleared.push(key)}}
    for(const [key,hash]of Object.entries(old.sent))if(sent[key]!==hash)throw new Error('personalSendHistoryChanged')
    for(const key of ['origin','enrollment','candidate','closure'] as const)if(old.facts[key]!==null && old.facts[key]!==facts[key])throw new Error('personalFactsChanged')
    if(update.clearProof && !proofs.includes(update.clearProof))proofs.push(update.clearProof)
    if(phase==='closed' && !facts.closure)throw new Error('personalClosureMissing')
    const value:PersonalJournal={...old,phase,originals:{...originals},sending,sent:{...sent},secret:{...secret},facts:{...facts},cleared,cleanupProofs:proofs,revision:old.revision+1}
    if(!Number.isSafeInteger(value.revision))throw new Error('personalJournalRevision')
    await write(p,path,value,false,guard);return immutable(value)
  })}
}
