import assert from 'node:assert/strict'
import test from 'node:test'
import { createHash } from 'node:crypto'
import { preloadRefusals, readEmbeddedSignature } from './personal-pin-signature.ts'

// Fixture SINTÉTICA de layout (Mach-O fino + SuperBlob + CodeDirectory SHA-256). Não é assinatura Apple/CMS nem prova AMFI/dyld/Team ID:
// só exercita o leitor e a camada pré-dlopen SÓ-RECUSA do contrato B (docs/reference/hub-linux-pin-native-load.md).
const sha=(b:Buffer)=>createHash('sha256').update(b).digest()
function image(o:{team?:string|null,flags?:number,cpu?:number,code?:number,signed?:boolean}={}){
  const page=4096,codeLen=2*page,team=o.team===undefined?'TEAMID1234':o.team,code=Buffer.alloc(codeLen,o.code??7)
  code.writeUInt32LE(0xfeedfacf,0);code.writeUInt32LE(o.cpu??0x0100000c,4);code.writeUInt32LE(6,12);code.writeUInt32LE(o.signed===false?0:1,16);code.writeUInt32LE(o.signed===false?0:16,20)
  if(o.signed===false)return code
  const ident=Buffer.from('app.donnabot.pin.native\0'),teamB=team===null?Buffer.alloc(0):Buffer.from(team+'\0'),head=92,identOffset=head,teamOffset=team===null?0:identOffset+ident.length,hashOffset=identOffset+ident.length+teamB.length
  const cd=Buffer.alloc(hashOffset+2*32);cd.writeUInt32BE(0xfade0c02,0);cd.writeUInt32BE(cd.length,4);cd.writeUInt32BE(0x20400,8);cd.writeUInt32BE(o.flags??0x10000,12)
  cd.writeUInt32BE(hashOffset,16);cd.writeUInt32BE(identOffset,20);cd.writeUInt32BE(0,24);cd.writeUInt32BE(2,28);cd.writeUInt32BE(codeLen,32);cd[36]=32;cd[37]=2;cd[38]=0;cd[39]=12;cd.writeUInt32BE(teamOffset,52)
  ident.copy(cd,identOffset);teamB.copy(cd,identOffset+ident.length)
  code.writeUInt32LE(0x1d,32);code.writeUInt32LE(16,36);code.writeUInt32LE(codeLen,40);code.writeUInt32LE(20+cd.length,44)  // header completo ANTES dos hashes
  for(let i=0;i<2;i++)sha(code.subarray(i*page,(i+1)*page)).copy(cd,hashOffset+i*32)
  const sb=Buffer.alloc(20+cd.length);sb.writeUInt32BE(0xfade0cc0,0);sb.writeUInt32BE(sb.length,4);sb.writeUInt32BE(1,8);sb.writeUInt32BE(0,12);sb.writeUInt32BE(20,16);cd.copy(sb,20)
  return Buffer.concat([code,sb])
}
const expected=(b:Buffer,over:Record<string,unknown>={})=>{const s=readEmbeddedSignature(b,'arm64');return{arch:'arm64' as const,teamId:'TEAMID1234',cdhash:s.cdhash,wholeSha256:sha(b).toString('hex'),...over}}

test('PL1 PIN contrato B: camada pré-dlopen SÓ-RECUSA aceita só a imagem exata da mesma Team ID, com runtime e páginas íntegras',()=>{
  const good=image(),want=expected(good)
  assert.deepEqual(preloadRefusals(good,want),[])
  const s=readEmbeddedSignature(good,'arm64');assert.equal(s.teamId,'TEAMID1234');assert.equal(s.hashType,2);assert.equal(s.cdhash.length,40)
  // Negativos (todos ANTES de qualquer carregamento): sem assinatura, ad-hoc, sem hardened runtime, outra Team ID, sem Team ID,
  // imagem TROCADA da MESMA Team ID (outros bytes ⇒ outro cdhash/hash inteiro), página adulterada, arch errada.
  const refuse=(b:Buffer,w=want)=>preloadRefusals(b,w)
  assert.deepEqual(refuse(image({signed:false})),['unsigned'])
  assert(refuse(image({flags:0x10002})).includes('adhoc'))
  assert(refuse(image({flags:0})).includes('no-hardened-runtime'))
  assert(refuse(image({team:'OTHERTEAM9'})).includes('team-id'))
  assert(refuse(image({team:null})).includes('team-id'))
  const swapped=image({code:9});assert.equal(readEmbeddedSignature(swapped,'arm64').teamId,'TEAMID1234');assert.deepEqual(refuse(swapped),['cdhash','whole-image'])
  const tampered=Buffer.from(good);tampered[5000]^=1;assert(refuse(tampered).includes('page-hash'))
  assert.deepEqual(refuse(image({cpu:0x01000007})),['arch'])
  assert.throws(()=>readEmbeddedSignature(Buffer.from([0xca,0xfe,0xba,0xbe,0,0,0,2]),'arm64'),/personalPinSignatureInvalid/)
})
