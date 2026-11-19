// Código próprio: formatos fechados SIM30/31. Compartilhado por beforePack/Main.
import { createHash } from 'node:crypto'
import asar from './personal-runtime-asar.mjs'
const POLICY = 'donna/personal-linux/runtime-policy/v1'
const RECEIPT = 'donna/personal-linux/runtime-receipt/v1'
const MAX = 1024 * 1024 * 1024
function need(ok) { if (!ok) throw new Error('personalRuntimeInvalid') }
function integer(n, max = Number.MAX_SAFE_INTEGER) { need(Number.isSafeInteger(n) && n >= 0 && n <= max); return n }
function hash(bytes) { return createHash('sha256').update(bytes).digest('hex') }
function canonical(bytes, limit) {
  need(bytes.length > 0 && bytes.length <= limit)
  const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes), v = JSON.parse(text)
  need(JSON.stringify(v) === text); return v
}
function checkedAdd(a,b) { return integer(integer(a) + integer(b)) }
function align(n, q) { return integer(Math.ceil(integer(n) / q) * q) }
function digest(v) { need(typeof v === 'string' && /^[a-f0-9]{64}$/.test(v)); return v }
function assetSum(bytes,name,expected){
  need(bytes.length<=65536 && bytes.every(v=>v<128));const rows=bytes.toString('ascii').split('\n').map(v=>/^([a-f0-9]{64}) [ *](\S+)$/.exec(v)).filter(v=>v && v[2]===name);need(rows.length===1 && rows[0][1]===expected)
}
function policy(p) {
  need(Array.isArray(p) && p.length === 12 && p[0] === POLICY && p[1] === 1 && ['darwin','linux'].includes(p[2]) && ['x64','arm64'].includes(p[3]) && p[4] === '44.5.1')
  for (const i of [5,6]) need(typeof p[i] === 'string' && /^[a-f0-9]{40}$/.test(p[i]))
  digest(p[7]); digest(p[9]); need(p[8] === `electron-v44.5.1-${p[2]}-${p[3]}.zip`)
  need(Array.isArray(p[10]) && p[10].length > 0 && p[10].length <= 128)
  let previous = ''
  for (const row of p[10]) {
    need(Array.isArray(row) && row.length === 2 && typeof row[0] === 'string' && /^https:\/\/raw\.githubusercontent\.com\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/[a-f0-9]{40}\//.test(row[0]) && row[0] > previous)
    const url=new URL(row[0]);need(url.origin==='https://raw.githubusercontent.com' && !url.search && !url.hash && !url.username && !url.password && row[0]===url.href);digest(row[1]); previous = row[0]
  }
  const b=p[11]; need(Array.isArray(b) && b.length===4 && b[0]==='26.15.3'); digest(b[1]); digest(b[2]); need(p[2]==='linux'? b[3]==='elf-identity':['original-zero','embedded-v1'].includes(b[3])); return p
}
function receipt(r,p) {
  policy(p)
  need(Array.isArray(r) && r.length === 14 && r[0] === RECEIPT && r[1] === 1 && r[2]===p[2] && r[3]===p[3] && r[4]===p[4] && r[5]==='24.21.0' && r[6]==='1.52.1' && r[7]===p[5] && r[8]===p[6] && r[9]===hash(Buffer.from(JSON.stringify(p[10]))) && r[10]===p[8] && r[11]===p[9] && r[13]===hash(Buffer.from(JSON.stringify(p))))
  need(Array.isArray(r[12]) && r[12].length === (p[2]==='linux'?1:2))
  r[12].forEach((v,i)=> {
    need(Array.isArray(v) && v[0] === (i===0?'executable':'framework'))
    if(p[2]==='linux'){need(v.length===6 && v[1]===64 && v[2]==='le' && v[3]===(p[3]==='x64'?62:183)); integer(v[4],MAX); need(v[4]>0);digest(v[5])}
    else {need(v.length===(p[3]==='x64'?12:8)); if(p[3]==='x64')need(v[1]==='unsigned-x64-v1'); for(let j=p[3]==='x64'?2:1;j<v.length-1;j++){if(j===v.length-2 && i===0)need(v[j]===null);else integer(v[j])} digest(v[v.length-1])}
  }); return r
}
function read(r,at,length){integer(at);integer(length,MAX);need(checkedAdd(at,length)<=r.size);const b=r.read(at,length);need(Buffer.isBuffer(b) && b.length===length);return b}
function whole(r,end,edits=[]) {
  need(end>0 && end<=r.size);const h=createHash('sha256');const ranges=edits.slice().sort((a,b)=>a[0]-b[0]);let previous=0
  for(const [at,b] of ranges){need(at>=previous && at+b.length<=end);previous=at+b.length}
  for(let at=0;at<end;at+=65536){const b=Buffer.from(read(r,at,Math.min(65536,end-at)));for(const [pos,value] of ranges){const lo=Math.max(at,pos),hi=Math.min(at+b.length,pos+value.length);if(lo<hi)value.copy(b,lo-at,lo-pos,hi-pos)}h.update(b)}return h.digest('hex')
}
function u64(b,at){const v=b.readBigUInt64LE(at);need(v<=BigInt(Number.MAX_SAFE_INTEGER));return Number(v)}
function fixed(b,at,n){const s=b.subarray(at,at+n);const zero=s.indexOf(0);need(s.every(x=>x<128) && (zero===-1 || s.subarray(zero).every(x=>x===0)));return s.subarray(0,zero===-1?n:zero).toString('ascii')}
function word(n,wide=false){const b=Buffer.alloc(wide?8:4);if(wide)b.writeBigUInt64LE(BigInt(integer(n)));else b.writeUInt32LE(integer(n,0xffffffff));return b}
function elf(r,arch) {
  integer(r.size,MAX);need(r.size>=64);const b=read(r,0,64),machine=arch==='x64'?62:183
  need(b.subarray(0,4).equals(Buffer.from([127,69,76,70])) && b[4]===2 && b[5]===1 && b[6]===1 && b.readUInt32LE(20)===1 && b.readUInt16LE(18)===machine && [2,3].includes(b.readUInt16LE(16)) && b.readUInt16LE(52)===64)
  const at=u64(b,32),size=b.readUInt16LE(54),count=b.readUInt16LE(56);need(size===56 && count>0 && count<=1024 && checkedAdd(at,size*count)<=r.size)
  for(let i=0;i<count;i++){const p=read(r,at+i*size,size);need(checkedAdd(u64(p,8),u64(p,32))<=r.size)}
  return ['executable',64,'le',machine,r.size,whole(r,r.size)]
}
function macho(r,arch,role) {
  integer(r.size,MAX);const h=read(r,0,32);need(h.readUInt32LE(0)===0xfeedfacf && h.readUInt32LE(4)===(arch==='x64'?16777223:16777228) && h.readUInt32LE(12)===(role==='executable'?2:6))
  const n=h.readUInt32LE(16),length=h.readUInt32LE(20);need(n>0 && n<=4096 && length>=8*n && length<=1024*1024);const commands=read(r,32,length),segments=[],sections=[],signatures=[];let at=0,asar=null
  for(let i=0;i<n;i++){
    need(at+8<=length);const cmd=commands.readUInt32LE(at),size=commands.readUInt32LE(at+4);need(size>=8 && size%8===0 && at+size<=length)
    if(cmd===0x19){need(size>=72);const count=commands.readUInt32LE(at+64);need(size===72+count*80);const seg={name:fixed(commands,at+8,16),at:32+at,vm:u64(commands,at+32),offset:u64(commands,at+40),size:u64(commands,at+48),prot:commands.readUInt32LE(at+60),count};need(checkedAdd(seg.offset,seg.size)<=r.size);segments.push(seg)
      for(let j=0;j<count;j++){const s=at+72+j*80,name=fixed(commands,s,16),parent=fixed(commands,s+16,16),len=u64(commands,s+40),offset=commands.readUInt32LE(s+48),type=commands.readUInt32LE(s+64)&255;need(parent===seg.name)
        if(![1,12,18].includes(type) && len>0){need(offset>=32+length && offset>=seg.offset && checkedAdd(offset,len)<=seg.offset+seg.size);sections.push({offset,len});if(parent==='__DATA_CONST' && name==='__asar_integrity'){need(asar===null && len===66);asar=offset+32}}
      }
    }else if(cmd===0x1d){need(size===16);signatures.push({at:32+at,dataoff:commands.readUInt32LE(at+8),size:commands.readUInt32LE(at+12),last:i===n-1})}
    at+=size
  }
  need(at===length && signatures.length<=1);const links=segments.filter(s=>s.name==='__LINKEDIT');need(links.length===1);const link=links[0];need(link.count===0 && !(link.prot&4) && link.offset+link.size===r.size)
  for(let i=0;i<segments.length;i++)for(let j=i+1;j<segments.length;j++){const a=segments[i],b=segments[j];if(a.size && b.size)need(a.offset+a.size<=b.offset || b.offset+b.size<=a.offset)}
  need(role==='framework'?asar!==null:asar===null)
  if(asar!==null){need(read(r,asar-32,32).equals(Buffer.from('AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A')))}
  return {cpu:h.readUInt32LE(4),sub:h.readUInt32LE(8),n,length,link,sections,signature:signatures[0],asar}
}
function asarEdit(r,m,mode,original,asarDigest) {
  if(m.asar===null)return []
  const b=read(r,m.asar,34)
  if(original || mode==='original-zero')need(b.every(v=>v===0))
  else {need(mode==='embedded-v1' && b[0]===1 && b[1]===1 && digest(asarDigest)===b.subarray(2).toString('hex'))}
  return [[m.asar,Buffer.alloc(34)]]
}
function image(r,platform,arch,role,mode,expected=null,asarDigest=null) {
  need(['x64','arm64'].includes(arch) && ['executable','framework'].includes(role))
  if(platform==='linux'){need(role==='executable');const item=elf(r,arch);if(expected)need(JSON.stringify(item)===JSON.stringify(expected));return item}
  need(platform==='darwin');const m=macho(r,arch,role),edits=asarEdit(r,m,mode,!expected,asarDigest)
  if(arch==='arm64'){
    const s=m.signature;need(s && s.dataoff%16===0 && s.size>0 && s.size<=16*1024*1024 && s.dataoff+s.size===r.size && s.dataoff>=m.link.offset && s.dataoff>=32+m.length && m.link.vm===align(m.link.size,16384));for(const section of m.sections)need(section.offset+section.len<=s.dataoff)
    edits.push([s.at+12,Buffer.alloc(4)],[m.link.at+32,Buffer.alloc(8)],[m.link.at+48,Buffer.alloc(8)])
    const item=[role,m.cpu,m.sub,s.dataoff,s.at,m.link.at,m.asar,whole(r,s.dataoff,edits)];if(expected)need(JSON.stringify(item)===JSON.stringify(expected));return item
  }
  if(!expected){
    const fixedOrigin=role==='executable'?[13076,2147483651,15,1120,648,4096,788]:[218108196,3,82,10016,3280,2150400,2146596]
    need(!m.signature && JSON.stringify([r.size,m.sub,m.n,m.length,m.link.at,m.link.vm,m.link.size])===JSON.stringify(fixedOrigin) && read(r,32+m.length,16).every(x=>x===0));for(const section of m.sections)need(section.offset>=32+m.length+16)
    return [role,'unsigned-x64-v1',m.cpu,m.sub,r.size,m.n,m.length,m.link.at,m.link.vm,m.link.size,m.asar,whole(r,r.size,edits)]
  }
  need(expected.length===12 && expected[1]==='unsigned-x64-v1');const [,,cpu,sub,originalSize,n,length,linkAt,vm,linkSize,slot]=expected,s=m.signature
  need(m.cpu===cpu && m.sub===sub && m.n===n+1 && m.length===length+16 && m.link.at===linkAt && m.asar===slot && s && s.last && s.at===32+length && s.dataoff===align(originalSize,16) && s.size>0 && s.size<=16*1024*1024 && s.dataoff+s.size===r.size && m.link.offset+linkSize===originalSize && m.link.size===r.size-m.link.offset && (m.link.vm===Math.max(vm,align(m.link.size,4096)) || m.link.vm===Math.max(vm,align(m.link.size,16384))) && read(r,originalSize,s.dataoff-originalSize).every(x=>x===0))
  for(const section of m.sections)need(section.offset+section.len<=originalSize && section.offset>=32+length+16)
  edits.push([16,word(n)],[20,word(length)],[s.at,Buffer.alloc(16)],[m.link.at+32,word(vm,true)],[m.link.at+48,word(linkSize,true)])
  need(whole(r,originalSize,edits)===expected[11]);return expected
}
export {canonical,policy,receipt,image,hash,assetSum,asar}
export default Object.freeze({canonical,policy,receipt,image,hash,assetSum,asar})
