/** Qualificação local do runtime admitido, sem Root/Owner nem opções de IPC. */
import { app } from 'electron'
import * as nodeFs from 'node:fs'
// Somente o recibo interno usa a visão virtual ASAR; artefatos e FDs usam raw.
const virtualFs=Object.freeze({statSync:nodeFs.statSync,readFileSync:nodeFs.readFileSync})
const nativePerformance=performance.now
import { dirname,basename,join,resolve } from 'node:path'
import { execFile } from 'node:child_process'
import { release } from 'node:os'
import policies from '../../../resources/personal-runtime-policy-v1.json'
import format from './personal-runtime-format.mjs'
const {canonical,policy,receipt,image,hash,asar}=format
import { RuntimeFile,runtimeRawFs,runtimeNeed as need } from './personal-runtime-files'
import { qualifyLinuxRuntime,type PersonalElapsed } from './personal-runtime-linux'
export type { PersonalElapsed } from './personal-runtime-linux'
const nativeHr=process.hrtime.bigint,nativeReport=process.report?.getReport
const bytes=(value:unknown)=>Buffer.from(JSON.stringify(value))
function nativeTool(tool:'/usr/bin/codesign'|'/usr/bin/plutil',args:readonly string[]):Promise<string>{return new Promise((resolve,reject)=>execFile(tool,[...args],{timeout:5000,maxBuffer:65536,encoding:'utf8'},(error,stdout)=>{if(error)reject(new Error('personalRuntimeUnavailable'));else resolve(stdout)}))}
export async function qualifyPersonalRuntime(current:()=>boolean):Promise<PersonalElapsed>{
  need(current() && app.isPackaged && process.type==='browser' && process.versions.electron==='44.5.1' && process.versions.node==='24.21.0' && process.versions.uv==='1.52.1' && ['arm64','x64'].includes(process.arch) && ['linux','darwin'].includes(process.platform) && !Object.hasOwn(process.env,'NODE_OPTIONS') && !process.execArgv.some(v=>/^--(inspect|require|import|eval)/.test(v)))
  const p=policy(policies.find(v=>v[2]===process.platform && v[3]===process.arch)),resources=process.resourcesPath,appPath=app.getAppPath()
  need(appPath===join(resources,'app.asar'))
  let elapsed:PersonalElapsed|undefined
  const files:RuntimeFile[]=[]
  try{
    const fs=runtimeRawFs()
    const archive=RuntimeFile.open(appPath);files.push(archive)
    const recordPath=join(appPath,'out/main/personal-runtime-receipt.json'),recordStat=virtualFs.statSync(recordPath)
    need(recordStat.isFile() && recordStat.size>0 && recordStat.size<=65536)
    const raw=virtualFs.readFileSync(recordPath),r=receipt(canonical(raw,65536),p);archive.check()
    if(process.platform==='linux')elapsed=qualifyLinuxRuntime(p,r,resources)
    else{
      need(Number.parseInt(release().split('.')[0],10)>=16 && process.hrtime.bigint===nativeHr && process.report?.getReport===nativeReport && nativeReport)
      const executable=fs.realpathSync(process.execPath),bundle=resolve(dirname(executable),'../..'),contents=join(bundle,'Contents'),framework=join(contents,'Frameworks/Electron Framework.framework/Versions/A/Electron Framework')
      need(dirname(executable)===join(contents,'MacOS') && basename(resources)==='Resources' && dirname(resources)===contents && fs.realpathSync(framework)===framework)
      for(const name of ['DYLD_INSERT_LIBRARIES','DYLD_LIBRARY_PATH','DYLD_FRAMEWORK_PATH','DYLD_FALLBACK_LIBRARY_PATH','DYLD_FALLBACK_FRAMEWORK_PATH'])need(!Object.hasOwn(process.env,name))
      const native=RuntimeFile.open(executable);files.push(native);const loaded=RuntimeFile.open(framework);files.push(loaded);const info=RuntimeFile.open(join(contents,'Info.plist'));files.push(info)
      const extra=RuntimeFile.open(join(resources,'default_app.asar'));files.push(extra);asar.defaultAsar(extra,p)
      const rawIntegrity=asar.darwinIntegrity(info.bytes(65536))
      const check=()=>{for(const name of ['DYLD_INSERT_LIBRARIES','DYLD_LIBRARY_PATH','DYLD_FRAMEWORK_PATH','DYLD_FALLBACK_LIBRARY_PATH','DYLD_FALLBACK_FRAMEWORK_PATH'])need(!Object.hasOwn(process.env,name));need(current() && process.hrtime.bigint===nativeHr && process.report?.getReport===nativeReport);for(const f of files)f.check();need(fs.realpathSync(process.execPath)===executable && fs.realpathSync(framework)===framework)}
      const report=nativeReport.call(process.report) as unknown
      need(report && typeof report==='object' && 'sharedObjects' in report && Array.isArray(report.sharedObjects))
      // O Electron Framework fixado tem LC_LOAD_DYLIB @rpath/libffmpeg.dylib (não-fraco) e LC_RPATH @loader_path/Libraries:
      // o processo carrega exatamente o framework e a libffmpeg do MESMO framework; qualquer outra, duplicata ou ausência recusa.
      const objects:unknown[]=report.sharedObjects,frameworks=objects.filter((v):v is string=>typeof v==='string' && v.includes('Electron Framework.framework/')).map(v=>fs.realpathSync(v))
      need(frameworks.length===2 && frameworks.includes(framework) && frameworks.includes(join(dirname(framework),'Libraries/libffmpeg.dylib')))
      // O relatório completo nunca é serializado, registrado ou enviado.
      check();const plist=await nativeTool('/usr/bin/plutil',['-extract','ElectronAsarIntegrity','json','-o','-',info.path]);check()
      const asarDigest=asar.darwinDigest(rawIntegrity,JSON.parse(plist))
      image(native,'darwin',p[3],'executable',p[11][3],r[12][0]);image(loaded,'darwin',p[3],'framework',p[11][3],r[12][1],asarDigest)
      check();await nativeTool('/usr/bin/codesign',['--verify','--deep','--strict','--all-architectures',bundle]);check()
      let alive=true,lastLo=0,lastHi=0
      const close=()=>{alive=false}
      elapsed=Object.freeze({quantum:1 as const,generation:hash(bytes(['darwin',process.pid,r[13],r[12]])),auxiliary(){return nativePerformance.call(performance)},current(){need(alive);check()},sample(){need(alive);check();const ns=nativeHr.call(process.hrtime);need(ns>0n && ns/1000000n<BigInt(Number.MAX_SAFE_INTEGER));const lo=Number(ns/1000000n),hi=lo+1;need(lo>=lastLo && hi>=lastHi);lastLo=lo;lastHi=hi;return Object.freeze({lo,hi})},close})
    }
    const original=elapsed;need(original)
    const close=()=>{original.close();for(const f of files)f.close()}
    const selected:PersonalElapsed=Object.freeze({quantum:original.quantum,generation:original.generation,auxiliary(){this.current();return original.auxiliary()},current(){try{need(current());original.current();for(const f of files)f.check()}catch(e){close();throw e}},sample(){try{this.current();const s=original.sample();this.current();return s}catch(e){close();throw e}},close})
    selected.current();return selected
  }catch(e){elapsed?.close();for(const f of files)f.close();throw e}
}
