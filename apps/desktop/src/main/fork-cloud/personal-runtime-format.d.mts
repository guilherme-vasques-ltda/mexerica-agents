import asar from './personal-runtime-asar.mjs'
export interface RuntimeReader { readonly size:number; read(offset:number,length:number):Buffer }
export type RuntimePolicy = readonly [string,1,'darwin'|'linux','x64'|'arm64',string,string,string,string,string,string,readonly (readonly [string,string])[],readonly [string,string,string,string]]
export type RuntimeReceipt = readonly [string,1,'darwin'|'linux','x64'|'arm64',string,string,string,string,string,string,string,string,readonly (readonly unknown[])[],string]
export function canonical(bytes:Uint8Array,limit:number):unknown
export function policy(value:unknown):RuntimePolicy
export function receipt(value:unknown,policy:RuntimePolicy):RuntimeReceipt
export function image(reader:RuntimeReader,platform:string,arch:string,role:string,mode:string,expected?:readonly unknown[]|null,asarDigest?:string|null):readonly unknown[]
export function hash(bytes:Uint8Array):string
export function assetSum(bytes:Buffer,name:string,expected:string):void

export {asar}
declare const format: {asar:typeof asar;canonical:typeof canonical;policy:typeof policy;receipt:typeof receipt;image:typeof image;hash:typeof hash;assetSum:typeof assetSum}
export default format
