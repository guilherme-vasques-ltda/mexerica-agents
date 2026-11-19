/** P30/17: história local autenticada, sem now/clock/lease/Owner atuais. */
import { unb64url,verifyEd25519,computerIdOf } from './crypto'
import type { PersonalManifest } from './personal-protocol'
import { personalCanonical } from './personal-protocol'
import { WIRE_SHAPES } from './personal-shapes'
import { personalDomain as D,personalHash,PersonalVerifier,verifyPersonalCeremony } from './personal-wire'
import { personalBytes,personalNeed,personalSame,personalImmutable,personalOriginalNames } from './personal-session'
import type { DeviceBinding } from '../fork-account/service'
import type { PersonalOriginRecord } from './personal-origin'

const historyBrand:unique symbol=Symbol('personalPinHistory')
export interface HistoricalPinEvidence {
  readonly [historyBrand]:true
  readonly body:readonly unknown[];readonly signedBytes:Uint8Array;readonly digest:string
  readonly namespace:string;readonly attemptId:string;readonly sourceId:string;readonly origin:PersonalOriginRecord
  readonly tuple:readonly unknown[];readonly hT:string;readonly originals:Readonly<Record<string,unknown>>
}
function safe(v:unknown,depth=0):boolean{
  if(depth>32)return false
  if(typeof v==='number')return Number.isSafeInteger(v) && v>=0
  if(typeof v==='string')return [...v].every(c=>{const n=c.codePointAt(0)!;return n<0xd800 || n>0xdfff})
  if(Array.isArray(v))return v.every(x=>safe(x,depth+1))
  if(v && typeof v==='object')return Object.entries(v).every(([k,x])=>safe(k,depth+1) && safe(x,depth+1))
  return v===null || typeof v==='boolean'
}
export function personalPinCanonical(raw:Uint8Array,limit=192*1024):unknown{
  personalNeed(raw.byteLength>0 && raw.byteLength<=limit)
  const text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(raw),value:unknown=JSON.parse(text)
  personalNeed(safe(value) && JSON.stringify(value)===text);return value
}
function exact(v:unknown,fields:readonly string[]):asserts v is Record<string,unknown>{personalNeed(v && typeof v==='object' && !Array.isArray(v) && Object.keys(v).length===fields.length && fields.every(k=>Object.hasOwn(v,k)))}
const hash=(v:unknown)=>typeof v==='string' && /^[a-f0-9]{64}$/.test(v)
const id=(v:unknown)=>typeof v==='string' && /^[A-Za-z0-9_./:-]{1,256}$/.test(v)
const int=(v:unknown):v is number=>typeof v==='number' && Number.isSafeInteger(v) && v>0
function primitiveField(token:string,v:unknown):boolean{
  if(token.startsWith('?'))return v===null || primitiveField(token.slice(1),v)
  if(token.startsWith('='))return v===token.slice(1)
  switch(token){
    case 'hash':return hash(v)
    case 'key':return !!unb64url(v,32)
    case 'cid':return !!unb64url(v,16)
    case 'grant':return typeof v==='string' && /^grt_[A-Za-z0-9_-]{22}$/.test(v) && !!unb64url(v.slice(4),16)
    case 'id':return id(v)
    case 'int':return int(v)
    case 'zero':return v===0
    case 'mode':return v==='command' || v==='device'
    case 'caps':return JSON.stringify(v)==='["control","owner.devices"]'
    default:return false
  }
}
// Restrito às seis primitivas exigidas ANTES de selecionar E17. Não exporta
// Signed nem um modo geral de verificação sem tempo para outros consumidores.
function primitive(kind:'host-state'|'activation'|'receipt'|'local-ticket'|'host-state-request'|'publish',value:unknown,authority:PersonalManifest,signKey:string):readonly unknown[]{
  const e=personalCanonical(personalBytes(value));personalNeed(Array.isArray(e) && e.length===2 && Array.isArray(e[0]))
  const p=e[0],tokens=WIRE_SHAPES[kind][1].split(' '),key=unb64url(signKey,32),sig=unb64url(e[1],64)
  personalNeed(p.length===tokens.length+2 && p[0]===D(kind) && personalSame(p[1],authority.context) && tokens.every((t,i)=>primitiveField(t,p[i+2])) && key && sig && verifyEd25519(key,sig,personalBytes(p)))
  return p
}

export function qualifyPersonalPinHistory(authority:PersonalManifest,binding:Readonly<DeviceBinding>,namespace:string,raw:Uint8Array):HistoricalPinEvidence{
  const signed=personalPinCanonical(raw);personalNeed(Array.isArray(signed) && signed.length===2 && Array.isArray(signed[0]))
  const P=signed[0],key=unb64url(binding.deviceKey,32),sig=unb64url(signed[1],64)
  personalNeed(P.length===30 && P[0]===D('local-pin') && P[1]===1 && P[2]==='personal-linux-v1' && P[3]===namespace && typeof P[4]==='string' && !!unb64url(P[4],16) && id(P[5]) && personalSame(P[7],authority.context) && P[14]===binding.userId && P[15]===binding.deviceId && P[16]===binding.deviceKey && key && sig && verifyEd25519(key,sig,personalBytes(P)))
  const origin=P[6];personalNeed(Array.isArray(origin) && personalBytes(origin).length<=4096 && origin[1]===P[5] && hash(origin[origin.length-1]))
  if(origin[0]==='localHelper')personalNeed(origin.length===4 && origin[2]==='/run/donnabot/bootstrap-helper-v1.sock')
  else personalNeed(origin[0]==='sshHelper' && origin.length===9 && hash(origin[2]) && typeof origin[3]==='string' && /^[a-z0-9.-]{1,253}$/.test(origin[3]) && int(origin[4]) && origin[4]<=65535 && typeof origin[5]==='string' && /^[A-Za-z_][A-Za-z0-9_-]*$/.test(origin[5]) && typeof origin[6]==='string' && /^[a-z0-9.-]{1,253}$/.test(origin[6]) && typeof origin[7]==='string' && /^SHA256:[A-Za-z0-9+/]{43}$/.test(origin[7]))
  const originals=P[29];exact(originals,personalOriginalNames)
  const host=unb64url(P[12],32);personalNeed(host && unb64url(P[13],32) && computerIdOf(host)===P[10])
  const E=primitive('host-state',originals['host-state'],authority,authority.issuerPublicKey)
  const V=primitive('activation',originals.activation,authority,P[12] as string)
  const R=primitive('receipt',originals.receipt,authority,P[12] as string)
  const Q=primitive('local-ticket',originals['local-ticket'],authority,P[12] as string)
  const request=primitive('host-state-request',originals['host-state-request'],authority,P[12] as string)
  const publication=primitive('publish',originals.publish,authority,binding.deviceKey)
  const G=originals.grant as unknown[][],T=P[8]
  personalNeed(Array.isArray(T) && T.length===19 && G && Array.isArray(G[0]) && personalSame(G[0][2],T) && personalHash(T)===P[9] && personalSame(T[0],authority.context))
  for(const[pin,tuple]of [[10,11],[11,10],[12,12],[13,13],[14,5],[15,6],[16,7]] as const)personalNeed(P[pin]===T[tuple])
  personalNeed(Q[6]===P[10] && Q[7]===P[12] && Q[8]===P[13] && Q[5]===P[11] && R[2]===P[9] && R[3]===personalHash(G[0]) && R[6]===P[17] && personalHash(R)===P[18])
  personalNeed(publication[2]===T[1] && publication[3]===P[9] && publication[4]===R[3] && publication[5]===P[17] && publication[6]===P[18])
  personalNeed(request[2]===P[9] && request[3]===R[3] && request[4]===P[17] && request[5]===P[18] && E[2]===P[9] && E[3]===R[3] && E[4]===P[17] && E[5]===P[18] && E[6]===personalHash(publication) && E[15]===request[6])
  for(const[e,t]of [[7,5],[8,6],[9,7],[10,11],[11,10],[13,16]] as const)personalNeed(E[e]===T[t])
  personalNeed(V[2]===P[9] && V[3]===P[17] && V[4]===P[18] && V[5]===P[14] && V[6]===P[15] && V[7]===P[16] && V[8]===P[10] && V[9]===P[11])
  personalNeed(int(E[16]) && int(E[17]) && int(V[13]) && int(V[14]) && int(R[7]) && E[17]>E[16] && E[17]-E[16]<=900000 && R[7]<=E[16] && E[16]<=V[13] && V[13]<V[14] && V[14]<=E[17] && E[16]>=(request[7] as number) && E[16]<(request[8] as number) && Number.isSafeInteger(E[17]+30000))
  // Só AGORA os bytes issuer-authenticated E17 escolhem a âncora histórica.
  const verifier=new PersonalVerifier(authority.context,{issuer:authority.issuerPublicKey,app:binding.deviceKey,hub:P[12] as string,box:P[13] as string})
  const checked=verifyPersonalCeremony(verifier,personalBytes(originals),{kind:'historical',now:E[17]})
  personalNeed(Object.keys(checked.originals).length===17 && personalSame(checked.tuple,T) && checked.hash===P[9])
  for(const[index,name]of [[19,'local-ticket'],[20,'grant'],[21,'receipt'],[22,'publish'],[23,'host-state-request'],[24,'host-state'],[25,'activation']] as const)personalNeed(personalSame(P[index],originals[name]))
  exact(P[27],['hostId','computerId','signKey','boxKey','name','version','minApp']);const hello=P[27]
  personalNeed(hello.computerId===P[10] && hello.signKey===P[12] && hello.boxKey===P[13] && ['hostId','name','version','minApp'].every(k=>typeof hello[k]==='string' && (hello[k] as string).length>0 && (hello[k] as string).length<=256))
  exact(P[28],['nonce','profile','C','hT','computerId','hubGeneration','grantId','hostStateRequest','hostState','publication','activation']);const proof=P[28]
  personalNeed(unb64url(P[26],32) && proof.nonce===P[26] && proof.profile===P[2] && personalSame(proof.C,P[7]) && proof.hT===P[9] && proof.computerId===P[10] && proof.hubGeneration===P[11] && proof.grantId===P[17] && personalSame(proof.hostStateRequest,P[23]) && personalSame(proof.hostState,P[24]) && personalSame(proof.publication,P[22]) && personalSame(proof.activation,P[25]))
  // Uint8Array não pode ser Object.freeze no Node; devolver cópia por getter.
  const owned=Buffer.from(raw)
  return Object.freeze({[historyBrand]:true as const,body:personalImmutable(P),get signedBytes(){return new Uint8Array(owned)},digest:personalHash(signed),namespace,attemptId:P[4] as string,sourceId:P[5] as string,origin:personalImmutable(origin) as unknown as PersonalOriginRecord,tuple:checked.tuple,hT:checked.hash,originals:personalImmutable(originals)})
}
