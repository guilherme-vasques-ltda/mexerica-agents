/** Descritores privados do runtime; nenhum caminho provém de IPC. */
import * as nodeFs from 'node:fs'
import type { BigIntStats } from 'node:fs'
type RawRuntimeFs=Pick<typeof nodeFs,'openSync'|'closeSync'|'fstatSync'|'statSync'|'lstatSync'|'readSync'|'statfsSync'|'readlinkSync'|'realpathSync'> & {readonly constants:Pick<typeof nodeFs.constants,'O_RDONLY'|'O_NOFOLLOW'|'O_DIRECTORY'>}
const nativeGetBuiltinModule=process.getBuiltinModule
function isRawFs(value:unknown):value is RawRuntimeFs {
  if(!value || typeof value!=='object' || !('constants' in value))return false
  const c=value.constants
  return !!c && typeof c==='object' && 'O_RDONLY' in c && typeof c.O_RDONLY==='number' && Number.isInteger(c.O_RDONLY) && 'O_NOFOLLOW' in c && typeof c.O_NOFOLLOW==='number' && Number.isInteger(c.O_NOFOLLOW) && 'O_DIRECTORY' in c && typeof c.O_DIRECTORY==='number' && Number.isInteger(c.O_DIRECTORY)
    && 'openSync' in value && typeof value.openSync==='function' && 'closeSync' in value && typeof value.closeSync==='function'
    && 'fstatSync' in value && typeof value.fstatSync==='function' && 'statSync' in value && typeof value.statSync==='function' && 'lstatSync' in value && typeof value.lstatSync==='function'
    && 'readSync' in value && typeof value.readSync==='function' && 'statfsSync' in value && typeof value.statfsSync==='function' && 'readlinkSync' in value && typeof value.readlinkSync==='function' && 'realpathSync' in value && typeof value.realpathSync==='function'
}
// Captura única antes do IPC. Electron nunca recorre ao FS virtual se o builtin falhar.
const capturedRawFs=(()=>{
  try{
    const raw:unknown=Object.hasOwn(process.versions,'electron') ? (typeof nativeGetBuiltinModule==='function' ? nativeGetBuiltinModule.call(process,'original-fs') : undefined) : nodeFs
    if(!isRawFs(raw))return null
    return Object.freeze({constants:Object.freeze({O_RDONLY:raw.constants.O_RDONLY,O_NOFOLLOW:raw.constants.O_NOFOLLOW,O_DIRECTORY:raw.constants.O_DIRECTORY}),openSync:raw.openSync,closeSync:raw.closeSync,fstatSync:raw.fstatSync,statSync:raw.statSync,lstatSync:raw.lstatSync,readSync:raw.readSync,statfsSync:raw.statfsSync,readlinkSync:raw.readlinkSync,realpathSync:raw.realpathSync})
  }catch{return null}
})()
// Ausência só recusa o uso pessoal; importar o serviço não bloqueia SSH legado.
export function runtimeRawFs():RawRuntimeFs {runtimeNeed(capturedRawFs);return capturedRawFs}
import type { RuntimeReader } from './personal-runtime-format.mjs'
export function runtimeNeed(value:unknown):asserts value {if(!value)throw new Error('personalRuntimeUnavailable')}
export function fileIdentity(s:BigIntStats):string{return [s.dev,s.ino,s.size,s.mode,s.uid,s.nlink,s.mtimeNs,s.ctimeNs].join(':')}
export class RuntimeFile implements RuntimeReader {
  readonly size:number
  readonly path:string
  private readonly fs=runtimeRawFs()
  private readonly fd:number
  private readonly identity:string
  private closed=false
  constructor(path:string,fd:number){
    this.path=path;this.fd=fd
    try{const s=this.fs.fstatSync(fd,{bigint:true});runtimeNeed(s.isFile() && s.nlink===1n && (s.uid===0n || s.uid===BigInt(process.getuid!())) && !(s.mode&0o022n) && s.size>0n && s.size<=1073741824n);this.size=Number(s.size);this.identity=fileIdentity(s);this.check()}catch(e){this.closed=true;this.fs.closeSync(fd);throw e}
  }
  static open(path:string){const fs=runtimeRawFs();return new RuntimeFile(path,fs.openSync(path,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW))}
  check(){runtimeNeed(!this.closed && fileIdentity(this.fs.fstatSync(this.fd,{bigint:true}))===this.identity && fileIdentity(this.fs.statSync(this.path,{bigint:true}))===this.identity && !this.fs.lstatSync(this.path).isSymbolicLink())}
  read(at:number,length:number):Buffer{this.check();const b=Buffer.alloc(length);runtimeNeed(this.fs.readSync(this.fd,b,0,length,at)===length);this.check();return b}
  bytes(limit:number):Buffer{runtimeNeed(this.size<=limit);return this.read(0,this.size)}
  close(){if(!this.closed){this.closed=true;this.fs.closeSync(this.fd)}}
}
