/** Laço de posse privado. Não expõe dispatcher ou canal genérico ao renderer. */
import { b64url, random, ephemeral, channelKeys, sha256, verifyEd25519, unb64url, sealFrame, openFrame, type DeviceSigner } from './crypto'
import { personalDomain, personalHash } from './personal-wire'
import { personalCanonical, type PersonalContext } from './personal-protocol'
import type { PersonalCapture } from './personal-origin'
import type { EpochWindow } from './personal-clock'
export class PersonalPossession {
  private socket: WebSocket | null=null
  private pending: {resolve(v:unknown):void;reject(e:Error):void;timer:ReturnType<typeof setTimeout>} | null=null
  private queue: unknown[]=[]
  private alive=true
  constructor(private readonly capture:PersonalCapture,private readonly current:()=>boolean,private readonly sample:()=>Readonly<{lo:number;hi:number}>,private readonly epoch:()=>EpochWindow) {}
  private check() { if (!this.alive || !this.current() || !this.capture.current()) throw new Error('personalPossessionChanged') }
  private async receive():Promise<unknown> {
    this.check(); if (this.queue.length) return this.queue.shift()
    if (this.pending) throw new Error('personalPossessionBusy')
    return new Promise((resolve,reject)=>{const timer=setTimeout(()=>this.close(),5000);this.pending={resolve,reject,timer}})
  }
  private send(raw:string) { this.check(); if (!this.socket || this.socket.readyState!==WebSocket.OPEN) throw new Error('personalPossessionClosed'); this.socket.send(raw) }
  async open(C:PersonalContext,T:readonly unknown[],G:unknown,signer:DeviceSigner,guard:()=>Promise<void>) {
    const checked=async()=>{this.check();await this.capture.check();await guard();this.check()}
    const deadline=this.sample().lo+5000
    const tuple=personalCanonical(Buffer.from(JSON.stringify(T))),grant=personalCanonical(Buffer.from(JSON.stringify(G)))
    const end=(value:unknown,index:number)=>{if(!Array.isArray(value) || !Array.isArray(value[0]))throw new Error('personalPossessionOriginal');const n=value[0][index];if(typeof n!=='number' || !Number.isSafeInteger(n) || n<=0)throw new Error('personalPossessionOriginal');return n}
    if(!Array.isArray(tuple) || tuple.length!==19 || typeof tuple[18]!=='number' || !Number.isSafeInteger(tuple[18]) || tuple[18]<=0)throw new Error('personalPossessionOriginal')
    const grantUntil=Math.min(tuple[18],end(grant,10));let generation:string|null=null
    const rpcBounds=new Map<number,Readonly<{generation:string;end:number}>>()
    const valid=(until:number,expiresAt=grantUntil)=>{
      this.check();if(this.sample().hi>=until)throw new Error('personalPossessionTimeout')
      let bound=rpcBounds.get(until)
      if(!bound){
        const w0=this.epoch();if(generation!==null && w0.generation!==generation)throw new Error('personalPossessionExpired')
        const remaining=until-this.sample().hi,end=w0.lower+remaining
        if(!Number.isSafeInteger(remaining) || remaining<=0 || remaining>5000 || !Number.isSafeInteger(end) || end<=0)throw new Error('personalPossessionTimeout')
        bound=Object.freeze({generation:w0.generation,end});rpcBounds.set(until,bound)
      }
      // lower anterior + restante M posterior só restringe os5s originais.
      // A mesma janela FINAL confere os originais E esse teto retido.
      const w=this.epoch()
      if(generation!==null && w.generation!==generation || w.generation!==bound.generation || w.upper>=expiresAt)throw new Error('personalPossessionExpired')
      if(w.upper>=bound.end)throw new Error('personalPossessionTimeout')
      generation=w.generation;this.check()
    }
    await checked();valid(deadline)
    const url=new URL(this.capture.possessionURL)
    if (url.protocol!=='ws:' || url.hostname!=='127.0.0.1' || url.pathname!=='/personal-possession' || url.search || url.username || url.password) throw new Error('personalPossessionEndpoint')
    const socket=new WebSocket(url); this.socket=socket
    socket.addEventListener('message',(event)=>{ try { if(typeof event.data!=='string') throw new Error(); const text=event.data; if(Buffer.byteLength(text)>256*1024)throw new Error(); const v:unknown=JSON.parse(text); if (Buffer.byteLength(text)>256*1024 || JSON.stringify(v)!==text || !this.current() || !this.capture.current()) throw new Error();
      const pending=this.pending; if (pending) {this.pending=null;clearTimeout(pending.timer);pending.resolve(v)} else {if(this.queue.length)throw new Error();this.queue.push(v)}
    } catch {this.close()} })
    socket.addEventListener('error',()=>this.close()); socket.addEventListener('close',()=>this.close())
    await new Promise<void>((resolve,reject)=>{const timer=setTimeout(()=>{this.close();reject(new Error('personalPossessionTimeout'))},5000);socket.addEventListener('open',()=>{clearTimeout(timer);resolve()});socket.addEventListener('error',()=>{clearTimeout(timer);reject(new Error('personalPossessionClosed'))});socket.addEventListener('close',()=>{clearTimeout(timer);reject(new Error('personalPossessionClosed'))})})
    await checked();valid(deadline); const e=ephemeral(), hT=personalHash(tuple), hG=personalHash((grant as unknown[])[0])
    const p=[personalDomain('possession-hello'),C,hT,hG,b64url(signer.publicKey),b64url(e.pub),b64url(random(32))]
    const hello=[...p,b64url(signer.sign(Buffer.from(JSON.stringify(p))))]
    const rawHello=JSON.stringify(hello);await checked();valid(deadline);this.send(rawHello)
    const welcome=await this.receive();await checked();valid(deadline)
    if (!Array.isArray(welcome) || welcome.length!==7 || welcome[0]!==personalDomain('possession-welcome') || JSON.stringify(welcome[1])!==JSON.stringify(C) || welcome[2]!==hT || welcome[3]!==hG) throw new Error('personalPossessionWelcome')
    const host=unb64url(tuple[12],32), epk=unb64url(welcome[4],32), signature=unb64url(welcome[6],64)
    const transcript=Buffer.from(JSON.stringify([personalDomain('possession-transcript'),hello,welcome[4]])), th=sha256(transcript)
    if (!host || !epk || !signature || welcome[5]!==b64url(th) || !verifyEd25519(host,signature,th)) throw new Error('personalPossessionIdentity')
    const keys=channelKeys(e.dh(epk),th); let tx=0,rx=0
    const exchange=async(request:readonly unknown[])=>{
      const until=this.sample().lo+5000,bytes=Buffer.from(JSON.stringify(request)),original=JSON.parse(bytes.toString()) as unknown[]
      const expiresAt=original[0]==='proof'?Math.min(grantUntil,end(original[2],6),end(original[3],8)):grantUntil
      try{
        await checked();valid(until,expiresAt);if(bytes.length>192*1024 || tx>=2)throw new Error('personalPossessionLimit')
        const counter=tx++,ciphertext=sealFrame(keys.d2h,counter,true,bytes),raw=JSON.stringify({t:'f',c:counter,d:ciphertext})
        // A cifra pode atravessar preempção: o mesmo upper/5s e os mesmos
        // originais são rechecados depois do ÚLTIMO await, antes do send.
        await checked();valid(until,expiresAt);this.send(raw)
        const frame=await this.receive() as {t?:unknown;c?:unknown;d?:unknown};if(!frame || Object.keys(frame).sort().join(',')!=='c,d,t' || frame.t!=='f' || frame.c!==rx || typeof frame.d!=='string')throw new Error('personalPossessionFrame');const opened=openFrame(keys.h2d,rx++,frame.d);if(!opened.final)throw new Error('personalPossessionFrame');const value=personalCanonical(opened.chunk);await checked();valid(until,expiresAt);return value
      }catch(e){this.close();throw e}
    }
    return Object.freeze({challenge:()=>exchange(['challenge',grant]),proof:(K:unknown,F:unknown)=>exchange(['proof',grant,K,F])})
  }
  close() { if(!this.alive)return;this.alive=false;const pending=this.pending;this.pending=null;if(pending){clearTimeout(pending.timer);pending.reject(new Error('personalPossessionUnknown'))}this.socket?.close();this.socket=null;this.queue=[] }
}
