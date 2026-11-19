/** P30 assinado na mesma admissão probe. Persistência só pela primitiva FS qualificada. */
import { b64url,type DeviceSigner } from './crypto'
import type { Principal } from './principal'
import type { DeviceBinding } from '../fork-account/service'
import type { PersonalManifest } from './personal-protocol'
import type { PersonalOriginRecord } from './personal-origin'
import type { PersonalProbeEvidence } from './personal-probe'
import { personalDomain as D,personalHash } from './personal-wire'
import { personalBytes,personalNeed,personalSame } from './personal-session'
import { qualifyPersonalPinHistory,type HistoricalPinEvidence } from './personal-pin-history'

export async function signPersonalPin(principal:Principal,binding:Readonly<DeviceBinding>,authority:PersonalManifest,signer:DeviceSigner,attemptId:string,sourceId:string,origin:PersonalOriginRecord,probe:PersonalProbeEvidence):Promise<HistoricalPinEvidence>{
  await probe.check();probe.current();personalNeed(principal.current() && b64url(signer.publicKey)===binding.deviceKey && origin[1]===sourceId)
  const o=probe.originals,T=probe.observation.tuple,R=o.receipt as unknown[][]
  personalNeed(T[5]===binding.userId && T[6]===binding.deviceId && T[7]===binding.deviceKey && personalSame(o.activation,probe.proof.activation))
  const P=[D('local-pin'),1,'personal-linux-v1',principal.namespace,attemptId,sourceId,origin,authority.context,T,probe.observation.hT,T[11],T[10],T[12],T[13],T[5],T[6],T[7],R[0][6],personalHash(R[0]),o['local-ticket'],o.grant,o.receipt,o.publish,o['host-state-request'],o['host-state'],o.activation,probe.nonce,probe.hello,probe.proof,o]
  personalNeed(P.length===30);probe.current()
  const signed=personalBytes([P,b64url(signer.sign(personalBytes(P)))]);probe.current()
  const evidence=qualifyPersonalPinHistory(authority,binding,principal.namespace,signed)
  await probe.check();probe.current();personalNeed(principal.current());return evidence
}
