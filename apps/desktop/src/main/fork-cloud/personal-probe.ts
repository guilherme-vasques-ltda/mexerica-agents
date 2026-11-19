/** Relay pessoal fechado: handshake e exatamente hello/proof, sem Channel/Caller. */
import { b64url,random,unb64url,signatureHeader,deviceHello,sealFrame,openFrame,type DeviceSigner,type ChannelKeys } from './crypto'
import { personalDomain as D,PersonalVerifier,verifyPersonalCeremony } from './personal-wire'
import type { PersonalElapsed } from './personal-runtime'
import type { EpochWindow } from './personal-clock'
import type { Principal } from './principal'
import type { DeviceBinding } from '../fork-account/service'
import type { PersonalManifest } from './personal-protocol'
import { nodeSocket,type SocketLike,type SocketFactory } from './relay-socket'
import { personalBytes,personalSame,personalNeed,personalImmutable,personalOriginalNames,requireLivePersonalObservation,type CurrentPersonalObservation,type CeremonyEffect,type PinnedEffect,type PersonalEffectGuard } from './personal-session'

function exact(v:unknown,fields:readonly string[]):asserts v is Record<string,unknown>{personalNeed(v && typeof v==='object' && !Array.isArray(v) && Object.keys(v).length===fields.length && fields.every(k=>Object.hasOwn(v,k)))}
const evidenceBrand:unique symbol=Symbol('personalProbeEvidence')
export interface PersonalProbeEvidence {
  readonly [evidenceBrand]:true
  readonly nonce:string;readonly hello:Readonly<Record<string,unknown>>;readonly proof:Readonly<Record<string,unknown>>
  readonly originals:Readonly<Record<string,unknown>>;readonly observation:CurrentPersonalObservation
  check():Promise<void>;current():void;close():void
}

/** A referência é fixa enquanto este cliente existe; nenhum accept/renew aqui. */
class ProbeBudget {
  private readonly start:number
  private readonly until:number
  private readonly anchor:EpochWindow
  private readonly afterHi:number
  private end:number
  private expiry:number
  constructor(private readonly elapsed:PersonalElapsed,private readonly effect:CeremonyEffect|PinnedEffect,expiry:number){
    effect.current();this.start=elapsed.sample().lo;this.until=this.start+5000
    this.anchor=effect.sample();this.afterHi=elapsed.sample().hi
    const remaining=this.until-this.afterHi;this.end=this.anchor.lower+remaining;this.expiry=expiry
    personalNeed(Number.isSafeInteger(this.until) && Number.isSafeInteger(remaining) && remaining>0 && remaining<=5000 && Number.isSafeInteger(this.end) && this.end>0)
    this.window()
  }
  remaining(){return Math.max(0,this.until-this.elapsed.sample().hi)}
  restrict(expiry:number){
    const delta=expiry-this.anchor.upper,rawEnd=this.start+delta,bound=this.anchor.lower+(rawEnd-this.afterHi)
    personalNeed(Number.isSafeInteger(delta) && delta>0 && delta<=900000 && Number.isSafeInteger(rawEnd) && Number.isSafeInteger(bound) && bound>0)
    this.end=Math.min(this.end,bound);this.expiry=Math.min(this.expiry,expiry);this.window()
  }
  window():EpochWindow{
    this.effect.current();if(this.elapsed.sample().hi>=this.until)throw new Error('personalProbeTimeout')
    const w=this.effect.sample()
    if(w.generation!==this.anchor.generation || w.upper>=this.expiry)throw new Error('personalProbeExpired')
    if(w.upper>=this.end)throw new Error('personalProbeTimeout')
    this.effect.current();return w
  }
}

export class PersonalProbe {
  private socket:SocketLike|null=null
  private alive=true
  private established=false
  private queue:unknown[]=[]
  private pending:{resolve(v:unknown):void;reject(e:Error):void}|null=null
  private keys:ChannelKeys|null=null
  private tx=0;private rx=0
  private timer:ReturnType<typeof setTimeout>|null=null
  private budget:ProbeBudget|null=null
  constructor(private readonly principal:Principal,private readonly binding:Readonly<DeviceBinding>,private readonly signer:DeviceSigner,private readonly authority:PersonalManifest,private readonly elapsed:PersonalElapsed,private readonly effect:CeremonyEffect|PinnedEffect,private readonly packet:Readonly<Record<string,unknown>>,private readonly first:CurrentPersonalObservation,private readonly read:(guard:PersonalEffectGuard)=>Promise<CurrentPersonalObservation>,private readonly factory:SocketFactory=nodeSocket){}
  private current(){this.effect.current();personalNeed(this.alive && this.principal.current() && b64url(this.signer.publicKey)===this.binding.deviceKey)}
  private async checked(){this.current();await this.effect.check();this.current();this.budget!.window()}
  private send(raw:string){this.current();this.budget!.window();personalNeed(this.socket);this.socket.send(raw)}
  private receive():Promise<unknown>{
    this.current();if(this.queue.length)return Promise.resolve(this.queue.shift())
    personalNeed(!this.pending);return new Promise((resolve,reject)=>{this.pending={resolve,reject}})
  }
  private async rpc(id:number,method:'hello'|'donnaPersonalSessionProof',body:Readonly<Record<string,unknown>>){
    personalNeed(this.keys && id===this.tx+1 && id<=2)
    const plain=personalBytes({id,m:method,b:body});personalNeed(plain.length<=1024)
    const raw=JSON.stringify({t:'f',c:this.tx,d:sealFrame(this.keys.d2h,this.tx,true,plain)});this.tx++
    await this.checked();this.send(raw)
    const frame=await this.receive();await this.checked();exact(frame,['t','c','d'])
    personalNeed(frame.t==='f' && frame.c===this.rx && typeof frame.d==='string' && frame.d.length<=180000)
    const opened=openFrame(this.keys.h2d,this.rx++,frame.d);personalNeed(opened.final && opened.chunk.length<=80*1024)
    const text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(opened.chunk),value:unknown=JSON.parse(text)
    personalNeed(JSON.stringify(value)===text);exact(value,['id','ok']);personalNeed(value.id===id)
    await this.checked();return value.ok
  }
  async open():Promise<PersonalProbeEvidence>{
    this.current();requireLivePersonalObservation(this.first)
    const V=this.first.activationCurrent as unknown[][],T=this.first.tuple,R=this.packet.receipt as unknown[][]
    this.budget=new ProbeBudget(this.elapsed,this.effect,V[0][14] as number)
    this.timer=setTimeout(()=>this.close(),this.budget.remaining())
    try{
      const token=await this.principal.token();await this.checked()
      const w=this.budget.window(),at=w.lower,until=Math.min(at+30000,V[0][14] as number),nonce=b64url(random(16)),path='/v1/personal-relay/device'
      personalNeed(Number.isSafeInteger(until) && w.upper<until)
      const url=new URL(path,this.principal.deployment.origin);personalNeed(url.protocol==='https:' && url.origin===this.principal.deployment.origin && !url.search && !url.hash && !url.username && !url.password)
      const p=[D('channel-open'),this.authority.context,'device','use',T[11],this.first.hT,R[0][6],this.binding.deviceKey,nonce,path,b64url(random(32)),at,until]
      const open=[p,b64url(this.signer.sign(personalBytes(p)))],http=signatureHeader(this.signer,'GET',url,new Uint8Array(),at,nonce)
      await this.checked();const final=this.budget.window();personalNeed(at<=final.lower && final.upper<until)
      const headers={Authorization:`Bearer ${token}`,'Codync-Sig':http,'X-Donna-Personal-Open':b64url(personalBytes(open))}
      url.protocol='wss:';const socket=this.factory(url.toString(),headers);this.socket=socket
      socket.addEventListener('error',()=>this.close());socket.addEventListener('close',()=>this.close())
      socket.addEventListener('message',event=>{try{
        this.current();personalNeed(typeof event.data==='string' && Buffer.byteLength(event.data)<=256*1024)
        const value=JSON.parse(event.data) as Record<string,unknown>;personalNeed(JSON.stringify(value)===event.data)
        if(this.established && value.t==='presence'){personalNeed(value.online===true);return}
        const pending=this.pending;if(pending){this.pending=null;pending.resolve(value)}else{personalNeed(this.queue.length<4);this.queue.push(value)}
      }catch{this.close()}})
      let presence:unknown
      do{presence=await this.receive();await this.checked();exact(presence,['t','online','lastSeenAt']);personalNeed(presence.t==='presence' && typeof presence.online==='boolean' && presence.lastSeenAt===null)}while(!presence.online)
      const host=unb64url(T[12],32);personalNeed(host)
      const hs=deviceHello(this.signer,host),helloRaw=JSON.stringify(hs.hello);await this.checked();this.send(helloRaw)
      const welcome=await this.receive();await this.checked();exact(welcome,['t','v','ek','sig']);personalNeed(welcome.t==='welcome' && welcome.v===1)
      this.keys=hs.finish(welcome);this.established=true
      const hello=await this.rpc(1,'hello',{});exact(hello,['hostId','computerId','signKey','boxKey','name','version','minApp'])
      personalNeed(hello.computerId===T[11] && hello.signKey===T[12] && hello.boxKey===T[13] && ['hostId','name','version','minApp'].every(k=>typeof hello[k]==='string' && (hello[k] as string).length>0 && (hello[k] as string).length<=256))
      const nonceRPC=b64url(random(32)),proof=await this.rpc(2,'donnaPersonalSessionProof',{nonce:nonceRPC})
      exact(proof,['nonce','profile','C','hT','computerId','hubGeneration','grantId','hostStateRequest','hostState','publication','activation'])
      personalNeed(proof.nonce===nonceRPC && proof.profile==='personal-linux-v1' && personalSame(proof.C,this.authority.context) && proof.hT===this.first.hT && proof.computerId===T[11] && proof.hubGeneration===T[10] && proof.grantId===R[0][6] && personalSame(proof.publication,this.packet.publish) && personalSame(proof.activation,this.first.activationCurrent))
      const originals:Record<string,unknown>={}
      for(const name of personalOriginalNames){const value=name==='activation'?proof.activation:name==='host-state'?proof.hostState:name==='host-state-request'?proof.hostStateRequest:this.packet[name];personalNeed(value!==undefined);originals[name]=value}
      const verifier=new PersonalVerifier(this.authority.context,{issuer:this.authority.issuerPublicKey,app:this.binding.deviceKey,hub:T[12] as string,box:T[13] as string}),now=this.budget.window()
      const ceremony=verifyPersonalCeremony(verifier,personalBytes(originals),{kind:'historical',now:now.upper});personalNeed(ceremony.hash===this.first.hT && personalSame(ceremony.tuple,T))
      const E=verifier.verify('host-state',personalBytes(proof.hostState),{kind:'current',now:now.upper}),v=verifier.verify('activation',personalBytes(proof.activation),{kind:'current',now:now.upper})
      personalNeed((R[0][7] as number)<=(E.payload[16] as number) && (E.payload[16] as number)<=(v.payload[13] as number) && (v.payload[14] as number)<=(E.payload[17] as number))
      this.budget.restrict(Math.min(E.payload[17] as number,v.payload[14] as number))
      const readGuard:PersonalEffectGuard={check:()=>this.checked(),current:()=>{this.current();this.budget!.window()},sample:()=>{this.current();return this.budget!.window()}}
      const observation=await this.read(readGuard);await this.checked();requireLivePersonalObservation(observation,proof.activation);personalNeed(observation.hT===this.first.hT && personalSame(observation.originals.publish,this.packet.publish) && personalSame(observation.originals.receipt,this.packet.receipt))
      const evidence:PersonalProbeEvidence={[evidenceBrand]:true,nonce:nonceRPC,hello:personalImmutable(hello),proof:personalImmutable(proof),originals:personalImmutable(originals),observation,check:()=>this.checked(),current:()=>{this.current();this.budget!.window()},close:()=>this.close()}
      evidence.current();return Object.freeze(evidence)
    }catch(error){this.close();throw error}
  }
  close(){
    if(!this.alive)return;this.alive=false;if(this.timer)clearTimeout(this.timer);this.timer=null
    const pending=this.pending;this.pending=null;pending?.reject(new Error('personalProbeUnknown'))
    this.keys?.d2h.fill(0);this.keys?.h2d.fill(0);this.keys=null;this.queue=[]
    const socket=this.socket;this.socket=null;try{socket?.close(1000,'personal probe complete')}catch{}
  }
}
