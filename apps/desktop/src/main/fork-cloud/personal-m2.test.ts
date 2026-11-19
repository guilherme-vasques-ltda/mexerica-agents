import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as os from 'node:os'
import * as path from 'node:path'
import { EventEmitter } from 'node:events'
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
const fixture=JSON.parse(fs.readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json',import.meta.url),'utf8')).personalBootstrap
const w=fixture.wireVectors,C=fixture.context,D=wire.personalDomain,bytes=(v:unknown)=>Buffer.from(JSON.stringify(v)),H=wire.personalHash
const signer=(role:'app'|'hub'|'issuer')=>crypto.signerFromSeed(crypto.unb64url(w.seeds[role],32)!)
const signed=(role:'app'|'hub'|'issuer',value:unknown[])=>[value,crypto.b64url(signer(role).sign(bytes(value)))]
const authority=protocol.inspectPersonalManifest(Buffer.from(fixture.manifestCanonical))
const primitives={'node:crypto':nodeCrypto,'@noble/ciphers/chacha.js':{chacha20poly1305},'@noble/curves/ed25519.js':nobleEd25519,'@noble/curves/utils.js':nobleUtils,'./crypto':crypto,'./personal-wire':wire,'./personal-protocol':protocol}
const globals={Buffer,Object,process,URL,TextEncoder,TextDecoder,Uint8Array,ArrayBuffer,DataView,performance,setTimeout,clearTimeout,queueMicrotask,Response}
const session=loadMainModule<typeof import('./personal-session.ts')>('fork-cloud/personal-session',primitives,globals)
const history=loadMainModule<typeof import('./personal-pin-history.ts')>('fork-cloud/personal-pin-history',primitives,globals)
const pinModule=loadMainModule<typeof import('./personal-pin.ts')>('fork-cloud/personal-pin',primitives,globals)
const binding:DeviceBinding={version:1,sessionId:'fixture-session',origin:C[3],issuer:C[4],deploymentId:C[5],manifestHash:C[6],userId:w.tuple[5],deviceId:w.tuple[6],deviceKey:w.tuple[7]}
const namespace='c'.repeat(64),attemptId=crypto.b64url(Buffer.alloc(16,61))
const origin=['localHelper','local-linux','/run/donnabot/bootstrap-helper-v1.sock','d'.repeat(64)] as const

function publicPin(){
  const packet=JSON.parse(JSON.stringify(w.packet)) as Record<string,any>,T=JSON.parse(JSON.stringify(w.tuple)),R=packet.receipt
  const originals=Object.fromEntries(session.personalOriginalNames.map(name=>[name,packet[name]])),nonce=crypto.b64url(Buffer.alloc(32,62))
  const hello={hostId:'fixture-public-host',computerId:T[11],signKey:T[12],boxKey:T[13],name:'fixture',version:'0.8.0',minApp:'0.5.0'}
  const proof={nonce,profile:'personal-linux-v1',C,hT:H(T),computerId:T[11],hubGeneration:T[10],grantId:R[0][6],hostStateRequest:packet['host-state-request'],hostState:packet['host-state'],publication:packet.publish,activation:packet.activation}
  const P=[D('local-pin'),1,'personal-linux-v1',namespace,attemptId,'local-linux',origin,C,T,H(T),T[11],T[10],T[12],T[13],T[5],T[6],T[7],R[0][6],H(R[0]),packet['local-ticket'],packet.grant,R,packet.publish,packet['host-state-request'],packet['host-state'],packet.activation,nonce,hello,proof,originals]
  const encode=(body:unknown[])=>bytes([body,crypto.b64url(signer('app').sign(bytes(body)))])
  const raw=encode(P),pin=history.qualifyPersonalPinHistory(authority,binding,namespace,raw)
  return{packet,T,P,raw,pin,encode}
}

test('PL1 M2 P30: 17 originais reais aceitam só história e alterações assinadas por A não fabricam issuer/H',()=>{
  const f=publicPin();assert.equal(f.pin.body.length,30);assert.equal(Object.keys(f.pin.originals).length,17);assert.equal(f.pin.digest,H(JSON.parse(f.raw.toString())))
  assert.equal((f.pin as unknown as Record<string,unknown>).epoch,undefined);assert.equal((f.pin as unknown as Record<string,unknown>).lease,undefined)
  for(const name of session.personalOriginalNames){
    const P=JSON.parse(JSON.stringify(f.P)),sig=crypto.unb64url(P[29][name][1],64)!;sig[0]^=1;P[29][name][1]=crypto.b64url(sig)
    assert.throws(()=>history.qualifyPersonalPinHistory(authority,binding,namespace,f.encode(P)),name)
  }
  const wrongAnchor=JSON.parse(JSON.stringify(f.P));wrongAnchor[29]['host-state'][0][17]++;assert.throws(()=>history.qualifyPersonalPinHistory(authority,binding,namespace,f.encode(wrongAnchor)))
  const extra=JSON.parse(JSON.stringify(f.P));extra[29].read=f.packet.read;assert.throws(()=>history.qualifyPersonalPinHistory(authority,binding,namespace,f.encode(extra)))
  const copy=JSON.parse(JSON.stringify(f.P));copy[24][0][17]++;assert.throws(()=>history.qualifyPersonalPinHistory(authority,binding,namespace,f.encode(copy)))
  assert.throws(()=>history.qualifyPersonalPinHistory(authority,{...binding,deviceId:'other'},namespace,f.raw));assert.throws(()=>history.qualifyPersonalPinHistory(authority,binding,'e'.repeat(64),f.raw))
  const owned=f.pin.signedBytes;owned.fill(0);assert.equal(Buffer.from(f.pin.signedBytes).toString(),f.raw.toString())
})

function coldFixture(options:{quantum?:1|1000;delay?:number;nonceWrong?:boolean;sleepInHeader?:boolean}={}){
  const f=publicPin(),app=signer('app'),wall=(f.pin.originals['host-state'] as any)[0][17]+90*86400000
  let mono=1000,active=true,requests=0,signatures=0,historyChecks=0
  const source={quantum:options.quantum??1,generation:'cold-current-runtime',current(){assert(active)},sample(){assert(active);return{lo:mono,hi:mono+(options.quantum??1)}},auxiliary(){assert(active);return mono},close(){active=false}}
  const {PersonalClock}=loadMainModule<typeof import('./personal-clock.ts')>('fork-cloud/personal-clock',primitives,globals),clock=new PersonalClock(source)
  const principal={namespace,seal:{},deployment:{origin:C[3]},current:()=>active,async token(){assert(active);return 'discardable-fixture-session'}} as unknown as Principal
  const guardedSigner={publicKey:app.publicKey,sign(message:Uint8Array){const sig=app.sign(message);signatures++;if(options.sleepInHeader && signatures===2)mono+=30001;return sig}}
  const fetcher=async(url:URL,init:RequestInit)=>{
    requests++;assert.equal(url.pathname,'/v1/personal-bootstrap/read');assert.equal(init.redirect,'error');assert.equal(init.cache,'no-store')
    const raw=Buffer.from(init.body as Uint8Array),body=JSON.parse(raw.toString());assert.deepEqual(Object.keys(body),['read']);const q=body.read[0]
    assert.equal(q.length,8);assert.equal(q[0],D('read'));assert.equal(q[2],f.T[1]);assert.equal(q[3],f.T[9]);assert.equal(q[4],H(f.T));assert.equal(q[6],wall);assert.equal(q[7],wall+30000)
    assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(body.read[1],64)!,bytes(q)))
    const headers=init.headers as Record<string,string>,fields=Object.fromEntries(headers['Codync-Sig'].split(',').map(x=>x.split('=')))
    assert.equal(Number(fields.ts),wall);assert.equal(fields.kid,binding.deviceKey);assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(fields.sig,64)!,Buffer.from(crypto.sigCanonical('POST',url.host,url.pathname,wall,fields.nonce,raw))))
    mono+=options.delay??10
    const state=signed('issuer',[D('state'),C,f.T[1],f.T[9],H(f.T),'published',7,f.packet.receipt[0][6],H(f.packet.grant[0]),H(f.packet.receipt[0]),null,null,null,null,options.nonceWrong?crypto.b64url(Buffer.alloc(32,98)):q[5],wall+1,wall+30000])
    return new Response(JSON.stringify({state,originals:{},activationCurrent:null}),{status:200})
  }
  class Wall extends Date{static override now(){return wall}}
  const cold=loadMainModule<typeof import('./personal-cold.ts')>('fork-cloud/personal-cold',primitives,{...globals,Date:Wall,fetch:fetcher})
  const guard={async check(){historyChecks++;assert(active);return f.pin},current(){assert(active)}}
  return{...f,clock,source,wall,stats:()=>({requests,signatures,historyChecks}),run:()=>cold.postPinnedClockRead(principal,guardedSigner,authority,f.pin,source,clock,guard),close(){clock.cut();source.close()}}
}

test('PL1 M2 CLOCK-S: PIN antigo sem helper só recebe época de S novo noncebound em q1/q1000',async()=>{
  for(const quantum of [1,1000] as const){const f=coldFixture({quantum});try{
    assert.throws(()=>f.clock.window(f.T[11]));const result=await f.run();assert.equal((result.state as unknown[][])[0][15],f.wall+1)
    const now=f.clock.window(f.T[11]);assert(now.lower>=f.wall+1);assert(now.upper<f.wall+30000);assert(f.stats().historyChecks>=3);assert.equal(f.stats().requests,1)
    assert.equal(Buffer.from(f.pin.signedBytes).toString(),f.raw.toString());assert((f.pin.originals['host-state'] as any)[0][17]<f.wall)
  }finally{f.close()}}
})

test('PL1 M2 CLOCK-S: sono no signer depois do Read original corta M30s antes de qualquer fetch',async()=>{
  const f=coldFixture({sleepInHeader:true});try{await assert.rejects(f.run());assert.throws(()=>f.clock.window(f.T[11]));assert.equal(f.stats().signatures,2);assert.equal(f.stats().requests,0)}finally{f.close()}
})

test('PL1 M2 CLOCK-S: nonce diferente e RTT além5s recusam época sem usar deadlineS como lease',async()=>{
  for(const options of [{nonceWrong:true},{delay:5001}]){const f=coldFixture(options);try{await assert.rejects(f.run());assert.equal(f.stats().requests,1);assert.throws(()=>f.clock.window(f.T[11]))}finally{f.close()}}
})

function probeFixture(options:{replaceV?:boolean;afterCipherSleep?:boolean}={}){
  const f=publicPin(),app=signer('app'),hub=signer('hub'),epoch=(f.packet.receipt[0][7] as number)+100
  let mono=1000,active=true,readCount=0,openHeaders=0,seals=0;const received:string[]=[],peers:Peer[]=[]
  const source={quantum:1 as const,generation:'probe-current-runtime',current(){assert(active)},sample(){assert(active);return{lo:mono,hi:mono+1}},auxiliary(){assert(active);return mono},close(){active=false}}
  const {PersonalClock}=loadMainModule<typeof import('./personal-clock.ts')>('fork-cloud/personal-clock',primitives,globals),clock=new PersonalClock(source),flight=clock.begin()
  clock.accept(authority,signed('issuer',[D('time'),C,crypto.b64url(crypto.random(32)),f.T[11],epoch,300000]),f.T[11],flight)
  const principal={namespace,seal:{},deployment:{origin:C[3]},current:()=>active,async token(){assert(active);return 'discardable-fixture-session'}} as unknown as Principal
  const observe=(activation:unknown=f.packet.activation)=>{
    const now=clock.window(f.T[11]),q=[D('read'),C,f.T[1],f.T[9],H(f.T),crypto.b64url(crypto.random(32)),now.lower,now.lower+30000],read=[q,crypto.b64url(app.sign(bytes(q)))] as const
    const originals=Object.fromEntries(Object.entries(session.personalRemoteNames).map(([remote,local])=>[remote,local==='activation'?activation:['fence','fence-ack','revocation'].includes(local)?null:f.packet[local]]))
    const state=signed('issuer',[D('state'),C,f.T[1],f.T[9],H(f.T),'hubActive',5,f.packet.receipt[0][6],H(f.packet.grant[0]),H(f.packet.receipt[0]),H((activation as unknown[][])[0]),null,null,null,q[5],now.lower,now.lower+30000])
    return session.verifyPersonalObservation(authority,binding,f.pin.originals,read,{state,originals,activationCurrent:activation},clock.window(f.T[11]))
  }
  class Peer{
    listeners=new Map<string,((e:any)=>void)[]>();closed=false;hello:any;keys!:crypto.ChannelKeys;tx=0;rx=0;ephemeral=crypto.ephemeral()
    constructor(url:string,headers:Record<string,string>){
      const u=new URL(url);assert.equal(u.protocol,'wss:');assert.equal(u.pathname,'/v1/personal-relay/device');assert.equal(u.search,'')
      const fields=Object.fromEntries(headers['Codync-Sig'].split(',').map(x=>x.split('='))),open=protocol.personalCanonical(crypto.unb64url(headers['X-Donna-Personal-Open'])!) as any[]
      const verifier=new wire.PersonalVerifier(C,w.keys);verifier.verifyOpen(bytes(open),{kind:'current',now:clock.window(f.T[11]).upper},{role:'device',kid:fields.kid,httpNonce:fields.nonce,pathAndQuery:u.pathname,httpUntil:Number(fields.ts)+30000})
      assert.equal(headers.Authorization,'Bearer discardable-fixture-session');assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(fields.sig,64)!,Buffer.from(crypto.sigCanonical('GET',u.host,u.pathname,Number(fields.ts),fields.nonce,new Uint8Array()))));openHeaders++
      peers.push(this);queueMicrotask(()=>this.emit({t:'presence',online:true,lastSeenAt:null}))
    }
    addEventListener(name:string,listener:(e:any)=>void){this.listeners.set(name,[...(this.listeners.get(name)??[]),listener])}
    emit(value:unknown){if(!this.closed)for(const listener of this.listeners.get('message')??[])listener({data:JSON.stringify(value)})}
    close(){if(this.closed)return;this.closed=true;for(const listener of this.listeners.get('close')??[])listener({code:1000,reason:''})}
    send(raw:string){
      assert(!this.closed);const value=JSON.parse(raw)
      if(!this.hello){this.hello=value;assert.equal(value.dk,binding.deviceKey);assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(value.sig,64)!,crypto.hs1Input(crypto.cidRaw(hub.publicKey),app.publicKey,crypto.unb64url(value.ek,32)!,crypto.unb64url(value.n,32)!)))
        const transcript=crypto.transcriptHash(crypto.cidRaw(hub.publicKey),app.publicKey,crypto.unb64url(value.ek,32)!,crypto.unb64url(value.n,32)!,this.ephemeral.pub);this.keys=crypto.channelKeys(this.ephemeral.dh(crypto.unb64url(value.ek,32)!),transcript)
        queueMicrotask(()=>this.emit({t:'welcome',v:1,ek:crypto.b64url(this.ephemeral.pub),sig:crypto.b64url(hub.sign(transcript))}));return}
      assert.equal(value.c,this.rx);const opened=crypto.openFrame(this.keys.d2h,this.rx++,value.d);assert(opened.final);const request=JSON.parse(Buffer.from(opened.chunk).toString());received.push(request.m)
      let result:unknown
      if(request.m==='hello'){assert.equal(request.id,1);assert.equal(JSON.stringify(request.b),'{}');result=f.P[27]}
      else{assert.equal(request.id,2);assert.equal(request.m,'donnaPersonalSessionProof');assert.equal(Object.keys(request.b).join(','),'nonce');result={...(f.P[28] as object),nonce:request.b.nonce}}
      const response={t:'f',c:this.tx,d:crypto.sealFrame(this.keys.h2d,this.tx++,true,bytes({id:request.id,ok:result}))};queueMicrotask(()=>this.emit(response))
    }
  }
  const effect=session.ceremonyEffect({async check(){assert(active)},current(){assert(active)},sample:()=>clock.window(f.T[11])}),first=observe()
  const cryptoBoundary={...crypto,sealFrame(...args:Parameters<typeof crypto.sealFrame>){const value=crypto.sealFrame(...args);seals++;if(options.afterCipherSleep)mono+=5001;return value}}
  const {PersonalProbe}=loadMainModule<typeof import('./personal-probe.ts')>('fork-cloud/personal-probe',{...primitives,'./crypto':cryptoBoundary,'./relay-socket':{nodeSocket:(url:string,headers:Record<string,string>)=>new Peer(url,headers)}},globals)
  const probe=new PersonalProbe(principal,binding,app,authority,source,effect,f.pin.originals,first,async guard=>{await guard.check();readCount++;if(options.replaceV){const p=JSON.parse(JSON.stringify(f.packet.activation[0]));p[13]++;guard.current();return observe(signed('hub',p))}guard.current();return observe()})
  return{...f,probe,principal,stats:()=>({readCount,openHeaders,seals,received:[...received],closed:peers.every(p=>p.closed)}),close(){probe.close();clock.cut();source.close()}}
}

test('PL1 M2 probe: Open13 handshake AEAD duasRPC e Read mesmaV produzem P30 sem Caller/FS/ready',async()=>{
  const f=probeFixture();try{
    const evidence=await f.probe.open();assert.deepEqual(f.stats().received,['hello','donnaPersonalSessionProof']);assert.equal(f.stats().readCount,1);assert.equal(f.stats().openHeaders,1);assert.equal(f.stats().closed,false)
    const pin=await pinModule.signPersonalPin(f.principal,binding,authority,signer('app'),attemptId,'local-linux',origin,evidence)
    assert.equal(pin.body.length,30);assert.equal(Object.keys(pin.originals).length,17);assert.equal(pin.hT,f.pin.hT);assert.equal(pin.body[26],evidence.nonce);assert.equal((evidence as unknown as Record<string,unknown>).call,undefined)
    evidence.close();assert.equal(f.stats().closed,true);assert.throws(()=>evidence.current())
  }finally{f.close()}
})

test('PL1 M2 probe: V posterior diferente corta mesmo binding antes de aceitar PIN',async()=>{
  const f=probeFixture({replaceV:true});try{await assert.rejects(f.probe.open());assert.equal(f.stats().readCount,1);assert.deepEqual(f.stats().received,['hello','donnaPersonalSessionProof']);assert.equal(f.stats().closed,true)}finally{f.close()}
})

test('PL1 M2 probe: cifra real atravessa5s original e nenhum request decifrado alcança peer',async()=>{
  const f=probeFixture({afterCipherSleep:true});try{await assert.rejects(f.probe.open());assert.equal(f.stats().seals,1);assert.equal(f.stats().closed,true);assert.equal(f.stats().received.length,0)}finally{f.close()}
})

test('PL1 M2 cercas: W nulo pósR não encerra, revisão superior exige Y/X e S terminal novos',()=>{
  const f=publicPin(),now=(f.packet.receipt[0][7] as number)+100,source={quantum:1 as const,generation:'fence-current-runtime',current(){},sample:()=>({lo:1000,hi:1001}),auxiliary:()=>1000,close(){}}
  const {PersonalClock}=loadMainModule<typeof import('./personal-clock.ts')>('fork-cloud/personal-clock',primitives,globals),clock=new PersonalClock(source),flight=clock.begin()
  clock.accept(authority,signed('issuer',[D('time'),C,crypto.b64url(crypto.random(32)),f.T[11],now,300000]),f.T[11],flight)
  const fences=loadMainModule<typeof import('./personal-fences.ts')>('fork-cloud/personal-fences',primitives,globals)
  const observe=(W:unknown,Y:unknown|null,X:unknown|null,terminal:boolean)=>{
    const w0=clock.window(f.T[11]),q=[D('read'),C,f.T[1],f.T[9],H(f.T),crypto.b64url(crypto.random(32)),w0.lower,w0.lower+30000],read=[q,crypto.b64url(signer('app').sign(bytes(q)))] as const
    const originals=Object.fromEntries(Object.entries(session.personalRemoteNames).map(([remote,local])=>[remote,local==='fence'?W:local==='fence-ack'?Y:local==='revocation'?X:f.packet[local]]))
    const state=signed('issuer',[D('state'),C,f.T[1],f.T[9],H(f.T),terminal?'revoked':'revoking',8,f.packet.receipt[0][6],H(f.packet.grant[0]),H(f.packet.receipt[0]),null,X?H((X as unknown[][])[0]):null,H((W as unknown[][])[0]),Y?H((Y as unknown[][])[0]):null,q[5],now,now+30000])
    return session.verifyPersonalObservation(authority,binding,f.pin.originals,read,{state,originals,activationCurrent:null},clock.window(f.T[11]))
  }
  try{
    const wp=JSON.parse(JSON.stringify(f.packet.fence[0]));wp[4]=null;wp[5]=null;const Wnull=signed('issuer',wp)
    const first=fences.verifyPersonalFenceObservation(observe(Wnull,null,null,false),null);assert(first);assert.equal(first.terminal,false);assert.equal(first.record.Y,null);assert.equal(first.record.X,null)
    const full=JSON.parse(JSON.stringify(f.packet.fence[0]));full[6]=2;const W=signed('issuer',full),yp=JSON.parse(JSON.stringify(f.packet['fence-ack'][0]));yp[3]=H(W[0]);const Y=signed('hub',yp),X=f.packet.revocation
    const pending=fences.verifyPersonalFenceObservation(observe(W,null,null,false),first.record);assert(pending);assert.equal(pending.terminal,false)
    assert.throws(()=>observe(W,f.packet['fence-ack'],X,true),'ACK da revisão anterior não acompanha W superior')
    const final=fences.verifyPersonalFenceObservation(observe(W,Y,X,true),pending.record);assert(final);assert.equal(final.record.revision,2);assert.equal(final.terminal,true)
    assert.throws(()=>fences.verifyPersonalFenceObservation(observe(Wnull,null,null,false),final.record))
    assert.equal(Buffer.from(f.pin.signedBytes).toString(),f.raw.toString())
  }finally{clock.cut();source.close()}
})

async function publicationFixture(options:{losePublication?:boolean;missingRemoteReceipt?:boolean;sleepAfterSignature?:boolean}={}){
  const f=publicPin(),dir=fs.mkdtempSync(path.join(os.tmpdir(),'personal-m2-publication-')),powerMonitor=new EventEmitter(),storageKey=nodeCrypto.randomBytes(32),epoch=f.packet.receipt[0][7]+100
  let active=true,mono=1000,storageCounter=0,reads=0,publishes=0,helperCaptures=0,publication:unknown|null=null
  const publishedBodies:string[]=[],signedHeaders:string[]=[]
  const safeStorage={isEncryptionAvailable:()=>true,getSelectedStorageBackend:()=>process.platform==='linux'?'gnome_libsecret':'keychain',encryptString(text:string){const counter=storageCounter++,head=Buffer.alloc(4);head.writeUInt32BE(counter);return Buffer.concat([head,Buffer.from(crypto.sealFrame(storageKey,counter,true,Buffer.from(text)))])},decryptString(sealed:Buffer){const counter=sealed.readUInt32BE(0);return Buffer.from(crypto.openFrame(storageKey,counter,sealed.subarray(4).toString()).chunk).toString()}}
  const p={namespace,seal:{epoch:1,sessionId:binding.sessionId,userId:binding.userId},deployment:{env:C[2],origin:C[3],issuer:C[4],deploymentId:C[5],manifestHash:C[6]},current:()=>active,async token(){assert(active);return 'discardable-fixture-session'},personalBootstrapStatus:{profile:'personal-linux-v1',wire:1,available:false,reason:'helperRequired'}} as Principal
  const account={sessionEpoch:1,on(){},off(){},verifiedDeviceBinding:()=>binding},app=signer('app')
  const serviceSigner:crypto.DeviceSigner={publicKey:app.publicKey,sign(input){const sig=app.sign(input),text=Buffer.from(input).toString();if(text.startsWith('codync-sig-v1\nPOST\n') && text.includes('\n/v1/personal-bootstrap/publish\n')){signedHeaders.push(text);if(options.sleepAfterSignature)mono+=30001}return sig}}
  const pins={securePinsAvailable:()=>true,async readPinBytes(name:string,limit:number){const handle=await fs.promises.open(name,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW);try{const s=await handle.stat();assert(s.isFile() && s.nlink===1 && s.size<=limit && (s.mode&0o777)===0o600);return await handle.readFile()}finally{await handle.close()}}}
  const provenance=H(['m2-fixture-origin',dir]),originPath=path.join(dir,'origin');fs.writeFileSync(originPath,'public-origin',{mode:0o600});const originStat=fs.statSync(originPath)
  const capturePersonal=async()=>{
    helperCaptures++;let alive=true
    const current=()=>alive && active,check=async()=>{assert(current());const s=await fs.promises.lstat(originPath);assert.equal(s.ino,originStat.ino);assert.equal(await fs.promises.readFile(originPath,'utf8'),'public-origin');assert(current())}
    return{kind:'localHelper',sourceId:'local-linux',possessionURL:'ws://127.0.0.1:19000/personal-possession',current,check,provenance:()=>provenance,originRecord:()=>['localHelper','local-linux','/run/donnabot/bootstrap-helper-v1.sock',provenance],validateAt(){assert(current())},helper:{async request(request:unknown[]){assert.equal(JSON.stringify(request),'["time"]');return signed('issuer',[D('time'),C,crypto.b64url(crypto.random(32)),f.T[11],epoch+(mono-1000),300000])}},close(){alive=false}}
  }
  const fetcher=async(url:URL,init:RequestInit)=>{
    const raw=Buffer.from(init.body as Uint8Array),body=JSON.parse(raw.toString()),route=url.pathname.split('/').pop()!,headers=init.headers as Record<string,string>,fields=Object.fromEntries(headers['Codync-Sig'].split(',').map(x=>x.split('=')))
    assert.equal(init.redirect,'error');assert.equal(init.cache,'no-store');assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(fields.sig,64)!,Buffer.from(crypto.sigCanonical('POST',url.host,url.pathname,Number(fields.ts),fields.nonce,raw))))
    if(route==='publish'){
      publishes++;publishedBodies.push(raw.toString());assert.deepEqual(Object.keys(body),['publish']);const checked=new wire.PersonalVerifier(C,w.keys).verify('publish',bytes(body.publish),{kind:'current',now:epoch+(mono-1000)+2})
      assert.equal(checked.payload[2],f.T[1]);assert.equal(checked.payload[3],f.pin.hT);assert.equal(checked.payload[4],H(f.packet.grant[0]));assert.equal(checked.payload[5],f.packet.receipt[0][6]);assert.equal(checked.payload[6],H(f.packet.receipt[0]));assert.equal(checked.payload[7],3)
      publication=body.publish;if(options.losePublication)throw new Error('controlledLostPublicationResponse')
      return new Response(JSON.stringify({intentId:f.T[1],candidateId:f.T[9]}),{status:200})
    }
    assert.equal(route,'read');reads++;const q=body.read[0],now=epoch+(mono-1000)
    const originals=Object.fromEntries(Object.entries(session.personalRemoteNames).map(([remote,local])=>[remote,local==='publish'?publication:local==='receipt' && options.missingRemoteReceipt?null:['activation','fence','fence-ack','revocation'].includes(local)?null:f.packet[local]]))
    const state=signed('issuer',[D('state'),C,f.T[1],f.T[9],f.pin.hT,publication?'published':'hubPending',publication?4:3,options.missingRemoteReceipt?null:f.packet.receipt[0][6],H(f.packet.grant[0]),options.missingRemoteReceipt?null:H(f.packet.receipt[0]),null,null,null,null,q[5],now,now+30000])
    return new Response(JSON.stringify({state,originals,activationCurrent:null}),{status:200})
  }
  const boundaries={...primitives,'node:fs':fs,'node:path':path,electron:{app:{getPath:()=>dir},safeStorage,powerMonitor},'./pins':pins,'./personal-origin':{capturePersonal},'./personal-authority':{personalAuthority:()=>authority},'./device-keys':{deviceSigner:async()=>serviceSigner},'./personal-runtime':{async qualifyPersonalRuntime(current:()=>boolean){let alive=true;return{quantum:1 as const,generation:crypto.b64url(crypto.random(16)),sample(){assert(alive && current());return{lo:mono,hi:mono+1}},auxiliary:()=>mono,current(){assert(alive && current())},close(){alive=false}}}}}
  const environment={...globals,fetch:fetcher}
  const {PersonalJournalStore}=loadMainModule<typeof import('./personal-journal.ts')>('fork-cloud/personal-journal',boundaries,environment),store=new PersonalJournalStore(p,binding)
  const initial=await store.create('command','local-linux'),originals=Object.fromEntries(session.personalOriginalNames.slice(0,13).map(name=>[name,JSON.stringify(f.packet[name])]))
  const receipt=await store.advance(initial,'receipt',originals,null,{facts:{origin:provenance,enrollment:null,candidate:null,closure:null}})
  const {PersonalService}=loadMainModule<typeof import('./personal-service.ts')>('fork-cloud/personal-service',boundaries,environment)
  const {ForkCloudService}=loadMainModule<typeof import('./service.ts')>('fork-cloud/service',{...boundaries,'./personal-service':{PersonalService},'./principal':{capturePrincipal:()=>p},'./account-api':{},'./enroll':{},'./connection':{},'./managed-connection':{}},environment)
  const service=new ForkCloudService(account as never,{} as never,{captureLocal:()=>null})
  return{service,store,initial:receipt,stats:()=>({reads,publishes,helperCaptures,publishedBodies:[...publishedBodies],signedHeaders:[...signedHeaders]}),async close(){service.dispose();active=false;storageKey.fill(0);for(const name of fs.readdirSync(dir))await fs.promises.unlink(path.join(dir,name));await fs.promises.rmdir(dir)}}
}

test('PL1 M2 invoke: R durável exige Read exato, publica A11 com revisãoS e conserva operação no unknown',async()=>{
  const f=await publicationFixture({losePublication:true});try{
    const first=await f.service.invoke({kind:'personalCheck',attemptId:f.initial.attemptId});assert.equal(first.personalAttempt?.phase,'unknown')
    const held=await f.store.load();assert(held?.originals.publish && held.sending?.route==='publish');const original=held.originals.publish,sending=JSON.stringify(held.sending)
    const next=await f.service.invoke({kind:'personalReconcile',attemptId:f.initial.attemptId}),after=await f.store.load()
    assert.equal(next.personalAttempt?.phase,'receipt');assert(after);assert.equal(after.originals.publish,original);assert.equal(after.sending,null);assert.equal(f.stats().publishes,1);assert.equal(f.stats().signedHeaders.length,1);assert.equal(JSON.parse(sending).body,f.stats().publishedBodies[0]);assert.equal(next.attachment,undefined)
    for(const[name,value]of Object.entries(f.initial.originals))assert.equal(after.originals[name],value)
  }finally{await f.close()}
})

test('PL1 M2 invoke: R ainda ausente no Worker só consulta e não cria publish ou operação nova',async()=>{
  const f=await publicationFixture({missingRemoteReceipt:true});try{
    await f.service.invoke({kind:'personalCheck',attemptId:f.initial.attemptId});const after=await f.store.load();assert(after);assert.equal(after.originals.publish,undefined);assert.equal(after.sending,null);assert.equal(f.stats().reads,1);assert.equal(f.stats().publishes,0)
  }finally{await f.close()}
})

test('PL1 M2 invoke: assinaturaHTTP cruza prazo de publish, original durável fica unknown e ZEROenvio',async()=>{
  const f=await publicationFixture({sleepAfterSignature:true});try{
    await f.service.invoke({kind:'personalCheck',attemptId:f.initial.attemptId});const after=await f.store.load();assert(after?.originals.publish && after.sending?.route==='publish');assert.equal(f.stats().signedHeaders.length,1);assert.equal(f.stats().publishes,0)
  }finally{await f.close()}
})

// A qualificação acima é de Clock/crypto/protocolo reais com transportes/tempo
// externos controlados. Não executa PINFS, helper/PID antigo, Owner ou runtimeSO.
