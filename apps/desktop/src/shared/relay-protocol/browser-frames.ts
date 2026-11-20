/**
 * Quadro binário do navegador do bot (`MXJ1`), espelho do codec do Hub
 * (`host/src/fork/browser/stream/codec.rs`). Viaja cifrado dentro dos frames `f` do canal
 * (docs/reference/remote-relay.md, "Quadros binários do navegador"); nunca vira JSON.
 *
 * Isomórfico e estrito: tamanho conferido antes de copiar o JPEG, UTF-8 válido, dimensões não
 * nulas, marcadores de início e fim do JPEG e nenhum byte excedente. Qualquer desvio é recusa.
 */

export const FRAME_MAGIC = [0x4d, 0x58, 0x4a, 0x31] as const
export const FRAME_HEADER = 44
export const FRAME_MAX_JPEG = 512 * 1024
export const FRAME_MAX_ID = 256
/** Maior quadro codificado completo (cabeçalho, identificadores e JPEG). */
export const FRAME_MAX_WIRE = FRAME_HEADER + 2 * FRAME_MAX_ID + FRAME_MAX_JPEG
/** Capacidade negociada no `hello`/`welcome`; sem ela o Hub nunca manda quadro. */
export const FRAMES_CAPABILITY = 'browserFramesV1'
/** Teto da imagem decodificada (RGBA ≈ 4 bytes por pixel): o JPEG comprimido pequeno não limita a memória do bitmap. */
export const FRAME_MAX_SIDE = 8192
export const FRAME_MAX_PIXELS = 16 * 1024 * 1024

export interface FrameSize {
  width: number
  height: number
}

export interface BrowserFrame {
  session: string
  target: string
  sequence: number
  targetGeneration: number
  /** Tamanho da imagem JPEG, em pixels. */
  image: FrameSize
  /** Tamanho da viewport da página, em pixels CSS. */
  viewport: FrameSize
  jpeg: Uint8Array
}

export type FrameError = 'invalid' | 'tooLarge'

export class FrameCodecError extends Error {
  readonly kind: FrameError
  constructor(kind: FrameError) {
    super(kind === 'tooLarge' ? 'frame.tooLarge' : 'frame.invalid')
    this.kind = kind
  }
}

const utf8 = new TextDecoder('utf-8', { fatal: true })

/** Os quatro primeiros bytes são `MXJ1`: decide, sem copiar, se a mensagem interna é um quadro. */
export function isFrameBytes(bytes: Uint8Array): boolean {
  return bytes.length >= FRAME_MAGIC.length && FRAME_MAGIC.every((byte, i) => bytes[i] === byte)
}

const validJpeg = (jpeg: Uint8Array) => jpeg.length >= 4 && jpeg[0] === 0xff && jpeg[1] === 0xd8 && jpeg[jpeg.length - 2] === 0xff && jpeg[jpeg.length - 1] === 0xd9

export const withinPixelLimit = (width: number, height: number) =>
  Number.isInteger(width) && Number.isInteger(height) && width > 0 && height > 0 && width <= FRAME_MAX_SIDE && height <= FRAME_MAX_SIDE && width * height <= FRAME_MAX_PIXELS

/** Dimensões reais lidas do marcador SOF do JPEG, sem decodificar; `null` se não houver um antes dos dados da imagem. */
export function jpegSize(jpeg: Uint8Array): FrameSize | null {
  let i = 2
  while (i + 3 < jpeg.length) {
    if (jpeg[i] !== 0xff) return null
    const marker = jpeg[i + 1]!
    if (marker === 0xff) {
      i += 1
      continue
    }
    if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd8)) {
      i += 2
      continue
    }
    if (marker === 0xd9 || marker === 0xda) return null
    const length = (jpeg[i + 2]! << 8) | jpeg[i + 3]!
    if (length < 2) return null
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
      if (length < 8 || i + 9 > jpeg.length) return null
      return { height: (jpeg[i + 5]! << 8) | jpeg[i + 6]!, width: (jpeg[i + 7]! << 8) | jpeg[i + 8]! }
    }
    i += 2 + length
  }
  return null
}

/** O quadro cabe no teto de pixels pelo cabeçalho e pelo próprio JPEG, antes de qualquer decodificação. */
export function isDrawable(frame: BrowserFrame): boolean {
  const real = jpegSize(frame.jpeg)
  return real !== null && withinPixelLimit(real.width, real.height) && withinPixelLimit(frame.image.width, frame.image.height)
}

/** Decodifica um quadro; lança `FrameCodecError`. O JPEG devolvido é uma cópia própria (nunca uma visão do buffer do canal). */
export function decodeFrame(bytes: Uint8Array): BrowserFrame {
  if (bytes.length > FRAME_MAX_WIRE) throw new FrameCodecError('tooLarge')
  if (bytes.length < FRAME_HEADER || !isFrameBytes(bytes)) throw new FrameCodecError('invalid')
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const sessionLen = view.getUint16(4)
  const targetLen = view.getUint16(6)
  const jpegLen = view.getUint32(8)
  if (sessionLen > FRAME_MAX_ID || targetLen > FRAME_MAX_ID || jpegLen > FRAME_MAX_JPEG) throw new FrameCodecError('tooLarge')
  if (sessionLen === 0 || targetLen === 0 || bytes.length !== FRAME_HEADER + sessionLen + targetLen + jpegLen) throw new FrameCodecError('invalid')
  const sequence = view.getBigUint64(12)
  const targetGeneration = view.getBigUint64(20)
  if (sequence > BigInt(Number.MAX_SAFE_INTEGER) || targetGeneration > BigInt(Number.MAX_SAFE_INTEGER)) throw new FrameCodecError('invalid')
  const sessionEnd = FRAME_HEADER + sessionLen
  const targetEnd = sessionEnd + targetLen
  let session: string
  let target: string
  try {
    session = utf8.decode(bytes.subarray(FRAME_HEADER, sessionEnd))
    target = utf8.decode(bytes.subarray(sessionEnd, targetEnd))
  } catch {
    throw new FrameCodecError('invalid')
  }
  const jpeg = bytes.slice(targetEnd)
  const image = { width: view.getUint32(28), height: view.getUint32(32) }
  const viewport = { width: view.getUint32(36), height: view.getUint32(40) }
  if (!validJpeg(jpeg) || [image.width, image.height, viewport.width, viewport.height].includes(0)) throw new FrameCodecError('invalid')
  return { session, target, sequence: Number(sequence), targetGeneration: Number(targetGeneration), image, viewport, jpeg }
}

/** Só para testes e fixtures: o app nunca emite quadro, só o Hub. */
export function encodeFrame(frame: BrowserFrame): Uint8Array {
  const enc = new TextEncoder()
  const session = enc.encode(frame.session)
  const target = enc.encode(frame.target)
  if (frame.jpeg.length > FRAME_MAX_JPEG || session.length > FRAME_MAX_ID || target.length > FRAME_MAX_ID) throw new FrameCodecError('tooLarge')
  if (!session.length || !target.length || !validJpeg(frame.jpeg) || [frame.image.width, frame.image.height, frame.viewport.width, frame.viewport.height].includes(0)) throw new FrameCodecError('invalid')
  const out = new Uint8Array(FRAME_HEADER + session.length + target.length + frame.jpeg.length)
  const view = new DataView(out.buffer)
  out.set(FRAME_MAGIC, 0)
  view.setUint16(4, session.length)
  view.setUint16(6, target.length)
  view.setUint32(8, frame.jpeg.length)
  view.setBigUint64(12, BigInt(frame.sequence))
  view.setBigUint64(20, BigInt(frame.targetGeneration))
  view.setUint32(28, frame.image.width)
  view.setUint32(32, frame.image.height)
  view.setUint32(36, frame.viewport.width)
  view.setUint32(40, frame.viewport.height)
  out.set(session, FRAME_HEADER)
  out.set(target, FRAME_HEADER + session.length)
  out.set(frame.jpeg, FRAME_HEADER + session.length + target.length)
  return out
}
