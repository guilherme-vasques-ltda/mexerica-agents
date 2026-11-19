import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import * as crypto from './crypto.ts'
import * as protocol from './personal-protocol.ts'
import * as wire from './personal-wire.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
import type { PersonalElapsed } from './personal-runtime.ts'
const vector=JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json',import.meta.url),'utf8')).personalBootstrap
const authority=protocol.inspectPersonalManifest(Buffer.from(vector.manifestCanonical)),E=vector.wireVectors.packet.time,computer=E[0][3]
function fixture(quantum:1|1000=1){let sample={lo:90,hi:91},perf=100,alive=true
  const source:PersonalElapsed={quantum,generation:'private-test-source',auxiliary(){return perf},current(){assert(alive)},sample(){assert(alive);return sample},close(){alive=false}}
  const {PersonalClock}=loadMainModule<typeof import('./personal-clock.ts')>('fork-cloud/personal-clock',{'./personal-protocol':protocol,'./personal-wire':wire,'./crypto':crypto},{Buffer,performance:{now:()=>perf}})
  const clock=new PersonalClock(source)
  return{clock,set(lo:number,hi=lo+quantum,performanceAt=lo){sample={lo,hi};perf=performanceAt},fail(){alive=false}}
}
test('PL1 C3b.1: time pré-A vincula E/C/computer; suspender e replay não renovam referência',()=>{
  const f=fixture(),flight=f.clock.begin();f.set(100);f.clock.accept(authority,E,computer,flight)
  assert.deepEqual({...f.clock.window(computer)},{generation:flight.generation,lower:E[0][4],upper:E[0][4]+12})
  f.set(120);assert.equal(f.clock.window(computer).lower,E[0][4]+19);assert.equal(f.clock.validity(computer),E[0][4]+32)
  f.clock.cut();assert.throws(()=>f.clock.validity(computer));assert.throws(()=>f.clock.accept(authority,E,computer,f.clock.begin()))
  const other=fixture(),old=other.clock.begin();other.set(6000);assert.throws(()=>other.clock.accept(authority,E,computer,old))
  const wrong=fixture();assert.throws(()=>wrong.clock.accept(authority,E,'A'.repeat(22),wrong.clock.begin()))
  const regressed=fixture(),request=regressed.clock.begin();regressed.set(100);regressed.clock.accept(authority,E,computer,request);regressed.set(99);assert.throws(()=>regressed.clock.validity(computer))
})
test('PL1 M1: q1000 e RTT independente não somam trânsito duas vezes nem antecipam emissão',()=>{
  const f=fixture(1000);f.set(99999,101001);const sent=f.clock.begin();f.set(100009,101011)
  const p=[...E[0]];p[4]=100010;const signer=crypto.signerFromSeed(crypto.unb64url(vector.wireVectors.seeds.issuer,32)!);const time=[p,crypto.b64url(signer.sign(Buffer.from(JSON.stringify(p))))]
  f.clock.accept(authority,time,computer,sent);const w=f.clock.window(computer);assert.equal(w.lower,100010);assert.equal(w.upper,101023)
  assert.throws(()=>f.clock.issuedAfter(computer,100012,200000),/NotCausal/)
  f.set(101014,102014);assert.equal(f.clock.issuedAfter(computer,100012,200000),100013)
  assert.throws(()=>f.clock.issuedAfter(computer,0,f.clock.validity(computer)),/Expired/)
})
test('PL1 M1: sono silencioso e pausas subepsilon acumuladas cortam; ambos avançando cobram validade',()=>{
  const f=fixture(),sent=f.clock.begin();f.set(100);f.clock.accept(authority,E,computer,sent)
  for(let n=1;n<=10;n++){f.set(100+n,101+n,100);assert.doesNotThrow(()=>f.clock.validity(computer))}
  assert.doesNotThrow(()=>f.clock.validity(computer));f.set(140,141,100);assert.throws(()=>f.clock.validity(computer),/Suspended/)
  const expired=fixture(),flight=expired.clock.begin();expired.set(100);expired.clock.accept(authority,E,computer,flight);expired.set(400000);assert.throws(()=>expired.clock.validity(computer))
})
test('PL1 M1: fonte perdida/flight de outra geração/cut não ressuscitam referência',()=>{
  const f=fixture(),sent=f.clock.begin();f.clock.cut();f.set(100);assert.throws(()=>f.clock.accept(authority,E,computer,sent));assert.throws(()=>f.clock.validity(computer))
  const failure=fixture(),request=failure.clock.begin();failure.set(100);failure.clock.accept(authority,E,computer,request);failure.fail();assert.throws(()=>failure.clock.validity(computer))
})
