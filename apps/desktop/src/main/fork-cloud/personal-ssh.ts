import { writePrivateFile } from '../../shared/fork-private-files'
/** OpenSSH próprio certificate-only. Não reutiliza TOFU/Terminal/ssh -G legado. */
import { spawn } from 'node:child_process'
import { constants, promises as fs } from 'node:fs'
import { createHash, randomBytes } from 'node:crypto'
import { createServer } from 'node:net'
import { join, parse, sep } from 'node:path'
import { app } from 'electron'
import { personalHash } from './personal-wire'
import { PersonalHelper } from './personal-helper'
import { personalSshTrust, type SshTrust } from './personal-trust'
import type { PersonalCapture } from './personal-origin'
import type { Principal } from './principal'
export interface PersonalSshProfile { readonly id: string; readonly host: string; readonly user: string | null; readonly port: number | null; readonly remotePort: number; readonly identityFile: string | null }
export interface Certificate { readonly fingerprint: string; readonly ca: string; readonly serial: string; readonly keyId: string; readonly principals: readonly string[]; readonly from: number; readonly until: number; readonly endpoint: string }
const hash = (bytes: Uint8Array) => createHash('sha256').update(bytes).digest('hex')
/** Log exclusivo do cliente (-E), nunca stderr do comando/bridge. Duplicatas recusam. */
export function certificate(log: string, trust: SshTrust, store: string): Certificate {
  if (!log || Buffer.byteLength(log) > 256*1024 || /[^\x09\x0a\x20-\x7e]/.test(log)) throw new Error('personalSshDiagnostic')
  const lines = log.split('\n').filter(Boolean)
  const one = (prefix: string) => { const found=lines.filter(l => l.startsWith(prefix)); if (found.length !== 1) throw new Error('personalSshDiagnostic'); return found[0] }
  const raw=one('debug1: Server host certificate: ') // textos-ok: linha de diagnóstico do ssh -v (protocolo), não é texto de interface
  const match=/^debug1: Server host certificate: ssh-ed25519-cert-v01@openssh.com (SHA256:[A-Za-z0-9+/]+), serial ([0-9]+) ID "([A-Za-z0-9_.:@/-]{1,128})" CA ssh-ed25519 (SHA256:[A-Za-z0-9+/]+) valid from ([0-9T:-]+) to ([0-9T:-]+)$/.exec(raw)
  if (!match || match[4] !== trust.caFingerprint) throw new Error('personalSshCertificate')
  const principalLines=lines.filter(l => l.startsWith('debug2: Server host certificate hostname: '))
  const principals=principalLines.map(l => l.slice('debug2: Server host certificate hostname: '.length)) // textos-ok: linha de diagnóstico do ssh -v (protocolo), não é texto de interface
  if (!principals.length || new Set(principals).size !== principals.length || !principals.includes(trust.principal) || principals.some(p => !/^[a-z0-9.-]{1,253}$/.test(p))) throw new Error('personalSshPrincipal')
  if (one('debug1: Host ') !== `debug1: Host '${trust.principal}' is known and matches the ED25519-CERT host certificate.` || one('debug1: Found CA key in ') !== `debug1: Found CA key in ${store}:1`) throw new Error('personalSshAnchor')
  const auth=one('Authenticated to ') // textos-ok: linha de diagnóstico do ssh -v (protocolo), não é texto de interface
  const endpoint=new RegExp(`^Authenticated to ${trust.host.replaceAll('.','\\.')} \\(\\[([A-Fa-f0-9:.]+)\\]:${trust.port}\\) using "publickey"\\.$`).exec(auth)?.[1]
  if (!endpoint) throw new Error('personalSshAuthentication')
  // O processo tem TZ=UTC; validade também será conferida contra time E, sem parede.
  const from=Date.parse(`${match[5]}Z`), until=Date.parse(`${match[6]}Z`)
  if (!Number.isSafeInteger(from) || !Number.isSafeInteger(until) || until<=from) throw new Error('personalSshValidity')
  return Object.freeze({fingerprint:match[1],ca:match[4],serial:match[2],keyId:match[3],principals:Object.freeze(principals),from,until,endpoint})
}
export async function protectedParents(path:string) {
  const root=parse(path).root;let here=root
  for(const name of path.slice(root.length).split(sep).filter(Boolean)) {
    here=join(here,name);const st=await fs.lstat(here)
    if(!st.isDirectory() || (st.uid!==0 && st.uid!==process.getuid?.()) || st.mode&0o022)throw new Error('personalSshParent')
  }
}
export async function privateFile(path: string, limit: number) {
  const file=await fs.open(path,constants.O_RDONLY|constants.O_NOFOLLOW)
  try { const before=await file.stat(); if (!before.isFile() || before.uid!==process.getuid?.() || (before.mode&0o777)!==0o600 || before.nlink!==1 || before.size>limit) throw new Error('personalSshFile')
    const bytes=await file.readFile(), after=await file.stat(), named=await fs.lstat(path)
    if (before.dev!==after.dev || before.ino!==after.ino || named.dev!==before.dev || named.ino!==before.ino || bytes.length>limit) throw new Error('personalSshFileChanged')
    return {bytes,dev:before.dev,ino:before.ino}
  } finally { await file.close() }
}
async function port() { const server=createServer(); await new Promise<void>((ok,no) => { server.once('error',no); server.listen(0,'127.0.0.1',ok) }); const address=server.address(); if (!address || typeof address==='string') throw new Error('personalTunnel'); const value=address.port; await new Promise<void>((ok,no) => server.close(e=>e?no(e):ok())); return value }
export async function capturePersonalSsh(p: Principal, profile: PersonalSshProfile): Promise<PersonalCapture> {
  const trust=personalSshTrust(profile.id)
  if (!p.current() || !trust || trust.schema!==1 || !/^[a-f0-9]{64}$/.test(trust.evidenceDigest) || !/^[a-z0-9.-]{1,253}$/.test(trust.host) || !/^[a-z0-9.-]{1,253}$/.test(trust.principal) || !/^[A-Za-z_][A-Za-z0-9_-]*$/.test(trust.user) || trust.host!==profile.host || trust.port!==(profile.port??22) || trust.user!==profile.user || profile.remotePort!==(p.deployment.env==='dev'?19423:19422) || !profile.identityFile?.startsWith('/') || !/^ssh-ed25519 [A-Za-z0-9+/]+={0,2}$/.test(trust.ca)) throw new Error('personalOriginUnavailable')
  const binary=await fs.lstat('/usr/bin/ssh'); if (!binary.isFile() || binary.uid!==0 || binary.mode&0o022) throw new Error('personalSshBinary')
  const caBytes=Buffer.from(trust.ca.split(' ')[1],'base64')
  if(`SHA256:${createHash('sha256').update(caBytes).digest('base64').replace(/=+$/,'')}`!==trust.caFingerprint)throw new Error('personalSshCA')
  await protectedParents(app.getPath('userData'))
  const dir=await fs.mkdtemp(join(app.getPath('userData'),'personal-capture-')); await fs.chmod(dir,0o700)
  const store=join(dir,'ca'), log=join(dir,'diagnostic')
  const storeBytes=Buffer.from(`@cert-authority ${trust.principal} ${trust.ca}\n`)
  await fs.writeFile(store,storeBytes,{flag:'wx',mode:0o600}); await fs.writeFile(log,'',{flag:'wx',mode:0o600})
  const anchor=await privateFile(store,4096), diagnostic=await privateFile(log,256*1024), localPort=await port()
  const options = ['StrictHostKeyChecking=yes','HostKeyAlgorithms=ssh-ed25519-cert-v01@openssh.com',`UserKnownHostsFile=${store}`,'GlobalKnownHostsFile=/dev/null','KnownHostsCommand=none',`HostKeyAlias=${trust.principal}`,'UpdateHostKeys=no','VerifyHostKeyDNS=no','CanonicalizeHostname=no','ProxyCommand=none','ProxyJump=none','ControlMaster=no','ControlPath=none','ForwardAgent=no','ForwardX11=no','ClearAllForwardings=no','ExitOnForwardFailure=yes','BatchMode=yes','IdentitiesOnly=yes','IdentityAgent=none','PermitLocalCommand=no','RequestTTY=no','LogLevel=DEBUG2']
  const argv=['-F','/dev/null','-E',log,...options.flatMap(o=>['-o',o]),'-p',String(trust.port),'-l',trust.user,'-i',profile.identityFile,'-L',`127.0.0.1:${localPort}:127.0.0.1:${profile.remotePort}`,trust.host,'/usr/libexec/donnabot/bootstrap-bridge-v1']
  const child=spawn('/usr/bin/ssh',argv,{stdio:['pipe','pipe','pipe'],env:{...process.env,TZ:'UTC'}})
  let alive=true, observed:Certificate|null=null
  const current=()=>alive && p.current() && child.exitCode===null && !child.killed
  child.on('exit',()=>{alive=false}); child.on('error',()=>{alive=false}); child.stderr.on('data',()=>{}) // stderr remoto descartado; nunca evidência/log do produto.
  const helper=new PersonalHelper(child.stdout,child.stdin,current,()=>{alive=false; child.kill()})
  const check=async()=>{
    if (!current()) throw new Error('personalCaptureChanged')
    await protectedParents(dir)
    const [ca,ownLog,bin]=await Promise.all([privateFile(store,4096),privateFile(log,256*1024),fs.lstat('/usr/bin/ssh')])
    if (!current() || ca.dev!==anchor.dev || ca.ino!==anchor.ino || hash(ca.bytes)!==hash(storeBytes) || ownLog.dev!==diagnostic.dev || ownLog.ino!==diagnostic.ino || bin.dev!==binary.dev || bin.ino!==binary.ino || !bin.isFile() || bin.uid!==0 || bin.mode&0o022) throw new Error('personalCaptureChanged')
    const cert=certificate(ownLog.bytes.toString('ascii'),trust,store)
    if (observed && JSON.stringify(observed)!==JSON.stringify(cert)) throw new Error('personalSshCertificateChanged')
    observed=cert
  }
  const validateAt=(epoch:number)=>{ if (!observed || !current() || epoch<observed.from || epoch>=observed.until || epoch>=trust.admittedUntil) throw new Error('personalSshCertificateExpired') }
  // A primeira resposta helper (time) exige canal autenticado; então check qualifica
  // metadados completos do MESMO child, CA e argv antes de qualquer ticket/consumo.
  const provenance=()=>{if(!observed || !current())throw new Error('personalSshUnproven');return personalHash(['sshHelper',trust,profile,observed])}
  return Object.freeze({kind:'sshHelper',sourceId:profile.id,helper,possessionURL:`ws://127.0.0.1:${localPort}/personal-possession`,current,check,provenance,originRecord:()=>Object.freeze(['sshHelper',profile.id,trust.evidenceDigest,trust.host,trust.port,trust.user,trust.principal,trust.caFingerprint,provenance()] as const),validateAt,close:()=>helper.close()})
}
/** Proveniência privada TOFU; serve ao legado, SEMPRE recusada como origem PL1. */
export async function recordPersonalTofu(dataDir: string, profile: PersonalSshProfile, lines: readonly string[]) {
  const record={schema:1,id:randomBytes(16).toString('base64url'),method:'keyscanUserConfirmed',at:Date.now(),profileId:profile.id,host:profile.host,port:profile.port??22,user:profile.user,keys:[...lines]}
  writePrivateFile(join(dataDir,'ssh-provenance.jsonl'),JSON.stringify(record)+'\n',true)
}
