/** Cliente Main de /personal-prepare. Uma operação/canal na mesma captura Q/H. */
import { b64url,random,ephemeral,unb64url,verifyEd25519,type DeviceSigner } from './crypto'
import { personalDomain as D } from './personal-wire'
import { personalCanonical,type PersonalContext } from './personal-protocol'
import type { PersonalCapture } from './personal-origin'
import type { EpochWindow } from './personal-clock'
import { PrepareFrames,prepareBytes,prepareDecode,prepareIdentity,prepareMessage,prepareTranscript } from './personal-prepare-wire'
export class PersonalPrepare {
  private socket:WebSocket|null=null
  private alive=true
  private pending:{resolve(v:unknown):void;reject(e:Error):void;timer:ReturnType<typeof setTimeout>}|null=null
  private queued:unknown[]=[]
  private frames:PrepareFrames|null=null
  constructor(private readonly capture:PersonalCapture,private readonly current:()=>boolean,private readonly sample:()=>Readonly<{lo:number;hi:number}>,private readonly epoch?:()=>EpochWindow){ }
  private check(){if(!this.alive || !this.current() || !this.capture.current())throw new Error('personalPrepareChanged')}
  private async checked(guard:()=>void|Promise<void>){this.check();await this.capture.check();await guard();this.check()}
  private receive():Promise<unknown>{this.check();if(this.queued.length)return Promise.resolve(this.queued.shift());if(this.pending)throw new Error('personalPrepareBusy');return new Promise((resolve,reject)=>{const timer=setTimeout(()=>this.close(),5000);this.pending={resolve,reject,timer}})}
  private send(frame:ArrayBuffer){this.check();if(!this.socket || this.socket.readyState!==WebSocket.OPEN)throw new Error('personalPrepareClosed');this.socket.send(frame)}
  async exchange(C:PersonalContext,Q:unknown,request:readonly unknown[],signer:DeviceSigner,guard:()=>void|Promise<void>):Promise<readonly unknown[]> {
    const deadline=this.sample().lo+5000,q=personalCanonical(prepareBytes(Q)) as unknown[][],original=personalCanonical(prepareBytes(request)) as readonly unknown[]
    const expiresAt=q?.[0]?.[12];if(typeof expiresAt!=='number' || !Number.isSafeInteger(expiresAt) || expiresAt<=0)throw new Error('personalPrepareOriginal')
    let generation:string|null=null
    const rpcBounds=new Map<number,Readonly<{generation:string;end:number}>>()
    const valid=(until:number)=>{
      this.check();if(this.sample().hi>=until)throw new Error('personalPrepareTimeout')
      let bound=rpcBounds.get(until)
      if(!bound){
        const w0=this.epoch?.();if(!w0 || generation!==null && w0.generation!==generation)throw new Error('personalPrepareExpired')
        const remaining=until-this.sample().hi,end=w0.lower+remaining
        if(!Number.isSafeInteger(remaining) || remaining<=0 || remaining>5000 || !Number.isSafeInteger(end) || end<=0)throw new Error('personalPrepareTimeout')
        bound=Object.freeze({generation:w0.generation,end});rpcBounds.set(until,bound)
      }
      // Limite RPC conservador fixado uma vez; a janela final vem depois
      // de TODAS as leituras M e confere também o Q original.
      const w=this.epoch?.()
      if(!w || generation!==null && w.generation!==generation || w.generation!==bound.generation || w.upper>=expiresAt)throw new Error('personalPrepareExpired')
      if(w.upper>=bound.end)throw new Error('personalPrepareTimeout')
      generation=w.generation;this.check()
    }
    const send=async(value:unknown,limit:number,until:number)=>{const frame=prepareMessage(value,limit),owned=new Uint8Array(frame.byteLength);owned.set(frame);await this.checked(guard);valid(until);this.send(owned.buffer)}
    await this.checked(guard);const url=new URL(this.capture.possessionURL)
    if(url.protocol!=='ws:' || url.hostname!=='127.0.0.1' || url.pathname!=='/personal-possession' || url.search || url.username || url.password)throw new Error('personalPrepareEndpoint')
    url.pathname='/personal-prepare';const socket=new WebSocket(url);socket.binaryType='arraybuffer';this.socket=socket
    socket.addEventListener('message',event=>{try{if(!(event.data instanceof ArrayBuffer))throw new Error();const message=prepareDecode(new Uint8Array(event.data),32768);this.check();const pending=this.pending;if(pending){this.pending=null;clearTimeout(pending.timer);pending.resolve(message)}else{if(this.queued.length)throw new Error();this.queued.push(message)}}catch{this.close()}})
    socket.addEventListener('error',()=>this.close());socket.addEventListener('close',()=>this.close())
    try{
      await new Promise<void>((resolve,reject)=>{const timer=setTimeout(()=>{this.close();reject(new Error('personalPrepareTimeout'))},5000);socket.addEventListener('open',()=>{clearTimeout(timer);resolve()});socket.addEventListener('error',()=>{clearTimeout(timer);reject(new Error('personalPrepareClosed'))});socket.addEventListener('close',()=>{clearTimeout(timer);reject(new Error('personalPrepareClosed'))})})
      await this.checked(guard);valid(deadline);const mode=q[0][3],identity=prepareIdentity(C,q),e=ephemeral()
      if(mode!=='command' && mode!=='device')throw new Error('personalPrepareMode')
      const hello=[D('prepare-hello'),1,C,identity.capture,identity.hq,identity.wireHash,mode,mode==='command'?b64url(signer.publicKey):null,b64url(e.pub),b64url(random(32))]
      await send(hello,16384,deadline);const welcome=await this.receive();await this.checked(guard);valid(deadline)
      if(!Array.isArray(welcome) || welcome.length!==9 || welcome[0]!==D('prepare-welcome') || welcome[1]!==1 || JSON.stringify(welcome.slice(2,6))!==JSON.stringify(hello.slice(2,6)))throw new Error('personalPrepareWelcome')
      const host=unb64url(q[0][7],32),hostEphemeral=unb64url(welcome[6],32),signature=unb64url(welcome[8],64),th=prepareTranscript(hello,welcome.slice(0,8))
      if(!host || !hostEphemeral || !unb64url(welcome[7],32) || !signature || !verifyEd25519(host,signature,th) || this.sample().hi>=deadline)throw new Error('personalPrepareIdentity')
      await this.checked(guard);valid(deadline);const finish0=[D('prepare-finish'),1,b64url(th)],sig=mode==='command'?b64url(signer.sign(prepareBytes(finish0))):null
      await send([...finish0,sig],16384,deadline)
      const opDeadline=this.sample().lo+5000;this.frames=new PrepareFrames(hello,th,e,hostEphemeral)
      const frame=this.frames.request(original);await send(frame,32768,opDeadline);const result=this.frames.response(await this.receive());await this.checked(guard);valid(opDeadline)
      if(!Array.isArray(result) || result[0]!==D('prepare-response') || result[1]!==1 || result[2]!==original[2] || result[3]!== (original[4]==='candidate'?original[5]:null))throw new Error('personalPrepareResponse')
      if(result.length===6 && result[4]==='error' && ['invalid','unauthenticated','conflict','expired','unavailable','unknown','rateLimited','closed'].includes(result[5]))throw new Error('personalPrepareUnknown')
      if(result.length!==7 || result[4]!=='ok' || result[5]!==original[4] || !Array.isArray(result[6]))throw new Error('personalPrepareResponse')
      return Object.freeze(result[6])
    } finally{this.close()}
  }
  close(){if(!this.alive)return;this.alive=false;const pending=this.pending;this.pending=null;if(pending){clearTimeout(pending.timer);pending.reject(new Error('personalPrepareUnknown'))}this.frames?.clear();this.frames=null;this.socket?.close();this.socket=null;this.queued=[]}
}
