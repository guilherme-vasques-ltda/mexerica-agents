import { isDrawable, withinPixelLimit, type BrowserFrame } from '../../../../shared/relay-protocol/browser-frames.ts'

interface Bitmap {
  width: number
  height: number
  close(): void
}

interface Tela {
  width: number
  height: number
  getContext(tipo: '2d'): { drawImage(imagem: never, x: number, y: number): void } | null
}

/**
 * Desenha o JPEG no canvas e devolve se deu certo. O quadro nunca vira `<img>`, URL ou texto.
 * Antes de decodificar confere o tamanho real da imagem (cabeçalho e marcador do próprio JPEG); o bitmap
 * sempre é fechado, também em erro; e um desenho que terminou depois de a tela ser desfeita não toca o canvas.
 */
export async function desenhar(
  canvas: Tela,
  frame: BrowserFrame,
  aindaVale: () => boolean = () => true,
  criar: (blob: Blob) => Promise<Bitmap> = (blob) => createImageBitmap(blob),
): Promise<boolean> {
  if (!isDrawable(frame)) return false
  let bitmap: Bitmap | null = null
  try {
    bitmap = await criar(new Blob([frame.jpeg as BlobPart], { type: 'image/jpeg' }))
    if (!aindaVale() || !withinPixelLimit(bitmap.width, bitmap.height)) return false
    const contexto = canvas.getContext('2d')
    if (!contexto) return false
    canvas.width = bitmap.width
    canvas.height = bitmap.height
    contexto.drawImage(bitmap as never, 0, 0)
    return true
  } catch {
    return false
  } finally {
    bitmap?.close()
  }
}
