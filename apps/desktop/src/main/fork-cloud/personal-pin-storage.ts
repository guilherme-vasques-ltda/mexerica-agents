/** Codec local fechado. SafeStorage é fornecido exclusivamente pelo Main. */
import { createHash,randomBytes } from 'node:crypto'
import type { DeviceBinding } from '../fork-account/service'
import type { HistoricalPinEvidence } from './personal-pin-history'
import { personalPinCanonical } from './personal-pin-history'
import { personalNeed,personalSame,personalImmutable } from './personal-session'

export interface PinCipher {
  current():void
  encryptString(value:string):Buffer
  decryptString(value:Buffer):string
}
export interface PinPublicationRecord {
  readonly binding:Readonly<DeviceBinding>;readonly operationId:string;readonly revision:number
  readonly signedBytesB64:string;readonly fileBytesB64:string;readonly sha256:string
  readonly phase:'prepared'|'pending'|'committed'|'unknown'
}
export const pinDigest=(raw:Uint8Array)=>createHash('sha256').update(raw).digest('hex')
export const pinKey=(namespace:string,computerId:string)=>pinDigest(Buffer.from(JSON.stringify(['personal-linux-v1',namespace,computerId])))
export const pinOperation=()=>randomBytes(16).toString('hex')
const magic=Buffer.from('DNPLPIN1','ascii')
export function sealPin(pin:HistoricalPinEvidence,cipher:PinCipher):Buffer {
  const bytes=Buffer.from(pin.signedBytes);personalPinCanonical(bytes)
  cipher.current();const sealed=cipher.encryptString(bytes.toString('utf8'));cipher.current()
  personalNeed(sealed.length>0 && sealed.length+12<=262144)
  const header=Buffer.alloc(12);magic.copy(header);header.writeUInt32BE(sealed.length,8)
  return Buffer.concat([header,sealed])
}
export function openPin(raw:Uint8Array,cipher:PinCipher):Uint8Array {
  const bytes=Buffer.from(raw);personalNeed(bytes.length>12 && bytes.length<=262144 && bytes.subarray(0,8).equals(magic) && bytes.readUInt32BE(8)===bytes.length-12)
  cipher.current();const text=cipher.decryptString(bytes.subarray(12));cipher.current()
  const signed=Buffer.from(text,'utf8');personalPinCanonical(signed);return signed
}
function base64(value:unknown,limit:number):Buffer {
  personalNeed(typeof value==='string' && value.length>0 && value.length<=4*Math.ceil(limit/3))
  const raw=Buffer.from(value,'base64');personalNeed(raw.length<=limit && raw.toString('base64')===value);return raw
}
export function validatePinRecord(value:unknown,binding:Readonly<DeviceBinding>):PinPublicationRecord {
  personalNeed(value && typeof value==='object' && !Array.isArray(value))
  const r=value as PinPublicationRecord
  personalNeed(Object.keys(r).sort().join(',')==='binding,fileBytesB64,operationId,phase,revision,sha256,signedBytesB64' && personalSame(r.binding,binding) && /^[a-f0-9]{32}$/.test(r.operationId) && Number.isSafeInteger(r.revision) && r.revision>=0 && ['prepared','pending','committed','unknown'].includes(r.phase))
  const signed=base64(r.signedBytesB64,192*1024),file=base64(r.fileBytesB64,262144);personalPinCanonical(signed)
  personalNeed(/^[a-f0-9]{64}$/.test(r.sha256) && pinDigest(file)===r.sha256 && file.length>12 && file.subarray(0,8).equals(magic) && file.readUInt32BE(8)===file.length-12)
  return personalImmutable(JSON.parse(JSON.stringify(r)) as PinPublicationRecord)
}
export function sealPinRecord(record:PinPublicationRecord,cipher:PinCipher):Buffer {
  validatePinRecord(record,record.binding);const text=JSON.stringify(record);personalNeed(Buffer.byteLength(text)<=768*1024)
  cipher.current();const sealed=cipher.encryptString(text);cipher.current();personalNeed(sealed.length>0 && sealed.length<=832*1024);return sealed
}
export function openPinRecord(raw:Uint8Array,binding:Readonly<DeviceBinding>,cipher:PinCipher):PinPublicationRecord {
  personalNeed(raw.length>0 && raw.length<=832*1024);cipher.current();const text=cipher.decryptString(Buffer.from(raw));cipher.current()
  const value=personalPinCanonical(Buffer.from(text),768*1024);return validatePinRecord(value,binding)
}
export function samePinOperation(a:PinPublicationRecord,b:PinPublicationRecord){
  return personalSame(a.binding,b.binding) && a.operationId===b.operationId && a.signedBytesB64===b.signedBytesB64 && a.fileBytesB64===b.fileBytesB64 && a.sha256===b.sha256
}
