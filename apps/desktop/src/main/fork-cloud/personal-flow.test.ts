import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as path from 'node:path'
import * as os from 'node:os'
import * as nodeCrypto from 'node:crypto'
import * as http from 'node:http'
import * as net from 'node:net'
import { EventEmitter } from 'node:events'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import * as nobleEd25519 from '@noble/curves/ed25519.js'
import * as nobleUtils from '@noble/curves/utils.js'
import * as crypto from './crypto.ts'
import * as wire from './personal-wire.ts'
import * as protocol from './personal-protocol.ts'
import { deferred,loadMainModule } from '../../shared/test-support/main-module.ts'
import type { Principal } from './principal.ts'
import type { PersonalJournal } from './personal-journal.ts'
import type { DeviceBinding } from '../fork-account/service.ts'
const all=JSON.parse(fs.readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json',import.meta.url),'utf8'))
const v=all.personalBootstrap,w=v.wireVectors,C=v.context,D=wire.personalDomain,bytes=(x:unknown)=>Buffer.from(JSON.stringify(x)),H=wire.personalHash
const signer=(role:'app'|'hub'|'issuer')=>crypto.signerFromSeed(crypto.unb64url(w.seeds[role],32)!)
const signed=(role:'app'|'hub'|'issuer',p:unknown[])=>[p,crypto.b64url(signer(role).sign(bytes(p)))]
const authority=protocol.inspectPersonalManifest(Buffer.from(v.manifestCanonical))
const primitives={'node:crypto':nodeCrypto,'@noble/ciphers/chacha.js':{chacha20poly1305},'@noble/curves/ed25519.js':nobleEd25519,'@noble/curves/utils.js':nobleUtils,'./crypto':crypto,'./personal-wire':wire,'./personal-protocol':protocol}
const globals={Buffer,Object,process,URL,TextEncoder,TextDecoder,Uint8Array,ArrayBuffer,DataView,performance,setTimeout,clearTimeout}
const prepare=loadMainModule<typeof import('./personal-prepare-wire.ts')>('fork-cloud/personal-prepare-wire',primitives,globals)

test('PL1 C3b.1: Main e Rust compõem os mesmos bytes prepare; direção, AAD, replay e limites recusam',()=>{
  const Q=w.packet['local-ticket'],id=prepare.prepareIdentity(C,Q),a=crypto.ephemeral(crypto.unb64url(all.handshake.deviceEphPriv,32)!),h=crypto.ephemeral(crypto.unb64url(all.handshake.hostEphPriv,32)!)
  const hello=[D('prepare-hello'),1,C,id.capture,id.hq,id.wireHash,'command',w.keys.app,crypto.b64url(a.pub),crypto.b64url(Buffer.alloc(32,81))]
  const welcome=[D('prepare-welcome'),1,C,id.capture,id.hq,id.wireHash,crypto.b64url(h.pub),crypto.b64url(Buffer.alloc(32,82))],th=prepare.prepareTranscript(hello,welcome)
  assert.equal(Buffer.from(th).toString('hex'),'014fc9fd817b2269abe353795a34b94a3aa95ae27c1a45a6957bc630b1018b4e')
  const body=`{"ticket":${JSON.stringify(Q)},"token":${JSON.stringify(crypto.b64url(Buffer.alloc(32,83)))}}`,digest=prepare.personalHashBytes(Buffer.from(body)),op=prepare.prepareOperation(C,id.capture,id.hq,id.wireHash,digest)
  assert.equal(digest,'032d9e050ce91a1d0f64d375abc2543683930125b4ca65baeace81882163a377')
  assert.equal(op,'445831d23af34ac025ebfd100584bb43524a9c4993eb77358bc0d028a79ca176')
  const request=[D('prepare-request'),1,crypto.b64url(Buffer.alloc(16,84)),id.hq,'candidate',op,digest,crypto.b64url(Buffer.alloc(32,83))],frames=new prepare.PrepareFrames(hello,th,a,h.pub),frame=frames.request(request)
  assert.equal(H(frame),'c746436aa5519f1d30e1851cfaf91755918c1e7361bd21bc2f4e9c7776e8c193');assert.throws(()=>frames.request(request))
  assert.throws(()=>frames.response(frame));assert.throws(()=>new prepare.PrepareFrames(hello,th,a,new Uint8Array(32)))
  const encoded=prepare.prepareMessage(hello,16384);assert.equal(JSON.stringify(prepare.prepareDecode(encoded,16384)),JSON.stringify(hello));assert.throws(()=>prepare.prepareDecode(encoded.slice(1),16384));assert.throws(()=>prepare.prepareMessage('x'.repeat(16385),16384));frames.clear()
})

/** Só OS/conta/HTTP/helper/peer são controlados. Service, IPC intent, journal FS,
 * assinaturas, POP e AEAD são os consumidores de produção. O Hub real/SQLite é
 * exercitado separadamente por personal_hub_vertical_* no Rust, com os vetores acima. */
function journalSyncBarrier() {
  let route:string|null=null
  const entered=deferred<PersonalJournal>(),resume=deferred()
  return {
    arm(value:string){assert.equal(route,null);route=value},
    async afterSync(journal:PersonalJournal){if(route!==null && journal.sending?.route===route){route=null;entered.resolve(journal);await resume.promise}},
    wait(pending:Promise<unknown>){return Promise.race([entered.promise,pending.then(()=>{throw new Error('fixture did not reach the requested journal fsync')})])},
    release(){resume.resolve()},
  }
}
function tokenAwaitBarrier() {
  let route:string|null=null
  const entered=deferred<PersonalJournal>(),resume=deferred()
  return {
    arm(value:string){assert.equal(route,null);route=value},
    async beforeReturn(journal:PersonalJournal){if(route!==null && journal.sending?.route===route){route=null;entered.resolve(journal);await resume.promise}},
    wait(pending:Promise<unknown>){return Promise.race([entered.promise,pending.then(()=>{throw new Error('fixture did not reach the requested token await')})])},
    release(){resume.resolve()},
  }
}
// Barreira privada por fixture: fsync já ocorreu no temporário, antes do rename.
function journalOriginalBarrier() {
  let name:string|null=null
  const entered=deferred<PersonalJournal>(),resume=deferred()
  return {
    arm(value:string){assert.equal(name,null);name=value},
    async afterSync(journal:PersonalJournal){if(name!==null && journal.originals[name]!==undefined && journal.sending===null){name=null;entered.resolve(journal);await resume.promise}},
    wait(pending:Promise<unknown>){return Promise.race([entered.promise,pending.then(()=>{throw new Error('fixture did not reach original fsync before rename')})])},
    release(){resume.resolve()},
  }
}
// Scheduler externo privado: mantém o await de drive e permite cair a conexão
// durante a espera, sem esperar cinco segundos de parede em cada rodada focal.
function r40DriveBarrier() {
  let armed=false
  const entered=deferred(),resume=deferred()
  return {
    arm(){assert.equal(armed,false);armed=true},
    async beforeResume(){if(armed){armed=false;entered.resolve();await resume.promise}},
    wait(pending:Promise<unknown>){return Promise.race([entered.promise,pending.then(()=>{throw new Error('fixture did not reach drive wait after status')})])},
    release(){resume.resolve()},
  }
}
async function fixture(mode:'command'|'device',options:{barrier?:ReturnType<typeof journalSyncBarrier>;tokenBarrier?:ReturnType<typeof tokenAwaitBarrier>;originalBarrier?:ReturnType<typeof journalOriginalBarrier>;afterHttpSignature?:(input:Uint8Array,signature:Uint8Array)=>void;originFile?:boolean;quantum?:1|1000;helperDelayMs?:number;workerDelayMs?:number;runtimeUnavailable?:boolean;fastDrive?:boolean;driveBarrier?:ReturnType<typeof r40DriveBarrier>;helperConnections?:boolean;challengeTtlMs?:number;afterPossessionSeal?:(plain:Uint8Array,ciphertext:string)=>void;afterPrepareRequest?:(inner:unknown,frame:unknown)=>void}={}) {
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'personal-flow-')),key=nodeCrypto.randomBytes(32),powerMonitor=new EventEmitter();let epochOffset=0,elapsedOffset=0,auxiliaryOffset=0;const now=()=>w.now+epochOffset,shift=(ms:number)=>{assert(Number.isSafeInteger(ms) && ms>0);elapsedOffset+=ms;epochOffset+=ms;auxiliaryOffset+=ms}
  let active=true,reads=0,posts=0,candidates=0,tickets=0,candidateDigest:string|null=null,lRetained=false,stopBeforeCandidate=false,lostCandidate=false,lostEnrollment=false,lostCommand=false,failRead=false
  const originals:Record<string,any>={},app=signer('app'),hub=signer('hub'),token=crypto.b64url(Buffer.alloc(32,83)),userCode='ABCD2345EFGH',enrollment={createdAt:w.now+47,expiresAt:w.now+500000,intervalMs:5000}
  // O signer continua síncrono e calcula Ed25519 real. O callback privado modela
  // preempção externa depois da assinatura, antes da próxima instrução do POST.
  const serviceSigner:crypto.DeviceSigner=options.afterHttpSignature?{publicKey:app.publicKey,sign(input){const signature=app.sign(input);if(Buffer.from(input).toString().startsWith('codync-sig-v1\nPOST\n'))options.afterHttpSignature!(input,signature);return signature}}:app
  const httpRequests:{route:string;body:string;timestamp:number;canonical:string;signature:string}[]=[]
  const effects={consent:0,status:0,retain:0,proof:0,appConsent:0,approve:0},originPath=path.join(dir,'captured-origin'),originBytes='independent-source-before-fsync'
  const delivered:{purpose:string;operation:string;request:string;ciphertext:string}[]=[],peers:Peer[]=[]
  let driveDelayPending=false,captures=0,helperServer:net.Server|null=null
  const helperClients:net.Socket[]=[],helperPeers:net.Socket[]=[],helperPath=path.join(dir,'helper.sock')
  if(options.originFile)fs.writeFileSync(originPath,originBytes,{mode:0o600})
  const binding:DeviceBinding={version:1,sessionId:'fixture-session',userId:w.packet.intent[0][4],origin:C[3],issuer:C[4],deploymentId:C[5],manifestHash:C[6],deviceId:w.packet.intent[0][5],deviceKey:crypto.b64url(app.publicKey)}
  const p={namespace:'fixture',seal:{epoch:1,sessionId:binding.sessionId,userId:binding.userId},deployment:{env:C[2],origin:C[3],issuer:C[4],deploymentId:C[5],manifestHash:C[6]},current:()=>active,token:async()=>{
    // Provedor de token externo controlado. Consulta o journal cifrado REAL
    // apenas para selecionar a espera do efeito, sem alterar CAS ou originais.
    if(options.tokenBarrier){const names=await fs.promises.readdir(dir),name=names.find(n=>n.startsWith('personal-journal-') && n.endsWith('.bin'));assert(name);const sealed=await fs.promises.readFile(path.join(dir,name));const journal=JSON.parse(safeStorage.decryptString(sealed)) as PersonalJournal;await options.tokenBarrier.beforeReturn(journal)}
    return 'discardable-test-session'
  },personalBootstrapStatus:{profile:'personal-linux-v1',wire:1,available:false,reason:'helperRequired'}} as Principal
  const account={sessionEpoch:1,on(){},off(){},verifiedDeviceBinding:()=>binding}
  const verifier=new wire.PersonalVerifier(C,w.keys),packet=()=>Object.fromEntries(Object.entries(originals).filter(([k])=>k!=='create'&&k!=='attach'&&k!=='cancel')),ceremony=()=>wire.verifyPersonalCeremony(verifier,bytes(packet()),{kind:'current',now:now()})
  const remote:Record<string,string>={intent:'intent',ticket:'local-ticket',appConsent:'app-consent',localConsent:'local-consent',boxChallenge:'box-challenge',boxProof:'box-proof',approval:'approve',localConfirmation:'local-confirm',consume:'consume',grant:'grant',challenge:'challenge',proof:'proof',receipt:'receipt'}
  const state=(read:any[])=>{
    const c=originals.intent && originals['local-ticket']?wire.verifyPersonalCeremony(verifier,bytes(packet()),{kind:'historical',now:now()}):null
    const phase=originals.cancel?'canceled':originals.intent[0][9]<=now()?'expired':originals.grant?'hubPending':originals['local-consent']?'personalConfirmed':'open'
    return signed('issuer',[D('state'),C,originals.intent[0][2],read[3],read[4],phase,1,null,originals.grant?H(originals.grant[0]):null,null,null,null,null,null,read[5],now(),c?.tuple[18]??originals.intent[0][9]])
  }
  const safeStorage={isEncryptionAvailable:()=>true,getSelectedStorageBackend:()=>'gnome_libsecret',encryptString(s:string){const n=nodeCrypto.randomBytes(12),c=nodeCrypto.createCipheriv('aes-256-gcm',key,n),data=Buffer.concat([c.update(s),c.final()]);return Buffer.concat([n,c.getAuthTag(),data])},decryptString(b:Buffer){const d=nodeCrypto.createDecipheriv('aes-256-gcm',key,b.subarray(0,12));d.setAuthTag(b.subarray(12,28));return Buffer.concat([d.update(b.subarray(28)),d.final()]).toString()}}
  const helper={async request(r:any[]){
    assert(active)
    if(r[0]==='time'){if(options.helperDelayMs){await new Promise<void>(resolve=>setTimeout(resolve,options.helperDelayMs));shift(options.helperDelayMs)}const at=now();return signed('issuer',[D('time'),C,crypto.b64url(crypto.random(32)),w.tuple[11],at,300000])}
    const at=now()
    if(r[0]==='ticket'){tickets++;assert.equal(r[1],mode);if(mode==='command')assert.equal(JSON.stringify(r[2]),JSON.stringify(originals.intent));const id=mode==='command'?originals.intent[0][2]:'pi_fixture_device';originals['local-ticket']=signed('hub',[D('local-ticket'),C,id,mode,'pc_fixture',w.tuple[10],w.tuple[11],w.keys.hub,w.keys.box,crypto.b64url(crypto.random(32)),'first-owner',at,at+550000]);return originals['local-ticket']}
    const c=ceremony()
    if(r[0]==='consent'){assert.equal(H(r[3][0]),H(originals['app-consent'][0]));effects.consent++;originals['local-consent']=signed('hub',[D('local-consent'),C,c.hash,H(originals['app-consent'][0]),'approve',crypto.b64url(crypto.random(32)),at,c.tuple[18]]);return originals['local-consent']}
    if(r[0]==='retain'){assert.equal(H(r[1][0]),H(originals['local-confirm'][0]));effects.retain++;lRetained=true;return['localConfirmation',c.hash,H(r[1][0])]}
    assert.equal(r[0],'status');assert.equal(r[1],c.hash);effects.status++
    if(originals['box-challenge'] && !originals['box-proof']){const secret=wire.personalBoxOpen(crypto.unb64url(w.seeds.box,32)!,c.originals['box-challenge']!,c.hash,c.originals['app-consent']!.hash,c.originals['local-consent']!.hash);originals['box-proof']=signed('hub',[D('box-proof'),C,c.hash,H(originals['box-challenge'][0]),secret,crypto.b64url(crypto.random(32)),at,originals['box-challenge'][0][10]])}
    if(originals.approve && !originals['local-confirm'])originals['local-confirm']=signed('hub',[D('local-confirm'),C,c.hash,H(originals.approve[0]),H(originals['box-proof'][0]),'approve',crypto.b64url(crypto.random(32)),at,originals.approve[0][8]])
    if(lRetained && !originals.grant){originals.consume=signed('hub',[D('consume'),C,c.hash,H(originals.approve[0]),H(originals['local-confirm'][0]),c.tuple[16],'op_fixture',at,originals['local-confirm'][0][8]]);originals.grant=signed('issuer',[D('grant'),C,c.tuple,H(originals['box-challenge'][0]),H(originals['box-proof'][0]),H(originals.approve[0]),H(originals['local-confirm'][0]),H(originals.consume[0]),crypto.b64url(crypto.random(32)),at,Math.min(at+60000,c.tuple[18] as number),['control','owner.devices']])}
    ceremony();if(options.fastDrive)driveDelayPending=true;return[originals['local-confirm']?'confirmed':'consented',c.hash,originals['local-ticket'],originals['local-consent'],originals['local-confirm']??null,null,1]
  }}
  const capture={kind:process.platform==='linux'?'localHelper':'sshHelper',sourceId:'fixture-source',helper,possessionURL:'ws://127.0.0.1:19000/personal-possession',current:()=>active,provenance:()=>H(['independent-fixture-ca-and-process']),validateAt(){assert(active)},async check(){assert(active);if(options.originFile && await fs.promises.readFile(originPath,'utf8')!==originBytes)throw new Error('controlledCapturedOriginChanged')},close(){}}
  let latestCapture=capture
  const captureSource=async()=>{
    captures++
    if(!options.helperConnections)return capture
    if(!helperServer){helperServer=net.createServer(socket=>{helperPeers.push(socket);socket.on('error',()=>{})});await new Promise<void>((resolve,reject)=>{helperServer!.once('error',reject);helperServer!.listen(helperPath,()=>{helperServer!.off('error',reject);resolve()})})}
    const identity=fs.statSync(helperPath),provenance=H(['localHelper',helperPath,identity.dev,identity.ino])
    const socket=net.connect(helperPath);helperClients.push(socket);socket.on('error',()=>{})
    await new Promise<void>((resolve,reject)=>{socket.once('connect',resolve);socket.once('error',reject)})
    const current=()=>active && !socket.destroyed && socket.readyState==='open'
    const check=async()=>{assert(current());const named=await fs.promises.stat(helperPath);assert(named.isSocket());assert.equal(named.dev,identity.dev);assert.equal(named.ino,identity.ino);assert(current())}
    latestCapture={...capture,current,provenance:()=>provenance,check,validateAt(){assert(current())},helper:{async request(r:any[]){await check();const reply=await helper.request(r);await check();return reply}},close(){socket.destroy()}}
    return latestCapture
  }
  // Transporte WebSocket externo controlado, com assinatura/DH/KDF/AEAD reais.
  class Peer {
    static OPEN=1;readyState=1;binaryType='';listeners=new Map<string,((e:any)=>void)[]>();hello:any;th!:Uint8Array;e=crypto.ephemeral();a2h!:Uint8Array;h2a!:Uint8Array;tx=0;rx=0;purpose:string
    constructor(url:URL){this.purpose=url.pathname;peers.push(this);queueMicrotask(()=>this.emit('open',{}))}
    addEventListener(n:string,f:(e:any)=>void){this.listeners.set(n,[...(this.listeners.get(n)??[]),f])}
    emit(n:string,e:any){for(const f of this.listeners.get(n)??[])f(e)}
    reply(value:any,binary=false){queueMicrotask(()=>{if(this.readyState===1){const b=binary?prepare.prepareMessage(value,32768):null;this.emit('message',{data:b?b.buffer.slice(b.byteOffset,b.byteOffset+b.byteLength):JSON.stringify(value)})}})}
    close(){if(this.readyState!==1)return;this.readyState=3;this.emit('close',{})}
    send(raw:Uint8Array|string){
      if(this.purpose==='/personal-prepare'){
        const message=prepare.prepareDecode(raw as Uint8Array,32768) as any[]
        if(!this.hello){this.hello=message;const Q=originals['local-ticket'],id=prepare.prepareIdentity(C,Q);assert.equal(JSON.stringify(message.slice(2,7)),JSON.stringify([C,id.capture,id.hq,id.wireHash,mode]));assert.equal(message[7],mode==='command'?binding.deviceKey:null)
          const welcome=[D('prepare-welcome'),1,C,id.capture,id.hq,id.wireHash,crypto.b64url(this.e.pub),crypto.b64url(crypto.random(32))];this.th=prepare.prepareTranscript(message,welcome);const dh=this.e.dh(crypto.unb64url(message[8],32)!);this.a2h=new Uint8Array(nodeCrypto.hkdfSync('sha256',dh,this.th,Buffer.from(D('prepare-key-a2h')),32));this.h2a=new Uint8Array(nodeCrypto.hkdfSync('sha256',dh,this.th,Buffer.from(D('prepare-key-h2a')),32));this.reply([...welcome,crypto.b64url(hub.sign(this.th))],true);return}
        if(message[0]===D('prepare-finish')){assert.equal(message[2],crypto.b64url(this.th));if(mode==='command')assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(message[3],64)!,bytes(message.slice(0,3))));else assert.equal(message[3],null);return}
        assert.equal(message[2],'a2h');assert.equal(message[3],0)
        const aad=(direction:string)=>bytes([D('prepare-frame'),1,C,this.hello[3],this.hello[4],this.hello[5],crypto.b64url(this.th),direction,0]),plain=chacha20poly1305(this.a2h,new Uint8Array(12),aad('a2h')).decrypt(crypto.unb64url(message[4])!),r=protocol.personalCanonical(plain) as any[];plain.fill(0)
        // Entrega possível é contada ANTES da validação de negócio do peer.
        delivered.push({purpose:'prepare',operation:r[4],request:JSON.stringify(r),ciphertext:message[4]})
        let facts:any[]
        if(mode==='command'){assert.equal(r[4],'candidate');assert.equal(r[7],token);const body=`{"ticket":${JSON.stringify(originals['local-ticket'])},"token":${JSON.stringify(r[7])}}`;assert.equal(r[6],prepare.personalHashBytes(Buffer.from(body)));assert.equal(r[5],prepare.prepareOperation(C,this.hello[3],this.hello[4],this.hello[5],r[6]));if(candidateDigest===null){candidateDigest=r[6];candidates++}else assert.equal(candidateDigest,r[6]);if(stopBeforeCandidate){active=false;this.close();return}if(lostCandidate){lostCandidate=false;this.close();return}facts=[this.hello[4],this.hello[5],H(originals.intent[0]),originals.intent[0][2],originals['local-ticket'][0][4],r[6]]}
        else{assert.equal(r[4],'enrollment');if(lostEnrollment){lostEnrollment=false;this.close();return}facts=[this.hello[4],this.hello[5],originals['local-ticket'][0][2],originals['local-ticket'][0][4],userCode,enrollment.createdAt,enrollment.expiresAt,5000]}
        const response=[D('prepare-response'),1,r[2],r[4]==='candidate'?r[5]:null,'ok',r[4],facts],sealed=chacha20poly1305(this.h2a,new Uint8Array(12),aad('h2a')).encrypt(bytes(response));this.reply([D('prepare-frame'),1,'h2a',0,crypto.b64url(sealed)],true);return
      }
      assert.equal(this.purpose,'/personal-possession');const r=JSON.parse(raw as string)
      if(!this.hello){this.hello=r;assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(r[7],64)!,bytes(r.slice(0,7))));this.th=crypto.sha256(bytes([D('possession-transcript'),r,crypto.b64url(this.e.pub)]));const keys=crypto.channelKeys(this.e.dh(crypto.unb64url(r[5],32)!),this.th);this.a2h=keys.d2h;this.h2a=keys.h2d;this.reply([D('possession-welcome'),C,r[2],r[3],crypto.b64url(this.e.pub),crypto.b64url(this.th),crypto.b64url(hub.sign(this.th))]);return}
      assert.equal(r.c,this.rx);const q=JSON.parse(Buffer.from(crypto.openFrame(this.a2h,this.rx++,r.d).chunk).toString())
      // R40: mesmo F rejeitado por ceremony já atravessou WebSocket.send.
      delivered.push({purpose:'possession',operation:q[0],request:JSON.stringify(q),ciphertext:r.d})
      const c=ceremony();assert.equal(JSON.stringify(q[1]),JSON.stringify(originals.grant));const at=now();let response:any
      if(q[0]==='challenge'){originals.challenge=signed('hub',[D('challenge'),C,H(originals.grant[0]),c.hash,crypto.b64url(crypto.random(32)),at,Math.min(at+(options.challengeTtlMs??60000),originals.grant[0][10])]);response=originals.challenge}
      else{assert.equal(q[0],'proof');assert.equal(JSON.stringify(q[2]),JSON.stringify(originals.challenge));originals.proof=q[3];assert.equal(originals.proof[0][8],originals.challenge[0][6]);ceremony();effects.proof++;const grantId=`grt_${crypto.b64url(Buffer.alloc(16,13))}`,pending=[D('pending'),C,c.hash,grantId,H(originals.grant[0]),H(originals.challenge[0]),H(originals.proof[0]),at,2,'pendingPublication'];originals.receipt=signed('hub',[D('receipt'),C,c.hash,H(originals.grant[0]),H(originals.challenge[0]),H(originals.proof[0]),grantId,at,2,1,'a'.repeat(64),H(pending)]);ceremony();response=originals.receipt}
      this.reply({t:'f',c:this.tx,d:crypto.sealFrame(this.h2a,this.tx++,true,bytes(response))})
    }
  }
  const fetcher=async(url:URL,init:RequestInit)=>{
    if(options.workerDelayMs){await new Promise<void>(resolve=>setTimeout(resolve,options.workerDelayMs));shift(options.workerDelayMs)}
    const route=url.pathname.split('/').pop()!,body=JSON.parse(Buffer.from(init.body as Uint8Array).toString()),headers=init.headers as Record<string,string>,sig=Object.fromEntries(headers['Codync-Sig'].split(',').map(s=>s.split('='))),raw=Buffer.from(init.body as Uint8Array)
    assert.equal(init.redirect,'error');assert.equal(init.cache,'no-store');assert.equal(headers.Authorization,'Bearer discardable-test-session');const input=crypto.sigCanonical('POST',url.host,url.pathname,Number(sig.ts),sig.nonce,raw);assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(sig.sig,64)!,Buffer.from(input)))
    if(options.afterHttpSignature)httpRequests.push({route,body:raw.toString(),timestamp:Number(sig.ts),canonical:input,signature:sig.sig})
    let reply:unknown;posts++
    if(route==='command'){const c=body.create[0];originals.create=body.create;originals.intent=signed('issuer',[D('intent'),C,c[2],'command',binding.userId,binding.deviceId,binding.deviceKey,c[3],c[5],c[6],1]);if(lostCommand){lostCommand=false;throw new Error('controlledLostResponse')}reply={intent:originals.intent,token}}
    else if(route==='attach'){assert.equal(body.userCode,userCode);assert.equal(body.attach[0][5],enrollment.createdAt);assert.equal(body.attach[0][6],enrollment.expiresAt);assert.notEqual(body.attach[0][5],originals['local-ticket'][0][11]);const a=body.attach[0];originals.attach=body.attach;originals.intent=signed('issuer',[D('intent'),C,a[2],'device',binding.userId,binding.deviceId,binding.deviceKey,a[4],a[5],a[6],1]);reply={intent:originals.intent}}
    else if(route==='read'){reads++;if(failRead)throw new Error('controlledReadUnavailable');reply={state:state(body.read[0]),originals:Object.fromEntries(Object.entries(remote).map(([r,l])=>[r,originals[l]??null])),activationCurrent:null}}
    else if(route==='cancel'){originals.cancel=body.cancel;reply={intentId:body.cancel[0][2],candidateId:null}}
    else{const names:Record<string,string>={'app-consent':'appConsent','box-challenge':'boxChallenge',approve:'approval'};assert(names[route]);originals[route]=body[names[route]];ceremony();if(route==='app-consent')effects.appConsent++;if(route==='approve')effects.approve++;reply={intentId:originals.intent[0][2],candidateId:originals['local-ticket'][0][4]}}
    return new Response(JSON.stringify(reply),{status:200})
  }
  // Seam SOMENTE neste teste/loadMainModule. Os handles/bytes/cifra/rename/CAS
  // são reais; suspende a conclusão do fsync já realizado, sem alterar a gravação.
  const fsBoundary=options.barrier || options.originalBarrier?{...fs,promises:{...fs.promises,async open(...args:Parameters<typeof fs.promises.open>){
    const file=await fs.promises.open(...args);let observed:PersonalJournal|null=null
    return new Proxy(file,{get(target,property){
      if(property==='writeFile')return async(...params:Parameters<typeof file.writeFile>)=>{await file.writeFile(...params);const data=params[0];if(Buffer.isBuffer(data) || data instanceof Uint8Array){const value=JSON.parse(safeStorage.decryptString(Buffer.from(data))) as PersonalJournal;if(value.schema===2)observed=value}}
      if(property==='sync')return async()=>{await file.sync();if(observed){await options.barrier?.afterSync(observed);await options.originalBarrier?.afterSync(observed)}}
      const value=Reflect.get(target,property,target);return typeof value==='function'?value.bind(target):value
    }})
  }}}:fs
  // Cifra e PrepareFrames de produção executam integralmente; callbacks privados
  // modelam preempção externa APÓS obter o ciphertext, sem trocar seus bytes.
  const cryptoBoundary=options.afterPossessionSeal?{...crypto,sealFrame(...args:Parameters<typeof crypto.sealFrame>){const ciphertext=crypto.sealFrame(...args);options.afterPossessionSeal!(args[3],ciphertext);return ciphertext}}:crypto
  const prepareBoundary=options.afterPrepareRequest?{...prepare,PrepareFrames:class extends prepare.PrepareFrames {override request(inner:unknown){const frame=super.request(inner);options.afterPrepareRequest!(inner,frame);return frame}}}:prepare
  const boundaries={...primitives,'./crypto':cryptoBoundary,'node:fs':fsBoundary,'node:path':path,electron:{app:{getPath:()=>dir},safeStorage,powerMonitor},'./device-keys':{deviceSigner:async()=>serviceSigner},'./personal-authority':{personalAuthority:()=>authority},'./personal-origin':{capturePersonal:captureSource},'./personal-prepare-wire':prepareBoundary,'./personal-runtime':{async qualifyPersonalRuntime(current:()=>boolean){
    // Fronteira externa de runtime controlada SOMENTE no loadMainModule deste teste.
    // Não qualifica Node/Electron real; relógio/journal/crypto/service continuam reais.
    if(options.runtimeUnavailable)throw new Error('personalRuntimeUnavailable');const quantum=options.quantum??1;let open=true;return{quantum,generation:nodeCrypto.randomBytes(16).toString('hex'),auxiliary(){return auxiliaryOffset},current(){assert(open && current())},sample(){assert(open && current());const lo=elapsedOffset;return{lo,hi:lo+quantum}},close(){open=false}}
  }}}
  let retryAt=-1,stalledRetries=0,progressFailed=false
  const scheduler:typeof setTimeout=((callback:(...args:any[])=>void,ms?:number,...args:any[])=>{
    // Neste grafo, só issued() espera pelo quantum: cada retry anda os três
    // relógios antes de retomar. Timers de transporte/drive não entram aqui.
    const quantum=options.quantum??1
    if(ms===quantum){
      stalledRetries=elapsedOffset===retryAt?stalledRetries+1:1;retryAt=elapsedOffset
      if(stalledRetries>8){progressFailed=true;throw new Error('pl1FixtureLogicalTimeStalled')}
      return setTimeout(()=>{shift(quantum);callback(...args)},0)
    }
    if(options.fastDrive && driveDelayPending && ms===5000){driveDelayPending=false;return setTimeout(()=>{void(options.driveBarrier?.beforeResume()??Promise.resolve()).then(()=>callback(...args))},0)}
    return setTimeout(callback,ms,...args)
  }) as typeof setTimeout
  const mainGlobals={...globals,performance:{now:()=>auxiliaryOffset},setTimeout:scheduler,WebSocket:Peer,fetch:fetcher,Response,queueMicrotask}
  const {PersonalService}=loadMainModule<typeof import('./personal-service.ts')>('fork-cloud/personal-service',boundaries,mainGlobals)
  const {PersonalJournalStore}=loadMainModule<typeof import('./personal-journal.ts')>('fork-cloud/personal-journal',boundaries,mainGlobals)
  const {ForkCloudService}=loadMainModule<typeof import('./service.ts')>('fork-cloud/service',{...boundaries,'./personal-service':{PersonalService},'./principal':{capturePrincipal:()=>p},'./account-api':{},'./enroll':{},'./connection':{},'./pins':{},'./managed-connection':{}},mainGlobals)
  const makeService=()=>new ForkCloudService(account as never,{} as never,{captureLocal:()=>null}),service=makeService(),services=[service]
  return{service,effects:()=>({...effects}),captureCurrent:()=>latestCapture.current(),time:now,httpRequests:()=>httpRequests.map(r=>({...r})),delivered:()=>delivered.map(frame=>({...frame})),captures:()=>captures,possessionOpen:()=>peers.some(peer=>peer.purpose==='/personal-possession' && peer.readyState===1),helperIdentity:()=>{const s=fs.statSync(helperPath);return{dev:s.dev,ino:s.ino,provenance:latestCapture.provenance()}},async dropHelperConnection(){assert(options.helperConnections);const socket=helperClients.at(-1);assert(socket && !socket.destroyed);const closed=new Promise<void>(resolve=>socket.once('close',resolve));socket.destroy();await closed;assert.equal(latestCapture.current(),false)},readJournalSync(){const names=fs.readdirSync(dir),name=names.find(n=>n.startsWith('personal-journal-') && n.endsWith('.bin'));assert(name);const encrypted=fs.readFileSync(path.join(dir,name));return{encrypted,journal:JSON.parse(safeStorage.decryptString(encrypted)) as PersonalJournal}},async changeCapturedOrigin(){assert(options.originFile);await fs.promises.writeFile(originPath,'independent-source-replaced-during-fsync');assert(capture.current())},replaceOriginSync(){assert(options.originFile);fs.writeFileSync(originPath,'independent-source-replaced-after-cipher');assert.equal(latestCapture.current(),true)},restart(){for(const old of services)old.dispose();const next=makeService();services.push(next);return next},originals,storage:new PersonalJournalStore(p,binding),dir,powerMonitor,stats:()=>({reads,posts,candidates,tickets}),controls:{advance(ms:number){shift(ms)},silentSleep(ms:number){assert(Number.isSafeInteger(ms) && ms>0);elapsedOffset+=ms;epochOffset+=ms},expire(ms:number){epochOffset+=ms},cut(){active=false},loseCandidate(){lostCandidate=true},loseEnrollment(){lostEnrollment=true},loseCommand(){lostCommand=true},failRead(v:boolean){failRead=v},swapDuringCandidate(){stopBeforeCandidate=true}},close(){for(const s of services)s.dispose();for(const socket of [...helperClients,...helperPeers])socket.destroy();helperServer?.close();key.fill(0);fs.rmSync(dir,{recursive:true,force:true});assert(!progressFailed,'PL1 fixture: mais de 8 retries causais sem avanço da fonte lógica (pl1FixtureLogicalTimeStalled)')}}
}

test('PL1 C3b.1: invoke command → journal/prepare/POP/G/K/F/R real Main; não publica nem retorna segredos',async()=>{
  const f=await fixture('command')
  try{const denied=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source',root:authority});assert.equal(denied.state.kind,'notSent');assert.equal(f.stats().posts,0);assert.equal(fs.readdirSync(f.dir).length,0)
    const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval');const id=started.personalAttempt!.attemptId
    const done=await f.service.invoke({kind:'personalApprove',attemptId:id});assert.equal(done.personalAttempt?.phase,'receipt');assert.equal(f.stats().candidates,1);assert.equal(f.stats().tickets,1)
    const journal=await f.storage.load();assert(journal?.originals.receipt);assert.equal(journal.sending,null);assert.equal(journal.secret.commandToken,null);assert.equal(journal.secret.candidateRequest,null);assert.equal(journal.secret.candidateBody,null)
    assert(!JSON.stringify(done).includes('token'));assert(!JSON.stringify(done).includes('donna/personal-linux/'));assert.equal(done.attachment,undefined);assert(!f.originals.publish);assert(!f.originals.activation)
    assert.equal(JSON.stringify(JSON.parse(journal.originals.receipt)),JSON.stringify(f.originals.receipt));assert.equal(f.originals.proof[0][8],f.originals.challenge[0][6]);assert(f.stats().reads>=4)
  }finally{f.close()}
})
test('PL1 C3b.1: device attach recebe datas Worker distintas de Q e cancel autenticado fecha sem consumo',async()=>{
  const f=await fixture('device')
  try{const started=await f.service.invoke({kind:'personalStart',mode:'device',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval');const journal=await f.storage.load();assert(journal?.originals.attach);assert.equal(f.stats().tickets,1);assert.equal(f.stats().candidates,0);assert.equal(journal.secret.userCode,null);assert(!JSON.stringify(started).includes('ABCD2345EFGH'))
    const stopped=await f.service.invoke({kind:'personalCancel',attemptId:started.personalAttempt!.attemptId});assert.equal(stopped.personalAttempt?.phase,'closed');assert.equal(f.stats().tickets,1)
  }finally{f.close()}
})
test('PL1 C3b.1: command resposta perdida reconcilia por Read e cancela I sem regenerar token/Q',async()=>{
  const f=await fixture('command');f.controls.loseCommand()
  try{const start=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(start.personalAttempt?.phase,'unknown');const id=start.personalAttempt!.attemptId;const before=f.stats()
    const end=await f.service.invoke({kind:'personalCancel',attemptId:id});assert.equal(end.personalAttempt?.phase,'closed');assert.equal(f.stats().tickets,0);assert.equal(f.stats().candidates,0);assert(f.stats().posts>before.posts);assert.equal((await f.storage.load())!.secret.commandToken,null)
  }finally{f.close()}
})
test('PL1 C3b.1: troca da sessão durante candidate impede avanço e retorno; originais continuam cifrados',async()=>{
  const f=await fixture('command');f.controls.swapDuringCandidate()
  try{const out=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(out.personalAttempt,undefined);assert.equal(f.stats().candidates,1);assert.equal(f.stats().reads,0);const files=fs.readdirSync(f.dir);assert.equal(files.filter(n=>n.endsWith('.bin')).length,1);for(const file of files)assert(!fs.readFileSync(path.join(f.dir,file)).includes(Buffer.from(crypto.b64url(Buffer.alloc(32,83)))))
  }finally{f.close()}
})

test('PL1 C3b.1: candidate incerto reconcilia os mesmos bytes sem outro encaminhamento ou Q',async()=>{
  const f=await fixture('command');f.controls.loseCandidate()
  try{const start=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(start.personalAttempt?.phase,'unknown');const before=await f.storage.load();assert(before?.secret.candidateRequest);const observed=await f.service.invoke({kind:'personalReconcile',attemptId:start.personalAttempt!.attemptId});assert.equal(observed.personalAttempt?.phase,'awaitingApproval');const after=await f.storage.load();assert.equal(f.stats().candidates,1);assert.equal(f.stats().tickets,1);assert.equal(after!.originals['local-ticket'],before.originals['local-ticket']);assert.equal(after!.sent.candidate,before.sent.candidate);assert.equal(after!.secret.candidateRequest,null)
  }finally{f.close()}
})
test('PL1 C3b.1: device sem I interrompe observação e preserva fatos; não cancela Worker nem gera outra cerimônia',async()=>{
  const f=await fixture('device');f.controls.loseEnrollment()
  try{const start=await f.service.invoke({kind:'personalStart',mode:'device',sourceId:'fixture-source'});assert.equal(start.personalAttempt?.phase,'unknown');const count=f.stats(),before=await f.storage.load();assert(before?.originals['local-ticket']);assert(!before.originals.intent)
    const stopped=await f.service.invoke({kind:'personalCancel',attemptId:start.personalAttempt!.attemptId});assert.equal(stopped.personalAttempt?.phase,'stopped');assert.deepEqual(f.stats(),count);const after=await f.storage.load();assert.equal(after!.originals['local-ticket'],before.originals['local-ticket']);assert.equal(after!.facts.closure,null);assert(!f.originals.cancel)
  }finally{f.close()}
})

test('PL1 C3b.1: restore só lê FS; reconcile explícito conserva Q/request/operation e não reenvia candidate',async()=>{
  const f=await fixture('command');f.controls.loseCandidate()
  try{const start=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(start.personalAttempt?.phase,'unknown');const before=await f.storage.load(),stats=f.stats(),next=f.restart()
    const restored=await next.invoke({kind:'state'});assert.equal(restored.personalAttempt?.phase,'unknown');assert.deepEqual(f.stats(),stats)
    const observed=await next.invoke({kind:'personalReconcile',attemptId:restored.personalAttempt!.attemptId});assert.equal(observed.personalAttempt?.phase,'awaitingApproval');const after=await f.storage.load();assert.equal(after!.originals['local-ticket'],before!.originals['local-ticket']);assert.equal(after!.sent.candidate,before!.sent.candidate);assert.equal(f.stats().candidates,1);assert.equal(f.stats().tickets,1)
    const count=f.stats();f.powerMonitor.emit('suspend');const cut=await next.invoke({kind:'state'});assert.equal(cut.personalAttempt?.phase,'unknown');assert.equal(cut.personalAttempt?.canApprove,false);assert.deepEqual(f.stats(),count);assert(!f.originals['app-consent'])
  }finally{f.close()}
})
test('PL1 C3b.1: enrollment público perdido recupera o begin original antes do primeiro attach, sem Q novo',async()=>{
  const f=await fixture('device');f.controls.loseEnrollment()
  try{const start=await f.service.invoke({kind:'personalStart',mode:'device',sourceId:'fixture-source'});assert.equal(start.personalAttempt?.phase,'unknown');const q=(await f.storage.load())!.originals['local-ticket'];const observed=await f.service.invoke({kind:'personalReconcile',attemptId:start.personalAttempt!.attemptId});assert.equal(observed.personalAttempt?.phase,'awaitingApproval');assert.equal((await f.storage.load())!.originals['local-ticket'],q);assert.equal(f.stats().tickets,1);assert.equal(f.stats().candidates,0)
  }finally{f.close()}
})

test('PL1 C3b.1: Read com janela nova após T expirada confirma histórico terminal, sem CA/consumo ou prazo novo',async()=>{
  const f=await fixture('command')
  try{const start=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(start.personalAttempt?.phase,'awaitingApproval');const before=await f.storage.load();f.controls.expire(700000);f.powerMonitor.emit('suspend')
    const count=f.stats();const restored=await f.service.invoke({kind:'state'});assert.deepEqual(f.stats(),count);assert.equal(restored.personalAttempt?.phase,'unknown')
    const closed=await f.service.invoke({kind:'personalCheck',attemptId:start.personalAttempt!.attemptId});assert.equal(closed.personalAttempt?.phase,'closed');const after=await f.storage.load();assert.equal(after!.originals.intent,before!.originals.intent);assert.equal(after!.originals['local-ticket'],before!.originals['local-ticket']);assert(!f.originals['app-consent']);assert(!f.originals.consume);assert.equal(f.stats().candidates,1);assert.equal(f.stats().tickets,1)
    const refused=await f.service.invoke({kind:'personalApprove',attemptId:start.personalAttempt!.attemptId});assert.notEqual(refused.personalAttempt?.phase,'receipt');assert(!f.originals['app-consent'])
  }finally{f.close()}
})


test('PL1 C3b.1: upgrade Main nativo envia GET vazio sem Origin/auth antes de qualquer frame',async()=>{
  const server=http.createServer(),seen:{value:boolean}={value:false}
  server.on('upgrade',(request,socket,head)=>{seen.value=true;assert.equal(request.method,'GET');assert.equal(request.url,'/personal-prepare');assert.equal(request.headers.origin,undefined);assert.equal(request.headers.authorization,undefined);assert.equal(request.headers.cookie,undefined);assert.equal(request.headers['sec-websocket-protocol'],undefined);assert(request.headers['content-length']===undefined || request.headers['content-length']==='0');assert.equal(request.headers['transfer-encoding'],undefined);assert.equal(head.length,0);socket.end('HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n')})
  await new Promise<void>(resolve=>server.listen(0,'127.0.0.1',resolve));const address=server.address();assert(address && typeof address!=='string')
  const capture={current:()=>true,async check(){},possessionURL:`ws://127.0.0.1:${address.port}/personal-possession`}
  const {PersonalPrepare}=loadMainModule<typeof import('./personal-prepare.ts')>('fork-cloud/personal-prepare',{...primitives,'./personal-prepare-wire':prepare},{...globals,WebSocket:globalThis.WebSocket})
  const peer=new PersonalPrepare(capture as never,()=>true,()=>({lo:0,hi:1}))
  try{await assert.rejects(peer.exchange(C,w.packet['local-ticket'],[D('prepare-request'),1,crypto.b64url(crypto.random(16)),H(w.packet['local-ticket'][0]),'enrollment'],signer('app'),()=>{}));assert(seen.value)}finally{peer.close();await new Promise<void>((resolve,reject)=>server.close(error=>error?reject(error):resolve()))}
})

test('PL1 R24-B1 consent: origem muda durante fsync de mark sem perder current; nenhum efeito helper sai',async()=>{
  const barrier=journalSyncBarrier(),f=await fixture('command',{barrier,originFile:true})
  let pending:Promise<unknown>|null=null
  try{
    const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval')
    barrier.arm('helper-consent');pending=f.service.invoke({kind:'personalApprove',attemptId:started.personalAttempt!.attemptId})
    const held=await barrier.wait(pending);assert.equal(held.sending?.route,'helper-consent');assert(held.originals['app-consent']);assert.equal(f.effects().consent,0);assert(!f.originals['local-consent'])
    await f.changeCapturedOrigin();assert.equal(f.captureCurrent(),true);barrier.release();await pending
    const after=await f.storage.load();assert(after);assert.equal(after.originals.intent,held.originals.intent);assert.equal(after.originals['local-ticket'],held.originals['local-ticket']);assert.equal(after.originals['app-consent'],held.originals['app-consent']);assert.equal(after.sent['helper-consent'],held.sent['helper-consent']);assert(!after.originals['local-consent']);assert(!after.originals.receipt)
    // A falha vermelha esperada é entrega após captura inválida, não shape/time/crypto.
    assert.equal(f.effects().consent,0,'consent foi entregue após a origem mudar durante o fsync de mark')
    assert.equal(f.effects().status,0);assert.equal(f.effects().retain,0);assert.equal(f.effects().proof,0);assert(!f.originals['local-consent'])
  }finally{barrier.release();if(pending)await pending;f.close()}
})

test('PL1 R24-B1 proof: origem muda durante fsync de mark possession; nenhum F ou R novo no peer',async()=>{
  const barrier=journalSyncBarrier(),f=await fixture('command',{barrier,originFile:true})
  let pending:Promise<unknown>|null=null
  try{
    const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval')
    barrier.arm('possession');pending=f.service.invoke({kind:'personalApprove',attemptId:started.personalAttempt!.attemptId})
    const held=await barrier.wait(pending);assert.equal(held.sending?.route,'possession');assert(held.originals.grant);assert(held.originals.challenge);assert(held.originals.proof);assert.equal(f.effects().proof,0);assert(!f.originals.receipt)
    await f.changeCapturedOrigin();assert.equal(f.captureCurrent(),true);barrier.release();await pending
    const after=await f.storage.load();assert(after);for(const name of ['intent','local-ticket','grant','challenge','proof'])assert.equal(after.originals[name],held.originals[name]);assert.equal(after.sent.possession,held.sent.possession);assert(!after.originals.receipt)
    // O peer é fronteira externa controlada; assinatura, POP, frames e journal
    // são reais. Atomicidade de R no Hub é coberta pelos focais Rust, não simulada aqui.
    assert.equal(f.effects().proof,0,'F foi entregue após a origem mudar durante o fsync de mark possession')
    assert(!f.originals.receipt)
  }finally{barrier.release();if(pending)await pending;f.close()}
})

test('PL1 R24-B1 token: origem muda durante token await de app-consent; nenhum POST de efeito sai',async()=>{
  const barrier=tokenAwaitBarrier(),f=await fixture('command',{tokenBarrier:barrier,originFile:true})
  let pending:Promise<unknown>|null=null
  try{
    const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval')
    barrier.arm('app-consent');pending=f.service.invoke({kind:'personalApprove',attemptId:started.personalAttempt!.attemptId})
    const held=await barrier.wait(pending);assert.equal(held.sending?.route,'app-consent');assert(held.originals['app-consent']);assert.equal(f.effects().appConsent,0);assert(!f.originals['app-consent'])
    const ca=JSON.parse(held.originals['app-consent']);assert.equal(ca[0][0],D('app-consent'));assert(crypto.verifyEd25519(signer('app').publicKey,crypto.unb64url(ca[1],64)!,bytes(ca[0])))
    await f.changeCapturedOrigin();assert.equal(f.captureCurrent(),true);barrier.release();await pending
    const after=await f.storage.load();assert(after);for(const name of ['intent','local-ticket','app-consent'])assert.equal(after.originals[name],held.originals[name]);assert.equal(after.sent['app-consent'],held.sent['app-consent']);assert(!after.originals['local-consent']);assert(!after.originals.receipt)
    // fetch de produção e assinatura HTTP são reais no consumidor; somente o
    // destino externo é controlado. Principal/aparelho/token seguem válidos.
    assert.equal(f.effects().appConsent,0,'POST app-consent saiu após a origem mudar durante await p.token()')
    assert.equal(f.effects().consent,0);assert.equal(f.effects().proof,0);assert(!f.originals['app-consent'])
  }finally{barrier.release();if(pending)await pending;f.close()}
})

test('PL1 M1: runtime indisponível impede journal/captura/HTTP antes de qualquer efeito',async()=>{
  const f=await fixture('command',{runtimeUnavailable:true})
  try{const result=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(result.attachment,undefined);assert.equal(result.personalAttempt,undefined);assert.deepEqual(f.stats(),{reads:0,posts:0,candidates:0,tickets:0});assert.equal(fs.readdirSync(f.dir).length,0)}finally{f.close()}
})
test('PL1 M1: q1000 e RTTs externos distintos compõem Main/journal/POP até R sem emitir F futuro',async()=>{
  const f=await fixture('command',{quantum:1000,helperDelayMs:7,workerDelayMs:3})
  try{const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval');const result=await f.service.invoke({kind:'personalApprove',attemptId:started.personalAttempt!.attemptId});assert.equal(result.personalAttempt?.phase,'receipt')
    assert(f.originals['box-challenge'][0][9]>=f.originals['local-consent'][0][6]);assert(f.originals.approve[0][7]>=f.originals['box-proof'][0][6]);assert(f.originals.proof[0][7]>=f.originals.challenge[0][5]);assert(f.originals.receipt[0][7]>=f.originals.proof[0][7]);assert.equal(f.stats().tickets,1);assert.equal(f.stats().candidates,1);assert.equal(result.attachment,undefined)
    const journal=await f.storage.load();assert.equal(journal!.originals.receipt,JSON.stringify(f.originals.receipt));assert.equal(journal!.sending,null)
    assert.equal(f.originals.create[0][5],w.now+7);assert.equal(f.originals['local-ticket'][0][11],f.originals.create[0][5]+3)
  }finally{f.close()}
})

test('PL1 R36-B1: sono no signer HTTP mantém corpo original válido mas impede POST com timestamp vencido',async()=>{
  let armed=false,interruptions=0
  const observed:{held:PersonalJournal|null;input:Buffer|null;signature:Uint8Array|null;at:number;bodyUntil:number;afterSleep:number}={held:null,input:null,signature:null,at:0,bodyUntil:0,afterSleep:0}
  const f=await fixture('command',{originFile:true,afterHttpSignature(input,signature){
    const lines=Buffer.from(input).toString().split('\n')
    if(!armed || lines[3]!=='/v1/personal-bootstrap/app-consent')return
    armed=false;interruptions++
    const held=f.readJournalSync().journal,ca=JSON.parse(held.originals['app-consent']),rawBody=bytes({appConsent:ca})
    assert.equal(held.sending?.route,'app-consent');assert.equal(held.sending.body,rawBody.toString());assert.equal(lines.length,7);assert.equal(lines[0],'codync-sig-v1');assert.equal(lines[1],'POST');assert.equal(lines[2],new URL(C[3]).host)
    const at=Number(lines[4]);assert(Number.isSafeInteger(at));assert(Math.abs(f.time()-at)<30000);assert(crypto.unb64url(lines[5],16));assert.equal(lines[6],crypto.b64url(crypto.sha256(rawBody)))
    assert(crypto.verifyEd25519(signer('app').publicKey,signature,input));assert(crypto.verifyEd25519(signer('app').publicKey,crypto.unb64url(ca[1],64)!,bytes(ca[0])))
    const verified=new wire.PersonalVerifier(C,w.keys).verify('app-consent',bytes(ca),{kind:'current',now:f.time()});assert.equal(verified.wire,held.originals['app-consent'])
    observed.held=held;observed.input=Buffer.from(input);observed.signature=Uint8Array.from(signature);observed.at=at;observed.bodyUntil=ca[0][6]
    // Hipótese explícita: preempção/sono do SO após Ed25519 síncrona, sem evento
    // powerMonitor e sem trocar Principal/current. M avança, performance não.
    f.controls.silentSleep(35000);observed.afterSleep=f.time();assert(observed.afterSleep>at+30000);assert(observed.afterSleep<observed.bodyUntil);assert.equal(f.captureCurrent(),true)
  }})
  try{
    const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval')
    armed=true;await f.service.invoke({kind:'personalApprove',attemptId:started.personalAttempt!.attemptId});assert.equal(interruptions,1)
    const held=observed.held;assert(held && observed.input && observed.signature)
    const loaded=await f.storage.load();assert(loaded);const after=JSON.parse(JSON.stringify(loaded)) as PersonalJournal;for(const name of ['intent','local-ticket','app-consent'])assert.equal(after.originals[name],held.originals[name]);assert.deepEqual(after.sending,held.sending);assert.equal(after.sent['app-consent'],held.sent['app-consent']);assert(!after.originals.receipt)
    const sent=f.httpRequests().filter(r=>r.route==='app-consent')
    for(const r of sent){assert.equal(r.canonical,observed.input.toString());assert.equal(r.signature,crypto.b64url(observed.signature));assert.equal(r.timestamp,observed.at);assert.equal(r.body,held.sending!.body)}
    // RED deve ser 1!=0 aqui: header e corpo foram assinados corretamente, e o
    // corpo ainda está no prazo; não é erro de fixture/crypto/timeout/Worker.
    assert.equal(sent.length,0,'POST saiu após sono silencioso entre assinatura HTTP e fetch')
    assert.equal(f.effects().consent,0);assert.equal(f.effects().proof,0)
  }finally{f.close()}
})

test('PL1 R36-B2: origem muda no fsync anterior ao rename e não pode congelar novo original CA',async()=>{
  const barrier=journalOriginalBarrier(),f=await fixture('command',{originalBarrier:barrier,originFile:true})
  let pending:Promise<unknown>|null=null
  try{
    const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval')
    const before=f.readJournalSync();assert(!before.journal.originals['app-consent']);assert.equal(before.journal.sending,null)
    barrier.arm('app-consent');pending=f.service.invoke({kind:'personalApprove',attemptId:started.personalAttempt!.attemptId})
    const held=await barrier.wait(pending),ca=JSON.parse(held.originals['app-consent']);assert.equal(held.sending,null);assert.equal(held.phase,before.journal.phase);assert(crypto.verifyEd25519(signer('app').publicKey,crypto.unb64url(ca[1],64)!,bytes(ca[0])))
    new wire.PersonalVerifier(C,w.keys).verify('app-consent',bytes(ca),{kind:'current',now:f.time()})
    // O temporário cifrado já sofreu writeFile+fsync REAIS; o arquivo público
    // anterior ainda é exatamente o mesmo, pois o rename não foi chamado.
    const unpublished=f.readJournalSync();assert.deepEqual(unpublished.encrypted,before.encrypted);assert.deepEqual(unpublished.journal,before.journal);assert.equal(f.effects().appConsent,0)
    await f.changeCapturedOrigin();assert.equal(f.captureCurrent(),true);barrier.release();await pending
    const loaded=await f.storage.load();assert(loaded);const after=JSON.parse(JSON.stringify(loaded)) as PersonalJournal
    // RED deve ser 1!=0 aqui por CA publicada apesar da captura alterada.
    assert.equal(Number(Object.hasOwn(after.originals,'app-consent')),0,'CA inválida foi congelada após fsync e antes do rename sem guarda completa')
    assert.deepEqual(after.originals,before.journal.originals);assert.deepEqual(after.sent,before.journal.sent);assert.deepEqual(after.sending,before.journal.sending);assert.deepEqual(after.secret,before.journal.secret);assert.deepEqual(after.facts,before.journal.facts);assert.equal(after.phase,'unknown')
    assert.equal(f.effects().appConsent,0);assert.equal(f.effects().consent,0);assert.equal(f.effects().proof,0);assert(!after.originals.receipt)
  }finally{barrier.release();if(pending)await pending;f.close()}
})

test('PL1 R40-B1: queda da conexão helper no await de drive corta o mesmo approve sem recaptura',async()=>{
  const driveBarrier=r40DriveBarrier(),f=await fixture('command',{fastDrive:true,driveBarrier,helperConnections:true})
  let pending:Promise<unknown>|null=null
  try{
    const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval');assert.equal(f.captures(),1)
    driveBarrier.arm();pending=f.service.invoke({kind:'personalApprove',attemptId:started.personalAttempt!.attemptId})
    await driveBarrier.wait(pending)
    const held=f.readJournalSync().journal,identity=f.helperIdentity(),effects=f.effects();assert(held.originals['local-ticket']);assert(held.originals['box-challenge']);assert.equal(effects.status,1);assert.equal(f.captureCurrent(),true)
    // O cliente Unix REAL cai. Listener e inode permanecem, permitindo que o
    // código antigo abra uma conexão distinta com a mesma provenance estável.
    await f.dropHelperConnection();assert.equal(f.captureCurrent(),false);assert.deepEqual(f.helperIdentity(),identity)
    driveBarrier.release();await pending
    const count=f.captures(),afterEffects=f.effects(),after=f.readJournalSync().journal,stats=f.stats()
    for(const [name,raw]of Object.entries(held.originals))assert.equal(after.originals[name],raw)
    // Restore explícito em NOVA instância lê história sem rede/recaptura; não
    // exigir o socket antigo vivo para simplesmente conservar os originais.
    const next=f.restart(),restored=await next.invoke({kind:'state'});assert(restored.personalAttempt);assert.equal(restored.personalAttempt.attemptId,held.attemptId);assert.deepEqual(f.stats(),stats);assert.equal(f.captures(),count)
    const retained=await f.storage.load();assert(retained);for(const [name,raw]of Object.entries(held.originals))assert.equal(retained.originals[name],raw)
    assert.equal(count,1,'o mesmo approve recapturou helper após perder a conexão original durante drive')
    assert.deepEqual(afterEffects,effects,'nenhum efeito seguinte pode sair na mesma operação após a queda da captura')
  }finally{driveBarrier.release();if(pending)await pending;f.close()}
})

test('PL1 R40-B2 fsync: F vence no mark possession e nenhum frame proof pode alcançar o peer',async()=>{
  const barrier=journalSyncBarrier(),f=await fixture('command',{barrier,fastDrive:true,challengeTtlMs:3000})
  let pending:Promise<unknown>|null=null
  try{
    const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval')
    barrier.arm('possession');pending=f.service.invoke({kind:'personalApprove',attemptId:started.personalAttempt!.attemptId})
    const held=await barrier.wait(pending),F=JSON.parse(held.originals.proof),K=JSON.parse(held.originals.challenge),G=JSON.parse(held.originals.grant)
    assert.equal(held.sending?.route,'possession');assert.equal(held.sending.body,JSON.stringify([G,K,F]));assert.equal(F[0][8],K[0][6]);assert(f.time()<F[0][8]);assert.equal(f.possessionOpen(),true)
    new wire.PersonalVerifier(C,w.keys).verify('proof',bytes(F),{kind:'current',now:f.time()})
    assert.equal(f.delivered().filter(frame=>frame.operation==='proof').length,0)
    // Tempo externo coerente: M, auxiliar e Worker avançam juntos. Referência,
    // origem e socket continuam válidos; só a janela AUTENTICADA K/F vence.
    const advance=F[0][8]-f.time()+25;assert(advance>0 && advance<4000);f.controls.advance(advance);assert(f.time()>=F[0][8]);assert(f.time()<G[0][10]);assert.equal(f.captureCurrent(),true);assert.equal(f.possessionOpen(),true)
    barrier.release();await pending
    const loaded=await f.storage.load();assert(loaded);const after=JSON.parse(JSON.stringify(loaded)) as PersonalJournal;assert.equal(after.originals.proof,held.originals.proof);assert.equal(after.originals.challenge,held.originals.challenge);assert.equal(after.originals.grant,held.originals.grant);assert.deepEqual(after.sending,held.sending);assert.equal(after.sent.possession,held.sent.possession);assert(!after.originals.receipt)
    const sent=f.delivered().filter(frame=>frame.purpose==='possession' && frame.operation==='proof')
    for(const frame of sent)assert.equal(JSON.stringify(JSON.parse(frame.request).slice(1)),held.sending!.body)
    // Conta ANTES de ceremony: rejeição correta de F vencido pelo peer não
    // transforma envio possível indevido em falso GREEN de effects.proof=0.
    assert.equal(sent.length,0,'frame F vencido atravessou send após fsync, mesmo se o peer o rejeitou')
  }finally{barrier.release();if(pending)await pending;f.close()}
})

test('PL1 R40-B2 posse cifra: prazo original vence após sealFrame real e nenhum proof sai',async()=>{
  let interruptions=0
  const observed:{held:PersonalJournal|null;ciphertext:string|null;before:number;after:number}={held:null,ciphertext:null,before:0,after:0}
  const f=await fixture('command',{fastDrive:true,afterPossessionSeal(plain,ciphertext){
    const request=JSON.parse(Buffer.from(plain).toString());if(request[0]!=='proof')return
    interruptions++;assert.equal(interruptions,1);const held=f.readJournalSync().journal,F=JSON.parse(held.originals.proof)
    assert.equal(held.sending?.route,'possession');assert.equal(JSON.stringify(request.slice(1)),held.sending.body);assert(crypto.unb64url(ciphertext));assert.equal(f.possessionOpen(),true);assert.equal(f.captureCurrent(),true);assert(f.time()<F[0][8])
    new wire.PersonalVerifier(C,w.keys).verify('proof',bytes(F),{kind:'current',now:f.time()})
    observed.held=held;observed.ciphertext=ciphertext;observed.before=f.time()
    // sealFrame já calculou a AEAD verdadeira. Preempção externa posterior
    // consome os5s originais do exchange, sem alterar um byte de F/cipher.
    f.controls.advance(6000);observed.after=f.time();assert(observed.after-observed.before>=6000);assert(observed.after<F[0][8]);assert.equal(f.captureCurrent(),true)
  }})
  try{
    const started=await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'awaitingApproval')
    await f.service.invoke({kind:'personalApprove',attemptId:started.personalAttempt!.attemptId});assert.equal(interruptions,1);const held=observed.held;assert(held && observed.ciphertext)
    const loaded=await f.storage.load();assert(loaded);const after=JSON.parse(JSON.stringify(loaded)) as PersonalJournal;assert.equal(after.originals.proof,held.originals.proof);assert.equal(after.originals.challenge,held.originals.challenge);assert.deepEqual(after.sending,held.sending);assert.equal(after.sent.possession,held.sent.possession);assert(!after.originals.receipt)
    const sent=f.delivered().filter(frame=>frame.purpose==='possession' && frame.operation==='proof');for(const frame of sent){assert.equal(frame.ciphertext,observed.ciphertext);assert.equal(JSON.stringify(JSON.parse(frame.request).slice(1)),held.sending!.body)}
    assert.equal(sent.length,0,'proof foi enviado depois de cifra real consumir o deadline original do exchange')
  }finally{f.close()}
})

test('PL1 R40-B2 prepare cifra: origem muda após PrepareFrames.request real e nenhum request sai',async()=>{
  let interruptions=0
  const observed:{held:PersonalJournal|null;request:string|null;ciphertext:string|null}={held:null,request:null,ciphertext:null}
  const f=await fixture('command',{originFile:true,afterPrepareRequest(inner,frame){
    assert(Array.isArray(inner) && inner[4]==='candidate');assert(Array.isArray(frame));interruptions++;assert.equal(interruptions,1)
    const held=f.readJournalSync().journal,Q=JSON.parse(held.originals['local-ticket']);assert.equal(held.sending?.route,'candidate');assert.equal(JSON.stringify(inner),held.secret.candidateRequest);assert.equal(frame[0],D('prepare-frame'));assert.equal(frame[2],'a2h');assert(crypto.unb64url(frame[4]));assert(f.time()<Q[0][12])
    new wire.PersonalVerifier(C,w.keys).verify('local-ticket',bytes(Q),{kind:'current',now:f.time()})
    observed.held=held;observed.request=JSON.stringify(inner);observed.ciphertext=frame[4]
    // O super.request de produção já derivou chaves e cifrou. Troca REAL do
    // fato externo de origem sem perder current; check assíncrono a detecta.
    f.replaceOriginSync();assert.equal(f.captureCurrent(),true)
  }})
  try{
    await f.service.invoke({kind:'personalStart',mode:'command',sourceId:'fixture-source'});assert.equal(interruptions,1);const held=observed.held;assert(held && observed.request && observed.ciphertext)
    const loaded=await f.storage.load();assert(loaded);const after=JSON.parse(JSON.stringify(loaded)) as PersonalJournal;assert.equal(after.originals['local-ticket'],held.originals['local-ticket']);assert.equal(after.originals.intent,held.originals.intent);assert.equal(after.secret.candidateRequest,held.secret.candidateRequest);assert.deepEqual(after.sending,held.sending);assert.equal(after.sent.candidate,held.sent.candidate);assert.equal(after.facts.candidate,null)
    const sent=f.delivered().filter(frame=>frame.purpose==='prepare' && frame.operation==='candidate');for(const frame of sent){assert.equal(frame.request,observed.request);assert.equal(frame.ciphertext,observed.ciphertext)}
    assert.equal(sent.length,0,'request candidate alcançou o peer após a origem mudar depois da cifra real')
  }finally{f.close()}
})

test('PL1 R40-B1 cancel: device sem I mantém stopped ao encerrar a captura real sem efeito Worker',async()=>{
  const f=await fixture('device',{helperConnections:true});f.controls.loseEnrollment()
  try{
    const started=await f.service.invoke({kind:'personalStart',mode:'device',sourceId:'fixture-source'});assert.equal(started.personalAttempt?.phase,'unknown')
    const before=f.readJournalSync().journal,id=started.personalAttempt!.attemptId,Q=before.originals['local-ticket']
    assert(Q);assert.equal(before.attemptId,id);assert.equal(before.mode,'device');assert(!before.originals.intent);assert(!before.originals.grant);assert(!before.originals.receipt);assert.equal(f.captures(),1);assert.equal(f.captureCurrent(),true)
    new wire.PersonalVerifier(C,w.keys).verify('local-ticket',Buffer.from(Q),{kind:'current',now:f.time()})
    const stats=f.stats(),effects=f.effects(),frames=f.delivered(),identity=f.helperIdentity()
    const output=await f.service.invoke({kind:'personalCancel',attemptId:id})
    const loaded=await f.storage.load();assert(loaded);const after=JSON.parse(JSON.stringify(loaded)) as PersonalJournal
    // Primeiro provar durabilidade e ausência de efeitos. O defeito é a
    // projeção imediata após close intencional, não perda de Q/journal.
    assert.equal(after.phase,'stopped');assert.equal(after.attemptId,id);assert.equal(after.mode,before.mode);assert.equal(after.sourceId,before.sourceId);assert.equal(after.namespace,before.namespace);assert.deepEqual(after.binding,before.binding)
    assert.equal(after.originals['local-ticket'],Q);assert.deepEqual(after.originals,before.originals);assert.deepEqual(after.sent,before.sent);assert.deepEqual(after.sending,before.sending);assert.deepEqual(after.facts,before.facts);assert.equal(after.facts.closure,null);assert.deepEqual(after.secret,before.secret);assert(Object.values(after.secret).every(value=>value===null))
    assert.equal(f.captures(),1);assert.equal(f.captureCurrent(),false);assert.deepEqual(f.helperIdentity(),identity);assert.deepEqual(f.stats(),stats);assert.deepEqual(f.effects(),effects);assert.deepEqual(f.delivered(),frames)
    assert(!after.originals.intent);assert(!after.originals.cancel);assert(!after.originals.revoke);assert(!after.originals.grant);assert(!after.originals.receipt);assert(!f.readJournalSync().encrypted.includes(Buffer.from(Q)));assert.equal(output.attachment,undefined)
    const publicOutput=JSON.stringify(output);assert(!publicOutput.includes(Q));assert(!publicOutput.includes('ABCD2345EFGH'));assert(!publicOutput.includes('donna/personal-linux/'));assert(!/commandToken|candidateBody|candidateRequest|deviceCode|userCode/.test(publicOutput))
    // RED esperado a072: undefined != stopped, somente DEPOIS dos fatos acima.
    // Não restaurar por outro state para esconder a resposta da própria ação.
    assert.equal(output.personalAttempt?.phase,'stopped','cancel gravou stopped e fechou a captura real, mas perdeu o DTO imediato')
    assert(output.personalAttempt)
    assert.equal(output.personalAttempt.attemptId,id);assert.equal(output.personalAttempt.canApprove,false);assert.equal(output.personalAttempt.canCheck,false);assert.equal(output.personalAttempt.canReconcile,false)
  }finally{f.close()}
})

// R42 precisa separar a sample RPC da fonte usada por PersonalClock. A fixture
// anterior não expõe essa fronteira e permanece INTEIRA acima. Só transporte,
// captura externa e M/auxiliar são controlados; relógio/canais/crypto são reais.
function r42FinalRpcSample(kind:'proof'|'prepare') {
  const packet=JSON.parse(JSON.stringify(w.packet)) as Record<string,any>,T=JSON.parse(JSON.stringify(w.tuple)),Q=packet['local-ticket'],G=packet.grant,K=packet.challenge,F=packet.proof
  const expiresAt:number=kind==='proof'?F[0][8]:Q[0][12],app=signer('app'),hub=signer('hub'),computer=T[11] as string
  const originals=JSON.stringify({T,Q,G,K,F}),sourceGeneration=crypto.b64url(crypto.random(16))
  let mono=1000,sourceAlive=true,armed=false,seals=0,jumps=0,operation=false,rpcUntil:number|null=null,errorMessage:string|null=null
  let lastEpoch:Readonly<{generation:string;lower:number;upper:number}>|null=null
  let leap:{beforeUpper:number;beforeM:number;afterM:number;beforeAux:number;afterAux:number;rpcUntil:number;rawHi:number;captureOpen:boolean;socketOpen:boolean}|null=null
  const delivered:{operation:string;request:string;ciphertext:string}[]=[],events:string[]=[]
  let sealedRequest:string|null=null,sealedCiphertext:string|null=null,peer:Peer|null=null
  const source={quantum:1 as const,generation:sourceGeneration,current(){assert(sourceAlive)},sample(){assert(sourceAlive);return{lo:mono,hi:mono+1}},auxiliary(){assert(sourceAlive);return mono},close(){sourceAlive=false}}
  const {PersonalClock}=loadMainModule<typeof import('./personal-clock.ts')>('fork-cloud/personal-clock',primitives,globals)
  const clock=new PersonalClock(source),flight=clock.begin()
  // Uma referência E assinada UMA VEZ no setup, perto do F/Q ORIGINAL. Não
  // alterar/reassinar F/Q ou fabricar EpochWindow para produzir a expiração.
  const time=signed('issuer',[D('time'),C,crypto.b64url(crypto.random(32)),computer,expiresAt-3,300000]),timeBytes=JSON.stringify(time)
  clock.accept(authority,time,computer,flight)
  const epoch=()=>{const value=clock.window(computer);lastEpoch=value;events.push(armed?'epochAfterCipher':'epoch');return value}
  const initial=epoch(),verifier=new wire.PersonalVerifier(C,w.keys)
  assert.equal(initial.upper,expiresAt-1)
  if(kind==='proof'){
    const names=['intent','local-ticket','app-consent','local-consent','box-challenge','box-proof','approve','local-confirm','consume','grant','challenge','proof']
    const checked=wire.verifyPersonalCeremony(verifier,bytes(Object.fromEntries(names.map(name=>[name,packet[name]]))),{kind:'current',now:initial.upper})
    assert.equal(JSON.stringify(checked.tuple),JSON.stringify(T));assert.equal(F[0][8],K[0][6]);assert(initial.upper<G[0][10] && initial.upper<T[18])
  }else{const intent=verifier.verify('intent',bytes(packet.intent),{kind:'current',now:initial.upper}),ticket=verifier.verify('local-ticket',bytes(Q),{kind:'current',now:initial.upper});assert.equal(JSON.stringify(verifier.tuple(intent,ticket)),JSON.stringify(T))}
  const id=prepare.prepareIdentity(C,Q),token=crypto.b64url(Buffer.alloc(32,83)),candidateBody=`{"ticket":${JSON.stringify(Q)},"token":${JSON.stringify(token)}}`,digest=prepare.personalHashBytes(Buffer.from(candidateBody))
  const request=kind==='prepare'?[D('prepare-request'),1,crypto.b64url(crypto.random(16)),id.hq,'candidate',prepare.prepareOperation(C,id.capture,id.hq,id.wireHash,digest),digest,token]:[]
  const expectedRequest=JSON.stringify(kind==='proof'?['proof',G,K,F]:request)
  const capture={possessionURL:'ws://127.0.0.1:19000/personal-possession',current:()=>sourceAlive,async check(){assert(sourceAlive);assert.equal(JSON.stringify({T,Q,G,K,F}),originals)},close(){sourceAlive=false}}
  const sample=()=>{
    const before=source.sample();if(operation && rpcUntil===null)rpcUntil=before.lo+5000
    if(armed){
      armed=false;jumps++;assert.equal(seals,1);assert.equal(jumps,1);assert(lastEpoch && rpcUntil!==null);assert.equal(lastEpoch.upper,expiresAt-1)
      const beforeM=mono,beforeAux=source.auxiliary();mono+=2
      const after=source.sample();assert(after.hi<rpcUntil)
      leap={beforeUpper:lastEpoch.upper,beforeM,afterM:mono,beforeAux,afterAux:source.auxiliary(),rpcUntil,rawHi:after.hi,captureOpen:capture.current(),socketOpen:peer?.readyState===1}
      events.push('rpcSampleJump')
    }
    return source.sample()
  }
  const arm=(plain:unknown,ciphertext:string)=>{assert(operation);assert(rpcUntil!==null);assert.equal(JSON.stringify(plain),expectedRequest);assert(crypto.unb64url(ciphertext));seals++;assert.equal(seals,1);sealedRequest=JSON.stringify(plain);sealedCiphertext=ciphertext;armed=true;events.push('cipherReal')}
  const cryptoBoundary={...crypto,sealFrame(...args:Parameters<typeof crypto.sealFrame>){const ciphertext=crypto.sealFrame(...args),plain=JSON.parse(Buffer.from(args[3]).toString());if(kind==='proof' && plain[0]==='proof')arm(plain,ciphertext);return ciphertext}}
  const prepareBoundary={...prepare,PrepareFrames:class extends prepare.PrepareFrames {override request(inner:unknown){const frame=super.request(inner);if(kind==='prepare')arm(inner,frame[4] as string);return frame}}}
  class Peer {
    static OPEN=1;readyState=0;binaryType='';listeners=new Map<string,((e:any)=>void)[]>();purpose:string;hello:any;th!:Uint8Array;e=crypto.ephemeral();a2h!:Uint8Array;h2a!:Uint8Array;tx=0;rx=0;finished=false
    constructor(url:URL){this.purpose=url.pathname;assert.equal(peer,null);peer=this;queueMicrotask(()=>{if(this.readyState===0){this.readyState=1;this.emit('open',{})}})}
    addEventListener(name:string,listener:(e:any)=>void){this.listeners.set(name,[...(this.listeners.get(name)??[]),listener])}
    emit(name:string,event:any){for(const listener of this.listeners.get(name)??[])listener(event)}
    close(){if(this.readyState===3)return;this.readyState=3;this.emit('close',{})}
    reply(value:unknown,binary=false){queueMicrotask(()=>{if(this.readyState!==1)return;if(binary){const encoded=prepare.prepareMessage(value,32768),owned=new Uint8Array(encoded.length);owned.set(encoded);this.emit('message',{data:owned.buffer})}else this.emit('message',{data:JSON.stringify(value)})})}
    send(raw:string|ArrayBuffer|Uint8Array){
      assert.equal(this.readyState,1);assert(capture.current())
      if(this.purpose==='/personal-possession'){
        assert.equal(typeof raw,'string');const message=JSON.parse(raw as string)
        if(!this.hello){
          this.hello=message;assert.equal(message[0],D('possession-hello'));assert.equal(message[2],H(T));assert.equal(message[3],H(G[0]));assert.equal(message[4],w.keys.app);assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(message[7],64)!,bytes(message.slice(0,7))))
          this.th=crypto.sha256(bytes([D('possession-transcript'),message,crypto.b64url(this.e.pub)]));const keys=crypto.channelKeys(this.e.dh(crypto.unb64url(message[5],32)!),this.th);this.a2h=keys.d2h;this.h2a=keys.h2d
          this.reply([D('possession-welcome'),C,message[2],message[3],crypto.b64url(this.e.pub),crypto.b64url(this.th),crypto.b64url(hub.sign(this.th))]);return
        }
        assert.equal(message.t,'f');assert.equal(message.c,this.rx);const opened=crypto.openFrame(this.a2h,this.rx++,message.d);assert(opened.final);const plain=protocol.personalCanonical(opened.chunk) as any[]
        // Contar entrega decifrada ANTES de validade/ceremony do peer.
        delivered.push({operation:plain[0],request:JSON.stringify(plain),ciphertext:message.d});events.push(`received:${plain[0]}`)
        assert.equal(JSON.stringify(plain[1]),JSON.stringify(G));let result:unknown
        if(plain[0]==='challenge'){assert.equal(JSON.stringify(plain),JSON.stringify(['challenge',G]));result=K}
        else{assert.equal(plain[0],'proof');assert.equal(JSON.stringify(plain),expectedRequest);result=packet.receipt}
        this.reply({t:'f',c:this.tx,d:crypto.sealFrame(this.h2a,this.tx++,true,bytes(result))});return
      }
      assert.equal(this.purpose,'/personal-prepare');assert.notEqual(typeof raw,'string')
      const encoded=raw instanceof ArrayBuffer?new Uint8Array(raw):raw as Uint8Array,message=prepare.prepareDecode(encoded,32768) as any[]
      if(!this.hello){
        this.hello=message;assert.equal(message[0],D('prepare-hello'));assert.equal(JSON.stringify(message.slice(2,8)),JSON.stringify([C,id.capture,id.hq,id.wireHash,'command',w.keys.app]))
        const welcome=[D('prepare-welcome'),1,C,id.capture,id.hq,id.wireHash,crypto.b64url(this.e.pub),crypto.b64url(crypto.random(32))];this.th=prepare.prepareTranscript(message,welcome)
        const dh=this.e.dh(crypto.unb64url(message[8],32)!);this.a2h=new Uint8Array(nodeCrypto.hkdfSync('sha256',dh,this.th,Buffer.from(D('prepare-key-a2h')),32));this.h2a=new Uint8Array(nodeCrypto.hkdfSync('sha256',dh,this.th,Buffer.from(D('prepare-key-h2a')),32));dh.fill(0)
        this.reply([...welcome,crypto.b64url(hub.sign(this.th))],true);return
      }
      if(message[0]===D('prepare-finish')){assert.equal(message[2],crypto.b64url(this.th));assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(message[3],64)!,bytes(message.slice(0,3))));this.finished=true;operation=true;return}
      assert(this.finished);assert.equal(message[0],D('prepare-frame'));assert.equal(message[2],'a2h');assert.equal(message[3],0)
      const aad=(direction:string)=>bytes([D('prepare-frame'),1,C,id.capture,id.hq,id.wireHash,crypto.b64url(this.th),direction,0]),plaintext=chacha20poly1305(this.a2h,new Uint8Array(12),aad('a2h')).decrypt(crypto.unb64url(message[4])!),plain=protocol.personalCanonical(plaintext) as any[];plaintext.fill(0)
      delivered.push({operation:plain[4],request:JSON.stringify(plain),ciphertext:message[4]});events.push(`received:${plain[4]}`)
      assert.equal(JSON.stringify(plain),expectedRequest)
      const facts=[id.hq,id.wireHash,H(packet.intent[0]),packet.intent[0][2],Q[0][4],digest],response=[D('prepare-response'),1,plain[2],plain[5],'ok','candidate',facts],cipher=chacha20poly1305(this.h2a,new Uint8Array(12),aad('h2a')).encrypt(bytes(response))
      this.reply([D('prepare-frame'),1,'h2a',0,crypto.b64url(cipher)],true)
    }
  }
  const dependencies={...primitives,'./crypto':cryptoBoundary,'./personal-prepare-wire':prepareBoundary},environment={...globals,WebSocket:Peer,queueMicrotask}
  const {PersonalPossession}=loadMainModule<typeof import('./personal-channel.ts')>('fork-cloud/personal-channel',dependencies,environment)
  const {PersonalPrepare}=loadMainModule<typeof import('./personal-prepare.ts')>('fork-cloud/personal-prepare',dependencies,environment)
  const possession=new PersonalPossession(capture as never,()=>sourceAlive,sample,epoch),preparation=new PersonalPrepare(capture as never,()=>sourceAlive,sample,epoch)
  const guard=async()=>{await capture.check();assert.equal(crypto.b64url(app.publicKey),w.keys.app)}
  return {
    async run(){try{if(kind==='proof'){const channel=await possession.open(C,T,G,app,guard),challenge=await channel.challenge();assert.equal(JSON.stringify(challenge),JSON.stringify(K));operation=true;await channel.proof(K,F)}else await preparation.exchange(C,Q,request,app,guard)}catch(error){errorMessage=error instanceof Error?error.message:String(error)}},
    assertFacts(){
      assert.equal(seals,1);assert.equal(jumps,1);assert(leap && sealedRequest && sealedCiphertext);assert.equal(leap.beforeUpper,expiresAt-1);assert.equal(leap.afterM-leap.beforeM,2);assert.equal(leap.afterAux-leap.beforeAux,2);assert.equal(leap.rpcUntil,leap.beforeM+5000);assert(leap.rawHi<leap.rpcUntil);assert.equal(leap.captureOpen,true);assert.equal(leap.socketOpen,true);assert.equal(capture.current(),true)
      const current=clock.window(computer);assert.equal(current.generation,initial.generation);assert.equal(current.upper,expiresAt+1);assert(current.upper>=expiresAt);assert.equal(JSON.stringify(time),timeBytes);assert.equal(JSON.stringify({T,Q,G,K,F}),originals);assert.equal(sealedRequest,expectedRequest);assert(events.indexOf('cipherReal')<events.indexOf('rpcSampleJump'))
      assert.throws(()=>verifier.verify(kind==='proof'?'proof':'local-ticket',bytes(kind==='proof'?F:Q),{kind:'current',now:current.upper}))
      assert.equal(errorMessage,kind==='proof'?'personalPossessionExpired':'personalPrepareExpired')
      const sent=delivered.filter(frame=>frame.operation===(kind==='proof'?'proof':'candidate'));for(const frame of sent){assert.equal(frame.request,expectedRequest);assert.equal(frame.ciphertext,sealedCiphertext)}return sent.length
    },
    close(){possession.close();preparation.close();peer?.close();clock.cut();source.close()},
  }
}

test('PL1 R42 F: leitura RPC final atravessa F original com clock real e nenhum proof pode sair',async()=>{
  const f=r42FinalRpcSample('proof')
  try{await f.run();const sent=f.assertFacts();assert.equal(sent,0,'proof saiu após RPCsample observar prazo F vencido, apesar dos5s ainda válidos')}finally{f.close()}
})

test('PL1 R42 Q: leitura RPC final atravessa Q original com clock real e nenhum candidate pode sair',async()=>{
  const f=r42FinalRpcSample('prepare')
  try{await f.run();const sent=f.assertFacts();assert.equal(sent,0,'candidate saiu após RPCsample observar prazo Q vencido, apesar dos5s ainda válidos')}finally{f.close()}
})

/** Driver separado: o prefixo61 e seu Peer ficam imutáveis. O Peer abaixo é
 * cópia local do meu focal anterior; só a fonte externa injeta o avanço M
 * DENTRO de PersonalClock.window, depois da última comparação RPC. */
function r42RpcObservedByEpoch(kind:'proof'|'prepare') {
  const packet=JSON.parse(JSON.stringify(w.packet)) as Record<string,any>,T=JSON.parse(JSON.stringify(w.tuple)),Q=packet['local-ticket'],G=packet.grant,K=packet.challenge,F=packet.proof
  const expiresAt:number=kind==='proof'?F[0][8]:Q[0][12],app=signer('app'),hub=signer('hub'),computer=T[11] as string
  const originals=JSON.stringify({T,Q,G,K,F}),sourceGeneration=crypto.b64url(crypto.random(16))
  const referenceEpoch=Math.max(Q[0][11],G[0][9],K[0][5],F[0][7])+100,referenceTtl=300000
  let mono=1000,sourceAlive=true,inEpoch=false,seals=0,jumps=0,operation=false,rpcUntil:number|null=null,rpcStart:number|null=null,rpcAssignments=0,errorMessage:string|null=null
  let stage:'idle'|'rpc'|'epoch'|'observed'='idle'
  let finalRpc:Readonly<{lo:number;hi:number}>|null=null,finalEpoch:Readonly<{generation:string;lower:number;upper:number}>|null=null
  let leap:{beforeM:number;afterM:number;beforeAux:number;afterAux:number;rpcUntil:number;rawHi:number;captureOpen:boolean;socketOpen:boolean}|null=null
  const epochReads:Readonly<{lo:number;hi:number}>[]=[],delivered:{operation:string;request:string;ciphertext:string}[]=[],events:string[]=[]
  let sealedRequest:string|null=null,sealedCiphertext:string|null=null,peer:Peer|null=null
  const source:import('./personal-runtime').PersonalElapsed={quantum:1 as const,generation:sourceGeneration,current(){assert(sourceAlive)},sample(){
    assert(sourceAlive)
    if(inEpoch && stage==='epoch'){
      assert(finalRpc && rpcUntil!==null);assert.equal(finalRpc.hi,rpcUntil-1);assert.equal(seals,1)
      const beforeM=mono,beforeAux=source.auxiliary();mono+=2;stage='observed';jumps++
      leap={beforeM,afterM:mono,beforeAux,afterAux:source.auxiliary(),rpcUntil,rawHi:finalRpc.hi,captureOpen:capture.current(),socketOpen:peer?.readyState===1}
      events.push('epochSourceJump')
    }
    const value={lo:mono,hi:mono+1};if(inEpoch && stage==='observed' && epochReads.length<2)epochReads.push(value)
    return value
  },auxiliary(){assert(sourceAlive);return mono},close(){sourceAlive=false}}
  const {PersonalClock}=loadMainModule<typeof import('./personal-clock.ts')>('fork-cloud/personal-clock',primitives,globals)
  const clock=new PersonalClock(source),flight=clock.begin()
  // A referência é assinada uma vez no setup, com folga para os5s; nenhum
  // original F/Q/G/T/K é renovado ou reassinado durante o ataque.
  const time=signed('issuer',[D('time'),C,crypto.b64url(crypto.random(32)),computer,referenceEpoch,referenceTtl]),timeBytes=JSON.stringify(time)
  clock.accept(authority,time,computer,flight)
  const epoch=()=>{inEpoch=true;try{const value=clock.window(computer);if(stage==='observed' && finalEpoch===null){finalEpoch=value;events.push('epochObservedDeadline')}return value}finally{inEpoch=false}}
  const initial=epoch(),verifier=new wire.PersonalVerifier(C,w.keys)
  assert.equal(initial.upper,referenceEpoch+2)
  const names=['intent','local-ticket','app-consent','local-consent','box-challenge','box-proof','approve','local-confirm','consume','grant','challenge','proof']
  const checked=wire.verifyPersonalCeremony(verifier,bytes(Object.fromEntries(names.map(name=>[name,packet[name]]))),{kind:'current',now:initial.upper})
  assert.equal(JSON.stringify(checked.tuple),JSON.stringify(T));assert.equal(F[0][8],K[0][6])
  const id=prepare.prepareIdentity(C,Q),token=crypto.b64url(Buffer.alloc(32,83)),candidateBody=`{"ticket":${JSON.stringify(Q)},"token":${JSON.stringify(token)}}`,digest=prepare.personalHashBytes(Buffer.from(candidateBody))
  const request=kind==='prepare'?[D('prepare-request'),1,crypto.b64url(crypto.random(16)),id.hq,'candidate',prepare.prepareOperation(C,id.capture,id.hq,id.wireHash,digest),digest,token]:[]
  const expectedRequest=JSON.stringify(kind==='proof'?['proof',G,K,F]:request)
  const capture={possessionURL:'ws://127.0.0.1:19000/personal-possession',current:()=>sourceAlive,async check(){assert(sourceAlive);assert.equal(JSON.stringify({T,Q,G,K,F}),originals)},close(){sourceAlive=false}}
  const sample=()=>{
    const value=source.sample()
    if(operation && rpcUntil===null){rpcStart=value.lo;rpcUntil=value.lo+5000;rpcAssignments++}
    if(stage==='rpc'){assert(rpcUntil!==null);assert.equal(value.hi,rpcUntil-1);finalRpc=value;stage='epoch';events.push('rpcFinalBelowDeadline')}
    return value
  }
  const arm=(plain:unknown,ciphertext:string)=>{
    assert(operation && rpcUntil!==null);assert.equal(stage,'idle');assert.equal(JSON.stringify(plain),expectedRequest);assert(crypto.unb64url(ciphertext))
    seals++;assert.equal(seals,1);sealedRequest=JSON.stringify(plain);sealedCiphertext=ciphertext
    // O tempo externo avança durante a cifra/guarda; o orçamento ORIGINAL
    // permanece. O salto seguinte só ocorrerá na fonte chamada pela epoch.
    mono=rpcUntil-2;stage='rpc';events.push('cipherReal')
  }
  const cryptoBoundary={...crypto,sealFrame(...args:Parameters<typeof crypto.sealFrame>){const ciphertext=crypto.sealFrame(...args),plain=JSON.parse(Buffer.from(args[3]).toString());if(kind==='proof' && plain[0]==='proof')arm(plain,ciphertext);return ciphertext}}
  const prepareBoundary={...prepare,PrepareFrames:class extends prepare.PrepareFrames {override request(inner:unknown){const frame=super.request(inner);if(kind==='prepare')arm(inner,frame[4] as string);return frame}}}
  class Peer {
    static OPEN=1;readyState=0;binaryType='';listeners=new Map<string,((e:any)=>void)[]>();purpose:string;hello:any;th!:Uint8Array;e=crypto.ephemeral();a2h!:Uint8Array;h2a!:Uint8Array;tx=0;rx=0;finished=false
    constructor(url:URL){this.purpose=url.pathname;assert.equal(peer,null);peer=this;queueMicrotask(()=>{if(this.readyState===0){this.readyState=1;this.emit('open',{})}})}
    addEventListener(name:string,listener:(e:any)=>void){this.listeners.set(name,[...(this.listeners.get(name)??[]),listener])}
    emit(name:string,event:any){for(const listener of this.listeners.get(name)??[])listener(event)}
    close(){if(this.readyState===3)return;this.readyState=3;this.emit('close',{})}
    reply(value:unknown,binary=false){queueMicrotask(()=>{if(this.readyState!==1)return;if(binary){const encoded=prepare.prepareMessage(value,32768),owned=new Uint8Array(encoded.length);owned.set(encoded);this.emit('message',{data:owned.buffer})}else this.emit('message',{data:JSON.stringify(value)})})}
    send(raw:string|ArrayBuffer|Uint8Array){
      assert.equal(this.readyState,1);assert(capture.current())
      if(this.purpose==='/personal-possession'){
        assert.equal(typeof raw,'string');const message=JSON.parse(raw as string)
        if(!this.hello){
          this.hello=message;assert.equal(message[0],D('possession-hello'));assert.equal(message[2],H(T));assert.equal(message[3],H(G[0]));assert.equal(message[4],w.keys.app);assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(message[7],64)!,bytes(message.slice(0,7))))
          this.th=crypto.sha256(bytes([D('possession-transcript'),message,crypto.b64url(this.e.pub)]));const keys=crypto.channelKeys(this.e.dh(crypto.unb64url(message[5],32)!),this.th);this.a2h=keys.d2h;this.h2a=keys.h2d
          this.reply([D('possession-welcome'),C,message[2],message[3],crypto.b64url(this.e.pub),crypto.b64url(this.th),crypto.b64url(hub.sign(this.th))]);return
        }
        assert.equal(message.t,'f');assert.equal(message.c,this.rx);const opened=crypto.openFrame(this.a2h,this.rx++,message.d);assert(opened.final);const plain=protocol.personalCanonical(opened.chunk) as any[]
        // Contar entrega decifrada ANTES de validade/ceremony do peer.
        delivered.push({operation:plain[0],request:JSON.stringify(plain),ciphertext:message.d});events.push(`received:${plain[0]}`)
        assert.equal(JSON.stringify(plain[1]),JSON.stringify(G));let result:unknown
        if(plain[0]==='challenge'){assert.equal(JSON.stringify(plain),JSON.stringify(['challenge',G]));result=K}
        else{assert.equal(plain[0],'proof');assert.equal(JSON.stringify(plain),expectedRequest);result=packet.receipt}
        this.reply({t:'f',c:this.tx,d:crypto.sealFrame(this.h2a,this.tx++,true,bytes(result))});return
      }
      assert.equal(this.purpose,'/personal-prepare');assert.notEqual(typeof raw,'string')
      const encoded=raw instanceof ArrayBuffer?new Uint8Array(raw):raw as Uint8Array,message=prepare.prepareDecode(encoded,32768) as any[]
      if(!this.hello){
        this.hello=message;assert.equal(message[0],D('prepare-hello'));assert.equal(JSON.stringify(message.slice(2,8)),JSON.stringify([C,id.capture,id.hq,id.wireHash,'command',w.keys.app]))
        const welcome=[D('prepare-welcome'),1,C,id.capture,id.hq,id.wireHash,crypto.b64url(this.e.pub),crypto.b64url(crypto.random(32))];this.th=prepare.prepareTranscript(message,welcome)
        const dh=this.e.dh(crypto.unb64url(message[8],32)!);this.a2h=new Uint8Array(nodeCrypto.hkdfSync('sha256',dh,this.th,Buffer.from(D('prepare-key-a2h')),32));this.h2a=new Uint8Array(nodeCrypto.hkdfSync('sha256',dh,this.th,Buffer.from(D('prepare-key-h2a')),32));dh.fill(0)
        this.reply([...welcome,crypto.b64url(hub.sign(this.th))],true);return
      }
      if(message[0]===D('prepare-finish')){assert.equal(message[2],crypto.b64url(this.th));assert(crypto.verifyEd25519(app.publicKey,crypto.unb64url(message[3],64)!,bytes(message.slice(0,3))));this.finished=true;operation=true;return}
      assert(this.finished);assert.equal(message[0],D('prepare-frame'));assert.equal(message[2],'a2h');assert.equal(message[3],0)
      const aad=(direction:string)=>bytes([D('prepare-frame'),1,C,id.capture,id.hq,id.wireHash,crypto.b64url(this.th),direction,0]),plaintext=chacha20poly1305(this.a2h,new Uint8Array(12),aad('a2h')).decrypt(crypto.unb64url(message[4])!),plain=protocol.personalCanonical(plaintext) as any[];plaintext.fill(0)
      delivered.push({operation:plain[4],request:JSON.stringify(plain),ciphertext:message[4]});events.push(`received:${plain[4]}`)
      assert.equal(JSON.stringify(plain),expectedRequest)
      const facts=[id.hq,id.wireHash,H(packet.intent[0]),packet.intent[0][2],Q[0][4],digest],response=[D('prepare-response'),1,plain[2],plain[5],'ok','candidate',facts],cipher=chacha20poly1305(this.h2a,new Uint8Array(12),aad('h2a')).encrypt(bytes(response))
      this.reply([D('prepare-frame'),1,'h2a',0,crypto.b64url(cipher)],true)
    }
  }
  const dependencies={...primitives,'./crypto':cryptoBoundary,'./personal-prepare-wire':prepareBoundary},environment={...globals,WebSocket:Peer,queueMicrotask}
  const {PersonalPossession}=loadMainModule<typeof import('./personal-channel.ts')>('fork-cloud/personal-channel',dependencies,environment)
  const {PersonalPrepare}=loadMainModule<typeof import('./personal-prepare.ts')>('fork-cloud/personal-prepare',dependencies,environment)
  const possession=new PersonalPossession(capture as never,()=>sourceAlive,sample,epoch),preparation=new PersonalPrepare(capture as never,()=>sourceAlive,sample,epoch)
  const guard=async()=>{await capture.check();assert.equal(crypto.b64url(app.publicKey),w.keys.app)}
  return {
    async run(){try{if(kind==='proof'){const channel=await possession.open(C,T,G,app,guard),challenge=await channel.challenge();assert.equal(JSON.stringify(challenge),JSON.stringify(K));operation=true;await channel.proof(K,F)}else await preparation.exchange(C,Q,request,app,guard)}catch(error){errorMessage=error instanceof Error?error.message:String(error)}},
    assertFacts(){
      assert.equal(seals,1);assert.equal(jumps,1);assert.equal(rpcAssignments,1);assert.equal(rpcStart,1000);assert.equal(rpcUntil,6000)
      assert(finalRpc && finalEpoch && leap && sealedRequest && sealedCiphertext)
      assert.equal(finalRpc.hi,rpcUntil!-1);assert.equal(leap.rawHi,finalRpc.hi);assert.equal(leap.rpcUntil,rpcUntil)
      assert.equal(leap.beforeM,5998);assert.equal(leap.afterM-leap.beforeM,2);assert.equal(leap.afterAux-leap.beforeAux,2)
      assert.equal(epochReads.length,2);for(const observed of epochReads){assert.equal(observed.hi,6001);assert(observed.hi>=rpcUntil!)}
      assert.equal(leap.captureOpen,true);assert.equal(leap.socketOpen,true);assert.equal(capture.current(),true);assert.equal(sourceAlive,true)
      assert(epochReads[1].hi<flight.sent.lo+referenceTtl);assert.equal(finalEpoch.generation,initial.generation);assert.equal(finalEpoch.upper,referenceEpoch+5002)
      for(const until of [T[18],Q[0][12],G[0][10],K[0][6],F[0][8]])assert(finalEpoch.upper<until)
      assert(finalEpoch.upper<expiresAt);assert.equal(JSON.stringify(time),timeBytes);assert.equal(JSON.stringify({T,Q,G,K,F}),originals)
      const current=clock.window(computer);assert.equal(current.upper,finalEpoch.upper);assert.equal(current.generation,initial.generation)
      const ceremony=wire.verifyPersonalCeremony(verifier,bytes(Object.fromEntries(names.map(name=>[name,packet[name]]))),{kind:'current',now:current.upper})
      assert.equal(JSON.stringify(ceremony.tuple),JSON.stringify(T));verifier.verify('local-ticket',bytes(Q),{kind:'current',now:current.upper});verifier.verify('proof',bytes(F),{kind:'current',now:current.upper})
      assert.equal(sealedRequest,expectedRequest);assert(events.indexOf('cipherReal')<events.indexOf('rpcFinalBelowDeadline'));assert(events.indexOf('rpcFinalBelowDeadline')<events.indexOf('epochSourceJump'));assert(events.indexOf('epochSourceJump')<events.indexOf('epochObservedDeadline'))
      assert.equal(errorMessage,kind==='proof'?'personalPossessionTimeout':'personalPrepareTimeout')
      const sent=delivered.filter(frame=>frame.operation===(kind==='proof'?'proof':'candidate'))
      for(const frame of sent){assert.equal(frame.request,expectedRequest);assert.equal(frame.ciphertext,sealedCiphertext);assert(events.indexOf('epochObservedDeadline')<events.indexOf(`received:${frame.operation}`))}
      return sent.length
    },
    close(){possession.close();preparation.close();peer?.close();clock.cut();source.close()},
  }
}

test('PL1 R42 RPC proof: Clock real observa M vencido dentro de epoch com F válido e nenhum proof pode sair',async()=>{
  const f=r42RpcObservedByEpoch('proof')
  try{await f.run();const sent=f.assertFacts();assert.equal(sent,0,'proof saiu depois de epoch observar RPC5s vencido, apesar de F ainda válido')}finally{f.close()}
})

test('PL1 R42 RPC candidate: Clock real observa M vencido dentro de epoch com Q válido e nenhum candidate pode sair',async()=>{
  const f=r42RpcObservedByEpoch('prepare')
  try{await f.run();const sent=f.assertFacts();assert.equal(sent,0,'candidate saiu depois de epoch observar RPC5s vencido, apesar de Q ainda válido')}finally{f.close()}
})
