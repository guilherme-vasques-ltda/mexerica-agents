/** Adaptador privado para syscalls por descritor. Não é uma API IPC/Renderer. */
import { personalNeed,personalSame } from './personal-session'
import type { PersonalEffectGuard } from './personal-session'
import { pinDigest } from './personal-pin-storage'

export interface PinIOGuard {check():Promise<void>;current():void}
// Handles opacos do addon; nenhum FD, basename ou flags atravessa o contrato.
export interface PinHandle {readonly generation:never}
export interface PinStat {dev:string;ino:string;uid:string;mode:string;nlink:string;size:string;regular:boolean;directory:boolean;symlink:boolean}
export type PinRole='pending'|'final'|'journal'
/** Contrato de implementação nativa pendente de qualificação Electron.
 * inspect/adoptPair são inspeções separadas; cada efeito é UM syscall.
 * adoptPair só arma o par após comparação completa JS e lock corrente; nunca
 * grava/recifra/recria a operação nem finge que o handle é o criador original.
 */
export interface PinNative {
  inspect(h:PinHandle):PinStat
  inspectName(d:PinHandle,key:string,operation:string,role:PinRole|'lock'|'journalTmp'):PinStat
  armOwnName(h:PinHandle):PinStat;armLinkedFinal(h:PinHandle):PinStat
  openRead(d:PinHandle,key:string,role:PinRole):PinHandle
  createPending(d:PinHandle,key:string,operation:string):PinHandle
  createFirstJournal(d:PinHandle,key:string,operation:string,lock:PinHandle):PinHandle
  createJournalTmp(d:PinHandle,key:string,operation:string):PinHandle
  createLock(d:PinHandle,key:string):PinHandle;openLock(d:PinHandle,key:string):PinHandle
  tryLock(h:PinHandle):void;unlock(h:PinHandle):void
  readOne(h:PinHandle,at:number,length:number):{count:number;bytes:Uint8Array}
  writeOne(h:PinHandle,at:number,bytes:Buffer):number
  sync(h:PinHandle):void;linkFinal(h:PinHandle):void;unlinkOwnPending(h:PinHandle):void
  publishJournal(h:PinHandle,lock:PinHandle):void
  // Única extensão de recuperação: abre nlink2 como candidato READ-ONLY;
  // inspectPairCandidate confere0600/euid/par2 mas não arma unlink/link.
  openPairCandidate(d:PinHandle,key:string):PinHandle
  inspectPairCandidate(h:PinHandle):PinStat
  adoptPair(h:PinHandle,lock:PinHandle):void
  close(h:PinHandle):void
}
export interface PinDirectory {
  current():void
  read(key:string,role:PinRole,guard:PinIOGuard):Promise<Uint8Array|null>
  transaction<T>(key:string,guard:PinIOGuard,action:(tx:PinTransaction)=>Promise<T>):Promise<T>
  close():void
}
export interface PinTransaction {
  read(role:PinRole):Promise<Uint8Array|null>
  journal(bytes:Uint8Array,operation:string,initial?:boolean):Promise<void>
  publish(bytes:Uint8Array,operation:string,recovery:boolean):Promise<void>
}
const missing=(e:unknown)=>!!e && typeof e==='object' && (e as {code?:unknown}).code==='ENOENT'
/** O qualificador deve reter toda a cadeia de pais até close. Seu current
 * revalida FD+nome/devino/proteções/generation após cada IO, sem pathfallback.
 * Este adaptador NÃO qualifica um addon fornecido pelo chamador: só Main interno.
 */
export class RetainedPinDirectory implements PinDirectory {
  private alive=true
  private readonly handles=new Set<PinHandle>()
  constructor(private readonly api:PinNative,private readonly directory:PinHandle,private readonly parents:{current():void;close():void}){}
  current(){personalNeed(this.alive);this.parents.current()}
  private async checked(g:PinIOGuard){this.current();g.current();await g.check();this.current();g.current()}
  private async io<T>(g:PinIOGuard,action:()=>T):Promise<T>{
    await this.checked(g);this.current();g.current()
    let result:T
    try{result=action()}catch(e){await this.checked(g);throw e}
    await this.checked(g);return result
  }
  private async own(g:PinIOGuard,open:()=>PinHandle){
    // Retain BEFORE post-IO guard; close this exact handle on a failed return.
    let opened:PinHandle|undefined
    try{return await this.io(g,()=>{opened=open();this.handles.add(opened);return opened})}
    catch(e){if(opened){try{this.closeHandle(opened)}catch{}}throw e}
  }
  private closeHandle(h:PinHandle){if(this.handles.delete(h))this.api.close(h)}
  private async directoryCheck(g:PinIOGuard){return this.io(g,()=>this.api.inspect(this.directory))}
  private async inspect(g:PinIOGuard,h:PinHandle){return this.io(g,()=>this.api.inspect(h))}
  private async readHandle(h:PinHandle,g:PinIOGuard,pair=false){
    const inspect=()=>pair?this.api.inspectPairCandidate(h):this.api.inspect(h)
    const first=await this.io(g,inspect),size=Number(first.size),chunks:Buffer[]=[]
    personalNeed(Number.isSafeInteger(size) && size>=0 && size<=832*1024)
    for(let at=0;at<size;){
      await this.io(g,inspect);const length=Math.min(65536,size-at),r=await this.io(g,()=>this.api.readOne(h,at,length))
      personalNeed(r.count===length && r.bytes.length>=length);chunks.push(Buffer.from(r.bytes.subarray(0,length)));at+=length
    }
    const last=await this.io(g,inspect);personalNeed(personalSame(first,last));return Buffer.concat(chunks)
  }
  async read(key:string,role:PinRole,g:PinIOGuard):Promise<Uint8Array|null>{
    personalNeed(/^[a-f0-9]{64}$/.test(key));let h:PinHandle|undefined
    try{
      await this.directoryCheck(g)
      try{h=await this.own(g,()=>this.api.openRead(this.directory,key,role))}catch(e){if(missing(e))return null;throw e}
      const data=await this.readHandle(h,g);await this.io(g,()=>this.api.armOwnName(h!));return data
    }finally{if(h)this.closeHandle(h)}
  }
  async transaction<T>(key:string,g:PinIOGuard,action:(tx:PinTransaction)=>Promise<T>):Promise<T>{
    personalNeed(/^[a-f0-9]{64}$/.test(key));let lock:PinHandle|undefined
    const owned=new Set<PinHandle>()
    const own=(open:()=>PinHandle)=>this.own(g,()=>{const h=open();owned.add(h);return h})
    try{
      await this.directoryCheck(g)
      try{lock=await own(()=>this.api.openLock(this.directory,key))}
      catch(e){if(!missing(e))throw e;await this.directoryCheck(g);lock=await own(()=>this.api.createLock(this.directory,key))}
      const locked=lock
      const armLock=async()=>{await this.inspect(g,locked);await this.io(g,()=>this.api.armOwnName(locked))}
      await armLock();await this.io(g,()=>this.api.tryLock(locked))
      await armLock();await this.io(g,()=>this.api.sync(locked));await this.directoryCheck(g);await this.io(g,()=>this.api.sync(this.directory))
      let active=true
      const live=()=>{personalNeed(active);g.current();this.current()}
      const write=async(h:PinHandle,bytes:Uint8Array)=>{
        for(let at=0;at<bytes.length;){live();await this.inspect(g,h);const piece=Buffer.from(bytes.subarray(at,at+65536)),n=await this.io(g,()=>this.api.writeOne(h,at,piece));personalNeed(n===piece.length);at+=n}
        await this.inspect(g,h);await this.io(g,()=>this.api.sync(h));const actual=await this.readHandle(h,g);personalNeed(Buffer.from(bytes).equals(actual))
      }
      const syncDir=async()=>{await this.directoryCheck(g);await this.io(g,()=>this.api.sync(this.directory))}
      const tx:PinTransaction={
        read:async role=>{live();return this.read(key,role,g)},
        journal:async(bytes,op,initial=false)=>{
          live();personalNeed(/^[a-f0-9]{32}$/.test(op) && bytes.length>0 && bytes.length<=832*1024)
          await this.directoryCheck(g)
          if(initial){
            // First record uses its FINAL closed name O_EXCL. A partial record
            // survives interruption and blocks any new cipher/operation after
            // restart; a lost temporary name must not erase unknown provenance.
            await armLock();await this.directoryCheck(g)
            const first=await own(()=>this.api.createFirstJournal(this.directory,key,op,locked))
            await write(first,bytes);await this.inspect(g,first);await this.io(g,()=>this.api.armOwnName(first));await this.io(g,()=>this.closeHandle(first));await syncDir();return
          }
          const h=await own(()=>this.api.createJournalTmp(this.directory,key,op))
          await write(h,bytes);await this.inspect(g,h);await this.io(g,()=>this.api.armOwnName(h));await armLock();live();await this.io(g,()=>this.api.publishJournal(h,locked));await syncDir()
        },
        publish:async(bytes,op,recovery)=>{
          live();personalNeed(/^[a-f0-9]{32}$/.test(op) && bytes.length>0 && bytes.length<=262144)
          // Under stable lock: existing final is never overwritten. For recovery
          // pair2, read() refuses and the explicit adoption path below handles it.
          let final:Uint8Array|null=null,pending:Uint8Array|null=null,pair=false,h:PinHandle|undefined
          try{final=await this.read(key,'final',g);pending=await this.read(key,'pending',g)}
          catch(e){if(!recovery || !(e && typeof e==='object' && (e as {code?:unknown}).code==='EPROOFPERM'))throw e;pair=true}
          if(pair){
            await this.directoryCheck(g);h=await own(()=>this.api.openPairCandidate(this.directory,key));const data=await this.readHandle(h,g,true)
            personalNeed(pinDigest(data)===pinDigest(bytes) && data.equals(Buffer.from(bytes)))
            const held=await this.io(g,()=>this.api.inspectPairCandidate(h!))
            const a=await this.io(g,()=>this.api.inspectName(this.directory,key,op,'pending')),b=await this.io(g,()=>this.api.inspectName(this.directory,key,op,'final'))
            personalNeed(a.dev===held.dev && a.ino===held.ino && a.dev===b.dev && a.ino===b.ino && a.nlink==='2' && b.nlink==='2' && a.size===String(bytes.length) && b.size===a.size)
            await armLock();await this.io(g,()=>this.api.adoptPair(h!,locked))
          }else{
            if(final){
              personalNeed(Buffer.from(final).equals(Buffer.from(bytes)) && pending===null)
              await this.directoryCheck(g);const existing=await own(()=>this.api.openRead(this.directory,key,'final'))
              personalNeed((await this.readHandle(existing,g)).equals(Buffer.from(bytes)))
              await this.inspect(g,existing);await this.io(g,()=>this.api.armOwnName(existing));await this.io(g,()=>this.api.sync(existing));await syncDir();return
            }
            if(pending){
              personalNeed(recovery && Buffer.from(pending).equals(Buffer.from(bytes)));await this.directoryCheck(g);h=await own(()=>this.api.openPairCandidate(this.directory,key))
              // The name may have changed since the earlier read: compare THIS retained candidate before adopt/link.
              const data=await this.readHandle(h,g,true);personalNeed(pinDigest(data)===pinDigest(bytes) && data.equals(Buffer.from(bytes)))
              await armLock();await this.io(g,()=>this.api.adoptPair(h!,locked))
            }
            else{personalNeed(!recovery || pending===null);await this.directoryCheck(g);h=await own(()=>this.api.createPending(this.directory,key,op));await write(h,bytes)}
            await this.inspect(g,h);await this.io(g,()=>this.api.armOwnName(h!));await this.io(g,()=>this.api.linkFinal(h!))
          }
          await this.inspect(g,h!);await this.io(g,()=>this.api.armOwnName(h!));await this.io(g,()=>this.api.armLinkedFinal(h!));await syncDir()
          // syncDir is separate IO; inspect/arm again before unlink.
          await this.inspect(g,h!);await this.io(g,()=>this.api.armOwnName(h!));await this.io(g,()=>this.api.armLinkedFinal(h!));await this.io(g,()=>this.api.unlinkOwnPending(h!));await syncDir()
          const stored=await this.read(key,'final',g);personalNeed(stored && Buffer.from(stored).equals(Buffer.from(bytes)))
        },
      }
      try{const result=await action(tx);await this.checked(g);return result}finally{active=false}
    }finally{
      // Close releases only OWN FDs/flock, even after guard failure. No unlink,
      // publication or retry is allowed as failure cleanup.
      for(const h of [...owned].reverse()){try{this.closeHandle(h)}catch{}}
    }
  }
  close(){if(!this.alive)return;this.alive=false;for(const h of [...this.handles].reverse()){try{this.closeHandle(h)}catch{}}this.parents.close()}
}
// Keep the effect type dependency explicit; guard does not grant Owner/Use.
export type PinCurrentEffect=PersonalEffectGuard
