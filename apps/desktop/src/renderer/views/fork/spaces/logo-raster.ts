// MX-79: o logo do Space segue ao Hub como data URL; o raster é reduzido a no máx. 256 px de lado
// antes de virar texto — um PNG de vários MB não precisa chegar inteiro. SVG não tem dimensões
// fixas e continua passando direto pelo FileReader do picker.
import { LOGO_MAX_SIDE } from './space-tile.ts'

const PNG_SIG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]

/**
 * APNG (PNG animado) tem o chunk `acTL` antes do primeiro `IDAT`. Bitmap/canvas só
 * entregam o 1º quadro — reenviar os bytes originais é o único caminho sem perder a animação.
 */
async function isAnimatedPNG(file: Blob): Promise<boolean> {
  const buf = new Uint8Array(await file.arrayBuffer())
  if (buf.length < 8 || PNG_SIG.some((b, i) => buf[i] !== b)) return false
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength)
  for (let off = 8; off + 8 <= buf.length;) {
    const len = view.getUint32(off)
    const type = String.fromCharCode(buf[off + 4]!, buf[off + 5]!, buf[off + 6]!, buf[off + 7]!)
    if (type === 'acTL') return true
    if (type === 'IDAT') return false
    off += 12 + len
  }
  return false
}

const blobDataURL = (blob: Blob): Promise<string> => new Promise((resolve, reject) => {
  const reader = new FileReader()
  reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : '')
  reader.onerror = () => reject(reader.error)
  reader.readAsDataURL(blob)
})

/** PNG que já cabe em LOGO_MAX_SIDE volta com os bytes originais; acima disso, desenha reduzido e reencoda em PNG. APNG sempre volta intacto. */
export async function logoRasterDataURL(file: Blob): Promise<string> {
  if (await isAnimatedPNG(file)) return blobDataURL(file)
  const bitmap = await createImageBitmap(file)
  try {
    const maior = Math.max(bitmap.width, bitmap.height)
    if (maior <= LOGO_MAX_SIDE) return await blobDataURL(file)
    const escala = LOGO_MAX_SIDE / maior
    const canvas = document.createElement('canvas')
    canvas.width = Math.max(1, Math.round(bitmap.width * escala))
    canvas.height = Math.max(1, Math.round(bitmap.height * escala))
    const ctx = canvas.getContext('2d')
    if (!ctx) throw new Error('logo-raster:2d')
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
    const png = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
    if (!png) throw new Error('logo-raster:png')
    return await blobDataURL(png)
  } finally {
    bitmap.close()
  }
}
