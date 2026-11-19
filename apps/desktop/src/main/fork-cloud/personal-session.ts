/** Observações atuais pessoais. V/S renováveis nunca substituem os originais PIN. */
import type { DeviceBinding } from '../fork-account/service'
import type { EpochWindow } from './personal-clock'
import type { PersonalManifest } from './personal-protocol'
import { verifyStateObservation } from './personal-initial'
import { PersonalVerifier,personalHash,verifyPersonalCeremony } from './personal-wire'
import type { HistoricalPinEvidence } from './personal-pin-history'

const observationBrand:unique symbol=Symbol('personalObservation')
const ceremonyBrand:unique symbol=Symbol('personalCeremonyEffect')
const pinnedBrand:unique symbol=Symbol('personalPinnedEffect')
export const personalOriginalNames=['intent','local-ticket','app-consent','local-consent','box-challenge','box-proof','approve','local-confirm','consume','grant','challenge','proof','receipt','publish','activation','host-state-request','host-state'] as const
export const personalRemoteNames:Readonly<Record<string,string>>={intent:'intent',ticket:'local-ticket',appConsent:'app-consent',localConsent:'local-consent',boxChallenge:'box-challenge',boxProof:'box-proof',approval:'approve',localConfirmation:'local-confirm',consume:'consume',grant:'grant',challenge:'challenge',proof:'proof',receipt:'receipt',publish:'publish',activation:'activation',fence:'fence',fenceAck:'fence-ack',revocation:'revocation'}
export const personalBytes=(v:unknown)=>Buffer.from(JSON.stringify(v))
export const personalSame=(a:unknown,b:unknown)=>JSON.stringify(a)===JSON.stringify(b)
export function personalNeed(v:unknown):asserts v{if(!v)throw new Error('personalSessionInvalid')}
export function personalImmutable<T>(v:T):T{if(v && typeof v==='object'){for(const item of Object.values(v))personalImmutable(item);Object.freeze(v)}return v}
export interface CurrentPersonalObservation {
  readonly [observationBrand]:true
  readonly state:readonly unknown[];readonly raw:unknown;readonly request:readonly [readonly unknown[],string]
  readonly originals:Readonly<Record<string,unknown>>;readonly activationCurrent:unknown|null
  readonly tuple:readonly unknown[];readonly hT:string
}
export interface PersonalEffectGuard {check():Promise<void>;sample():EpochWindow;current():void}
export interface CeremonyEffect extends PersonalEffectGuard {readonly [ceremonyBrand]:true}
export interface PinnedEffect extends PersonalEffectGuard {readonly [pinnedBrand]:true}
export function ceremonyEffect(guard:PersonalEffectGuard):CeremonyEffect{return Object.freeze({...guard,[ceremonyBrand]:true as const})}
export function pinnedEffect(pin:HistoricalPinEvidence,guard:PersonalEffectGuard):PinnedEffect{
  personalNeed(pin.digest && pin.originals && pin.tuple);return Object.freeze({...guard,[pinnedBrand]:true as const})
}

export function verifyPersonalObservation(authority:PersonalManifest,binding:Readonly<DeviceBinding>,packet:Readonly<Record<string,unknown>>,request:readonly [readonly unknown[],string],reply:Readonly<Record<string,unknown>>,window:EpochWindow):CurrentPersonalObservation {
  const state=verifyStateObservation(authority,reply.state,request[0],window.upper)
  personalNeed(reply.originals && typeof reply.originals==='object' && !Array.isArray(reply.originals))
  const originals=JSON.parse(JSON.stringify(reply.originals)) as Record<string,unknown>
  personalNeed(Object.keys(originals).every(k=>Object.hasOwn(personalRemoteNames,k)))
  const Q=packet['local-ticket'] as unknown[][],I=packet.intent as unknown[][]
  personalNeed(Q && I && I[0][4]===binding.userId && I[0][5]===binding.deviceId && I[0][6]===binding.deviceKey)
  const verifier=new PersonalVerifier(authority.context,{issuer:authority.issuerPublicKey,app:binding.deviceKey,hub:Q[0][7] as string,box:Q[0][8] as string})
  const merged:Record<string,unknown>=Object.fromEntries(Object.entries(packet).filter(([name])=>!['activation','host-state-request','host-state'].includes(name)))
  // Observações substituem só a cópia de verificação, jamais packet/PIN/journal.
  for(const[remote,local]of Object.entries(personalRemoteNames))if(originals[remote]!==null && originals[remote]!==undefined)merged[local]=originals[remote]
  for(const[remote,local]of Object.entries(personalRemoteNames))if(!['activation','fence','fence-ack','revocation'].includes(local) && packet[local]!==undefined && originals[remote]!==null && originals[remote]!==undefined)personalNeed(personalSame(packet[local],originals[remote]))
  const checked=verifyPersonalCeremony(verifier,personalBytes({...merged,read:request,state:reply.state}),{kind:'historical',now:window.upper})
  personalNeed(checked.tuple[5]===binding.userId && checked.tuple[6]===binding.deviceId && checked.tuple[7]===binding.deviceKey)
  for(const[index,local]of [[8,'grant'],[9,'receipt'],[10,'activation'],[11,'revocation'],[12,'fence'],[13,'fence-ack']] as const)if(state[index]!==null)personalNeed(checked.originals[local]?.hash===state[index])
  const activation=reply.activationCurrent??null
  if(state[10]!==null){
    personalNeed(activation!==null && personalSame(activation,originals.activation) && ['hubActive'].includes(state[5] as string) && state[12]===null)
    const v=verifier.verify('activation',personalBytes(activation),{kind:'current',now:window.upper})
    personalNeed(v.hash===state[10] && v.payload[2]===checked.hash && v.payload[3]===state[7] && v.payload[4]===state[9])
  }else personalNeed(activation===null)
  return personalImmutable({[observationBrand]:true as const,state,raw:JSON.parse(JSON.stringify(reply.state)),request,originals,activationCurrent:activation,tuple:checked.tuple,hT:checked.hash})
}

export function requireLivePersonalObservation(observation:CurrentPersonalObservation,expectedActivation?:unknown){
  personalNeed(observation.state[5]==='hubActive' && observation.state[12]===null && observation.state[11]===null && observation.activationCurrent!==null && observation.state[10]===personalHash((observation.activationCurrent as unknown[][])[0]))
  if(expectedActivation!==undefined)personalNeed(personalSame(observation.activationCurrent,expectedActivation))
  return observation
}
