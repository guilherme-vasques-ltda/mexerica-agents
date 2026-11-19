import assert from 'node:assert/strict'
import test from 'node:test'
import { Duplex } from 'node:stream'
import * as nodeCrypto from 'node:crypto'
import * as protocol from './personal-protocol.ts'
import * as wire from './personal-wire.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
function consumer(current:()=>boolean=()=>true) {
  let emitted:Buffer|null=null, terminated=0
  const output={on(){},off(){},write(bytes:Buffer,done:(e?:Error)=>void){emitted=bytes;done()}}
  const input=new Duplex({read(){},write(_chunk,_enc,done){done()}})
  const {PersonalHelper}=loadMainModule<typeof import('./personal-helper.ts')>('fork-cloud/personal-helper',{'node:crypto':nodeCrypto,'./personal-protocol':protocol,'./personal-wire':wire},{Buffer,setTimeout,clearTimeout})
  const helper=new PersonalHelper(input,output as never,current,()=>{terminated++})
  return {helper,input,get emitted(){return emitted},get terminated(){return terminated}}
}
function response(request:Buffer,value:unknown) { const a=JSON.parse(request.subarray(4).toString());const bytes=Buffer.from(JSON.stringify([wire.personalDomain('helper'),1,a[2],'ok',a[3],value]));const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);return Buffer.concat([header,bytes]) }
test('PL1 C3b.1: helper aceita resposta fragmentada só da operação capturada',async()=>{
  const c=consumer(), pending=c.helper.request(['time']);assert(c.emitted)
  const reply=response(c.emitted,['synthetic-time']);c.input.emit('data',reply.subarray(0,3));c.input.emit('data',reply.subarray(3));assert.deepEqual(await pending,['synthetic-time']);c.helper.close()
})
test('PL1 C3b.1: tamanho inválido ou sessão trocada produz unknown sem reemitir',async()=>{
  for(const mode of ['size','session']) {let current=true;const c=consumer(()=>current), pending=c.helper.request(['time']);assert(c.emitted);const original=Buffer.from(c.emitted)
    if(mode==='size'){const size=Buffer.alloc(4);size.writeUInt32BE(16385);c.input.emit('data',size)}else{current=false;c.input.emit('data',response(original,[]))}
    await assert.rejects(pending,/personalHelperUnknown/);assert.equal(c.terminated,1);assert.deepEqual(c.emitted,original);await assert.rejects(c.helper.request(['time']))
  }
})
