import { test } from 'node:test'
import assert from 'node:assert/strict'
import { logoRasterDataURL } from './logo-raster.ts'
import { LOGO_MAX_SIDE } from './space-tile.ts'

// MX-79: o raster do logo do Space é reduzido a 256 px antes de virar data URL; quem já cabe
// volta com os bytes originais. O teste sobe os três globais de navegador que a função usa.
type Bitmap = { width: number; height: number; close: () => void }
const desenhos: { w: number; h: number }[] = []
let fechado = 0

const bitmapFalso = (w: number, h: number): Bitmap => ({ width: w, height: h, close: () => { fechado += 1 } })

class FileReaderFalso {
  result: string | ArrayBuffer | null = null
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  error: unknown = null
  readAsDataURL(blob: Blob) {
    void blob.arrayBuffer().then((buf) => {
      this.result = `data:${blob.type};base64,${Buffer.from(buf).toString('base64')}`
      this.onload?.()
    })
  }
}

const comNavegador = async <T>(bitmap: Bitmap, fn: () => Promise<T>): Promise<T> => {
  const antes = { cib: globalThis.createImageBitmap, doc: globalThis.document, fr: globalThis.FileReader }
  ;(globalThis as Record<string, unknown>).createImageBitmap = async () => bitmap
  ;(globalThis as Record<string, unknown>).document = {
    createElement: () => ({
      width: 0, height: 0,
      getContext: () => ({ drawImage: (_b: unknown, _x: number, _y: number, w: number, h: number) => { desenhos.push({ w, h }) } }),
      toBlob: (cb: (b: Blob | null) => void) => cb(new Blob(['png-reencodado'], { type: 'image/png' })),
    }),
  }
  ;(globalThis as Record<string, unknown>).FileReader = FileReaderFalso
  try { return await fn() } finally {
    Object.assign(globalThis, { createImageBitmap: antes.cib, document: antes.doc, FileReader: antes.fr })
  }
}

const png = () => new Blob(['pixels-do-png'], { type: 'image/png' })

test('logo maior que 256 px é desenhado reduzido e reencodado em PNG', async () => {
  desenhos.length = 0; fechado = 0
  const url = await comNavegador(bitmapFalso(1024, 512), () => logoRasterDataURL(png()))
  assert.deepEqual(desenhos, [{ w: 256, h: 128 }])
  assert.match(url, /^data:image\/png;base64,/)
  assert.equal(url.includes(Buffer.from('png-reencodado').toString('base64')), true)
  assert.equal(fechado, 1, 'o bitmap é liberado')
})

test('proporção fora de quadrado mantém a razão dentro de 256', async () => {
  desenhos.length = 0
  await comNavegador(bitmapFalso(2000, 1000), () => logoRasterDataURL(png()))
  assert.deepEqual(desenhos, [{ w: 256, h: 128 }])
  desenhos.length = 0
  await comNavegador(bitmapFalso(400, 800), () => logoRasterDataURL(png()))
  assert.deepEqual(desenhos, [{ w: 128, h: 256 }])
})

test('PNG que já cabe volta com os bytes originais, sem canvas', async () => {
  desenhos.length = 0
  const url = await comNavegador(bitmapFalso(256, LOGO_MAX_SIDE), () => logoRasterDataURL(png()))
  assert.deepEqual(desenhos, [])
  assert.equal(url, `data:image/png;base64,${Buffer.from('pixels-do-png').toString('base64')}`)
})

// MX-79: APNG animado não pode passar por bitmap/canvas (só entrega o 1º quadro) — volta intacto.
const chunkPng = (tipo: string, dados: number[]) => [0, 0, 0, dados.length, ...[...tipo].map((c) => c.charCodeAt(0)), ...dados, 0, 0, 0, 0]
const pngBytes = (comAnimacao: boolean) => new Uint8Array([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
  ...chunkPng('IHDR', new Array(13).fill(0)),
  ...(comAnimacao ? chunkPng('acTL', [0, 0, 0, 2, 0, 0, 0, 0]) : []),
  ...chunkPng('IDAT', [0x78, 0x9c]),
])
const blobPng = (bytes: Uint8Array) => new Blob([bytes.buffer as ArrayBuffer], { type: 'image/png' })

test('APNG volta com os bytes originais, sem decodificar nem canvas', async () => {
  const antes = { fr: globalThis.FileReader, cib: globalThis.createImageBitmap }
  ;(globalThis as Record<string, unknown>).FileReader = FileReaderFalso
  let bitmapChamado = 0
  ;(globalThis as Record<string, unknown>).createImageBitmap = async () => { bitmapChamado += 1; throw new Error('APNG não pode virar bitmap') }
  try {
    const bytes = pngBytes(true)
    const url = await logoRasterDataURL(blobPng(bytes))
    assert.equal(bitmapChamado, 0)
    assert.equal(url, `data:image/png;base64,${Buffer.from(bytes).toString('base64')}`)
  } finally {
    Object.assign(globalThis, { FileReader: antes.fr, createImageBitmap: antes.cib })
  }
})

test('PNG estático com a mesma estrutura de chunks segue o fluxo normal', async () => {
  desenhos.length = 0
  const url = await comNavegador(bitmapFalso(1024, 512), () => logoRasterDataURL(blobPng(pngBytes(false))))
  assert.deepEqual(desenhos, [{ w: 256, h: 128 }])
  assert.match(url, /^data:image\/png;base64,/)
})
