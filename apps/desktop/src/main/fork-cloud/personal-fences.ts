/** Cerca verificada por revisão; S terminal noncebound confirma, ACK isolado não. */
import { personalHash } from './personal-wire'
import { personalNeed,personalSame,personalImmutable,type CurrentPersonalObservation } from './personal-session'

export interface PersonalFenceRevision {
  readonly revision:number;readonly W:string;readonly Y:string|null;readonly X:string|null
}
export function verifyPersonalFenceObservation(observation:CurrentPersonalObservation,previous:PersonalFenceRevision|null){
  const {state,originals}=observation,W=originals.fence as unknown[][]|null,Y=originals.fenceAck as unknown[][]|null,X=originals.revocation as unknown[][]|null
  if(state[12]===null){personalNeed(previous===null);return null}
  personalNeed(W && personalHash(W[0])===state[12] && W[0][2]===observation.hT)
  const revision=W[0][6];personalNeed(typeof revision==='number' && Number.isSafeInteger(revision) && revision>0)
  const current:PersonalFenceRevision={revision,W:JSON.stringify(W),Y:Y?JSON.stringify(Y):null,X:X?JSON.stringify(X):null}
  if(previous){
    personalNeed(revision>=previous.revision)
    if(revision===previous.revision){personalNeed(current.W===previous.W && (previous.Y===null || current.Y===previous.Y) && (previous.X===null || current.X===previous.X))}
  }
  if(Y)personalNeed(Y[0][3]===state[12] && personalSame(Y[0][4],W[0][4]) && personalSame(Y[0][5],W[0][5]) && state[13]===personalHash(Y[0]))
  else personalNeed(state[13]===null)
  if(X)personalNeed(W[0][4]!==null && W[0][4]===X[0][3] && W[0][5]===X[0][4] && X[0][9]===W[0][7] && state[11]===personalHash(X[0]))
  else personalNeed(state[11]===null)
  const terminal=state[5]==='revoked' && W[0][4]!==null && W[0][5]!==null && Y!==null && X!==null && state[13]!==null && state[11]!==null
  return personalImmutable({record:current,terminal,read:observation.request,state:observation.raw})
}
