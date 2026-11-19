/** PinStore Main: história não é lease; primeiro commit retém o MESMO probe. */
import type { Principal } from './principal'
import type { DeviceBinding } from '../fork-account/service'
import type { PersonalManifest } from './personal-protocol'
import type { PersonalProbeEvidence } from './personal-probe'
import { qualifyPersonalPinHistory,type HistoricalPinEvidence } from './personal-pin-history'
import type { PinnedEffect,CurrentPersonalObservation } from './personal-session'
import { personalNeed,personalSame,personalImmutable,verifyPersonalObservation,personalRemoteNames } from './personal-session'
import { verifyPersonalFenceObservation,type PersonalFenceRevision } from './personal-fences'
import { pinKey,pinOperation,sealPin,openPin,sealPinRecord,openPinRecord,validatePinRecord,samePinOperation,pinDigest,type PinCipher,type PinPublicationRecord } from './personal-pin-storage'
import type { PinDirectory,PinIOGuard } from './personal-pin-fs'

const preparedBrand:unique symbol=Symbol('preparedPersonalPin')
export interface PreparedPin {readonly [preparedBrand]:true;readonly history:HistoricalPinEvidence;readonly signedBytes:Uint8Array;readonly fileBytes:Uint8Array}
export type PinCommitResult={readonly kind:'stored';readonly history:HistoricalPinEvidence;readonly record:PinPublicationRecord}|{readonly kind:'unknown';readonly record:PinPublicationRecord}
export interface PinRecoveryContext {readonly history:HistoricalPinEvidence;readonly current:PinnedEffect;readonly probe:PersonalProbeEvidence}
const queues=new Map<string,Promise<unknown>>()
async function queued<T>(key:string,action:()=>Promise<T>){const before=queues.get(key)??Promise.resolve(),next=before.catch(()=>{}).then(action);queues.set(key,next);try{return await next}finally{if(queues.get(key)===next)queues.delete(key)}}
export class PersonalPinStore {
  private alive=true
  private readonly prepared=new WeakMap<PreparedPin,{probe:PersonalProbeEvidence;record:PinPublicationRecord;used:boolean}>()
  constructor(private readonly principal:Principal,private readonly binding:Readonly<DeviceBinding>,private readonly authority:PersonalManifest,private readonly cipher:PinCipher,private readonly directory:PinDirectory){}
  current(){personalNeed(this.alive && this.principal.current());this.cipher.current();this.directory.current()}
  private async checked(g:PinIOGuard){this.current();g.current();await g.check();this.current();g.current()}
  private guardedCipher(g:PinIOGuard):PinCipher {
    // Runtime/backend/directory checks may themselves block: the EXACT operation
    // guard must run LAST, immediately before/after the native cipher call.
    return{current:()=>{this.current();g.current()},encryptString:s=>this.cipher.encryptString(s),decryptString:b=>this.cipher.decryptString(b)}
  }
  private key(h:HistoricalPinEvidence){personalNeed(h.namespace===this.principal.namespace && h.tuple[5]===this.binding.userId && h.tuple[6]===this.binding.deviceId && h.tuple[7]===this.binding.deviceKey);return pinKey(h.namespace,h.tuple[11] as string)}
  private history(bytes:Uint8Array){this.current();const h=qualifyPersonalPinHistory(this.authority,this.binding,this.principal.namespace,bytes);this.key(h);this.current();return h}
  async prepare(history:HistoricalPinEvidence,probe:PersonalProbeEvidence):Promise<PreparedPin>{
    await this.checked(probe);const h=this.history(history.signedBytes)
    personalNeed(h.digest===history.digest && h.body[26]===probe.nonce && personalSame(h.body[27],probe.hello) && personalSame(h.body[28],probe.proof) && personalSame(h.originals,probe.originals))
    // Read, PIN cipher, operation ID and the durable 'prepared' reservation all
    // happen under the SAME stable lock and probe. Any existing/partial record or
    // orphan PIN object blocks BEFORE encrypting: only explicit same-byte reconcile.
    const key=this.key(h),signed=Buffer.from(h.signedBytes)
    const record=await queued(key,()=>this.directory.transaction(key,probe,async tx=>{
      await this.checked(probe);personalNeed(await tx.read('journal')===null);await this.checked(probe)
      personalNeed(await tx.read('final')===null && await tx.read('pending')===null);await this.checked(probe)
      const file=sealPin(h,this.guardedCipher(probe));await this.checked(probe)
      const reserved=validatePinRecord({binding:this.binding,operationId:pinOperation(),revision:0,signedBytesB64:signed.toString('base64'),fileBytesB64:file.toString('base64'),sha256:pinDigest(file),phase:'prepared'},this.binding)
      const sealed=sealPinRecord(reserved,this.guardedCipher(probe));await this.checked(probe);await tx.journal(sealed,reserved.operationId,true);await this.checked(probe)
      return reserved
    }))
    const file=Buffer.from(record.fileBytesB64,'base64')
    const value:PreparedPin=Object.freeze({[preparedBrand]:true as const,history:h,get signedBytes(){return new Uint8Array(signed)},get fileBytes(){return new Uint8Array(file)}})
    this.prepared.set(value,{probe,record,used:false});return value
  }
  private async record(raw:Uint8Array|null,g:PinIOGuard){await this.checked(g);return raw?openPinRecord(raw,this.binding,this.guardedCipher(g)):null}
  async commit(value:PreparedPin,probe:PersonalProbeEvidence):Promise<PinCommitResult>{
    const entry=this.prepared.get(value);personalNeed(entry && entry.probe===probe && !entry.used);entry.used=true
    const key=this.key(value.history),original=entry.record
    return queued(key,async()=>{
      let record=original
      try{
        await this.checked(probe)
        return await this.directory.transaction(key,probe,async tx=>{
          // prepare already reserved these EXACT bytes durably; any change stops here.
          await this.checked(probe);const durable=await this.record(await tx.read('journal'),probe)
          personalNeed(durable && samePinOperation(durable,original) && durable.revision===original.revision && durable.phase===original.phase);await this.checked(probe)
          await tx.publish(Buffer.from(record.fileBytesB64,'base64'),record.operationId,false);await this.checked(probe)
          record=validatePinRecord({...record,revision:record.revision+1,phase:'committed'},this.binding)
          const committed=sealPinRecord(record,this.guardedCipher(probe));await this.checked(probe);await tx.journal(committed,record.operationId);await this.checked(probe)
          return{kind:'stored' as const,history:value.history,record}
        })
      }catch{ return{kind:'unknown' as const,record:personalImmutable({...record,phase:'unknown' as const})} }
    })
  }
  async loadHistorical(computerId:string,g:PinIOGuard):Promise<HistoricalPinEvidence|null>{
    await this.checked(g);const key=pinKey(this.principal.namespace,computerId),raw=await this.directory.read(key,'final',g);await this.checked(g);if(!raw)return null
    const h=this.history(openPin(raw,this.guardedCipher(g)));personalNeed(h.tuple[11]===computerId);await this.checked(g);return h
  }
  async publication(computerId:string,g:PinIOGuard){await this.checked(g);const record=await this.record(await this.directory.read(pinKey(this.principal.namespace,computerId),'journal',g),g);await this.checked(g);return record}
  async reconcile(expected:PinPublicationRecord,ctx:PinRecoveryContext):Promise<PinCommitResult>{
    const original=validatePinRecord(expected,this.binding),h=this.history(Buffer.from(original.signedBytesB64,'base64')),key=this.key(h)
    personalNeed(h.digest===ctx.history.digest && personalSame(h.tuple,ctx.probe.observation.tuple))
    for(const name of ['intent','local-ticket','app-consent','local-consent','box-challenge','box-proof','approve','local-confirm','consume','grant','challenge','proof','receipt','publish'])personalNeed(personalSame(h.originals[name],ctx.probe.originals[name]))
    // Current probe/S/V/E govern this explicit operation, not the old 5s budget.
    // No helper capture, re-sign, encryptPin, new operationId or restored flight.
    const g:PinIOGuard={check:async()=>{await ctx.current.check();await ctx.probe.check()},current:()=>{ctx.current.current();ctx.probe.current()}}
    return queued(key,async()=>{
      let record=original
      try{return await this.directory.transaction(key,g,async tx=>{
        await this.checked(g);const durable=await this.record(await tx.read('journal'),g);personalNeed(durable && samePinOperation(durable,original) && durable.revision===original.revision && durable.phase===original.phase)
        await this.checked(g);personalNeed(this.history(openPin(Buffer.from(durable.fileBytesB64,'base64'),this.guardedCipher(g))).digest===h.digest);await this.checked(g)
        await tx.publish(Buffer.from(durable.fileBytesB64,'base64'),durable.operationId,true);await this.checked(g)
        record=validatePinRecord({...durable,revision:durable.revision+1,phase:'committed'},this.binding);const sealed=sealPinRecord(record,this.guardedCipher(g));await this.checked(g);await tx.journal(sealed,record.operationId);await this.checked(g)
        return{kind:'stored' as const,history:h,record}
      })}catch{return{kind:'unknown' as const,record:personalImmutable({...record,phase:'unknown' as const})}}
    })
  }
  async loadRecovery(computerId:string,g:PinIOGuard){
    const record=await this.publication(computerId,g);await this.checked(g);if(!record)return null
    const history=this.history(Buffer.from(record.signedBytesB64,'base64'))
    personalNeed(history.tuple[11]===computerId && this.history(openPin(Buffer.from(record.fileBytesB64,'base64'),this.guardedCipher(g))).digest===history.digest)
    await this.checked(g);return{history,record}
  }
  /** The durable send marker is written BEFORE token/HTTP. An existing marker
   * permits only Read, even if the first process never reached fetch. */
  async beginRevoke(pin:HistoricalPinEvidence,g:PinIOGuard,create:()=>unknown){
    const key=pinDigest(Buffer.from(JSON.stringify(['personal-linux-v1',pin.namespace,pin.tuple[11],'revoke'])))
    return queued(key,()=>this.directory.transaction(key,g,async tx=>{
      await this.checked(g);const raw=await tx.read('journal');await this.checked(g)
      if(raw)return null // Fail closed; never replace/replay an earlier operation.
      g.current();const revoke=create();g.current();const text=JSON.stringify({schema:1,binding:this.binding,pin:pin.digest,revision:0,revoke,sending:true})
      personalNeed(Buffer.byteLength(text)<=16384);const cipher=this.guardedCipher(g);cipher.current();const sealed=cipher.encryptString(text);cipher.current();personalNeed(sealed.length<=832*1024)
      // First (and only) marker: FINAL name O_EXCL, so a partial write still blocks a new token/send.
      await this.checked(g);await tx.journal(sealed,pinOperation(),true);await this.checked(g);return revoke
    }))
  }
  /** Fence history is append-only inside its own encrypted/CAS record; never
   * rewrite P30 or the publication record to clear a revoked device. */
  async observeFence(pin:HistoricalPinEvidence,observation:CurrentPersonalObservation,g:PinIOGuard){
    await this.checked(g);personalNeed(observation.hT===pin.hT && personalSame(observation.tuple,pin.tuple))
    const key=pinDigest(Buffer.from(JSON.stringify(['personal-linux-v1',pin.namespace,pin.tuple[11],'fences'])))
    return queued(key,()=>this.directory.transaction(key,g,async tx=>{
      const raw=await tx.read('journal');await this.checked(g)
      let previous:PersonalFenceRevision|null=null,entries:unknown[]=[]
      if(raw){
        const cipher=this.guardedCipher(g);cipher.current();const text=cipher.decryptString(Buffer.from(raw));cipher.current();personalNeed(Buffer.byteLength(text)<=768*1024)
        const record=JSON.parse(text) as {schema:number;pin:string;binding:DeviceBinding;revision:number;entries:{record:PersonalFenceRevision;terminal:boolean;read:unknown;state:unknown}[]}
        personalNeed(JSON.stringify(record)===text && Object.keys(record).sort().join(',')==='binding,entries,pin,revision,schema' && record.schema===1 && record.pin===pin.digest && personalSame(record.binding,this.binding) && Number.isSafeInteger(record.revision) && record.revision>0 && record.revision===record.entries.length && record.entries.length<=128)
        // Re-verify signed historic S/W/Y/X and each monotone revision. The
        // cipher/last JSON record alone is not authority for a fence.
        for(const entry of record.entries){
          const state=(entry.state as unknown[][])?.[0],request=entry.read as readonly [readonly unknown[],string]
          personalNeed(Array.isArray(state) && typeof state[15]==='number')
          const originalMap:Record<string,unknown>={}
          for(const[remote,local]of Object.entries(personalRemoteNames))originalMap[remote]=local==='fence'?JSON.parse(entry.record.W):local==='fence-ack'?(entry.record.Y?JSON.parse(entry.record.Y):null):local==='revocation'?(entry.record.X?JSON.parse(entry.record.X):null):pin.originals[local]??null
          const historical=verifyPersonalObservation(this.authority,this.binding,pin.originals,request,{state:entry.state,originals:originalMap,activationCurrent:null},{generation:'fence-history-only',lower:state[15],upper:state[15]})
          const verified=verifyPersonalFenceObservation(historical,previous);personalNeed(verified && personalSame(verified,entry));previous=verified.record
        }
        entries=record.entries
      }
      const next=verifyPersonalFenceObservation(observation,previous);await this.checked(g);if(!next)return null
      // A new observed S may enrich Y/X of same W; older entries stay immutable.
      // First record uses the FINAL name O_EXCL; appends keep tmp/rename under lock.
      personalNeed(entries.length<128)
      const value={schema:1,pin:pin.digest,binding:this.binding,revision:entries.length+1,entries:[...entries,next]},text=JSON.stringify(value);personalNeed(Buffer.byteLength(text)<=768*1024)
      const cipher=this.guardedCipher(g);cipher.current();const sealed=cipher.encryptString(text);cipher.current();personalNeed(sealed.length<=832*1024);await this.checked(g);await tx.journal(sealed,pinOperation(),raw===null);await this.checked(g);return next
    }))
  }
  close(){if(!this.alive)return;this.alive=false;this.directory.close()}
}
