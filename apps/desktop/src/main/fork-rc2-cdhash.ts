import { createHash } from 'node:crypto'

const invalid = () => new Error('invalid_rc2_artifact')

const MH_MAGIC_64 = 0xfeedfacf
const CPU_TYPE_ARM64 = 0x0100000c
const LC_CODE_SIGNATURE = 0x1d
const CSMAGIC_EMBEDDED_SIGNATURE = 0xfade0cc0
const CSMAGIC_CODEDIRECTORY = 0xfade0c02
const CSSLOT_CODEDIRECTORY = 0
const CSSLOT_REQUIREMENTS = 2
const CSSLOT_ENTITLEMENTS = 5
const CSSLOT_DER_ENTITLEMENTS = 7
const CSSLOT_ALTERNATE_CODEDIRECTORIES = 0x1000
const CSSLOT_SIGNATURESLOT = 0x10000
const CS_ADHOC = 0x00000002
const CSHASH_SHA256 = 2
const CODE_SIGNATURE_HEADER = 0x58

const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest()

// Mede o cdhash efetivo do binário assinado ad hoc (SHA-256 do CodeDirectory truncado em 20
// bytes, como o kernel) e o confronta com o pin do manifesto. Fail-closed: qualquer
// estrutura fora do contrato da candidata interna recusa antes de comparar o pin.
export function verifyRc2AdHocSignature(bytes: Buffer, cdhash: string): void {
  if (bytes.length < 32 || bytes.readUInt32LE(0) !== MH_MAGIC_64 || bytes.readUInt32LE(4) !== CPU_TYPE_ARM64) throw invalid()
  const sizeofcmds = bytes.readUInt32LE(20)
  if (sizeofcmds === 0 || sizeofcmds > bytes.length - 32) throw invalid()
  let signature: { dataoff: number, datasize: number } | null = null
  let cursor = 32
  for (let remaining = bytes.readUInt32LE(16); remaining > 0; remaining--) {
    if (cursor + 8 > 32 + sizeofcmds) throw invalid()
    const cmd = bytes.readUInt32LE(cursor)
    const cmdsize = bytes.readUInt32LE(cursor + 4)
    if (cmdsize < 8 || cmdsize % 8 !== 0 || cursor + cmdsize > 32 + sizeofcmds) throw invalid()
    if (cmd === LC_CODE_SIGNATURE) {
      if (signature) throw invalid()
      if (cmdsize !== 16) throw invalid()
      signature = { dataoff: bytes.readUInt32LE(cursor + 8), datasize: bytes.readUInt32LE(cursor + 12) }
    }
    cursor += cmdsize
  }
  if (!signature) throw invalid()
  const { dataoff, datasize } = signature
  if (dataoff < cursor || datasize < 12 || dataoff > bytes.length - datasize) throw invalid()
  if (bytes.readUInt32BE(dataoff) !== CSMAGIC_EMBEDDED_SIGNATURE) throw invalid()
  const superLength = bytes.readUInt32BE(dataoff + 4)
  const count = bytes.readUInt32BE(dataoff + 8)
  if (superLength < 12 || superLength > datasize || count === 0 || count >= 0x10000 || 12 + count * 8 > superLength) throw invalid()
  let directory = -1
  for (let index = 0; index < count; index++) {
    const type = bytes.readUInt32BE(dataoff + 12 + index * 8)
    const offset = bytes.readUInt32BE(dataoff + 12 + index * 8 + 4)
    if (offset < 12 || dataoff + offset > dataoff + superLength) throw invalid()
    if (type === CSSLOT_CODEDIRECTORY) {
      if (directory !== -1) throw invalid()
      directory = offset
    } else if ((type >= CSSLOT_ALTERNATE_CODEDIRECTORIES && type < CSSLOT_SIGNATURESLOT) || type === CSSLOT_SIGNATURESLOT) {
      throw invalid()
    } else if (type !== CSSLOT_REQUIREMENTS && type !== CSSLOT_ENTITLEMENTS && type !== CSSLOT_DER_ENTITLEMENTS) {
      throw invalid()
    }
  }
  if (directory === -1) throw invalid()
  const cd = dataoff + directory
  if (cd + CODE_SIGNATURE_HEADER > dataoff + superLength) throw invalid()
  if (bytes.readUInt32BE(cd) !== CSMAGIC_CODEDIRECTORY) throw invalid()
  const length = bytes.readUInt32BE(cd + 4)
  if (length < CODE_SIGNATURE_HEADER || cd + length > dataoff + superLength) throw invalid()
  const version = bytes.readUInt32BE(cd + 8)
  const flags = bytes.readUInt32BE(cd + 12)
  const hashOffset = bytes.readUInt32BE(cd + 16)
  const identOffset = bytes.readUInt32BE(cd + 20)
  const nSpecialSlots = bytes.readUInt32BE(cd + 24)
  const nCodeSlots = bytes.readUInt32BE(cd + 28)
  const codeLimit = bytes.readUInt32BE(cd + 32)
  const hashSize = bytes.readUInt8(cd + 36)
  const hashType = bytes.readUInt8(cd + 37)
  const pageSize = bytes.readUInt8(cd + 39)
  const scatterOffset = bytes.readUInt32BE(cd + 44)
  const teamOffset = bytes.readUInt32BE(cd + 48)
  const codeLimit64 = bytes.readBigUInt64BE(cd + 56)
  if (version < 0x20400 || (flags & CS_ADHOC) === 0 || teamOffset !== 0 || scatterOffset !== 0) throw invalid()
  if (hashSize !== 32 || hashType !== CSHASH_SHA256 || (pageSize !== 12 && pageSize !== 14)) throw invalid()
  if (codeLimit !== dataoff || codeLimit > bytes.length) throw invalid()
  if (codeLimit64 !== 0n && codeLimit64 !== BigInt(codeLimit)) throw invalid()
  const page = 1 << pageSize
  const expected = Math.ceil(codeLimit / page)
  if (nCodeSlots === 0 || nCodeSlots !== expected) throw invalid()
  if (hashOffset < CODE_SIGNATURE_HEADER || hashOffset / hashSize < nSpecialSlots ||
      (length - hashOffset) / hashSize < nCodeSlots) throw invalid()
  if (identOffset === 0 || identOffset >= length || !bytes.subarray(cd + identOffset, cd + length).includes(0)) throw invalid()
  const stored = bytes.subarray(cd + hashOffset, cd + hashOffset + nCodeSlots * hashSize)
  // A última página parcial é hasheada só nos bytes presentes, sem preenchimento: é o que o
  // kernel confere por página (XNU ubc_subr.c); preencher com zeros recusa assinatura válida.
  for (let index = 0; index < nCodeSlots; index++) {
    const start = index * page
    const end = Math.min(start + page, codeLimit)
    if (!sha256(bytes.subarray(start, end)).equals(stored.subarray(index * hashSize, (index + 1) * hashSize))) throw invalid()
  }
  const measured = sha256(bytes.subarray(cd, cd + length)).subarray(0, 20).toString('hex')
  if (measured !== cdhash) throw invalid()
}
