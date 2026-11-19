/** Único ingresso HTTPS Read8 pré-época. Parede só limita transporte; S dá época. */
import { b64url,random,signatureHeader,type DeviceSigner } from './crypto'
import type { Principal } from './principal'
import type { PersonalManifest } from './personal-protocol'
import type { PersonalElapsed } from './personal-runtime'
import type { PersonalClock } from './personal-clock'
import type { HistoricalPinEvidence } from './personal-pin-history'
import { personalDomain as D } from './personal-wire'
import { verifyPinnedStateObservation } from './personal-initial'
import { personalBytes,personalNeed,personalSame } from './personal-session'

export interface PinnedClockReadGuard {check():Promise<HistoricalPinEvidence>;current():void}
export async function postPinnedClockRead(principal:Principal,signer:DeviceSigner,authority:PersonalManifest,pin:HistoricalPinEvidence,elapsed:PersonalElapsed,clock:PersonalClock,guard:PinnedClockReadGuard){
  guard.current();elapsed.current();personalNeed(principal.current() && pin.namespace===principal.namespace && b64url(signer.publicKey)===pin.tuple[7])
  const start=elapsed.sample(),budgetUntil=start.lo+30000,w=Date.now(),end=w+30000
  personalNeed(Number.isSafeInteger(budgetUntil) && Number.isSafeInteger(w) && w>0 && Number.isSafeInteger(end))
  const payload=[D('read'),authority.context,pin.tuple[1],pin.tuple[9],pin.hT,b64url(random(32)),w,end]
  const read=[payload,b64url(signer.sign(personalBytes(payload)))] as const,raw=personalBytes({read}),url=new URL('/v1/personal-bootstrap/read',principal.deployment.origin)
  personalNeed(url.protocol==='https:' && url.origin===principal.deployment.origin && !url.search && !url.hash && raw.length<=16384)
  const check=async()=>{const current=await guard.check();guard.current();elapsed.current();personalNeed(principal.current() && current.digest===pin.digest && personalSame(current.tuple,pin.tuple));if(elapsed.sample().hi>=budgetUntil)throw new Error('personalPinnedReadTimeout')}
  const token=await principal.token();await check()
  // Timestamp/corpo/nonce são os MESMOS depois de signer, FS e token; sem restamp.
  const header=signatureHeader(signer,'POST',url,raw,w,b64url(random(16)));await check();guard.current()
  const flight=clock.beginPinnedRead(pin,read,start)
  personalNeed(flight.sent.hi<budgetUntil && principal.current())
  // Nenhum await entre flight final e fetch; nenhuma Window de parede foi criada.
  const response=await fetch(url,{method:'POST',body:raw,headers:{Authorization:`Bearer ${token}`,'Codync-Sig':header,'Content-Type':'application/json','Cache-Control':'no-store'},redirect:'error',cache:'no-store',signal:AbortSignal.timeout(5000)})
  await check();personalNeed(response.ok && response.body)
  const reader=response.body.getReader(),chunks:Uint8Array[]=[];let size=0
  try{for(;;){const {done,value}=await reader.read();await check();if(done)break;size+=value.length;personalNeed(size<=192*1024);chunks.push(value)}}finally{await reader.cancel()}
  await check();const text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(Buffer.concat(chunks)),reply:unknown=JSON.parse(text)
  personalNeed(reply && typeof reply==='object' && !Array.isArray(reply) && JSON.stringify(reply)===text && Object.keys(reply).sort().join(',')==='activationCurrent,originals,state')
  const value=reply as Record<string,unknown>
  // Primitivas issuer/A, C/tupla/nonce/Read verificados ANTES de aceitar época.
  verifyPinnedStateObservation(authority,value.state,read,pin.tuple);guard.current()
  clock.acceptPinnedState(authority,value.state,pin,read,flight);guard.current()
  return Object.freeze({reply:value,read,state:value.state})
}
