import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as path from 'node:path'
import * as os from 'node:os'
import { fileURLToPath } from 'node:url'
import format from './personal-runtime-format.mjs'
import * as runtimeFiles from './personal-runtime-files.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
const policies=JSON.parse(fs.readFileSync(new URL('../../../resources/personal-runtime-policy-v1.json',import.meta.url),'utf8'))
const reader=(bytes:Buffer)=>({size:bytes.length,read(at:number,n:number){return bytes.subarray(at,at+n)}})
function elf(){const b=Buffer.alloc(128);b.set([127,69,76,70,2,1,1]);b.writeUInt16LE(3,16);b.writeUInt16LE(62,18);b.writeUInt32LE(1,20);b.writeBigUInt64LE(64n,32);b.writeUInt16LE(64,52);b.writeUInt16LE(56,54);b.writeUInt16LE(1,56);b.writeBigUInt64LE(128n,96);return b}
function macho(){const b=Buffer.alloc(4128);b.writeUInt32LE(0xfeedfacf,0);b.writeUInt32LE(16777228,4);b.writeUInt32LE(2,12);b.writeUInt32LE(2,16);b.writeUInt32LE(88,20);b.writeUInt32LE(0x19,32);b.writeUInt32LE(72,36);b.write('__LINKEDIT',40);b.writeBigUInt64LE(16384n,64);b.writeBigUInt64LE(4096n,72);b.writeBigUInt64LE(32n,80);b.writeUInt32LE(1,92);b.writeUInt32LE(0x1d,104);b.writeUInt32LE(16,108);b.writeUInt32LE(4112,112);b.writeUInt32LE(16,116);return b}
test('PL1 M1: Policy12/Receipt14 reais têm seleção fechada; metadata não admite arquitetura/fonte extra',()=>{
  assert.equal(policies.length,4)
  for(const p of policies){assert.equal(format.policy(p),p);const changed=structuredClone(p);changed.push(null);assert.throws(()=>format.policy(changed));const wrong=structuredClone(p);wrong[3]='ia32';assert.throws(()=>format.policy(wrong));const source=structuredClone(p);source[10][0][0]+='?fallback=1';assert.throws(()=>format.policy(source))}
  assert.throws(()=>format.canonical(Buffer.from(' [1]'),65536));assert.throws(()=>format.canonical(Buffer.alloc(65537),65536))
})
test('PL1 M1: parser ELF hash inteiro e bounds; fixtures sintéticas não qualificam artefato',()=>{
  const b=elf(),image=format.image(reader(b),'linux','x64','executable','elf-identity');assert.equal(image[5],format.hash(b))
  const changed=Buffer.from(b);changed[127]=1;assert.throws(()=>format.image(reader(changed),'linux','x64','executable','elf-identity',image))
  const overflow=Buffer.from(b);overflow.writeBigUInt64LE(2n**63n,32);assert.throws(()=>format.image(reader(overflow),'linux','x64','executable','elf-identity'))
  assert.throws(()=>format.image(reader(b),'linux','arm64','executable','elf-identity'))
})
test('PL1 M1: Mach-O arm64 normaliza somente os campos permitidos, código/layout fora da máscara recusam',()=>{
  const original=macho(),expected=format.image(reader(original),'darwin','arm64','executable','original-zero');assert.equal(expected.length,8)
  const code=Buffer.from(original);code[1000]=1;assert.throws(()=>format.image(reader(code),'darwin','arm64','executable','original-zero',expected))
  const bad=Buffer.from(original);bad.writeUInt32LE(4100,112);assert.throws(()=>format.image(reader(bad),'darwin','arm64','executable','original-zero',expected))
  assert.throws(()=>format.image(reader(original),'darwin','x64','executable','original-zero'))
})
test('PL1 M1: RuntimeFile usa FS real e rejeita symlink/hardlink/substituição sem executar imagens',()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'personal-runtime-')),file=path.join(dir,'image'),link=path.join(dir,'link');let opened:runtimeFiles.RuntimeFile|null=null
  try{fs.writeFileSync(file,elf(),{mode:0o600});opened=runtimeFiles.RuntimeFile.open(file);assert.equal(opened.size,128);fs.symlinkSync(file,link);assert.throws(()=>runtimeFiles.RuntimeFile.open(link));fs.unlinkSync(link);fs.linkSync(file,link);assert.throws(()=>runtimeFiles.RuntimeFile.open(file));fs.unlinkSync(link);opened.close();opened=runtimeFiles.RuntimeFile.open(file);assert.doesNotThrow(()=>opened!.check())
    fs.renameSync(file,path.join(dir,'old'));fs.writeFileSync(file,elf(),{mode:0o600});assert.throws(()=>opened!.check())
  }finally{opened?.close();fs.rmSync(dir,{recursive:true,force:true})}
})
test('PL1 M1: Node/dev não viram runtime qualificado nem invocam codesign/plutil',async()=>{
  let calls=0
  const runtime=loadMainModule<typeof import('./personal-runtime.ts')>('fork-cloud/personal-runtime',{
    electron:{app:{isPackaged:false}},'node:fs':fs,'node:path':path,'node:os':os,'node:child_process':{execFile(){calls++}},'../../../resources/personal-runtime-policy-v1.json':policies,'./personal-runtime-format.mjs':format,'./personal-runtime-files':runtimeFiles,'./personal-runtime-linux':{},
  },{process,Buffer,TextDecoder,Number,performance})
  await assert.rejects(runtime.qualifyPersonalRuntime(()=>true),{message:'personalRuntimeUnavailable'});assert.equal(calls,0)
})
test('PL1 M1: metadados fecham os hashes dos 67 patches e do builder sem binários no repositório',()=>{
  const inputs=JSON.parse(fs.readFileSync(fileURLToPath(new URL('../../../resources/personal-runtime-build-inputs-v1.json',import.meta.url)),'utf8'))
  assert.equal(inputs.patchOrder.length,67);const pairs=inputs.sourceFiles.map((f:{url:string;sha256:string})=>[f.url,f.sha256]);assert.equal(pairs.length,93)
  for(const p of policies){assert.equal(JSON.stringify(p[10]),JSON.stringify(pairs));assert.equal(p[11][2],format.hash(Buffer.from(JSON.stringify(inputs.builderFiles.map((f:{path:string;sha256:string})=>[f.path,f.sha256])))))}
})
function unsignedX64(){const b=Buffer.alloc(13076);b.writeUInt32LE(0xfeedfacf,0);b.writeUInt32LE(16777223,4);b.writeUInt32LE(2147483651,8);b.writeUInt32LE(2,12);b.writeUInt32LE(15,16);b.writeUInt32LE(1120,20);let at=32
  for(const n of [64,64,64,64,64,64,64,64,104,72,80,80,80,80,112]){b.writeUInt32LE(0x31,at);b.writeUInt32LE(n,at+4);at+=n}
  b.writeUInt32LE(0x19,648);b.write('__LINKEDIT',656);b.writeBigUInt64LE(4096n,680);b.writeBigUInt64LE(12288n,688);b.writeBigUInt64LE(788n,696);b.writeUInt32LE(1,708);return b
}
test('PL1 M1: projeção x64 unsigned é união exata, não inventa assinatura original nem amplia máscara',()=>{
  // Layout sintético para fronteira de projeção. NÃO é assinatura nativa/pacote oficial.
  const original=unsignedX64(),item=format.image(reader(original),'darwin','x64','executable','original-zero');assert.equal(item.length,12);assert.equal(item[1],'unsigned-x64-v1');assert.equal(item[11],format.hash(original))
  assert.throws(()=>format.image(reader(original),'darwin','x64','executable','original-zero',item))
  const signed=Buffer.alloc(13120);original.copy(signed);signed.writeUInt32LE(16,16);signed.writeUInt32LE(1136,20);signed.writeBigUInt64LE(832n,696);signed.writeUInt32LE(0x1d,1152);signed.writeUInt32LE(16,1156);signed.writeUInt32LE(13088,1160);signed.writeUInt32LE(32,1164)
  assert.equal(format.image(reader(signed),'darwin','x64','executable','original-zero',item),item)
  for(const offset of [24,3000,13077]){const changed=Buffer.from(signed);changed[offset]^=1;assert.throws(()=>format.image(reader(changed),'darwin','x64','executable','original-zero',item))}
  const moved=Buffer.from(signed);moved.writeUInt32LE(13096,1160);assert.throws(()=>format.image(reader(moved),'darwin','x64','executable','original-zero',item))
})
test('PL1 M1: SHASUMS oficial admite marcador binário, exige uma linha exata sem fallback',()=>{
  const name='electron-v44.5.1-darwin-x64.zip',sum='e567d13833d0e161d7749727355b98643461df3395b537cfa7bdddf8a8bfedff',line=sum+' *'+name+'\n'
  assert.doesNotThrow(()=>format.assetSum(Buffer.from(line),name,sum));assert.throws(()=>format.assetSum(Buffer.from(line+line),name,sum));assert.throws(()=>format.assetSum(Buffer.from(line),name,'0'.repeat(64)));assert.throws(()=>format.assetSum(Buffer.from(line),'other.zip',sum))
})

test('PL1 M1: builtin raw é capturado uma vez; arquivo e FD usam FS real sem qualificar runtime',()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'personal-raw-')),file=path.join(dir,'image');let opened:runtimeFiles.RuntimeFile|undefined,calls=0
  const nativeProcess={versions:{electron:'44.5.1'},getuid:process.getuid,getBuiltinModule(name:string){calls++;assert.equal(name,'original-fs');return fs}}
  try{
    fs.writeFileSync(file,elf(),{mode:0o600})
    const module=loadMainModule<typeof runtimeFiles>('fork-cloud/personal-runtime-files',{'node:fs':fs},{process:nativeProcess,Buffer})
    nativeProcess.getBuiltinModule=()=>{throw new Error('getter substituído após import')}
    opened=module.RuntimeFile.open(file);assert.deepEqual(opened.bytes(128),elf());opened.check();opened.close()
    const fd=fs.openSync(file,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW);opened=new module.RuntimeFile(file,fd);assert.equal(opened.size,128);opened.check();assert.equal(calls,1)
  }finally{opened?.close();fs.rmSync(dir,{recursive:true,force:true})}
})
test('PL1 M1: Electron sem builtin raw importa sem efeito e recusa uso sem fallback Node fs',()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'personal-no-raw-')),file=path.join(dir,'image')
  try{
    fs.writeFileSync(file,elf(),{mode:0o600})
    const nativeProcess={versions:{electron:'44.5.1'},getuid:process.getuid}
    const missing=loadMainModule<typeof runtimeFiles>('fork-cloud/personal-runtime-files',{'node:fs':fs},{process:nativeProcess,Buffer})
    assert.throws(()=>missing.runtimeRawFs(),{message:'personalRuntimeUnavailable'});assert.throws(()=>missing.RuntimeFile.open(file),{message:'personalRuntimeUnavailable'})
    const throwing=loadMainModule<typeof runtimeFiles>('fork-cloud/personal-runtime-files',{'node:fs':fs},{process:{...nativeProcess,getBuiltinModule(){throw new Error('builtin indisponível')}},Buffer})
    assert.throws(()=>throwing.RuntimeFile.open(file),{message:'personalRuntimeUnavailable'});assert.equal(fs.statSync(file).isFile(),true)
  }finally{fs.rmSync(dir,{recursive:true,force:true})}
})

// Só fixture de layout para o parser/hash: NÃO gera assinatura nativa/codesign.
function signedX64DistinctVm(vm:bigint){
  const original=unsignedX64(),signed=Buffer.alloc(32400);original.copy(signed)
  signed.writeUInt32LE(16,16);signed.writeUInt32LE(1136,20);signed.writeBigUInt64LE(vm,680);signed.writeBigUInt64LE(20112n,696)
  signed.writeUInt32LE(0x1d,1152);signed.writeUInt32LE(16,1156);signed.writeUInt32LE(13088,1160);signed.writeUInt32LE(19312,1164)
  return{original,signed}
}
test('PL1 R38: x64 signed VM16K conserva o mesmo item original quando os resultados4K e16K diferem',()=>{
  const {original,signed}=signedX64DistinctVm(32768n),item=format.image(reader(original),'darwin','x64','executable','original-zero')
  assert.equal(item.length,12);assert.equal(item[1],'unsigned-x64-v1');assert.equal(item[8],4096);assert.equal(item[11],format.hash(original))
  const size=Number(signed.readBigUInt64LE(696)),vm4=Math.max(4096,Math.ceil(size/4096)*4096),vm16=Math.max(4096,Math.ceil(size/16384)*16384)
  assert.equal(size,20112);assert.equal(vm4,20480);assert.equal(vm16,32768);assert.notEqual(vm4,vm16)
  assert.equal(signed.readBigUInt64LE(680),BigInt(vm16));assert.equal(signed.readUInt32LE(1160),13088);assert.equal(signed.readUInt32LE(1164)+13088,signed.length);assert(signed.subarray(original.length,13088).every(v=>v===0))
  // RED previsto na74 é personalRuntimeInvalid exclusivamente na premissa4K;
  // os bytes originais/itens12 não são reescritos para aceitar o signed atual.
  assert.equal(format.image(reader(signed),'darwin','x64','executable','original-zero',item),item)
})
test('PL1 R38: VM fora das duas fórmulas e mutações de prefixo CPU padding assinatura ou proteção recusam',()=>{
  const {original,signed}=signedX64DistinctVm(20480n),item=format.image(reader(original),'darwin','x64','executable','original-zero')
  // Controle4K com o MESMO tamanho distinto: deve passar já na fonte vulnerável.
  assert.equal(format.image(reader(signed),'darwin','x64','executable','original-zero',item),item)
  const arbitrary=Buffer.from(signed);arbitrary.writeBigUInt64LE(24576n,680);assert.notEqual(24576,20480);assert.notEqual(24576,32768);assert.throws(()=>format.image(reader(arbitrary),'darwin','x64','executable','original-zero',item),{message:'personalRuntimeInvalid'})
  for(const offset of [24,3000,12300,13077]){const changed=Buffer.from(signed);changed[offset]^=1;assert.throws(()=>format.image(reader(changed),'darwin','x64','executable','original-zero',item),{message:'personalRuntimeInvalid'})}
  const capability=Buffer.from(signed);capability.writeUInt32LE(3,8);assert.throws(()=>format.image(reader(capability),'darwin','x64','executable','original-zero',item),{message:'personalRuntimeInvalid'})
  const cpu=Buffer.from(signed);cpu.writeUInt32LE(16777228,4);assert.throws(()=>format.image(reader(cpu),'darwin','x64','executable','original-zero',item),{message:'personalRuntimeInvalid'})
  const moved=Buffer.from(signed);moved.writeUInt32LE(13096,1160);assert.throws(()=>format.image(reader(moved),'darwin','x64','executable','original-zero',item),{message:'personalRuntimeInvalid'})
  const executableLinkedit=Buffer.from(signed);executableLinkedit.writeUInt32LE(5,708);assert.throws(()=>format.image(reader(executableLinkedit),'darwin','x64','executable','original-zero',item),{message:'personalRuntimeInvalid'})
})

// RUN1 Electron (05/11): a guarda de imagens do ramo Darwin recusava o app real. O Mach-O fixado de "Electron Framework"
// (zip electron-v44.5.1-darwin-arm64 1d757030…) tem LC_LOAD_DYLIB @rpath/libffmpeg.dylib não-fraco e LC_RPATH @loader_path/Libraries,
// então o processo browser já carrega Versions/A/Libraries/libffmpeg.dylib dentro do framework. Costura de módulos: fronteiras de
// SO/formato simuladas; isto NÃO qualifica Mac/Electron/assinatura, só prova a decisão da guarda no código produtivo.
function darwinGuard(objects:unknown[],links:Record<string,string>={}){
  const bundle='/private/tmp/b/DONNA PIN FD Proof.app',contents=bundle+'/Contents',fwDir=contents+'/Frameworks/Electron Framework.framework'
  const framework=fwDir+'/Versions/A/Electron Framework',executable=contents+'/MacOS/DONNA PIN FD Proof',resources=contents+'/Resources'
  const real:Record<string,string>={[executable]:executable,[framework]:framework,[fwDir+'/Electron Framework']:framework,[fwDir+'/Versions/A/Libraries/libffmpeg.dylib']:fwDir+'/Versions/A/Libraries/libffmpeg.dylib',...links}
  const realpathSync=(p:string)=>{if(!(p in real))throw new Error('ENOENT');return real[p]}
  const tools:string[]=[],hr=()=>1n,report={getReport:()=>({sharedObjects:objects})}
  const fakeProcess={platform:'darwin',arch:'arm64',type:'browser',versions:{electron:'44.5.1',node:'24.21.0',uv:'1.52.1'},env:{},execArgv:[],execPath:executable,resourcesPath:resources,pid:7,hrtime:{bigint:hr},report,getuid:()=>501}
  class File{path:string;constructor(path:string){this.path=path}check(){}bytes(){return Buffer.alloc(1)}close(){}static open(p:string){return new File(p)}}
  const p=['policy',1,'darwin','arm64',0,0,0,0,0,0,[],[0,0,0,'mode']],r=Object.assign(new Array(14).fill(0),{12:['exe','fw'],13:'id'})
  const fakeFormat={canonical:()=>r,policy:()=>p,receipt:()=>r,image:()=>null,hash:()=>'h',asar:{defaultAsar(){},darwinIntegrity:()=>({}),darwinDigest:()=>'d'}}
  const runtime=loadMainModule<typeof import('./personal-runtime.ts')>('fork-cloud/personal-runtime',{
    electron:{app:{isPackaged:true,getAppPath:()=>resources+'/app.asar'}},'node:fs':{statSync:()=>({isFile:()=>true,size:10}),readFileSync:()=>Buffer.from('[]')},'node:path':path,'node:os':{release:()=>'25.0.0'},
    'node:child_process':{execFile(tool:string,_a:string[],_o:unknown,cb:(e:Error|null,s:string)=>void){tools.push(tool);cb(new Error('parada do teste depois da guarda'),'')}},
    '../../../resources/personal-runtime-policy-v1.json':[p],'./personal-runtime-format.mjs':fakeFormat,'./personal-runtime-files':{RuntimeFile:File,runtimeRawFs:()=>({realpathSync}),runtimeNeed:runtimeFiles.runtimeNeed},'./personal-runtime-linux':{},
  },{process:fakeProcess,Buffer,TextDecoder,Number,performance})
  return{tools,run:()=>runtime.qualifyPersonalRuntime(()=>true),fwDir,framework,executable}
}
test('PL1 RUN1: Darwin aceita o framework exato com a libffmpeg do MESMO framework; cardinalidade exata, sem allowlist genérica',async()=>{
  const base=darwinGuard([]),ffmpeg=base.fwDir+'/Versions/A/Libraries/libffmpeg.dylib',loaded=[base.executable,base.fwDir+'/Electron Framework',ffmpeg,'/usr/lib/libSystem.B.dylib']
  // Caso real da RUN1: framework (via link de topo) + libffmpeg ⇒ a guarda deve passar e o fluxo seguir para o plutil.
  const ok=darwinGuard(loaded);await assert.rejects(ok.run(),{message:'personalRuntimeUnavailable'});assert.deepEqual(ok.tools,['/usr/bin/plutil'])
  const swift=base.fwDir+'/Versions/A/Libraries/libvk_swiftshader.dylib',other='/Applications/Other.app/Contents/Frameworks/Electron Framework.framework/Versions/A/Electron Framework',alias='/private/tmp/b/alias/Electron Framework.framework/Electron Framework'
  const refused:[string,unknown[],Record<string,string>?][]=[
    ['framework duplicado',[...loaded,base.framework]],['libffmpeg ausente',loaded.filter(v=>v!==ffmpeg)],['libffmpeg duplicada',[...loaded,ffmpeg]],['framework ausente',loaded.filter(v=>v!==base.fwDir+'/Electron Framework')],
    ['framework de outro bundle',[...loaded,other],{[other]:other}],['alias com realpath estranho',[...loaded,alias],{[alias]:'/private/tmp/evil/Electron Framework'}],
    ['outra imagem de código no framework',[...loaded,swift],{[swift]:swift}],['libffmpeg fora do framework exato',[base.executable,base.fwDir+'/Electron Framework','/private/tmp/x/Electron Framework.framework/Versions/A/Libraries/libffmpeg.dylib'],{'/private/tmp/x/Electron Framework.framework/Versions/A/Libraries/libffmpeg.dylib':'/private/tmp/x/Electron Framework.framework/Versions/A/Libraries/libffmpeg.dylib'}],
  ]
  for(const [name,objects,links] of refused){const c=darwinGuard(objects,links);await assert.rejects(c.run(),{message:'personalRuntimeUnavailable'},name);assert.deepEqual(c.tools,[],name)}
})
