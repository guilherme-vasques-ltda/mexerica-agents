/** EpochWindow: lower emite, upper vence; fonte qualificada inclui suspensão. */
import { personalCanonical,type PersonalManifest } from './personal-protocol'
import { personalDomain } from './personal-wire'
import { unb64url,verifyEd25519 } from './crypto'
import type { PersonalElapsed } from './personal-runtime'
import type { HistoricalPinEvidence } from './personal-pin-history'
import { verifyPinnedStateObservation } from './personal-initial'
export interface EpochWindow {readonly generation:string;readonly lower:number;readonly upper:number}
type Sample=Readonly<{lo:number;hi:number}>
export interface PersonalTimeFlight {readonly generation:string;readonly sent:Sample}
export interface PersonalPinnedFlight extends PersonalTimeFlight {readonly pin:HistoricalPinEvidence;readonly read:string;readonly budgetUntil:number}
interface Reference {readonly flight:PersonalTimeFlight;readonly received:Sample;readonly epoch:number;readonly until:number;readonly hub:string;readonly performanceAt:number;readonly origin:Sample}
function safe(n:number){if(!Number.isSafeInteger(n) || n<0)throw new Error('personalTimeInvalid');return n}
export class PersonalClock {
  private reference:Reference|null=null
  private readonly flights=new WeakSet<PersonalTimeFlight>()
  private readonly pinnedFlights=new WeakSet<PersonalPinnedFlight>()
  private readonly seen=new Set<string>()
  private validityFloor=0
  private lastIssued=0
  private generation=0
  private lastSample:Sample|null=null
  constructor(private readonly source:PersonalElapsed){}
  cut(){this.reference=null;this.generation++}
  private sample():Sample {
    try{this.source.current();const first=this.source.sample(),last=this.source.sample(),lo=safe(first.lo),hi=safe(last.hi);if(hi<lo || hi-lo>2*this.source.quantum+5 || this.lastSample && (lo<this.lastSample.lo || hi<this.lastSample.hi))throw new Error('personalTimeRegression');this.lastSample=Object.freeze({lo,hi});return this.lastSample}catch(e){this.cut();throw e}
  }
  begin():PersonalTimeFlight{const flight=Object.freeze({generation:`${this.source.generation}:${this.generation}`,sent:this.sample()});this.flights.add(flight);return flight}
  accept(authority:PersonalManifest,time:unknown,computer:string,flight:PersonalTimeFlight){
    this.reference=null
    try{
      const received=this.sample(),canonical=personalCanonical(Buffer.from(JSON.stringify(time)))
      if(!this.flights.delete(flight) || flight.generation!==`${this.source.generation}:${this.generation}` || !Array.isArray(canonical) || canonical.length!==2 || !Array.isArray(canonical[0]))throw new Error('personalTimeShape')
      const p=canonical[0],rtt=safe(received.hi-flight.sent.lo),key=unb64url(authority.issuerPublicKey,32),sig=unb64url(canonical[1],64)
      if(p.length!==6 || p[0]!==personalDomain('time') || JSON.stringify(p[1])!==JSON.stringify(authority.context) || !unb64url(p[2],32) || !unb64url(computer,16) || p[3]!==computer || !key || !sig || !verifyEd25519(key,sig,Buffer.from(JSON.stringify(p))) || !Number.isSafeInteger(p[4]) || p[4]<=0 || !Number.isSafeInteger(p[5]) || p[5]<=rtt || p[5]>300000 || rtt>5000 || this.seen.has(p[2]) || received.lo<flight.sent.lo || received.hi<flight.sent.hi)throw new Error('personalTimeBinding')
      const until=safe(flight.sent.lo+p[5]);if(received.hi>=until)throw new Error('personalTimeExpired')
      this.seen.add(p[2]);this.reference=Object.freeze({flight,received,epoch:p[4],until,hub:computer,performanceAt:this.source.auxiliary(),origin:received});this.window(computer)
    }catch(e){this.cut();throw e}
  }
  /** Read PIN fechado, assinado uma vez; nenhum accept em canal ativo. */
  beginPinnedRead(pin:HistoricalPinEvidence,read:unknown,originalStart:Sample):PersonalPinnedFlight{
    this.cut()
    const sent=this.sample(),budgetUntil=safe(originalStart.lo+30000)
    if(!Number.isSafeInteger(originalStart.hi) || originalStart.hi<originalStart.lo || sent.lo<originalStart.lo || sent.hi<originalStart.hi || sent.hi>=budgetUntil)throw new Error('personalPinnedReadTimeout')
    const flight=Object.freeze({generation:`${this.source.generation}:${this.generation}`,sent,pin,read:JSON.stringify(read),budgetUntil})
    this.pinnedFlights.add(flight);return flight
  }
  acceptPinnedState(authority:PersonalManifest,state:unknown,pin:HistoricalPinEvidence,read:unknown,flight:PersonalPinnedFlight){
    this.reference=null
    try{
      const received=this.sample()
      if(!this.pinnedFlights.delete(flight) || flight.pin!==pin || flight.read!==JSON.stringify(read) || flight.generation!==`${this.source.generation}:${this.generation}` || received.lo<flight.sent.lo || received.hi<flight.sent.hi || received.hi>=flight.budgetUntil)throw new Error('personalPinnedFlightInvalid')
      const p=verifyPinnedStateObservation(authority,state,read,pin.tuple),q=JSON.parse(flight.read)[0] as number[],rtt=safe(received.hi-flight.sent.lo),ttl=Math.min(30000,q[7]-(p[15] as number)),nonce=`S:${pin.digest}:${p[14]}`
      if(rtt>5000 || !Number.isSafeInteger(ttl) || ttl<=rtt || this.seen.has(nonce))throw new Error('personalPinnedFlightExpired')
      const until=Math.min(safe(flight.sent.lo+ttl),flight.budgetUntil);if(received.hi>=until)throw new Error('personalPinnedFlightExpired')
      this.seen.add(nonce);this.reference=Object.freeze({flight,received,epoch:p[15] as number,until,hub:pin.tuple[11] as string,performanceAt:this.source.auxiliary(),origin:received})
      this.window(pin.tuple[11] as string)
      return p
    }catch(error){this.cut();throw error}
  }
  window(hub:string):EpochWindow {
    try{
      const r=this.reference,n=this.sample();if(!r || r.hub!==hub || r.flight.generation!==`${this.source.generation}:${this.generation}` || n.hi>=r.until)throw new Error('personalTimeUnavailable')
      // Detector acumulado desde a referência ORIGINAL; só corta, nunca autoriza.
      const elapsed=this.source.auxiliary()-r.performanceAt,min=Math.max(0,n.lo-r.origin.hi),max=Math.max(0,n.hi-r.origin.lo),epsilon=2*this.source.quantum+10
      if(!Number.isFinite(elapsed) || elapsed<0 || elapsed<min-epsilon || elapsed>max+epsilon)throw new Error('personalSuspended')
      const lower=safe(r.epoch+Math.max(0,n.lo-r.received.hi)),upper=safe(r.epoch+1+Math.max(0,n.hi-r.flight.sent.lo));this.validityFloor=Math.max(this.validityFloor,upper)
      if(lower>upper || this.validityFloor>=safe(r.epoch+(r.until-r.flight.sent.lo)))throw new Error('personalTimeExpired')
      return Object.freeze({generation:r.flight.generation,lower,upper:this.validityFloor})
    }catch(e){this.cut();throw e}
  }
  validity(hub:string){return this.window(hub).upper}
  issuedAfter(hub:string,predecessor:number,until:number){const w=this.window(hub);if(w.lower<safe(predecessor) || w.lower<this.lastIssued)throw new Error('personalTimeNotCausal');if(w.upper>=safe(until))throw new Error('personalTimeExpired');this.lastIssued=w.lower;return w.lower}
}
