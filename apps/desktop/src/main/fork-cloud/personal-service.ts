/** Intenções PL1 do Main até R durável. Não entrega attachment, ready ou contas. */
import { powerMonitor } from 'electron'
import type { AccountService,DeviceBinding } from '../fork-account/service'
import type { Principal } from './principal'
import { personalAuthority } from './personal-authority'
import { deviceSigner } from './device-keys'
import { b64url,random,ephemeral,unb64url,type DeviceSigner } from './crypto'
import { capturePersonal,type PersonalCapture } from './personal-origin'
import type { PersonalSshProfile } from './personal-ssh'
import { PersonalClock,type EpochWindow } from './personal-clock'
import { qualifyPersonalRuntime,type PersonalElapsed } from './personal-runtime'
import { PersonalJournalStore,type PersonalJournal,type PersonalJournalGuard } from './personal-journal'
import { personalAppPost,type PersonalAppRoute,type PersonalAppGuard } from './personal-api'
import type { HelperRequest } from './personal-helper'
import { PersonalPossession } from './personal-channel'
import { PersonalPrepare } from './personal-prepare'
import { verifyPersonalObservation,requireLivePersonalObservation,ceremonyEffect,type CurrentPersonalObservation,type PersonalEffectGuard } from './personal-session'
import { PersonalProbe,type PersonalProbeEvidence } from './personal-probe'
import { prepareIdentity,prepareOperation,personalHashBytes } from './personal-prepare-wire'
import { verifyInitialIntent,verifyStateObservation } from './personal-initial'
import { PersonalVerifier,personalDomain as D,personalHash,personalBoxSeal,verifyPersonalCeremony,type PersonalKind } from './personal-wire'
import type { PersonalManifest } from './personal-protocol'
import type { PersonalAttemptState } from '../../shared/fork-cloud'
const json=(v:unknown):string=>{const s=JSON.stringify(v);if(s===undefined)throw new Error('personalOriginalInvalid');return s}
const equal=(a:unknown,b:unknown)=>json(a)===json(b)
const names:Readonly<Record<string,PersonalKind>>={intent:'intent',ticket:'local-ticket',appConsent:'app-consent',localConsent:'local-consent',boxChallenge:'box-challenge',boxProof:'box-proof',approval:'approve',localConfirmation:'local-confirm',consume:'consume',grant:'grant',challenge:'challenge',proof:'proof',receipt:'receipt',fence:'fence',fenceAck:'fence-ack',revocation:'revocation',publish:'publish',activation:'activation'}
type ReadResult={state:readonly unknown[];raw:unknown;request:unknown;originals:Record<string,unknown>;activationCurrent:unknown|null;observation?:CurrentPersonalObservation}
export class PersonalService {
  private capture:PersonalCapture|null=null
  private journal:PersonalJournal|null=null
  private readonly storage:PersonalJournalStore
  private readonly clock:PersonalClock
  private possession:PersonalPossession|null=null
  private prepare:PersonalPrepare|null=null
  private probe:PersonalProbe|null=null
  private pinStore:import('./personal-pin-store').PersonalPinStore|null=null
  private pinned:import('./personal-pinned').PersonalPinnedSession|null=null
  private busy=false
  private alive=true
  private computer:string|null=null
  private approvalAt:number|null=null
  private readonly cut=()=>this.stop()
  private constructor(private readonly p:Principal,private readonly binding:Readonly<DeviceBinding>,private readonly signer:DeviceSigner,private readonly authority:PersonalManifest,private readonly profile:(id:string)=>PersonalSshProfile|null,private readonly elapsed:PersonalElapsed){
    this.clock=new PersonalClock(elapsed);this.storage=new PersonalJournalStore(p,binding);powerMonitor.on('suspend',this.cut);powerMonitor.on('resume',this.cut)
  }
  static async create(account:AccountService,p:Principal,profile:(id:string)=>PersonalSshProfile|null):Promise<PersonalService>{
    const authority=personalAuthority(p),binding=account.verifiedDeviceBinding(p.seal)
    if(!authority || !binding || !p.current())throw new Error('personalUnavailable')
    const original=Object.freeze({...binding}),fingerprint=json(original)
    const current=()=>p.current() && json(account.verifiedDeviceBinding(p.seal))===fingerprint
    const principal:Principal=Object.freeze({...p,seal:p.seal,deployment:Object.freeze({...p.deployment}),current,async token(){if(!current())throw new Error('personalBindingChanged');const token=await p.token();if(!current())throw new Error('personalBindingChanged');return token}})
    const signer=await deviceSigner(principal,'read');if(!current() || b64url(signer.publicKey)!==original.deviceKey)throw new Error('personalBindingChanged')
    const elapsed=await qualifyPersonalRuntime(current);if(!current()){elapsed.close();throw new Error('personalBindingChanged')}
    return new PersonalService(principal,original,signer,authority,profile,elapsed)
  }
  static async restore(account:AccountService,p:Principal,profile:(id:string)=>PersonalSshProfile|null):Promise<PersonalService|null>{
    const service=await this.create(account,p,profile)
    try{service.journal=await service.storage.load();service.guard();if(!service.journal){service.stop();return null}await service.restorePinned();return service}catch(e){service.stop();throw e}
  }
  private async restorePinned(){
    if(this.j().phase!=='receipt' || !this.has('local-ticket'))return
    const {personalPinNativeAdmitted}=await import('./personal-pin-native');this.guard()
    if(!personalPinNativeAdmitted())return
    const {openPersonalPinStore}=await import('./personal-pin-main');this.guard()
    const {PersonalPinnedSession}=await import('./personal-pinned');this.guard()
    this.pinStore=await openPersonalPinStore(this.p,this.binding,this.authority,()=>this.guard());this.guard()
    const computer=(this.original('local-ticket') as unknown[][])[0][6];if(typeof computer!=='string')throw new Error('personalHubChanged')
    const g={check:async()=>{this.guard()},current:()=>this.guard()}
    let pin:import('./personal-pin-history').HistoricalPinEvidence|null=null
    try{pin=await this.pinStore.loadHistorical(computer,g)}catch(e){if(!(e && typeof e==='object' && (e as {code?:unknown}).code==='EPROOFPERM'))throw e}
    this.guard()
    if(pin){this.pinned=new PersonalPinnedSession(this.p,this.binding,this.signer,this.authority,this.elapsed,this.clock,this.pinStore,pin);return}
    const recovery=await this.pinStore.loadRecovery(computer,g);this.guard()
    if(recovery)this.pinned=new PersonalPinnedSession(this.p,this.binding,this.signer,this.authority,this.elapsed,this.clock,this.pinStore,recovery.history,recovery.record)
  }
  stop(){if(!this.alive)return;this.alive=false;this.clock.cut();this.elapsed.close();this.prepare?.close();this.possession?.close();this.probe?.close();this.pinned?.close();this.pinStore?.close();this.capture?.close();powerMonitor.off('suspend',this.cut);powerMonitor.off('resume',this.cut)}
  current(){return this.alive && this.p.current()}
  private guard(){try{this.elapsed.current();if(!this.current() || b64url(this.signer.publicKey)!==this.binding.deviceKey)throw new Error('personalBindingChanged');if(this.capture && !this.capture.current())throw new Error('personalCaptureChanged')}catch(e){this.stop();throw e}}
  private j(){if(!this.journal)throw new Error('personalJournalMissing');return this.journal}
  private original(name:string):unknown{const raw=this.j().originals[name];if(raw===undefined)throw new Error('personalOriginalMissing');return JSON.parse(raw)}
  private has(name:string){return this.j().originals[name]!==undefined}
  private packet(){const p:Record<string,unknown>={};for(const[k,v]of Object.entries(this.j().originals))p[k]=JSON.parse(v);return p}
  private verifier(){const q=this.original('local-ticket') as unknown[][];return new PersonalVerifier(this.authority.context,{issuer:this.authority.issuerPublicKey,app:this.binding.deviceKey,hub:q[0][7] as string,box:q[0][8] as string})}
  private now(){if(!this.computer)throw new Error('personalTimeUnavailable');return this.clock.validity(this.computer)}
  private async issued(predecessor=0,until=Number.MAX_SAFE_INTEGER):Promise<number>{
    for(;;){await this.checked();if(!this.computer)throw new Error('personalTimeUnavailable');try{return this.clock.issuedAfter(this.computer,predecessor,until)}catch(e){if(!(e instanceof Error) || e.message!=='personalTimeNotCausal')throw e}await new Promise<void>(resolve=>setTimeout(resolve,this.elapsed.quantum))}
  }
  private ceremony(historical=false){return verifyPersonalCeremony(this.verifier(),Buffer.from(json(this.packet())),historical?{kind:'historical',now:this.now()}:{kind:'current',now:this.now()})}
  private stamp(kind:PersonalKind,p:readonly unknown[],sample?:EpochWindow){
    const indices:Partial<Record<PersonalKind,readonly [number,number]>>={create:[5,6],attach:[5,6],'app-consent':[5,6],'box-challenge':[9,10],approve:[7,8],proof:[7,8],read:[6,7],cancel:[5,6],publish:[9,10],revoke:[7,8]}
    const pair=indices[kind];if(!pair || !this.computer)throw new Error('personalOperationInvalid');const w=sample??this.clock.window(this.computer),at=p[pair[0]],until=p[pair[1]]
    if(typeof at!=='number' || typeof until!=='number' || !Number.isSafeInteger(at) || !Number.isSafeInteger(until) || at>w.lower || w.upper>=until)throw new Error('personalTimeExpired')
    return w
  }
  private signed(kind:PersonalKind,fields:readonly unknown[]):readonly [readonly unknown[],string]{this.guard();const p=[D(kind),this.authority.context,...fields];this.stamp(kind,p);const s=b64url(this.signer.sign(Buffer.from(json(p))));this.guard();this.stamp(kind,p);return[p,s]}
  private captureWindow(capture:PersonalCapture|null):EpochWindow {
    this.guard();if(!capture || this.capture!==capture || !capture.current())throw new Error('personalCaptureChanged')
    if(!this.computer)throw new Error('personalTimeUnavailable')
    const w=this.clock.window(this.computer);capture.validateAt(w.upper)
    if(this.journal?.facts.origin && capture.provenance()!==this.journal.facts.origin)throw new Error('personalOriginChanged')
    return w
  }
  private bodyGuard(route:PersonalAppRoute,body:Readonly<Record<string,unknown>>):PersonalAppGuard {
    const captured=this.capture,kind:Record<PersonalAppRoute,PersonalKind>={command:'create',attach:'attach','app-consent':'app-consent','box-challenge':'box-challenge',approve:'approve',read:'read',cancel:'cancel',publish:'publish',revoke:'revoke'},field:Record<PersonalAppRoute,string>={command:'create',attach:'attach','app-consent':'appConsent','box-challenge':'boxChallenge',approve:'approval',read:'read',cancel:'cancel',publish:'publish',revoke:'revoke'}
    const envelope:unknown=JSON.parse(json(body[field[route]]))
    return{check:()=>this.checked(captured),sample:()=>{const w=this.captureWindow(captured);if(!Array.isArray(envelope) || !Array.isArray(envelope[0]))throw new Error('personalOriginalMissing');return this.stamp(kind[route],envelope[0],w)}}
  }

  private async remember(name:string,value:unknown,phase=this.j().phase){
    const captured=this.capture,added=!this.has(name),raw=json(value),original:unknown=JSON.parse(raw)
    const guard:PersonalJournalGuard={check:()=>this.checked(captured),current:()=>{const w=this.captureWindow(captured);if(added && ['create','attach','app-consent','box-challenge','approve','proof','cancel','publish','revoke'].includes(name)){if(!Array.isArray(original) || !Array.isArray(original[0]))throw new Error('personalOriginalMissing');this.stamp(name as PersonalKind,original[0],w)}}}
    await guard.check();guard.current()
    this.journal=await this.storage.advance(this.j(),phase,{...this.j().originals,[name]:raw},this.j().sending,{},guard)
    await guard.check();guard.current()
  }
  private async saveSecret(secret:PersonalJournal['secret'],facts=this.j().facts,proof?:string){this.journal=await this.storage.advance(this.j(),this.j().phase,this.j().originals,this.j().sending,{secret,facts,clearProof:proof});this.guard()}
  private async phase(phase:PersonalJournal['phase']){this.journal=await this.storage.advance(this.j(),phase);this.guard()}
  private async mark(route:string,body:string){const j=this.j();if(j.sending || j.sent[route])throw new Error('personalOutcomeUnknown');this.journal=await this.storage.advance(j,'unknown',j.originals,{operationId:b64url(random(16)),route,body},{sent:{...j.sent,[route]:personalHashBytes(Buffer.from(body))}});this.guard()}
  private async resolve(phase:PersonalJournal['phase']='running'){this.journal=await this.storage.advance(this.j(),phase,this.j().originals,null);this.guard()}
  private async checked(capture=this.capture){this.guard();if(!capture)throw new Error('personalCaptureMissing');if(this.capture!==capture)throw new Error('personalCaptureChanged');await capture.check();this.guard();if(this.capture!==capture || !capture.current())throw new Error('personalCaptureChanged');if(this.computer)capture.validateAt(this.now());if(this.journal?.facts.origin && capture.provenance()!==this.journal.facts.origin)throw new Error('personalOriginChanged')}
  // Fecha sobre a captura original; nenhum envio requalifica outra fonte por conta própria.
  private effectGuard(){const capture=this.capture;if(!capture)throw new Error('personalCaptureMissing');return()=>this.checked(capture)}
  private async helperEffect(request:Exclude<HelperRequest,readonly ['time']>){const capture=this.capture,original=this.effectGuard(),deadline=this.elapsed.sample().lo+5000,guard=async()=>{await original();if(this.elapsed.sample().hi>=deadline)throw new Error('personalHelperUnknown')};await guard();const reply=await capture!.helper.request(request);await guard();return reply}
  private async qualify(){
    this.guard();const j=this.j()
    if(!this.capture){
      const profile=this.profile(j.sourceId),fingerprint=json(profile),p=this.p,current=()=>this.current() && json(this.profile(j.sourceId))===fingerprint
      const captured:Principal=Object.freeze({...p,current,async token(){if(!current())throw new Error('personalBindingChanged');const token=await p.token();if(!current())throw new Error('personalBindingChanged');return token}})
      this.capture=await capturePersonal(captured,j.sourceId,profile);this.guard();if(process.platform==='linux' && this.capture.kind!=='localHelper')throw new Error('personalOriginUnavailable');this.clock.cut()
    }
    let renew=false;try{this.now()}catch{renew=true}
    if(renew){const flight=this.clock.begin(),time=await this.capture.helper.request(['time']);await this.capture.check();this.guard()
      if(!Array.isArray(time) || !Array.isArray(time[0]) || typeof time[0][3]!=='string')throw new Error('personalTimeInvalid')
      const computer=time[0][3];if(this.computer && this.computer!==computer || this.has('local-ticket') && (this.original('local-ticket') as unknown[][])[0][6]!==computer)throw new Error('personalHubChanged')
      this.computer=computer;this.clock.accept(this.authority,time,computer,flight);this.capture.validateAt(this.now())
    }
    const proof=this.capture.provenance()
    if(j.facts.origin!==null && j.facts.origin!==proof)throw new Error('personalOriginChanged')
    if(j.facts.origin===null){this.journal=await this.storage.advance(this.j(),this.j().phase,this.j().originals,this.j().sending,{facts:{...this.j().facts,origin:proof}});this.guard()}
    await this.checked()
  }
  snapshot():PersonalAttemptState|null{if(!this.journal || !this.current())return null;const j=this.journal,phase=!this.capture && !['receipt','closed','stopped'].includes(j.phase)?'unknown':j.phase
    const canApprove=phase==='awaitingApproval' && !this.busy && !j.sending && this.approvalAt!==null && performance.now()-this.approvalAt<5000
    return Object.freeze({attemptId:j.attemptId,mode:j.mode,phase,canApprove,canCheck:!this.busy && !['closed','stopped'].includes(phase),canReconcile:!this.busy && !['closed','stopped','receipt'].includes(phase),...(this.computer?{computerId:this.computer}:{})})
  }
  private async exclusive(action:()=>Promise<void>){this.guard();if(this.busy)throw new Error('personalBusy');this.busy=true;try{await action();this.guard()}catch(e){if(this.journal && this.current() && !['closed','stopped','receipt'].includes(this.journal.phase)){try{await this.phase('unknown')}catch{}}throw e}finally{this.busy=false}}
  private async request(route:PersonalAppRoute,body:Readonly<Record<string,unknown>>){await this.checked();await this.mark(route,json(body));await this.checked();const reply=await personalAppPost(this.p,this.signer,route,body,this.bodyGuard(route,body));await this.checked();return reply}
  private validateIds(reply:Record<string,unknown>){const c=this.ceremony(true);if(Object.keys(reply).length!==2 || reply.intentId!==c.tuple[1] || reply.candidateId!==c.tuple[9])throw new Error('personalReplyMismatch')}
  private async workerOriginal(route:'app-consent'|'box-challenge'|'approve',field:string,name:string){this.ceremony();const response=await this.request(route,{[field]:this.original(name)});this.validateIds(response);await this.resolve()}
  private async publishReceipt(observed:ReadResult):Promise<ReadResult>{
    if(!this.has('receipt'))throw new Error('personalReceiptMissing')
    // R local não é prova de que o Worker recebeu R. Unknown só consulta.
    if(!observed.originals.receipt)return observed
    if(!equal(observed.originals.receipt,this.original('receipt')))throw new Error('personalReceiptChanged')
    if(this.j().sending)return observed
    if(observed.originals.publish){
      if(!this.has('publish') || !equal(observed.originals.publish,this.original('publish')))throw new Error('personalPublicationChanged')
      return observed
    }
    if(this.j().sent.publish)return observed
    const c=this.ceremony(true),R=c.originals.receipt!,G=c.originals.grant!
    if(observed.state[5]!=='hubPending' || observed.state[12]!==null || observed.state[9]!==R.hash || observed.state[8]!==G.hash || observed.state[7]!==R.payload[6])return observed
    if(!this.has('publish')){
      const now=await this.issued(R.payload[7] as number,Math.min(G.payload[10] as number,c.tuple[18] as number))
      await this.remember('publish',this.signed('publish',[c.tuple[1],c.hash,G.hash,R.payload[6],R.hash,observed.state[6],b64url(random(16)),now,Math.min(now+30000,G.payload[10] as number,c.tuple[18] as number)]))
    }
    const response=await this.request('publish',{publish:this.original('publish')});this.validateIds(response);await this.resolve('receipt')
    return this.read()
  }
  private async probePublished(observed:ReadResult):Promise<PersonalProbeEvidence>{
    if(!observed.observation)throw new Error('personalObservationMissing')
    const first=requireLivePersonalObservation(observed.observation),capture=this.capture
    const effect=ceremonyEffect({check:()=>this.checked(capture),sample:()=>this.captureWindow(capture),current:()=>{this.guard();if(!capture || capture!==this.capture || !capture.current())throw new Error('personalCaptureChanged')}})
    this.probe=new PersonalProbe(this.p,this.binding,this.signer,this.authority,this.elapsed,effect,this.packet(),first,async guard=>{const next=await this.read(guard);if(!next.observation)throw new Error('personalObservationMissing');return next.observation})
    return this.probe.open()
  }
  private async observePublished(observed:ReadResult,recover=false){
    const published=await this.publishReceipt(observed)
    if(!published.observation || published.activationCurrent===null || published.state[5]!=='hubActive' || this.j().sending)return
    // Resolve implementation before the probe starts; every await still uses
    // the original ceremony capture. No lazy load resets a live probe budget.
    const capture=this.capture
    const {openPersonalPinStore}=await import('./personal-pin-main');await this.checked(capture)
    const {signPersonalPin}=await import('./personal-pin');await this.checked(capture)
    const {PersonalPinnedSession}=await import('./personal-pinned');await this.checked(capture)
    this.pinStore??=await openPersonalPinStore(this.p,this.binding,this.authority,()=>this.guard());await this.checked(capture)
    if(!this.computer)throw new Error('personalTimeUnavailable')
    // A durable/partial prior operation is inspected BEFORE signing a new P30.
    // Unknown is never repaired by a new nonce/PIN/cipher/operationId.
    const previous=await this.pinStore.loadRecovery(this.computer,{check:()=>this.checked(capture),current:()=>this.guard()});await this.checked(capture)
    if(previous){
      this.guard();if(this.capture!==capture)throw new Error('personalCaptureChanged')
      this.capture=null;capture?.close();this.clock.cut();this.pinned?.close()
      this.pinned=new PersonalPinnedSession(this.p,this.binding,this.signer,this.authority,this.elapsed,this.clock,this.pinStore,previous.history,previous.record)
      if(recover){const result=await this.pinned.recover(previous.record);if(result.kind!=='stored')throw new Error('personalPinOutcomeUnknown');await this.pinSettled()}
      await this.pinned.refresh();return
    }

    let stored:import('./personal-pin-history').HistoricalPinEvidence|null=null,reserved=false
    try{
      const evidence=await this.probePublished(published);evidence.current()
      if(!capture || capture!==this.capture)throw new Error('personalCaptureChanged')
      const history=await signPersonalPin(this.p,this.binding,this.authority,this.signer,this.j().attemptId,this.j().sourceId,capture.originRecord(),evidence)
      // From prepare on, a durable PIN record may exist: failure is unknown, never notSent.
      reserved=true;const prepared=await this.pinStore.prepare(history,evidence),result=await this.pinStore.commit(prepared,evidence)
      if(result.kind!=='stored')throw new Error('personalPinOutcomeUnknown')
      evidence.current();stored=result.history
    }catch(e){if(reserved)await this.pinUnknown();throw e}finally{this.probe?.close();this.probe=null}
    if(!stored)throw new Error('personalPinOutcomeUnknown')
    this.guard();if(this.capture!==capture)throw new Error('personalCaptureChanged')
    // Retire only after durable commit; first probe is closed BEFORE new Read/Open.
    this.capture=null;capture?.close();this.clock.cut()
    this.pinned?.close();this.pinned=new PersonalPinnedSession(this.p,this.binding,this.signer,this.authority,this.elapsed,this.clock,this.pinStore,stored)
    await this.pinned.refresh()

  }
  async start(mode:'command'|'device',sourceId:string){return this.exclusive(async()=>{
    if(this.journal || await this.storage.load())throw new Error('personalJournalExists');this.guard();this.journal=await this.storage.create(mode,sourceId);await this.qualify()
    if(mode==='command'){
      const now=await this.issued(),create=this.signed('create',[`pi_${b64url(random(16))}`,b64url(random(32)),'command',now,now+600000]);await this.remember('create',create)
      const response=await this.request('command',{create});if(Object.keys(response).sort().join(',')!=='intent,token' || !unb64url(response.token,32))throw new Error('personalCommandInvalid')
      const I=verifyInitialIntent(this.authority,this.binding,response.intent,this.now());if(I[2]!==create[0][2] || I[3]!=='command' || I[7]!==create[0][3])throw new Error('personalCommandMismatch')
      const captured=this.capture,rawIntent=json(response.intent),guard:PersonalJournalGuard={check:()=>this.checked(captured),current:()=>{const w=this.captureWindow(captured);verifyInitialIntent(this.authority,this.binding,JSON.parse(rawIntent),w.upper)}}
      const j=this.j();this.journal=await this.storage.advance(j,'running',{...j.originals,intent:rawIntent},null,{secret:{...j.secret,commandToken:response.token as string}},guard);this.guard()
    }
    await this.obtainTicket()
    if(mode==='command')await this.prepareCandidate();else await this.attachDevice()
    const observed=await this.read();if(!this.isPreparatory(observed))throw new Error('personalNotPreparable');this.approvalAt=performance.now();await this.phase('awaitingApproval')
  })}
  private async obtainTicket(){
    if(this.has('local-ticket'))return
    const mode=this.j().mode,I=mode==='command'?this.original('intent'):null;await this.checked();await this.mark('helper-ticket',json([mode,I]))
    const Q=await this.helperEffect(['ticket',mode,I]);await this.checked()
    const q=Q as unknown[][],v=new PersonalVerifier(this.authority.context,{issuer:this.authority.issuerPublicKey,app:this.binding.deviceKey,hub:q?.[0]?.[7] as string,box:q?.[0]?.[8] as string})
    const checked=v.verify('local-ticket',Buffer.from(json(Q)),{kind:'current',now:this.now()})
    if(checked.payload[3]!==mode || checked.payload[6]!==this.computer || mode==='command' && checked.payload[2]!== (I as unknown[][])[0][2])throw new Error('personalTicketMismatch')
    await this.remember('local-ticket',Q);await this.resolve()
  }
  private async exchange(request:readonly unknown[]){const capture=this.capture;await this.checked(capture);this.prepare=new PersonalPrepare(capture!,()=>this.current(),()=>this.elapsed.sample(),()=>this.captureWindow(capture));try{return await this.prepare.exchange(this.authority.context,this.original('local-ticket'),request,this.signer,()=>this.checked(capture))}finally{this.prepare.close();this.prepare=null}}
  private async prepareCandidate(){
    const j=this.j(),Q=this.original('local-ticket'),id=prepareIdentity(this.authority.context,Q)
    if(j.facts.candidate)return
    let request:readonly unknown[]
    if(j.secret.candidateRequest){request=JSON.parse(j.secret.candidateRequest)}else{
      if(!j.secret.commandToken)throw new Error('personalCandidateSecretMissing')
      const body=`{"ticket":${json(Q)},"token":${json(j.secret.commandToken)}}`,digest=personalHashBytes(Buffer.from(body)),op=prepareOperation(this.authority.context,id.capture,id.hq,id.wireHash,digest)
      request=[D('prepare-request'),1,b64url(random(16)),id.hq,'candidate',op,digest,j.secret.commandToken]
      await this.saveSecret({...j.secret,candidateBody:body,candidateRequest:json(request)})
    }
    if(!this.j().sending)await this.mark('candidate',json(request.slice(0,7)))
    else if(this.j().sending!.route!=='candidate')throw new Error('personalOtherOutcomeUnknown')
    const reply=await this.exchange(request);if(reply.length!==6 || reply[0]!==id.hq || reply[1]!==id.wireHash || reply[2]!==personalHash((this.original('intent') as unknown[][])[0]) || reply[3]!== (Q as unknown[][])[0][2] || reply[4]!== (Q as unknown[][])[0][4] || reply[5]!==request[6])throw new Error('personalCandidateMismatch')
    this.journal=await this.storage.advance(this.j(),'running',this.j().originals,null,{facts:{...this.j().facts,candidate:json(reply)}});this.guard()
  }
  private async attachDevice(){
    if(this.has('intent'))return
    let facts=this.j().facts.enrollment?JSON.parse(this.j().facts.enrollment!) as unknown[]:null
    if(!facts){const Q=this.original('local-ticket'),id=prepareIdentity(this.authority.context,Q),request=[D('prepare-request'),1,b64url(random(16)),id.hq,'enrollment'],r=await this.exchange(request)
      if(r.length!==8 || r[0]!==id.hq || r[1]!==id.wireHash || r[2]!== (Q as unknown[][])[0][2] || r[3]!== (Q as unknown[][])[0][4] || typeof r[4]!=='string' || !/^[A-Z2-7]{12}$/.test(r[4]) || !Number.isSafeInteger(r[5]) || !Number.isSafeInteger(r[6]) || (r[6] as number)<=(r[5] as number) || r[7]!==5000)throw new Error('personalEnrollmentMismatch')
      facts=[r[0],r[1],r[2],r[3],r[5],r[6],r[7]];await this.saveSecret({...this.j().secret,userCode:r[4]},{...this.j().facts,enrollment:json(facts)})
    }
    if(!this.has('attach')){await this.issued(facts[4] as number,facts[5] as number);const attach=this.signed('attach',[facts[2],facts[0],b64url(random(32)),facts[4],facts[5]]);await this.remember('attach',attach)}
    const response=await this.request('attach',{attach:this.original('attach'),ticket:this.original('local-ticket'),userCode:this.j().secret.userCode})
    if(Object.keys(response).join(',')!=='intent')throw new Error('personalAttachResponse')
    const I=verifyInitialIntent(this.authority,this.binding,response.intent,this.now()),attach=(this.original('attach') as unknown[][])[0]
    if(I[2]!==facts[2] || I[3]!=='device' || I[7]!==attach[4] || I[8]!==facts[4] || I[9]!==facts[5])throw new Error('personalAttachMismatch')
    await this.remember('intent',response.intent);this.ceremony();await this.resolve()
  }
  private async read(probe?:PersonalEffectGuard):Promise<ReadResult>{
    await this.checked();if(probe)await probe.check();const target=this.has('intent')&&this.has('local-ticket')?this.ceremony(true):null
    const intentId=target?.tuple[1]??(this.has('create')?(this.original('create') as unknown[][])[0][2]:(this.original('local-ticket') as unknown[][])[0][2])
    const now=await this.issued(),read=this.signed('read',[intentId,target?.tuple[9]??null,target?.hash??null,b64url(random(32)),now,now+30000])
    const original=this.bodyGuard('read',{read}),guard:PersonalAppGuard=probe?{check:async()=>{await original.check();await probe.check()},sample:()=>this.stamp('read',read[0],probe.sample())}:original
    const reply=await personalAppPost(this.p,this.signer,'read',{read},guard);await this.checked();if(probe)await probe.check();const state=verifyStateObservation(this.authority,reply.state,read[0],probe?probe.sample().upper:this.now())
    const originals=reply.originals as Record<string,unknown>;if(!originals || typeof originals!=='object' || Array.isArray(originals))throw new Error('personalReadShape')
    if(originals.intent){const I=verifyInitialIntentHistorical(this.authority,this.binding,originals.intent,probe?probe.sample().upper:this.now());if(I[2]!==intentId)throw new Error('personalReadIntentMismatch');if(this.has('intent')){if(!equal(this.original('intent'),originals.intent))throw new Error('personalIntentChanged')}else{this.validateRecoveredIntent(I);await this.remember('intent',originals.intent)}}
    if(this.has('intent') && this.has('local-ticket')){
      const merged=Object.fromEntries(Object.entries(this.packet()).filter(([name])=>!['activation','host-state-request','host-state'].includes(name)))
      for(const[remote,local]of Object.entries(names))if(originals[remote]!==null && originals[remote]!==undefined){
        if(['activation','fence','fence-ack','revocation'].includes(local)){merged[local]=originals[remote];continue}
        if(this.has(local) && !equal(this.original(local),originals[remote]))throw new Error('personalOriginalChanged')
        if(['app-consent','box-challenge','approve','proof','cancel','publish'].includes(local) && !this.has(local))throw new Error('personalOwnOriginalMissing')
        merged[local]=originals[remote]
      }
      verifyPersonalCeremony(this.verifier(),Buffer.from(json(read[0][3]!==null?{...merged,state:reply.state,read}:merged)),{kind:'historical',now:probe?probe.sample().upper:this.now()})
      for(const[remote,local]of Object.entries(names))if(!['activation','fence','fence-ack','revocation'].includes(local) && originals[remote]!==null && originals[remote]!==undefined && !this.has(local))await this.remember(local,originals[remote])
    }
    await this.confirmSending(originals,state)
    const result:ReadResult={state,raw:reply.state,request:read,originals,activationCurrent:reply.activationCurrent??null}
    if(this.has('receipt'))result.observation=verifyPersonalObservation(this.authority,this.binding,this.packet(),read,reply,probe?probe.sample():this.captureWindow(this.capture))
    if(['canceled','expired'].includes(state[5] as string) && state[8]===null && !this.has('grant') && !this.has('receipt') && !this.j().sending){const proof=personalHash([read,reply.state]),secret=emptySecrets();this.journal=await this.storage.advance(this.j(),'closed',this.j().originals,null,{secret,clearProof:proof,facts:{...this.j().facts,closure:json({read,state:reply.state})}});this.guard();return result}
    if((this.isPreparatory(result) && this.j().facts.candidate) || (this.has('intent') && this.j().mode==='device')){
      const secret={...this.j().secret};if(this.j().facts.candidate){secret.commandToken=null;secret.candidateBody=null;secret.candidateRequest=null}if(this.has('intent'))secret.userCode=null
      if(!equal(secret,this.j().secret))await this.saveSecret(secret,this.j().facts,personalHash([read,reply.state]))
    }
    if(probe)await probe.check();return result
  }
  private validateRecoveredIntent(I:readonly unknown[]){
    if(this.j().mode==='command'){const create=(this.original('create') as unknown[][])[0];if(I[3]!=='command' || I[7]!==create[3])throw new Error('personalRecoveredIntentMismatch')}
    else{const attach=(this.original('attach') as unknown[][])[0],facts=JSON.parse(this.j().facts.enrollment??'null') as unknown[]|null;if(!facts || I[3]!=='device' || I[7]!==attach[4] || I[8]!==facts[4] || I[9]!==facts[5])throw new Error('personalRecoveredAttachMismatch')}
  }
  private async confirmSending(originals:Record<string,unknown>,state:readonly unknown[]){
    const sending=this.j().sending;if(!sending)return
    const map:Record<string,string>={'app-consent':'appConsent','box-challenge':'boxChallenge',approve:'approval','helper-consent':'localConsent',attach:'intent',command:'intent',possession:'receipt','helper-retain-L':'localConfirmation',publish:'publish'}
    const remote=map[sending.route]
    if(remote && originals[remote]){
      if(sending.route==='command' && !this.j().secret.commandToken)return // O token da resposta perdida não é regenerável.
      if(sending.route==='helper-retain-L'){if(!this.has('local-confirm') || !equal(this.original('local-confirm'),originals.localConfirmation))return}
      else if(sending.route==='helper-consent'){if(!this.has('local-consent'))return}else if(sending.route!=='attach' && sending.route!=='command' && sending.route!=='possession'){
        const local=names[remote];if(!local || !this.has(local) || !equal(this.original(local),originals[remote]))return
      }
      await this.resolve(['possession','publish'].includes(sending.route)?'receipt':'running');return
    }
    if(['canceled','expired'].includes(state[5] as string) && state[8]===null)await this.resolve()
  }
  private isPreparatory(read:ReadResult){return this.has('intent') && this.has('local-ticket') && read.state[3]!==null && read.state[4]===this.ceremony(true).hash && ['open','personalConfirmed'].includes(read.state[5] as string) && read.state.slice(7,14).every(v=>v===null) && !this.j().sending && !this.has('grant')}
  async approve(attemptId:string){return this.exclusive(async()=>{
    if(this.j().attemptId!==attemptId || this.j().phase!=='awaitingApproval')throw new Error('personalApprovalUnavailable');await this.qualify();const observed=await this.read();if(!this.isPreparatory(observed))throw new Error('personalNotPreparable')
    const c=this.ceremony(),now=await this.issued(Math.max((this.original('intent') as unknown[][])[0][8] as number,(this.original('local-ticket') as unknown[][])[0][11] as number),c.tuple[18] as number);if(!this.has('app-consent'))await this.remember('app-consent',this.signed('app-consent',[c.hash,'independent-identity-v1','approve',now,c.tuple[18]]))
    if(!this.j().sent['app-consent'])await this.workerOriginal('app-consent','appConsent','app-consent');await this.drive()
  })}
  private async consent(){
    if(this.has('local-consent'))return
    await this.checked();await this.mark('helper-consent',json([this.original('intent'),this.original('local-ticket'),this.original('app-consent')]))
    const CH=await this.helperEffect(['consent',this.original('intent'),this.original('local-ticket'),this.original('app-consent')]);await this.checked()
    const packet={...this.packet(),'local-consent':CH};verifyPersonalCeremony(this.verifier(),Buffer.from(json(packet)),{kind:'current',now:this.now()});await this.remember('local-consent',CH);await this.resolve()
  }
  private async box(){
    if(!this.has('box-challenge')){const c=this.ceremony(),CA=c.originals['app-consent']!,CH=c.originals['local-consent']!,now=await this.issued(Math.max(CA.payload[5] as number,CH.payload[6] as number),Math.min(c.tuple[18] as number,CA.payload[6] as number,CH.payload[7] as number)),e=ephemeral(),secret=b64url(random(32)),nonce=b64url(random(32)),until=Math.min(now+60000,c.tuple[18] as number,CA.payload[6] as number,CH.payload[7] as number)
      const aad=[D('box-aad'),this.authority.context,c.hash,CA.hash,CH.hash,nonce,b64url(e.pub),personalHash([D('box-secret-hash'),secret]),now,until],cipher=personalBoxSeal(e,c.tuple[13] as string,aad,secret)
      await this.remember('box-challenge',this.signed('box-challenge',[c.hash,CA.hash,CH.hash,nonce,b64url(e.pub),aad[7],cipher,now,until]));this.ceremony()
    }
    if(!this.j().sent['box-challenge'])await this.workerOriginal('box-challenge','boxChallenge','box-challenge')
  }
  private async approval(){
    if(!this.has('approve')){const c=this.ceremony(),P=c.originals['box-proof']!,B=c.originals['box-challenge']!,now=await this.issued(Math.max(P.payload[6] as number,B.payload[9] as number),Math.min(P.payload[7] as number,B.payload[10] as number));await this.remember('approve',this.signed('approve',[c.hash,B.hash,P.hash,'independent-identity-v1','approve',now,Math.min(P.payload[7] as number,B.payload[10] as number)]))}
    if(!this.j().sent.approve)await this.workerOriginal('approve','approval','approve')
  }
  private async tick(){
    const read=await this.read();if(['closed','stopped','receipt'].includes(this.j().phase))return false
    if(this.j().sending)return false
    if(this.has('grant')){await this.possess(false);return false}
    if(!this.has('app-consent')){if(this.isPreparatory(read)){this.approvalAt=performance.now();await this.phase('awaitingApproval')}return false}
    this.ceremony()
    if(!this.has('local-consent')){if(!this.j().sent['helper-consent'])await this.consent();else return false}
    if(!this.has('box-challenge') || !this.j().sent['box-challenge'])await this.box()
    if(this.has('box-proof') && (!this.has('approve') || !this.j().sent.approve))await this.approval()
    // Helper conserva a sessão privada; ausência desse contexto não vira lookup global.
    const c=this.ceremony(),status=await this.helperEffect(['status',c.hash]);await this.checked()
    if(!Array.isArray(status) || status.length!==7 || status[1]!==c.hash || !equal(status[2],this.original('local-ticket')))throw new Error('personalHelperStatusMismatch')
    if(status[3]!==null && !equal(status[3],this.original('local-consent')))throw new Error('personalHelperConsentMismatch')
    if(status[4]!==null){const packet={...this.packet(),'local-confirm':status[4]};verifyPersonalCeremony(this.verifier(),Buffer.from(json(packet)),{kind:'current',now:this.now()});if(!this.has('local-confirm'))await this.remember('local-confirm',status[4]);if(!this.j().sent['helper-retain-L'])await this.retainL()}
    await this.phase('running');return true
  }
  private async retainL(){
    await this.checked();const L=this.original('local-confirm'),c=this.ceremony();if(!this.j().sending)await this.mark('helper-retain-L',json(L));else if(this.j().sending!.route!=='helper-retain-L')throw new Error('personalOtherOutcomeUnknown')
    const ack=await this.helperEffect(['retain',L]);await this.checked();if(!Array.isArray(ack) || ack.length!==3 || ack[0]!=='localConfirmation' || ack[1]!==c.hash || ack[2]!==personalHash((L as unknown[][])[0]))throw new Error('personalRetainMismatch');await this.resolve()
  }
  // Post-R PIN failure: exclusive() keeps 'receipt', so project unknown locally, only
  // with valid Authority; a confirmed stored recovery returns to 'receipt'.
  private async pinUnknown(){if(!this.journal || !this.current() || this.j().phase!=='receipt')return;try{await this.phase('unknown')}catch{}}
  private async pinSettled(){const j=this.j();if(j.phase==='unknown' && !j.sending && this.has('receipt'))await this.phase('receipt')}
  private async drive(){const capture=this.capture;await this.checked(capture);for(let round=0;round<13;round++){if(!await this.tick())return;await new Promise<void>(resolve=>setTimeout(resolve,5000));await this.checked(capture)}throw new Error('personalProgressUnknown')}
  async check(attemptId:string){return this.exclusive(async()=>{if(this.j().attemptId!==attemptId)throw new Error('personalAttemptChanged');if(['closed','stopped'].includes(this.j().phase))return;if(this.pinned){await this.pinned.refresh();return}await this.qualify();const observed=await this.read();if(this.has('receipt')){await this.observePublished(observed);return}if(this.isPreparatory(observed) && !this.has('app-consent')){this.approvalAt=performance.now();await this.phase('awaitingApproval')}})}
  async reconcile(attemptId:string){return this.exclusive(async()=>{
    if(this.j().attemptId!==attemptId)throw new Error('personalAttemptChanged');if(['closed','stopped'].includes(this.j().phase))return;if(this.pinned){const record=await this.pinStore!.publication(this.pinned.pin.tuple[11] as string,{check:async()=>{this.guard()},current:()=>this.guard()});if(!record)throw new Error('personalPinRecordMissing');const result=await this.pinned.recover(record);if(result.kind!=='stored')throw new Error('personalPinOutcomeUnknown');await this.pinSettled();await this.pinned.refresh();return}await this.qualify()
    if(!this.has('local-ticket')){if(this.has('create'))await this.read();throw new Error('personalTicketContextUnavailable')}
    // Antes de attach não há I/Authority para Read da conta. Recupera somente
    // fatos públicos do MESMO Q; attach só se nunca houve envio possível dele.
    if(this.j().mode==='device' && !this.has('intent') && !this.j().sent.attach && !this.j().sending)await this.attachDevice()
    if(this.j().sending?.route==='candidate'){await this.prepareCandidate();await this.read()}
    else if(this.j().sending?.route==='helper-retain-L' && this.has('local-confirm')){await this.read();if(this.j().sending)await this.retainL()}
    else if(this.j().sending?.route==='possession' && this.has('proof')){await this.possess(true);return}
    else await this.read()
    if(this.has('receipt')){await this.observePublished(await this.read(),true);return}
    if(['stopped','closed'].includes(this.j().phase) || this.j().sending)return
    if(this.j().mode==='device' && !this.has('intent')){if(this.j().sent.attach)return;await this.attachDevice()}
    if(this.j().mode==='command' && !this.j().facts.candidate){await this.prepareCandidate();await this.read()}
    await this.drive()
  })}
  async cancel(attemptId:string){return this.exclusive(async()=>{
    if(this.j().attemptId!==attemptId)throw new Error('personalAttemptChanged');if(this.j().phase==='closed')return
    if(this.pinned){await this.pinned.revoke();return}
    if(!this.has('intent') && this.j().mode==='device'){
      const capture=this.capture;await this.phase('stopped');this.guard()
      if(this.capture!==capture){this.stop();throw new Error('personalCaptureChanged')}
      // Só o encerramento local já durável aposenta a referência validada.
      // Perda inesperada continua passando pelo guard B1, sem recaptura.
      this.capture=null;capture?.close();this.clock.cut();return
    }
    await this.qualify();const observed=await this.read();if(this.j().phase==='closed')return
    if(this.has('grant') || observed.state[8]!==null)throw new Error('personalConsumedNeedsFence')
    if(this.j().sending){
      // A resposta command perdida deixa o token indisponível. S atual confirma I,
      // permitindo somente seu cancelamento; nunca novo command/candidate.
      if(this.j().sending!.route!=='command' || !this.has('intent') || this.has('local-ticket') || observed.state[3]!==null || !['open','personalConfirmed'].includes(observed.state[5] as string))return
      await this.resolve()
    }
    if(!this.has('cancel')){const now=await this.issued(),c=this.has('intent')&&this.has('local-ticket')?this.ceremony(true):null,id=c?.tuple[1]??(this.original('create') as unknown[][])[0][2],revision=c?.tuple[16]??(this.original('intent') as unknown[][])[0][10];await this.remember('cancel',this.signed('cancel',[id,revision,b64url(random(32)),now,now+30000]))}
    if(!this.j().sent.cancel){const response=await this.request('cancel',{cancel:this.original('cancel')});if(response.intentId!==(this.original('cancel') as unknown[][])[0][2] || response.candidateId!==null)throw new Error('personalCancelMismatch');await this.resolve()}
    await this.read()
  })}
  private async possess(reconcile:boolean){
    const capture=this.capture;await this.checked(capture);if(!this.has('grant'))throw new Error('personalGrantMissing')
    const historical=reconcile && this.has('proof') && this.has('challenge'),c=this.ceremony(historical),G=this.original('grant');this.possession=new PersonalPossession(capture!,()=>this.current(),()=>this.elapsed.sample(),()=>this.captureWindow(capture))
    try{const channel=await this.possession.open(this.authority.context,c.tuple,G,this.signer,()=>this.checked(capture));await this.checked(capture);const K=await channel.challenge();await this.checked(capture)
      if(this.has('challenge')){if(!equal(this.original('challenge'),K))throw new Error('personalChallengeChanged')}else{if(historical)throw new Error('personalChallengeMissing');await this.remember('challenge',K)}
      const checked=this.ceremony(historical)
      if(!this.has('proof')){if(historical)throw new Error('personalProofMissing');const k=checked.originals.challenge!,now=await this.issued(Math.max(k.payload[5] as number,checked.originals.grant!.payload[9] as number),k.payload[6] as number);await this.remember('proof',this.signed('proof',[checked.originals.grant!.hash,k.hash,checked.hash,checked.originals.approve!.hash,checked.originals['local-confirm']!.hash,now,k.payload[6]]));this.ceremony()}
      if(!this.j().sending)await this.mark('possession',json([G,K,this.original('proof')]))
      else if(this.j().sending!.route!=='possession')throw new Error('personalOtherOutcomeUnknown')
      const R=await channel.proof(K,this.original('proof'));await this.checked();const packet={...this.packet(),receipt:R};verifyPersonalCeremony(this.verifier(),Buffer.from(json(packet)),{kind:'current',now:this.now()})
      await this.remember('receipt',R);await this.resolve('receipt')
    }finally{this.possession.close();this.possession=null}
  }
}
function emptySecrets():PersonalJournal['secret']{return{commandToken:null,candidateBody:null,candidateRequest:null,userCode:null}}
function verifyInitialIntentHistorical(authority:PersonalManifest,binding:Readonly<DeviceBinding>,I:unknown,now:number){
  const p=(I as unknown[][])?.[0],until=p?.[9]
  if(typeof until!=='number' || !Number.isSafeInteger(until))throw new Error('personalHistoricalIntent')
  // Somente assinatura/shape/binding históricos. Nenhuma ação usa esse instante como now.
  return verifyInitialIntent(authority,binding,I,Math.min(now,until-1))
}
