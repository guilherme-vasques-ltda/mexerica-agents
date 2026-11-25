import { createHash } from 'node:crypto'

const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest()

export interface Rc2Assinatura {
  bytes: Buffer
  sha256: string
  cdhash: string
  dataoff: number
  cd: number
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
  body.writeUInt32LE(options.duplicateCodeSignature ? 2 : 1, 16)
  body.writeUInt32LE(options.duplicateCodeSignature ? 32 : 16, 20)
  body.writeUInt32LE(0x1d, 32)
  body.writeUInt32LE(16, 36)
  if (options.duplicateCodeSignature) {
    body.writeUInt32LE(0x1d, 48)
    body.writeUInt32LE(16, 52)
  }
  const codeLimit = body.length
  const codeSlots = Math.ceil(codeLimit / (1 << pageSize))
  const hashOffset = 0x58
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
  const dataoff = codeLimit
  const slots = options.slots ?? [{ type: 0 }]
  const superblob = Buffer.alloc(12 + slots.length * 8 + length)
  superblob.writeUInt32BE(0xfade0cc0, 0)
  superblob.writeUInt32BE(superblob.length, 4)
  superblob.writeUInt32BE(slots.length, 8)
  slots.forEach((slot, index) => {
    superblob.writeUInt32BE(slot.type, 12 + index * 8)
    superblob.writeUInt32BE(20 + (slot.offset ?? 0), 12 + index * 8 + 4)
  })
  const datasize = superblob.length
  body.writeUInt32LE(dataoff, 40)
  body.writeUInt32LE(datasize, 44)
  for (let index = 0; index < codeSlots; index++) {
    const page = Buffer.alloc(1 << pageSize)
    body.copy(page, 0, index * (1 << pageSize), Math.min((index + 1) * (1 << pageSize), codeLimit))
    sha256(page).copy(cd, hashOffset + index * hashSize)
  }
  cd.copy(superblob, 20)
  const bytes = Buffer.concat([body, superblob])
  return {
    bytes,
    sha256: sha256(bytes).toString('hex'),
    cdhash: sha256(cd).subarray(0, 20).toString('hex'),
    dataoff,
    cd: dataoff + 12 + slots.length * 8,
    hashOffset,
    pageSize,
  }
}

export const sha256Hex = (bytes: Buffer) => sha256(bytes).toString('hex')
