/** Composição privada PIN → CLOCK-S → NOVO probe, sem helper antigo/Use. */
import type { Principal } from './principal'
import type { DeviceBinding } from '../fork-account/service'
import { b64url,random,type DeviceSigner } from './crypto'
import type { PersonalManifest } from './personal-protocol'
import type { PersonalElapsed } from './personal-runtime'
import type { PersonalClock } from './personal-clock'
import type { HistoricalPinEvidence } from './personal-pin-history'
import { PersonalProbe,type PersonalProbeEvidence } from './personal-probe'
import { postPinnedClockRead } from './personal-cold'
import { personalAppPost } from './personal-api'
import { personalDomain as D,personalHash,PersonalVerifier } from './personal-wire'
import { personalBytes,personalNeed,personalSame,pinnedEffect,verifyPersonalObservation,requireLivePersonalObservation,type CurrentPersonalObservation,type PinnedEffect,type PersonalEffectGuard } from './personal-session'
import type { PersonalPinStore } from './personal-pin-store'
import { samePinOperation,type PinPublicationRecord } from './personal-pin-storage'

export class PersonalPinnedSession {
  private alive=true
  private probe:PersonalProbe|null=null
  private evidence:PersonalProbeEvidence|null=null
  constructor(private readonly principal:Principal,private readonly binding:Readonly<DeviceBinding>,private readonly signer:DeviceSigner,private readonly authority:PersonalManifest,private readonly elapsed:PersonalElapsed,private readonly clock:PersonalClock,private readonly store:PersonalPinStore,readonly pin:HistoricalPinEvidence,private readonly recovery:PinPublicationRecord|null=null){}
  current(){this.elapsed.current();this.store.current();personalNeed(this.alive && this.principal.current() && b64url(this.signer.publicKey)===this.binding.deviceKey)}
  private async samePin(){
    const g={check:async()=>{this.current()},current:()=>this.current()}
    const recovered=this.recovery?await this.store.loadRecovery(this.pin.tuple[11] as string,g):null
    if(this.recovery)personalNeed(recovered && samePinOperation(recovered.record,this.recovery))
    const h=this.recovery?recovered?.history:await this.store.loadHistorical(this.pin.tuple[11] as string,g)
    this.current();personalNeed(h && h.digest===this.pin.digest && personalSame(h.tuple,this.pin.tuple));return h
  }
  private effect():PinnedEffect {
    const hub=this.pin.tuple[11] as string,anchor=this.clock.window(hub)
    const sample=()=>{this.current();const w=this.clock.window(hub);personalNeed(w.generation===anchor.generation);return w}
    return pinnedEffect(this.pin,{check:async()=>{await this.samePin();sample()},current:()=>{sample()},sample})
  }
  private async read(effect:PersonalEffectGuard):Promise<CurrentPersonalObservation>{
    await effect.check();effect.current();const w=effect.sample(),q=[D('read'),this.authority.context,this.pin.tuple[1],this.pin.tuple[9],this.pin.hT,b64url(random(32)),w.lower,w.lower+30000]
    personalNeed(Number.isSafeInteger(q[7]));effect.current();const read=[q,b64url(this.signer.sign(personalBytes(q)))] as const
    const guard={check:async()=>{this.current();await effect.check();effect.current()},sample:()=>{effect.current();const now=effect.sample();personalNeed(now.generation===w.generation && now.lower>=(q[6] as number) && now.upper<(q[7] as number));return now}}
    await guard.check();guard.sample();const reply=await personalAppPost(this.principal,this.signer,'read',{read},guard);await guard.check()
    return verifyPersonalObservation(this.authority,this.binding,this.pin.originals,read,reply,guard.sample())
  }
  async refresh():Promise<CurrentPersonalObservation>{
    this.current();this.evidence?.close();this.evidence=null;this.probe?.close();this.probe=null
    // Only this private Read may enter before an epoch; no EpochWindow(Date.now).
    const cold=await postPinnedClockRead(this.principal,this.signer,this.authority,this.pin,this.elapsed,this.clock,{check:()=>this.samePin(),current:()=>this.current()})
    this.current();const w=this.clock.window(this.pin.tuple[11] as string),first=verifyPersonalObservation(this.authority,this.binding,this.pin.originals,cold.read,cold.reply,w),effect=this.effect()
    await this.store.observeFence(this.pin,first,effect);this.current()
    // W/revoking/terminal observations cannot open even a new proof channel.
    requireLivePersonalObservation(first)
    const probe=new PersonalProbe(this.principal,this.binding,this.signer,this.authority,this.elapsed,effect,this.pin.originals,first,guard=>this.read(guard));this.probe=probe
    try{const evidence=await probe.open();this.current();evidence.current();this.evidence=evidence;return evidence.observation}catch(e){probe.close();this.probe=null;throw e}
  }
  async recover(record:PinPublicationRecord){
    // Explicit recovery obtains CURRENT CLOCK-S/S/V/E/probe, preserving old P30.
    await this.refresh();const probe=this.evidence;personalNeed(probe)
    try{return await this.store.reconcile(record,{history:this.pin,current:this.effect(),probe})}finally{probe.close();this.evidence=null;this.probe?.close();this.probe=null}
  }
  async revoke(){
    await this.refresh();const evidence=this.evidence;personalNeed(evidence)
    const base=this.effect(),effect=pinnedEffect(this.pin,{check:async()=>{await base.check();await evidence.check()},current:()=>{base.current();evidence.current()},sample:()=>{evidence.current();return base.sample()}})
    try{return await this.revokeCurrent(effect)}finally{evidence.close();this.evidence=null;this.probe?.close();this.probe=null}
  }
  private async revokeCurrent(effect:PinnedEffect){
    await effect.check();const observed=await this.read(effect)
    await this.store.observeFence(this.pin,observed,effect);this.current()
    if(observed.state[12]!==null)return observed // Existing W: only Read, never replay/newID.
    const w=effect.sample(),verifier=new PersonalVerifier(this.authority.context,{issuer:this.authority.issuerPublicKey,app:this.binding.deviceKey,hub:this.pin.tuple[12] as string,box:this.pin.tuple[13] as string})
    const revoke=await this.store.beginRevoke(this.pin,effect,()=>{
      const q=[D('revoke'),this.authority.context,this.pin.tuple[1],this.pin.hT,personalHash((this.pin.originals.grant as unknown[][])[0]),b64url(random(16)),'ownerRevoked',w.lower,w.lower+30000]
      effect.current();const signed=[q,b64url(this.signer.sign(personalBytes(q)))];effect.current()
      verifier.verify('revoke',personalBytes(signed),{kind:'current',now:effect.sample().upper});return signed
    })
    if(revoke){
      const guard={check:()=>effect.check(),sample:()=>{effect.current();const now=effect.sample();personalNeed(now.generation===w.generation && now.lower>=w.lower && now.upper<w.lower+30000);verifier.verify('revoke',personalBytes(revoke),{kind:'current',now:now.upper});return now}}
      await personalAppPost(this.principal,this.signer,'revoke',{revoke},guard)
    }
    const next=await this.read(effect);await this.store.observeFence(this.pin,next,effect);this.current();return next
  }
  close(){if(!this.alive)return;this.alive=false;this.evidence?.close();this.evidence=null;this.probe?.close();this.probe=null}
}
