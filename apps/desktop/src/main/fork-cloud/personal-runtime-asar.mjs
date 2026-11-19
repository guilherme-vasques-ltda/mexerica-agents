// Código próprio SIM37: somente o segundo ASAR oficial e o plist XML fechado.
import {createHash} from 'node:crypto'
const hash=bytes=>createHash('sha256').update(bytes).digest('hex')
const DEFAULT_SIZE=110862
const DEFAULT_HASH='0eb2491b0a9ac94790389d39c09dd5005c6c1f0665842943829bbd0193f6cc4f'
const HEADER_HASH='f80143a17fad2075fe6cf176e07033a41a878dba7f31e7ab4f6f14fcc34162dc'
const KEYS=Object.freeze(['Resources/app.asar','Resources/default_app.asar'])
const ZIPS=Object.freeze({arm64:'1d75703019bb16461ae65f3081d7e6f5c0b11e901d0ccb5c343bcf7bcdd6435c',x64:'e567d13833d0e161d7749727355b98643461df3395b537cfa7bdddf8a8bfedff'})
function need(ok){if(!ok)throw new Error('personalRuntimeInvalid')}
function defaultAsar(reader,p){
  need(Array.isArray(p) && p.length===12 && p[0]==='donna/personal-linux/runtime-policy/v1' && p[1]===1 && ['arm64','x64'].includes(p[3]) && p[8]===`electron-v44.5.1-darwin-${p[3]}.zip` && Array.isArray(p[11]) && p[11].length===4 && p[11][3]==='original-zero')
  need(p[2]==='darwin' && p[4]==='44.5.1' && p[9]===ZIPS[p[3]] && reader.size===DEFAULT_SIZE)
  const raw=reader.read(0,DEFAULT_SIZE)
  need(raw.length===DEFAULT_SIZE && hash(raw)===DEFAULT_HASH && hash(raw.subarray(16,16+3316))===HEADER_HASH)
}
function integrity(value){
  need(value && typeof value==='object' && !Array.isArray(value) && Object.keys(value).length===2 && KEYS.every(k=>Object.hasOwn(value,k)))
  const entries={}
  for(const key of KEYS){const e=value[key];need(e && typeof e==='object' && !Array.isArray(e) && Object.keys(e).sort().join(',')==='algorithm,hash' && e.algorithm==='SHA256' && typeof e.hash==='string' && /^[a-f0-9]{64}$/.test(e.hash));entries[key]=Object.freeze({algorithm:'SHA256',hash:e.hash})}
  need(entries[KEYS[1]].hash===HEADER_HASH);return Object.freeze(entries)
}
function darwinIntegrity(raw){
  need(raw.length>0 && raw.length<=65536)
  // Não resolve DTD/entidades. Só este prólogo literal da pipeline efetiva é admitido.
  const text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(raw)
  const prolog='<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
  need(text.startsWith(prolog));let at=prolog.length,nodes=0
  const space=()=>{while(at<text.length && /[ \t\r\n]/.test(text[at]))at++}
  const take=tag=>{space();need(text.startsWith(tag,at));at+=tag.length}
  const leaf=tag=>{take('<'+tag+'>');const end=text.indexOf('</'+tag+'>',at);need(end>=at && end-at<=16384);const value=text.slice(at,end);need(!/[<&\u0000-\u001f\u007f]/.test(value));at=end+tag.length+3;return value}
  const value=depth=>{
    need(depth<=16 && ++nodes<=1024);space()
    if(text.startsWith('<dict>',at)){
      take('<dict>');const result=Object.create(null),seen=new Set()
      for(;;){space();if(text.startsWith('</dict>',at)){take('</dict>');return result}const key=leaf('key');need(/^[A-Za-z0-9_.:/-]{1,256}$/.test(key) && !seen.has(key) && seen.size<128);seen.add(key);result[key]=value(depth+1)}
    }
    if(text.startsWith('<array>',at)){take('<array>');const result=[];for(;;){space();if(text.startsWith('</array>',at)){take('</array>');return result}need(result.length<1024);result.push(value(depth+1))}}
    if(text.startsWith('<string/>',at)){take('<string/>');return ''}
    if(text.startsWith('<string>',at))return leaf('string')
    if(text.startsWith('<true/>',at)){take('<true/>');return true}
    if(text.startsWith('<false/>',at)){take('<false/>');return false}
    need(false)
  }
  take('<plist version="1.0">');const root=value(0);take('</plist>');space();need(at===text.length && root && typeof root==='object' && !Array.isArray(root) && Object.hasOwn(root,'ElectronAsarIntegrity'))
  return integrity(root.ElectronAsarIntegrity)
}
function darwinDigest(rawIntegrity,effective){
  const a=integrity(rawIntegrity),b=integrity(effective)
  need(KEYS.every(k=>a[k].algorithm===b[k].algorithm && a[k].hash===b[k].hash))
  return hash(Buffer.from(KEYS.map(k=>k+a[k].algorithm+a[k].hash).join(''),'utf8'))
}
export {defaultAsar,darwinIntegrity,darwinDigest}
export default Object.freeze({defaultAsar,darwinIntegrity,darwinDigest})
