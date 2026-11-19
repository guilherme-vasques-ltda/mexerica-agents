/** Main privado (contrato B, docs/reference/hub-linux-pin-native-load.md). Leitor PURO da assinatura embutida de um Mach-O e camada
 * pré-dlopen SÓ-RECUSA. Não autentica sozinho (a autenticação primária é do sistema no carregamento: Hardened Runtime + Library Validation +
 * library constraint por cdhash), não lê CMS/certificado, não admite imagem e não é usado pela factory enquanto ela estiver fechada. */
import { createHash } from 'node:crypto'

export type PinImageArch='arm64'|'x64'
export interface EmbeddedSignature{cdhash:string,codeDirectorySha256:string,hashType:number,flags:number,teamId:string|null,identifier:string,pageSize:number,codeLimit:number}
export interface PinImageExpectation{arch:PinImageArch,teamId:string,cdhash:string,wholeSha256:string}

const CPU:Record<PinImageArch,number>={arm64:0x0100000c,x64:0x01000007}
const CS_ADHOC=0x2,CS_RUNTIME=0x10000,LC_CODE_SIGNATURE=0x1d,SUPERBLOB=0xfade0cc0,CODEDIRECTORY=0xfade0c02,SHA256=2
function need(ok:unknown):asserts ok{if(!ok)throw new Error('personalPinSignatureInvalid')}
const sha=(b:Uint8Array)=>createHash('sha256').update(b).digest()
const cstr=(b:Buffer,at:number)=>{need(at>0 && at<b.length);const end=b.indexOf(0,at);need(end>at);return b.subarray(at,end).toString('latin1')}

/** Lê o CodeDirectory SHA-256 do Mach-O FINO da arch exata (fat/outra arch ⇒ erro; sem fallback) e confere TODAS as páginas de código. */
export function readEmbeddedSignature(image:Buffer,arch:PinImageArch):EmbeddedSignature&{pagesOk:boolean}{
  need(image.length>=32 && image.readUInt32LE(0)===0xfeedfacf)
  if(image.readUInt32LE(4)!==CPU[arch])throw new Error('personalPinSignatureArch')
  const ncmds=image.readUInt32LE(16),sizeofcmds=image.readUInt32LE(20);need(32+sizeofcmds<=image.length && ncmds<=4096)
  let at=32,sig:{off:number,size:number}|null=null
  for(let i=0;i<ncmds;i++){need(at+8<=32+sizeofcmds);const cmd=image.readUInt32LE(at),size=image.readUInt32LE(at+4);need(size>=8 && at+size<=32+sizeofcmds)
    if(cmd===LC_CODE_SIGNATURE){need(sig===null && size>=16);sig={off:image.readUInt32LE(at+8),size:image.readUInt32LE(at+12)}}
    at+=size}
  if(!sig)throw new Error('personalPinSignatureUnsigned')
  need(sig.off+sig.size===image.length && sig.size>=12)
  const sb=image.subarray(sig.off);need(sb.readUInt32BE(0)===SUPERBLOB && sb.readUInt32BE(4)<=sb.length);const count=sb.readUInt32BE(8);need(count>0 && count<=64 && 12+8*count<=sb.length)
  const cds:Buffer[]=[]
  for(let i=0;i<count;i++){const type=sb.readUInt32BE(12+8*i),off=sb.readUInt32BE(16+8*i)
    if(type===0||(type>=0x1000 && type<0x1005)){need(off+8<=sb.length && sb.readUInt32BE(off)===CODEDIRECTORY);const len=sb.readUInt32BE(off+4);need(off+len<=sb.length);cds.push(sb.subarray(off,off+len))}}
  const strong=cds.filter(cd=>cd.length>=56 && cd[37]===SHA256);need(strong.length===1);const cd=strong[0]
  const version=cd.readUInt32BE(8),flags=cd.readUInt32BE(12),hashOffset=cd.readUInt32BE(16),identOffset=cd.readUInt32BE(20),nCode=cd.readUInt32BE(28),codeLimit=cd.readUInt32BE(32),hashSize=cd[36],pageLog=cd[39]
  need(hashSize===32 && pageLog>=12 && pageLog<=16 && codeLimit===sig.off && hashOffset+nCode*32<=cd.length)
  const page=2**pageLog;need(nCode===Math.ceil(codeLimit/page))
  let pagesOk=true
  for(let i=0;i<nCode;i++)if(!sha(image.subarray(i*page,Math.min((i+1)*page,codeLimit))).equals(cd.subarray(hashOffset+i*32,hashOffset+(i+1)*32)))pagesOk=false
  const teamOffset=version>=0x20200?cd.readUInt32BE(52):0,full=sha(cd)
  return{cdhash:full.subarray(0,20).toString('hex'),codeDirectorySha256:full.toString('hex'),hashType:SHA256,flags,teamId:teamOffset?cstr(cd,teamOffset):null,identifier:cstr(cd,identOffset),pageSize:page,codeLimit,pagesOk}
}

/** Camada EXTRA antes do dlopen, sobre os bytes do FD retido: devolve motivos de recusa. Lista vazia NÃO admite nada (só diz que esta camada
 * não recusou); a admissão também exige a validação do sistema no carregamento e as provas pendentes do contrato. */
export function preloadRefusals(image:Buffer,want:PinImageExpectation):string[]{
  let s
  try{s=readEmbeddedSignature(image,want.arch)}catch(e){const m=(e as Error).message;return[m==='personalPinSignatureUnsigned'?'unsigned':m==='personalPinSignatureArch'?'arch':'malformed']}
  const r:string[]=[]
  if(!s.pagesOk)r.push('page-hash')
  if(s.flags&CS_ADHOC)r.push('adhoc')
  if(!(s.flags&CS_RUNTIME))r.push('no-hardened-runtime')
  if(s.teamId===null || s.teamId!==want.teamId)r.push('team-id')
  if(s.cdhash!==want.cdhash)r.push('cdhash')
  if(sha(image).toString('hex')!==want.wholeSha256)r.push('whole-image')
  return r
}
