import { createHash } from 'node:crypto'

const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest()

export interface Rc2Assinatura {
  bytes: Buffer
  sha256: string
  cdhash: string
  dataoff: number
  cd: number
  cms: number
  hashOffset: number
  pageSize: number
}

interface Opcoes {
  pageSize?: number
  flags?: number
  hashType?: number
  hashSize?: number
  teamOffset?: number
  ident?: string
  duplicateCodeSignature?: boolean
  slots?: Array<{ type: number, offset?: number }>
  // Overrides autoconsistentes para focais que isolam guardas: assinatura coerente consigo
  // mesma, mas fora do contrato (codeLimit menor que o dataoff, slots além do corpo).
  codeLimitOverride?: number
  codeSlotsOverride?: number
  // Reproduz o formato que o codesign ad hoc real embute: requirements vazio e BlobWrapper
  // CMS vazio, além do CodeDirectory.
  formatoReal?: boolean
  // Overrides para focais que isolam guardas de caminhada e layout, coerentes consigo mesmos.
  sizeofcmdsOverride?: number
  lcCmdsizeOverride?: number
  hashOffsetOverride?: number
  cputypeOverride?: number
  // Anuncia um segundo comando de load que o arquivo não contém.
  ncmdsExtra?: boolean
}

// Constrói um Mach-O ARM64 com assinatura ad hoc embutida estruturalmente válida
// (SuperBlob → CodeDirectory sha256 → hashes de página) e devolve o cdhash efetivo
// calculado de forma independente do verificador de produção.
export function assinarArm64(payload: Buffer | string, options: Opcoes = {}): Rc2Assinatura {
  const pageSize = options.pageSize ?? 12
  const hashSize = options.hashSize ?? 32
  const flags = options.flags ?? 0x00000002
  const ident = Buffer.from(`${options.ident ?? 'fixture.hub.invalid.example'}\0`)
  const body = Buffer.concat([
    Buffer.from([0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0x00, 0x00, 0x01]), Buffer.alloc(24),
    Buffer.alloc(options.duplicateCodeSignature ? 32 : 16), Buffer.from(payload),
  ])
  if (options.cputypeOverride) body.writeUInt32LE(options.cputypeOverride, 4)
  body.writeUInt32LE(options.duplicateCodeSignature || options.ncmdsExtra ? 2 : 1, 16)
  body.writeUInt32LE(options.sizeofcmdsOverride ?? (options.duplicateCodeSignature ? 32 : options.lcCmdsizeOverride ?? 16), 20)
  body.writeUInt32LE(0x1d, 32)
  body.writeUInt32LE(options.lcCmdsizeOverride ?? 16, 36)
  if (options.duplicateCodeSignature) {
    body.writeUInt32LE(0x1d, 48)
    body.writeUInt32LE(16, 52)
  }
  const dataoff = body.length
  const codeLimit = options.codeLimitOverride ?? dataoff
  const codeSlots = options.codeSlotsOverride ?? Math.ceil(codeLimit / (1 << pageSize))
  const hashOffset = options.hashOffsetOverride ?? 0x58
  const length = hashOffset + codeSlots * hashSize + ident.length
  const cd = Buffer.alloc(length)
  cd.writeUInt32BE(0xfade0c02, 0)
  cd.writeUInt32BE(length, 4)
  cd.writeUInt32BE(0x20400, 8)
  cd.writeUInt32BE(flags, 12)
  cd.writeUInt32BE(hashOffset, 16)
  cd.writeUInt32BE(hashOffset + codeSlots * hashSize, 20)
  cd.writeUInt32BE(0, 24)
  cd.writeUInt32BE(codeSlots, 28)
  cd.writeUInt32BE(codeLimit, 32)
  cd.writeUInt8(hashSize, 36)
  cd.writeUInt8(options.hashType ?? 2, 37)
  cd.writeUInt8(0, 38)
  cd.writeUInt8(pageSize, 39)
  if (options.teamOffset) cd.writeUInt32BE(options.teamOffset, 48)
  ident.copy(cd, hashOffset + codeSlots * hashSize)
  const requisito = Buffer.alloc(12)
  requisito.writeUInt32BE(0xfade0c01, 0)
  requisito.writeUInt32BE(12, 4)
  const cmsVazio = Buffer.alloc(8)
  cmsVazio.writeUInt32BE(0xfade0b01, 0)
  cmsVazio.writeUInt32BE(8, 4)
  const slots = options.slots ?? (options.formatoReal
    ? [{ type: 0 }, { type: 2, offset: length }, { type: 0x10000, offset: length + requisito.length }]
    : [{ type: 0 }])
  const extra = options.formatoReal ? requisito.length + cmsVazio.length : 0
  const superblob = Buffer.alloc(12 + slots.length * 8 + length + extra)
  superblob.writeUInt32BE(0xfade0cc0, 0)
  superblob.writeUInt32BE(superblob.length, 4)
  superblob.writeUInt32BE(slots.length, 8)
  slots.forEach((slot, index) => {
    superblob.writeUInt32BE(slot.type, 12 + index * 8)
    superblob.writeUInt32BE(12 + slots.length * 8 + (slot.offset ?? 0), 12 + index * 8 + 4)
  })
  const datasize = superblob.length
  body.writeUInt32LE(dataoff, 40)
  body.writeUInt32LE(datasize, 44)
  if (options.duplicateCodeSignature) {
    body.writeUInt32LE(dataoff, 56)
    body.writeUInt32LE(datasize, 60)
  }
  for (let index = 0; index < codeSlots; index++) {
    sha256(body.subarray(index * (1 << pageSize), Math.min((index + 1) * (1 << pageSize), codeLimit)))
      .copy(cd, hashOffset + index * hashSize)
  }
  cd.copy(superblob, 12 + slots.length * 8)
  if (options.formatoReal) {
    requisito.copy(superblob, 12 + slots.length * 8 + length)
    cmsVazio.copy(superblob, 12 + slots.length * 8 + length + requisito.length)
  }
  const bytes = Buffer.concat([body, superblob])
  return {
    bytes,
    sha256: sha256(bytes).toString('hex'),
    cdhash: sha256(cd).subarray(0, 20).toString('hex'),
    dataoff,
    cd: dataoff + 12 + slots.length * 8,
    cms: dataoff + 12 + slots.length * 8 + length + requisito.length,
    hashOffset,
    pageSize,
  }
}

export const sha256Hex = (bytes: Buffer) => sha256(bytes).toString('hex')
