/** I inicial não requer chave Hub inventada para satisfazer o verificador composto. */
import { unb64url,verifyEd25519 } from './crypto'
import { personalCanonical,type PersonalManifest } from './personal-protocol'
import { personalDomain as D } from './personal-wire'
import { personalHash } from './personal-wire'
import type { DeviceBinding } from '../fork-account/service'
export function verifyInitialIntent(authority:PersonalManifest,binding:Readonly<DeviceBinding>,value:unknown,now:number){
  const e=personalCanonical(Buffer.from(JSON.stringify(value)))
  if(!Array.isArray(e) || e.length!==2 || !Array.isArray(e[0]))throw new Error('personalIntentShape')
  const p=e[0],id=(v:unknown)=>typeof v==='string' && /^[A-Za-z0-9_./:-]{1,256}$/.test(v)
  const key=unb64url(authority.issuerPublicKey,32),sig=unb64url(e[1],64)
  if(p.length!==11 || p[0]!==D('intent') || JSON.stringify(p[1])!==JSON.stringify(authority.context) || !id(p[2]) || !['command','device'].includes(p[3]) || p[4]!==binding.userId || p[5]!==binding.deviceId || p[6]!==binding.deviceKey || !unb64url(p[7],32) || !Number.isSafeInteger(p[8]) || !Number.isSafeInteger(p[9]) || p[8]<=0 || p[8]>now+30000 || p[9]<=now || p[9]<=p[8] || p[9]-p[8]>600000 || !Number.isSafeInteger(p[10]) || p[10]<=0 || !key || !sig || !verifyEd25519(key,sig,Buffer.from(JSON.stringify(p))))throw new Error('personalIntentInvalid')
  return Object.freeze([...p])
}

/** S fresco confirma observação/história; não concede lease ou autoridade de consumo. */
export function verifyStateObservation(authority:PersonalManifest,state:unknown,read:readonly unknown[],now:number){
  const e=personalCanonical(Buffer.from(JSON.stringify(state)))
  if(!Array.isArray(e) || e.length!==2 || !Array.isArray(e[0]))throw new Error('personalStateShape')
  const p=e[0],key=unb64url(authority.issuerPublicKey,32),sig=unb64url(e[1],64),hash=(v:unknown)=>v===null || typeof v==='string' && /^[a-f0-9]{64}$/.test(v)
  if(p.length!==17 || p[0]!==D('state') || JSON.stringify(p[1])!==JSON.stringify(authority.context) || p[2]!==read[2] || p[3]!==read[3] || p[4]!==read[4] || p[14]!==read[5] || read[0]!==D('read') || JSON.stringify(read[1])!==JSON.stringify(authority.context) || typeof read[6]!=='number' || typeof read[7]!=='number' || now>=read[7] || p[15]<read[6]-30000 || p[15]>read[7] || !['open','personalConfirmed','hubPending','published','hubActive','canceled','expired','refused','revoking','revoked'].includes(p[5]) || !Number.isSafeInteger(p[6]) || p[6]<=0 || !(p[7]===null || typeof p[7]==='string' && /^grt_[A-Za-z0-9_-]{22}$/.test(p[7])) || !p.slice(8,14).every(hash) || !Number.isSafeInteger(p[15]) || p[15]<=0 || p[15]>now+30000 || !Number.isSafeInteger(p[16]) || p[16]<=0 || !key || !sig || !verifyEd25519(key,sig,Buffer.from(JSON.stringify(p))))throw new Error('personalStateBinding')
  return Object.freeze([...p])
}

/** Ingresso exclusivo CLOCK-S pré-época. Nenhum now/Window/lease é recebido. */
export function verifyPinnedStateObservation(authority:PersonalManifest,state:unknown,read:unknown,tuple:readonly unknown[]){
  const r=personalCanonical(Buffer.from(JSON.stringify(read))),e=personalCanonical(Buffer.from(JSON.stringify(state)))
  if(!Array.isArray(r) || r.length!==2 || !Array.isArray(r[0]) || !Array.isArray(e) || e.length!==2 || !Array.isArray(e[0]))throw new Error('personalPinnedStateShape')
  const q=r[0],p=e[0],a=unb64url(tuple[7],32),rs=unb64url(r[1],64),issuer=unb64url(authority.issuerPublicKey,32),sig=unb64url(e[1],64),hash=(v:unknown)=>v===null || typeof v==='string' && /^[a-f0-9]{64}$/.test(v)
  if(q.length!==8 || q[0]!==D('read') || JSON.stringify(q[1])!==JSON.stringify(authority.context) || q[2]!==tuple[1] || q[3]!==tuple[9] || q[4]!==personalHash(tuple) || !unb64url(q[5],32) || !Number.isSafeInteger(q[6]) || q[6]<=0 || !Number.isSafeInteger(q[7]) || q[7]-q[6]!==30000 || !a || !rs || !verifyEd25519(a,rs,Buffer.from(JSON.stringify(q))))throw new Error('personalPinnedReadInvalid')
  if(p.length!==17 || p[0]!==D('state') || JSON.stringify(p[1])!==JSON.stringify(authority.context) || p[2]!==q[2] || p[3]!==q[3] || p[4]!==q[4] || p[14]!==q[5] || !['open','personalConfirmed','hubPending','published','hubActive','canceled','expired','refused','revoking','revoked'].includes(p[5]) || !Number.isSafeInteger(p[6]) || p[6]<=0 || !(p[7]===null || typeof p[7]==='string' && /^grt_[A-Za-z0-9_-]{22}$/.test(p[7]) && unb64url(p[7].slice(4),16)) || !p.slice(8,14).every(hash) || !Number.isSafeInteger(p[15]) || p[15]<q[6] || p[15]>=q[7] || !Number.isSafeInteger(p[16]) || p[16]<=0 || !issuer || !sig || !verifyEd25519(issuer,sig,Buffer.from(JSON.stringify(p))))throw new Error('personalPinnedStateInvalid')
  return Object.freeze([...p])
}
