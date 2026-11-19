import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as path from 'node:path'
import * as os from 'node:os'
import {createHash} from 'node:crypto'
import format from './personal-runtime-format.mjs'
import {RuntimeFile} from './personal-runtime-files.ts'
const appHash='a'.repeat(64),defaultHash='f80143a17fad2075fe6cf176e07033a41a878dba7f31e7ab4f6f14fcc34162dc'
const prolog='<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
const entry=(key:string,hash:string)=>`<key>${key}</key><dict><key>algorithm</key><string>SHA256</string><key>hash</key><string>${hash}</string></dict>`
const pair=entry('Resources/app.asar',appHash)+entry('Resources/default_app.asar',defaultHash)
const property=(items=pair)=>`<key>ElectronAsarIntegrity</key><dict>${items}</dict>`
const xml=(fields=property())=>Buffer.from(`${prolog}\n<plist version="1.0"><dict><key>CFBundleName</key><string>Fixture</string>${fields}<key>Items</key><array><string/><true/><false/></array></dict></plist>`)
const effective=()=>({'Resources/app.asar':{algorithm:'SHA256',hash:appHash},'Resources/default_app.asar':{algorithm:'SHA256',hash:defaultHash}})
const policies=JSON.parse(fs.readFileSync(new URL('../../../resources/personal-runtime-policy-v1.json',import.meta.url),'utf8'))

test('PL1 extraASAR: XML bruto fechado e JSON efetivo conferem ambas entradas na ordem UTF16',()=>{
  // Plist fictício próprio; isto não prova pacote assinado nem wholeASAR oficial.
  const parsed=format.asar.darwinIntegrity(xml()),expected=effective()
  assert.deepEqual(parsed,expected)
  const digest=createHash('sha256').update('Resources/app.asarSHA256'+appHash+'Resources/default_app.asarSHA256'+defaultHash).digest('hex')
  assert.equal(format.asar.darwinDigest(parsed,expected),digest)
  assert.equal(format.asar.darwinDigest(parsed,Object.fromEntries(Object.entries(expected).reverse())),digest)
  assert.notEqual(digest,createHash('sha256').update('Resources/app.asarSHA256'+appHash).digest('hex'))
  assert.throws(()=>format.asar.darwinDigest(parsed,{...expected,'Resources/app.asar':{algorithm:'SHA256',hash:'b'.repeat(64)}}))
})
test('PL1 extraASAR: duplicatas sintáticas raiz e nested recusam mesmo se plutil colapsaria valores',()=>{
  assert.throws(()=>format.asar.darwinIntegrity(xml(property()+property())))
  assert.throws(()=>format.asar.darwinIntegrity(xml(property(pair.replace('<key>algorithm</key><string>SHA256</string>','<key>algorithm</key><string>wrong</string><key>algorithm</key><string>SHA256</string>')))))
  assert.throws(()=>format.asar.darwinIntegrity(xml(property(pair.replace('<key>hash</key>',`<key>hash</key><string>${'b'.repeat(64)}</string><key>hash</key>`)))))
  assert.throws(()=>format.asar.darwinIntegrity(Buffer.from(xml().toString().replace('<key>CFBundleName</key>','<key>CFBundleName</key><string>ignored</string><key>CFBundleName</key>'))))
})
test('PL1 extraASAR: ausência terceira chave alias entidade algoritmo e hash extras recusam',()=>{
  assert.throws(()=>format.asar.darwinIntegrity(xml(property(entry('Resources/app.asar',appHash)))))
  assert.throws(()=>format.asar.darwinIntegrity(xml(property(pair+entry('Resources/third.asar',appHash)))))
  assert.throws(()=>format.asar.darwinIntegrity(xml(property(pair.replace('Resources/default_app.asar','Resources/./default_app.asar')))))
  assert.throws(()=>format.asar.darwinIntegrity(Buffer.from(xml().toString().replace('ElectronAsarIntegrity','ElectronAsarIntegr&#x69;ty'))))
  assert.throws(()=>format.asar.darwinIntegrity(xml(property(pair.replace('<string>SHA256</string>','<string>sha256</string>')))))
  assert.throws(()=>format.asar.darwinIntegrity(xml(property(pair.replace(defaultHash,'0'.repeat(64))))))
})
test('PL1 extraASAR: serialização XML limitada não resolve DTD entidades comentários ou binário',()=>{
  assert.throws(()=>format.asar.darwinIntegrity(Buffer.from('bplist00')))
  assert.throws(()=>format.asar.darwinIntegrity(Buffer.concat([Buffer.from([239,187,191]),xml()])))
  assert.throws(()=>format.asar.darwinIntegrity(Buffer.from(xml().toString().replace(prolog,prolog.replace('PropertyList-1.0.dtd">','PropertyList-1.0.dtd" [<!ENTITY alias "ElectronAsarIntegrity">]>')))))
  assert.throws(()=>format.asar.darwinIntegrity(Buffer.from(xml().toString().replace('<string>Fixture</string>','<string>&amp;</string>'))))
  assert.throws(()=>format.asar.darwinIntegrity(Buffer.from(xml().toString().replace('<dict>','<dict><!-- ambiguous -->'))))
  assert.throws(()=>format.asar.darwinIntegrity(xml(property()+'<key>Deep</key>'+'<array>'.repeat(17)+'<true/>'+'</array>'.repeat(17))))
  assert.throws(()=>format.asar.darwinIntegrity(Buffer.alloc(65537,32)))
  assert.throws(()=>format.asar.darwinIntegrity(Buffer.concat([xml(),Buffer.from('<plist/>')])))
})
test('PL1 extraASAR: hash inteiro usa FS e crypto reais; tamanho correto não qualifica bytes arbitrários',()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'personal-extra-')),name=path.join(dir,'default_app.asar'),p=format.policy(policies.find((v:unknown[])=>v[2]==='darwin' && v[3]==='arm64'))
  let file:RuntimeFile|undefined
  try{
    fs.writeFileSync(name,Buffer.alloc(110862),{mode:0o600});file=RuntimeFile.open(name)
    assert.equal(file.size,110862);assert.throws(()=>format.asar.defaultAsar(file!,p),{message:'personalRuntimeInvalid'});file.close()
    const wrong=Buffer.alloc(110863);wrong[110862]=1;fs.writeFileSync(name,wrong);file=RuntimeFile.open(name);assert.throws(()=>format.asar.defaultAsar(file!,p),{message:'personalRuntimeInvalid'});file.close()
    fs.unlinkSync(name);assert.throws(()=>RuntimeFile.open(name))
    // Positivo wholeASAR original e mutação FORA do header cabem ao gate dos ZIPs
    // oficiais e pacote real do root. Nenhum asset baixado/code extra entra no teste.
  }finally{file?.close();fs.rmSync(dir,{recursive:true,force:true})}
})
test('PL1 extraASAR: FD raw retido recusa symlink hardlink modos e troca após espera',async()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'personal-extra-fd-')),name=path.join(dir,'default_app.asar'),other=path.join(dir,'other');let file:RuntimeFile|undefined
  try{
    fs.writeFileSync(name,Buffer.alloc(110862),{mode:0o600});fs.symlinkSync(name,other);assert.throws(()=>RuntimeFile.open(other));fs.unlinkSync(other)
    fs.linkSync(name,other);assert.throws(()=>RuntimeFile.open(name));fs.unlinkSync(other)
    fs.chmodSync(name,0o620);assert.throws(()=>RuntimeFile.open(name));fs.chmodSync(name,0o600)
    file=RuntimeFile.open(name);assert.doesNotThrow(()=>file!.check());await Promise.resolve();fs.renameSync(name,other);fs.writeFileSync(name,Buffer.alloc(110862),{mode:0o600});assert.throws(()=>file!.check())
    file.close();assert.throws(()=>file!.read(0,1))
  }finally{file?.close();fs.rmSync(dir,{recursive:true,force:true})}
})
