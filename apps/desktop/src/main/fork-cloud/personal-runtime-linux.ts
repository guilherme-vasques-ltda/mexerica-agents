/** Linux localHelper: ELF executado e procfs nativo, sem addon/child/parede. */
const nativePerformance=performance.now
import { dirname,basename,resolve } from 'node:path'
import format,{type RuntimePolicy,type RuntimeReceipt} from './personal-runtime-format.mjs'
const {image}=format
import { RuntimeFile,fileIdentity,runtimeRawFs,runtimeNeed as need } from './personal-runtime-files'
export interface PersonalElapsed {readonly quantum:1|1000;readonly generation:string;sample():Readonly<{lo:number;hi:number}>;auxiliary():number;current():void;close():void}
export function qualifyLinuxRuntime(policy:RuntimePolicy,receipt:RuntimeReceipt,resourcesPath:string):PersonalElapsed {
  need(process.platform==='linux' && process.getuid && !Object.hasOwn(process.env,'LD_PRELOAD') && !Object.hasOwn(process.env,'LD_AUDIT'))
  const fs=runtimeRawFs()
  const pid=String(process.pid),euid=BigInt(process.getuid()),C=fs.constants,owned:number[]=[]
  const directory=(path:string,uid:bigint)=>{const fd=fs.openSync(path,C.O_RDONLY|C.O_DIRECTORY|C.O_NOFOLLOW);owned.push(fd);const s=fs.fstatSync(fd,{bigint:true});need(s.isDirectory() && s.uid===uid && !(s.mode&0o022n));return fd}
  let elf:RuntimeFile|undefined
  try{
    const root=directory('/',0n);need(fs.readlinkSync('/proc/self')===pid);const proc=directory(`/proc/${pid}/fd/${root}/proc`,0n),procIdentity=[fs.fstatSync(proc,{bigint:true}).dev,fs.fstatSync(proc,{bigint:true}).ino,fs.fstatSync(proc,{bigint:true}).uid,fs.fstatSync(proc,{bigint:true}).mode].join(':'),base=`/proc/${pid}/fd/`
    const isProc=(fd:number)=>{const s=fs.statfsSync(base+fd,{bigint:true});need(s.type===0x9fa0n && fs.fstatSync(fd,{bigint:true}).dev===fs.fstatSync(proc,{bigint:true}).dev)};isProc(proc)
    const open=(names:readonly string[],uid:bigint)=>{let parent=proc;for(const n of names.slice(0,-1)){parent=directory(`${base}${parent}/${n}`,names[0]===pid?euid:0n);isProc(parent)}const filePath=`${base}${parent}/${names[names.length-1]}`,fd=fs.openSync(filePath,C.O_RDONLY|C.O_NOFOLLOW);owned.push(fd);const s=fs.fstatSync(fd,{bigint:true});need(s.isFile() && s.uid===uid && !(s.mode&0o022n));isProc(fd);return{fd,filePath,identity:[s.dev,s.ino,s.uid,s.mode].join(':')}}
    const uptime=open(['uptime'],0n),boot=open(['sys','kernel','random','boot_id'],0n),stat=open([pid,'stat'],euid),offsets=open([pid,'timens_offsets'],euid)
    const text=(f:{fd:number},limit:number)=>{const b=Buffer.alloc(limit),n=fs.readSync(f.fd,b,0,limit,0);need(n>0 && n<limit && b.subarray(0,n).every(x=>x<128));return b.subarray(0,n).toString('ascii')}
    const processStart=()=>{const raw=text(stat,4096),end=raw.lastIndexOf(')');need(end>0 && raw.slice(0,raw.indexOf(' '))===pid);const fields=raw.slice(end+2).trim().split(/\s+/);need(fields.length>=20 && /^\d+$/.test(fields[19]));return fields[19]}
    const bootId=()=>{const b=text(boot,64);need(/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\n$/.test(b));return b}
    const namespaces=()=>{const m=fs.readlinkSync(`/proc/${pid}/ns/mnt`),t=fs.readlinkSync(`/proc/${pid}/ns/time`);need(/^mnt:\[\d+\]$/.test(m) && /^time:\[\d+\]$/.test(t));const lines=text(offsets,256).trimEnd().split('\n');need(lines.length===2);for(let i=0;i<2;i++){const fields=lines[i].trim().split(/\s+/);need(fields.length===3 && fields[0]===['monotonic','boottime'][i] && fields.slice(1).every(s=>/^-?\d+$/.test(s) && BigInt(s)===0n))}return m+':'+t}
    const initialBoot=bootId(),starttime=processStart(),ns=namespaces(),exeLink=`/proc/${pid}/exe`,target=fs.readlinkSync(exeLink),execPath=fs.realpathSync(process.execPath)
    need(!target.endsWith(' (deleted)') && target===execPath && resolve(dirname(resourcesPath),basename(execPath))===execPath)
    const executableFd=fs.openSync(exeLink,C.O_RDONLY);elf=new RuntimeFile(execPath,executableFd);const executableIdentity=fileIdentity(fs.fstatSync(executableFd,{bigint:true}));image(elf,'linux',policy[3],'executable',policy[11][3],receipt[12][0])
    let alive=true,lastLo=0,lastHi=0
    const check=()=>{need(alive && fs.readlinkSync('/proc/self')===pid && process.pid.toString()===pid && !fs.lstatSync('/proc').isSymbolicLink() && [fs.statSync('/proc',{bigint:true}).dev,fs.statSync('/proc',{bigint:true}).ino,fs.statSync('/proc',{bigint:true}).uid,fs.statSync('/proc',{bigint:true}).mode].join(':')===procIdentity);for(const f of [uptime,boot,stat,offsets]){const s=fs.fstatSync(f.fd,{bigint:true}),p=fs.statSync(f.filePath,{bigint:true});need([s.dev,s.ino,s.uid,s.mode].join(':')===f.identity && [p.dev,p.ino,p.uid,p.mode].join(':')===f.identity);isProc(f.fd)}need(bootId()===initialBoot && processStart()===starttime && namespaces()===ns && fs.readlinkSync(exeLink)===execPath && fileIdentity(fs.statSync(exeLink,{bigint:true}))===executableIdentity);elf!.check()}
    const close=()=>{if(alive){alive=false;elf!.close();for(const fd of owned.splice(0))fs.closeSync(fd)}}
    const reading=()=>{const value=text(uptime,256),match=/^(\d+)\.(\d{2}) (\d+)\.(\d{2})\n$/.exec(value);need(match);const ms=BigInt(match[1])*1000n+BigInt(match[2])*10n;need(ms>0n && ms<2n**42n);return Number(ms)}
    const elapsed:PersonalElapsed=Object.freeze({quantum:1000 as const,generation:['linux',pid,starttime,initialBoot,ns,receipt[13]].join(':'),auxiliary(){return nativePerformance.call(performance)},current(){try{check()}catch(e){close();throw e}},sample(){try{check();const lo=reading(),hi=reading()+1000;check();need(lo>=lastLo && hi>=lastHi && hi>=lo && hi-lo<=2005);lastLo=lo;lastHi=hi;return Object.freeze({lo,hi})}catch(e){close();throw e}},close})
    elapsed.current();return elapsed
  }catch(e){elf?.close();for(const fd of owned.splice(0))fs.closeSync(fd);throw e}
}
