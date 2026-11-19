/** Efeitos de A enumerados. A função não recebe rota/corpo do renderer. */
import { b64url, random, signatureHeader, type DeviceSigner } from './crypto'
import { personalCanonical } from './personal-protocol'
import type { Principal } from './principal'
import type { EpochWindow } from './personal-clock'
export interface PersonalAppGuard {check():Promise<void>;sample():EpochWindow}
export type PersonalAppRoute='command'|'attach'|'app-consent'|'box-challenge'|'approve'|'read'|'cancel'|'publish'|'revoke'
const fields:Record<PersonalAppRoute,readonly string[]>={command:['create'],attach:['attach','ticket','userCode'],'app-consent':['appConsent'],'box-challenge':['boxChallenge'],approve:['approval'],read:['read'],cancel:['cancel'],publish:['publish'],revoke:['revoke']}
export async function personalAppPost(p:Principal,signer:DeviceSigner,route:PersonalAppRoute,body:Readonly<Record<string,unknown>>,guard:PersonalAppGuard):Promise<Record<string,unknown>> {
  const allowed=fields[route]
  if (!p.current() || !allowed || Object.keys(body).length!==allowed.length || allowed.some(k=>!Object.hasOwn(body,k))) throw new Error('personalRequestInvalid')
  const raw=Buffer.from(JSON.stringify(body)),url=new URL(`/v1/personal-bootstrap/${route}`,p.deployment.origin)
  if (raw.length>192*1024 || url.protocol!=='https:' || url.origin!==p.deployment.origin || url.search) throw new Error('personalRequestInvalid')
  const token=await p.token(); await guard.check(); if (!p.current()) throw new Error('personalBindingChanged')
  const original=guard.sample(),at=original.lower,httpUntil=at+30000
  if(!Number.isSafeInteger(at) || at<=0 || !Number.isSafeInteger(httpUntil) || original.upper>=httpUntil)throw new Error('personalTimeUnavailable')
  const header=signatureHeader(signer,'POST',url,raw,at,b64url(random(16)))
  // Assinar pode atravessar sono/preempção. Não trocar o timestamp já assinado.
  await guard.check()
  const final=guard.sample()
  if(final.generation!==original.generation || at>final.lower || final.upper>=httpUntil)throw new Error('personalTimeExpired')
  if (!p.current()) throw new Error('personalBindingChanged')
  const response=await fetch(url,{method:'POST',body:raw,headers:{Authorization:`Bearer ${token}`,'Codync-Sig':header,'Content-Type':'application/json','Cache-Control':'no-store'},redirect:'error',cache:'no-store',signal:AbortSignal.timeout(5000)})
  await guard.check();guard.sample()
  if (!p.current() || !response.ok || !response.body) throw new Error('personalRequestUnknown')
  const reader=response.body.getReader(); let size=0; const chunks:Uint8Array[]=[]
  try { for (;;) { const {done,value}=await reader.read(); await guard.check();guard.sample(); if (done) break; size+=value.length; if (size>192*1024) throw new Error('personalResponseTooLarge'); chunks.push(value) } } finally { await reader.cancel() }
  await guard.check();guard.sample(); if (!p.current()) throw new Error('personalBindingChanged')
  const rawResponse=Buffer.concat(chunks)
  // packet pode conter múltiplos envelopes; canonical individual conserva 16KiB.
  const text=new TextDecoder('utf-8',{fatal:true}).decode(rawResponse), value=JSON.parse(text) as Record<string,unknown>
  if (!value || typeof value!=='object' || Array.isArray(value) || JSON.stringify(value)!==text) throw new Error('personalResponseInvalid')
  for (const v of Object.values(value)) if (Array.isArray(v)) personalCanonical(Buffer.from(JSON.stringify(v)))
  return value
}
