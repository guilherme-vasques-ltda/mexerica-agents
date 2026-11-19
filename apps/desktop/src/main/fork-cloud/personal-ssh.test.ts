import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as path from 'node:path'
import * as os from 'node:os'
import * as nodeCrypto from 'node:crypto'
import * as net from 'node:net'
import { PassThrough } from 'node:stream'
import { EventEmitter } from 'node:events'
import * as wire from './personal-wire.ts'
import * as protocol from './personal-protocol.ts'
import * as crypto from './crypto.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
import type { Principal } from './principal.ts'
import type { SshTrust } from './personal-trust.ts'

test('PL1 C3b.1: captura SSH controlada liga client/argv/CA/log próprio/túnel; stderr e TOFU não qualificam',async()=>{
  // Apenas o processo SSH externo é controlado. Não instala sshd/CA real nem prova
  // OpenSSH remoto: arquivos, adapter, parser, frames helper e guardas são reais.
  const dir=fs.mkdtempSync(path.join(os.homedir(),'.personal-ssh-test-'));fs.chmodSync(dir,0o700)
  const blob=Buffer.concat([Buffer.from([0,0,0,11]),Buffer.from('ssh-ed25519'),Buffer.from([0,0,0,32]),Buffer.alloc(32,14)]),ca=`ssh-ed25519 ${blob.toString('base64')}`,fp=`SHA256:${nodeCrypto.createHash('sha256').update(blob).digest('base64').replace(/=+$/,'')}`
  const trust:SshTrust={schema:1,qualificationId:'fixture',evidenceDigest:'e'.repeat(64),ca,caFingerprint:fp,host:'hub.example.invalid',port:22,user:'fixture',principal:'hub.example.invalid',admittedUntil:1900000000000}
  let allowed=true,live=true,log='',store='',args:readonly string[]=[],spawned=0
  const emitter=new EventEmitter()
  const child=Object.assign(emitter,{stdout:new PassThrough(),stdin:new PassThrough(),stderr:new PassThrough(),exitCode:null as number|null,killed:false,kill(){this.killed=true;emitter.emit('exit',null)}})
  const {capturePersonalSsh}=loadMainModule<typeof import('./personal-ssh.ts')>('fork-cloud/personal-ssh',{
    'node:fs':fs,'node:path':path,'node:crypto':nodeCrypto,'node:net':net,'./personal-wire':wire,'./personal-protocol':protocol,'./crypto':crypto,
    electron:{app:{getPath:()=>dir}},'./personal-trust':{personalSshTrust:()=>allowed?trust:null},
    'node:child_process':{spawn(binary:string,argv:string[],options:{env:{TZ:string}}){assert.equal(binary,'/usr/bin/ssh');assert.equal(options.env.TZ,'UTC');args=argv;log=argv[argv.indexOf('-E')+1];store=argv.find(a=>a.startsWith('UserKnownHostsFile='))!.slice('UserKnownHostsFile='.length);spawned++;return child}},
  },{Buffer,Object,process,Date,TextDecoder,setTimeout,clearTimeout})
  const p={current:()=>live,deployment:{env:'main'}} as Principal,profile={id:'fixture',host:trust.host,user:trust.user,port:22,remotePort:19422,identityFile:'/synthetic/discardable-test-key'}
  const diagnostic=()=>`debug1: Server host certificate: ssh-ed25519-cert-v01@openssh.com SHA256:AAAA, serial 7 ID "fixture" CA ssh-ed25519 ${fp} valid from 2026-01-01T00:00:00 to 2030-01-01T00:00:00\ndebug2: Server host certificate hostname: ${trust.principal}\ndebug1: Host '${trust.principal}' is known and matches the ED25519-CERT host certificate.\ndebug1: Found CA key in ${store}:1\nAuthenticated to ${trust.host} ([192.0.2.19]:22) using "publickey".\n`
  let capture:Awaited<ReturnType<typeof capturePersonalSsh>>|null=null
  try{
    allowed=false;await assert.rejects(capturePersonalSsh(p,profile),/personalOriginUnavailable/);assert.equal(spawned,0);allowed=true
    capture=await capturePersonalSsh(p,profile);assert.equal(spawned,1);assert(args.includes('StrictHostKeyChecking=yes'));assert(args.includes('HostKeyAlgorithms=ssh-ed25519-cert-v01@openssh.com'));assert(args.includes('KnownHostsCommand=none'));assert(args.includes('GlobalKnownHostsFile=/dev/null'));assert(args.includes('ProxyJump=none'));assert.equal(args.at(-1),'/usr/libexec/donnabot/bootstrap-bridge-v1');assert.equal(args[args.indexOf('-F')+1],'/dev/null');assert.equal(args[args.indexOf('-L')+1].split(':').slice(-2).join(':'),'127.0.0.1:19422')
    child.stderr.write(diagnostic());await assert.rejects(capture.check(),/personalSshDiagnostic/)
    fs.writeFileSync(log,diagnostic());await capture.check();capture.validateAt(1800000000100);const proof=capture.provenance();assert.match(proof,/^[a-f0-9]{64}$/)
    const request=capture.helper.request(['time']);const frame=child.stdin.read() as Buffer;assert(frame);const r=JSON.parse(frame.subarray(4).toString());assert.equal(r[3],'time');const response=Buffer.from(JSON.stringify([wire.personalDomain('helper'),1,r[2],'ok','time',['synthetic-public-time']])),header=Buffer.alloc(4);header.writeUInt32BE(response.length);child.stdout.write(Buffer.concat([header,response]));assert.equal(JSON.stringify(await request),'["synthetic-public-time"]')
    fs.writeFileSync(log,diagnostic().replace('ED25519-CERT','ED25519'));await assert.rejects(capture.check(),/personalSshAnchor/)
    fs.writeFileSync(log,diagnostic());fs.renameSync(store,store+'.old');fs.copyFileSync(store+'.old',store);fs.chmodSync(store,0o600);await assert.rejects(capture.check(),/personalCaptureChanged/);live=false;assert.equal(capture.current(),false)
  }finally{capture?.close();child.stdout.destroy();child.stdin.destroy();child.stderr.destroy();fs.rmSync(dir,{recursive:true,force:true})}
})
