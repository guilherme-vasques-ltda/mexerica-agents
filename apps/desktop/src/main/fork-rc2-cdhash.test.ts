import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { assinarArm64 } from '../shared/test-support/rc2-assinatura.ts'
import { verifyRc2AdHocSignature } from './fork-rc2-cdhash.ts'

const corpo = 'corpo de fixture do hub rc.2 para o gate de cdhash'
const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest()

// Pin recalculado sobre o CodeDirectory mutado: mutação estrutural recusa só pela
// própria guarda, nunca porque o pin parou de corresponder ao conteúdo alterado.
function pinDe(bytes: Buffer, cd: number): string {
  const length = bytes.readUInt32BE(cd + 4)
  return sha256(bytes.subarray(cd, cd + length)).subarray(0, 20).toString('hex')
}

test('assinatura ad hoc estruturalmente válida com pin exato passa nos dois tamanhos de página', () => {
  for (const pageSize of [12, 14]) {
    const fixture = assinarArm64(`${corpo} ${pageSize}`, { pageSize })
    verifyRc2AdHocSignature(fixture.bytes, fixture.cdhash)
  }
})

test('formato real do codesign ad hoc (requirements vazio e CMS BlobWrapper vazio) passa', () => {
  const fixture = assinarArm64(corpo, { formatoReal: true })
  assert.equal(fixture.bytes.readUInt32BE(fixture.cms), 0xfade0b01)
  verifyRc2AdHocSignature(fixture.bytes, fixture.cdhash)
})

test('CMS só entra vazio: blob com conteúdo ou magic errado recusa, com pin coerente', () => {
  const fixture = assinarArm64(corpo, { formatoReal: true })
  const recusa = (mutate: (bytes: Buffer) => void, label: string) => {
    const bytes = Buffer.from(fixture.bytes)
    mutate(bytes)
    assert.throws(() => verifyRc2AdHocSignature(bytes, fixture.cdhash), /invalid_rc2_artifact/, label)
  }
  recusa(bytes => { bytes.writeUInt32BE(9, fixture.cms + 4) }, 'CMS com conteúdo')
  recusa(bytes => { bytes.writeUInt32BE(0xfade0b02, fixture.cms) }, 'magic errado no CMS')
  recusa(bytes => { bytes.writeUInt32BE(0, fixture.cms + 4) }, 'CMS com length zero')
})

test('página parcial no fim do corpo passa sem preenchimento e byte trocado nela recusa', () => {
  const fixture = assinarArm64(Buffer.alloc(8200, 7))
  assert.equal(Math.ceil(fixture.dataoff / (1 << 12)), 3)
  assert.notEqual(fixture.dataoff % (1 << 12), 0, 'última página precisa ser parcial')
  verifyRc2AdHocSignature(fixture.bytes, fixture.cdhash)
  const bytes = Buffer.from(fixture.bytes)
  bytes[fixture.dataoff - 1] ^= 0xff
  assert.throws(() => verifyRc2AdHocSignature(bytes, pinDe(bytes, fixture.cd)), /invalid_rc2_artifact/, 'hash da página sem zeros detecta a troca')
})

test('cabeçalho fora do contrato recusa antes de ler assinatura', () => {
  const fixture = assinarArm64(corpo)
  const duplicado = assinarArm64(corpo, { duplicateCodeSignature: true })
  for (const [label, bytes, pin] of [
    ['cabeçalho truncado', fixture.bytes.subarray(0, 31), fixture.cdhash],
    ['cputype x64', Buffer.concat([Buffer.from([0xcf, 0xfa, 0xed, 0xfe, 0x07, 0x00, 0x00, 0x01]), fixture.bytes.subarray(8)]), fixture.cdhash],
    ['sem comando de assinatura', (() => { const bytes = Buffer.from(fixture.bytes); bytes.writeUInt32LE(0x19, 32); return bytes })(), fixture.cdhash],
    ['comando duplicado', duplicado.bytes, duplicado.cdhash],
    ['assinatura truncada', fixture.bytes.subarray(0, fixture.dataoff + 30), fixture.cdhash],
  ] as Array<[string, Buffer, string]>) {
    assert.throws(() => verifyRc2AdHocSignature(bytes, pin), /invalid_rc2_artifact/, label)
  }
})

test('cputype x64 com assinatura e pin coerentes recusa só pela guarda de arquitetura', () => {
  const fixture = assinarArm64(corpo, { cputypeOverride: 0x01000007 })
  assert.throws(() => verifyRc2AdHocSignature(fixture.bytes, fixture.cdhash), /invalid_rc2_artifact/)
})

test('tabela de recusas da caminhada e do layout: um focal por linha com pin coerente', () => {
  // Guardas intertravadas da caminhada de load commands cobertas como grupo: cada linha
  // recusa com o conteúdo coerente consigo mesmo, e a remoção da linha inteira é detectada.
  const recusa = (fixture: ReturnType<typeof assinarArm64>, label: string) =>
    assert.throws(() => verifyRc2AdHocSignature(fixture.bytes, fixture.cdhash), /invalid_rc2_artifact/, label)
  recusa(assinarArm64(corpo, { sizeofcmdsOverride: 4 }), 'sizeofcmds abaixo do comando de assinatura')
  recusa(assinarArm64(corpo, { lcCmdsizeOverride: 0 }), 'cmdsize zero')
  recusa(assinarArm64(corpo, { lcCmdsizeOverride: 9, sizeofcmdsOverride: 9 }), 'cmdsize desalinhado')
  recusa(assinarArm64(corpo, { lcCmdsizeOverride: 24, sizeofcmdsOverride: 24 }), 'cmdsize do comando de assinatura != 16')
  recusa(assinarArm64(corpo, { sizeofcmdsOverride: 0x7fffffff }), 'sizeofcmds além do arquivo')
  {
    // o cabeçalho anuncia dois comandos e o arquivo termina dentro do segundo: a guarda limita o
    // cursor antes da leitura, em vez de depender do catch para converter RangeError.
    const anunciado = assinarArm64(corpo, { ncmdsExtra: true })
    const truncado = anunciado.bytes.subarray(0, 50)
    assert.throws(() => verifyRc2AdHocSignature(truncado, anunciado.cdhash), /invalid_rc2_artifact/, 'comando anunciado além do arquivo')
  }
  recusa(assinarArm64(corpo, { hashOffsetOverride: 0x40 }), 'hashOffset abaixo do cabeçalho do CodeDirectory')
  {
    // corpo com duas páginas reais: nCodeSlots == esperado; encolher o CodeDirectory e
    // apontar o identificador para dentro da área de hash (contém NUL) isola a guarda de ajuste.
    const fixture = assinarArm64(Buffer.alloc(8200, 7))
    const bytes = Buffer.from(fixture.bytes)
    bytes.writeUInt32BE(89, fixture.cd + 20)
    bytes.writeUInt32BE(bytes.readUInt32BE(fixture.cd + 4) - 32, fixture.cd + 4)
    assert.throws(() => verifyRc2AdHocSignature(bytes, pinDe(bytes, fixture.cd)), /invalid_rc2_artifact/, 'slots de código além do fim do CodeDirectory')
  }
})

test('tabela de recusas do SuperBlob: um focal por linha com o pin coerente', () => {
  const fixture = assinarArm64(corpo)
  // Nenhuma mutação toca o CodeDirectory: o pin segue coerente e cada recusa vem só da sua guarda.
  const recusa = (mutate: (bytes: Buffer) => void, label: string) => {
    const bytes = Buffer.from(fixture.bytes)
    mutate(bytes)
    assert.throws(() => verifyRc2AdHocSignature(bytes, fixture.cdhash), /invalid_rc2_artifact/, label)
  }
  recusa(bytes => bytes.writeUInt32BE(0xfade0cc1, fixture.dataoff), 'magic do SuperBlob')
  recusa(bytes => bytes.writeUInt32BE(0xffffffff, fixture.dataoff + 4), 'length acima dos dados')
  recusa(bytes => bytes.writeUInt32BE(0, fixture.dataoff + 8), 'count zero')
  recusa(bytes => bytes.writeUInt32BE(8, fixture.dataoff + 8), 'count acima do length')
  recusa(bytes => bytes.writeUInt32BE(4, fixture.dataoff + 20), 'offset de entrada abaixo do cabeçalho do índice')
  {
    const estranho = assinarArm64(corpo, { slots: [{ type: 0xdead }, { type: 0 }] })
    assert.throws(() => verifyRc2AdHocSignature(estranho.bytes, estranho.cdhash), /invalid_rc2_artifact/, 'tipo de slot fora da allowlist com CodeDirectory presente')
  }
  {
    const alternativo = assinarArm64(corpo, { slots: [{ type: 0x1000 }, { type: 0 }] })
    assert.throws(() => verifyRc2AdHocSignature(alternativo.bytes, alternativo.cdhash), /invalid_rc2_artifact/, 'diretório de código alternativo com CodeDirectory presente')
  }
  recusa(bytes => bytes.writeUInt32BE(0x10000, fixture.dataoff + 12), 'slot CMS apontando para fora do formato')
  recusa(bytes => bytes.writeUInt32BE(5, fixture.dataoff + 12), 'sem CodeDirectory')
  assert.throws(() => verifyRc2AdHocSignature(assinarArm64(corpo, { slots: [{ type: 0 }, { type: 0 }] }).bytes,
    assinarArm64(corpo, { slots: [{ type: 0 }, { type: 0 }] }).cdhash), /invalid_rc2_artifact/, 'CodeDirectory duplicado')
})

test('tabela de recusas do CodeDirectory: um focal por linha com o pin recalculado', () => {
  const fixture = assinarArm64(corpo)
  // Cada mutação altera o CodeDirectory: o pin é recalculado sobre o conteúdo mutado, então
  // a recusa só pode vir da guarda estrutural correspondente.
  const recusa = (mutate: (bytes: Buffer) => void, label: string) => {
    const bytes = Buffer.from(fixture.bytes)
    mutate(bytes)
    assert.throws(() => verifyRc2AdHocSignature(bytes, pinDe(bytes, fixture.cd)), /invalid_rc2_artifact/, label)
  }
  recusa(bytes => bytes.writeUInt32BE(0xfade0c03, fixture.cd), 'magic do CodeDirectory')
  recusa(bytes => bytes.writeUInt32BE(0x58, fixture.cd + 4), 'length curto')
  recusa(bytes => bytes.writeUInt32BE(0xffff, fixture.cd + 4), 'length acima do SuperBlob')
  recusa(bytes => bytes.writeUInt32BE(0x20300, fixture.cd + 8), 'versão antiga')
  recusa(bytes => bytes.writeUInt32BE(0, fixture.cd + 12), 'sem flag ad hoc')
  recusa(bytes => bytes.writeUInt32BE(0x40, fixture.cd + 48), 'teamOffset presente')
  recusa(bytes => bytes.writeUInt32BE(0x40, fixture.cd + 44), 'scatterOffset presente')
  recusa(bytes => bytes.writeUInt8(1, fixture.cd + 37), 'hashType sha1')
  recusa(bytes => bytes.writeUInt8(20, fixture.cd + 36), 'hashSize 20')
  recusa(bytes => bytes.writeUInt8(0, fixture.cd + 39), 'pageSize 0')
  recusa(bytes => bytes.writeUInt8(13, fixture.cd + 39), 'pageSize 13')
  recusa(bytes => bytes.writeUInt32BE(0, fixture.cd + 28), 'nCodeSlots zero')
  recusa(bytes => { bytes.writeUInt32BE(0x58, fixture.cd + 16); bytes.writeUInt32BE(3, fixture.cd + 24) }, 'slots especiais acima do hashOffset')
  recusa(bytes => bytes.writeUInt32BE(0, fixture.cd + 20), 'identificador ausente')
  recusa(bytes => bytes.writeUInt32BE(fixture.bytes.length, fixture.cd + 20), 'identificador fora do CodeDirectory')
  recusa(bytes => { bytes[bytes.readUInt32BE(fixture.cd + 4) + fixture.cd - 1] = 0x58 }, 'identificador sem terminador NUL')
  recusa(bytes => bytes.writeBigUInt64BE(0xffffn, fixture.cd + 56), 'codeLimit64 divergente')
  recusa(bytes => { bytes[fixture.cd + fixture.hashOffset + 3] ^= 0xff }, 'hash de página divergente no slot')
  assert.throws(() => verifyRc2AdHocSignature(fixture.bytes, '00'.repeat(20)), /invalid_rc2_artifact/, 'pin divergente com bytes intactos')
})

test('codeLimit menor que o dataoff recusa só pela guarda de cobertura, com pin coerente', () => {
  const fixture = assinarArm64('corpo com codeLimit encurtado para isolar a guarda', { codeLimitOverride: 48 })
  const bytes = fixture.bytes
  assert.throws(() => verifyRc2AdHocSignature(bytes, pinDe(bytes, fixture.cd)), /invalid_rc2_artifact/)
})

test('nCodeSlots além do corpo recusa só pela guarda de contagem, com pin coerente', () => {
  const fixture = assinarArm64('corpo com slot de pagina alem do fim', { codeSlotsOverride: 2 })
  const bytes = fixture.bytes
  assert.throws(() => verifyRc2AdHocSignature(bytes, pinDe(bytes, fixture.cd)), /invalid_rc2_artifact/)
})

test('CMS no limite: blob válido começando na emenda do SuperBlob recusa pelo limite', () => {
  const fixture = assinarArm64(corpo, { formatoReal: true })
  const bytes = Buffer.concat([fixture.bytes, Buffer.alloc(16)])
  const superLength = bytes.readUInt32BE(fixture.dataoff + 4)
  // a entrada do CMS passa a apontar para os últimos 4 bytes do SuperBlob, com um BlobWrapper
  // válido na emenda: sem a guarda de limite, o parser aceitaria o blob fora do SuperBlob.
  bytes.writeUInt32BE(superLength - 4, fixture.dataoff + 12 + 2 * 8 + 4)
  bytes.writeUInt32BE(0xfade0b01, fixture.dataoff + superLength - 4)
  bytes.writeUInt32BE(8, fixture.dataoff + superLength)
  assert.throws(() => verifyRc2AdHocSignature(bytes, fixture.cdhash), /invalid_rc2_artifact/)
})

test('CD enxertado de outro binário com pin correto é recusado pelos hashes de página', () => {
  const original = assinarArm64('corpo-A-do-hub-com-tamanho-igual')
  const outro = assinarArm64('corpo-B-do-hub-com-tamanho-igual')
  const enxertado = Buffer.concat([outro.bytes.subarray(0, outro.dataoff), original.bytes.subarray(original.dataoff)])
  assert.throws(() => verifyRc2AdHocSignature(enxertado, original.cdhash), /invalid_rc2_artifact/)
})

test('byte trocado no corpo recusa mesmo quando só o hash do arquivo seria recalculado', () => {
  const fixture = assinarArm64(corpo)
  const bytes = Buffer.from(fixture.bytes)
  bytes[48] ^= 0xff
  assert.notEqual(sha256(bytes).toString('hex'), fixture.sha256)
  assert.throws(() => verifyRc2AdHocSignature(bytes, pinDe(bytes, fixture.cd)), /invalid_rc2_artifact/)
})

test('vetor ad hoc gerado pelo codesign do macOS confirma cdhash medido e hashes de página', async t => {
  if (process.platform !== 'darwin') return t.skip('fora do macOS: o vetor só tem valor em runner com toolchain')
  const dir = mkdtempSync(join(realpathSync(tmpdir()), 'rc2-codesign-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  writeFileSync(join(dir, 'main.c'), 'int main(void) { return 0; }\n')
  const vetor = join(dir, 'vetor-adhoc')
  try {
    execFileSync('cc', ['-arch', 'arm64', '-o', vetor, join(dir, 'main.c')], { cwd: dir, timeout: 120000 })
    execFileSync('codesign', ['--force', '--sign', '-', vetor], { timeout: 60000 })
  } catch { return t.skip('sem toolchain arm64 neste ambiente') }
  // O codesign escreve a exibição em stderr; stdout fica vazio.
  const display = spawnSync('codesign', ['-dvv', vetor], { timeout: 60000 })
  assert.equal(display.error, undefined)
  assert.equal(display.status, 0)
  const reported = /CDHash=([0-9a-f]{40})/.exec(display.stderr.toString('utf8'))
  assert.ok(reported, 'saída do codesign sem CDHash')
  verifyRc2AdHocSignature(readFileSync(vetor), reported![1])
})

test('toda forma de truncamento recusa com o erro estável do contrato, sem exceção estranha', () => {
  const formatos = [
    assinarArm64(corpo),
    assinarArm64(corpo, { formatoReal: true }),
    assinarArm64(Buffer.alloc(8200, 7)),
    assinarArm64(corpo, { duplicateCodeSignature: true }),
    assinarArm64(corpo, { ncmdsExtra: true }),
  ]
  for (const fixture of formatos) {
    for (let length = 0; length < fixture.bytes.length; length++) {
      const truncated = fixture.bytes.subarray(0, length)
      assert.throws(() => verifyRc2AdHocSignature(truncated, fixture.cdhash), /invalid_rc2_artifact/)
    }
  }
})
