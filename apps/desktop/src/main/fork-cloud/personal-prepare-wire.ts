/** Wire prepare v1 fechado; sem API IPC ou alteração dos quadros de posse/uso. */
import { hkdfSync } from 'node:crypto'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import { b64url,unb64url,sha256,type Ephemeral } from './crypto'
import { personalCanonical,type PersonalContext } from './personal-protocol'
import { personalDomain as D,personalHash } from './personal-wire'
const need=(v:unknown)=>{if(!v)throw new Error('personalPrepareWire')}
export const prepareBytes=(v:unknown)=>Buffer.from(JSON.stringify(v))
export function prepareMessage(value:unknown,limit:number):Uint8Array {const raw=prepareBytes(value);need(raw.length>0 && raw.length<=limit);const size=Buffer.alloc(4);size.writeUInt32BE(raw.length);return Buffer.concat([size,raw])}
export function prepareDecode(message:Uint8Array,limit:number):unknown {
  const bytes=Buffer.from(message);need(bytes.length>=5 && bytes.length<=limit+4 && bytes.readUInt32BE(0)===bytes.length-4)
  const raw=bytes.subarray(4);if(raw.length<=16384)return personalCanonical(raw)
  // Apenas outer frame cifrado pode exceder o limite de envelope individual.
  const text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(raw),v:unknown=JSON.parse(text)
  need(JSON.stringify(v)===text && Array.isArray(v) && v.length===5 && v[0]===D('prepare-frame') && v[1]===1 && (v[2]==='a2h'||v[2]==='h2a') && v[3]===0 && typeof v[4]==='string' && !!unb64url(v[4]))
  return v
}
export function prepareIdentity(C:PersonalContext,Q:unknown) {
  const q=personalCanonical(prepareBytes(Q));need(Array.isArray(q) && q.length===2 && Array.isArray(q[0]))
  const p=(q as unknown[][])[0],hq=personalHash(p),wireHash=personalHashBytes(prepareBytes(q))
  return Object.freeze({hq,wireHash,capture:personalHash([D('prepare-capture'),1,C,p[5],p[6],p[7],p[8],hq,wireHash])})
}
export const personalHashBytes=(bytes:Uint8Array)=>Buffer.from(sha256(bytes)).toString('hex')
export const prepareOperation=(C:PersonalContext,capture:string,hq:string,wireHash:string,digest:string)=>personalHash([D('prepare-operation'),1,C,capture,hq,wireHash,'candidate',digest])
export const prepareTranscript=(hello:unknown,welcome0:unknown)=>sha256(prepareBytes([D('prepare-transcript'),1,'GET','/personal-prepare',hello,welcome0]))
export class PrepareFrames {
  private readonly a2h:Uint8Array
  private readonly h2a:Uint8Array
  private sent=false
  private received=false
  constructor(private readonly hello:readonly unknown[],private readonly th:Uint8Array,e:Ephemeral,hostEphemeral:Uint8Array) {
    const shared=e.dh(hostEphemeral)
    try{need(shared.some(b=>b!==0));this.a2h=new Uint8Array(hkdfSync('sha256',shared,th,Buffer.from(D('prepare-key-a2h')),32));this.h2a=new Uint8Array(hkdfSync('sha256',shared,th,Buffer.from(D('prepare-key-h2a')),32))}finally{shared.fill(0)}
  }
  private aad(direction:'a2h'|'h2a'){return prepareBytes([D('prepare-frame'),1,this.hello[2],this.hello[3],this.hello[4],this.hello[5],b64url(this.th),direction,0])}
  request(inner:unknown){need(!this.sent);const raw=prepareBytes(inner);personalCanonical(raw);const ciphertext=chacha20poly1305(this.a2h,new Uint8Array(12),this.aad('a2h')).encrypt(raw);this.sent=true;return[D('prepare-frame'),1,'a2h',0,b64url(ciphertext)]}
  response(frame:unknown){need(this.sent && !this.received && Array.isArray(frame));const f=frame as unknown[];need(f.length===5 && f[0]===D('prepare-frame') && f[1]===1 && f[2]==='h2a' && f[3]===0);const ciphertext=unb64url(f[4]);need(ciphertext && ciphertext.length>=17 && ciphertext.length<=16400);const plain=chacha20poly1305(this.h2a,new Uint8Array(12),this.aad('h2a')).decrypt(ciphertext!);try{this.received=true;return personalCanonical(plain)}finally{plain.fill(0)}}
  clear(){this.a2h.fill(0);this.h2a.fill(0)}
}
