import test from 'node:test'
import assert from 'node:assert/strict'
import { jpegSintetico } from './browser-frames-fixtures.ts'
import { decodeFrame, encodeFrame, FRAME_HEADER, FRAME_MAX_PIXELS, FRAME_MAX_SIDE, isDrawable, jpegSize, withinPixelLimit, FRAME_MAX_JPEG, FRAME_MAX_WIRE, FrameCodecError, isFrameBytes, type BrowserFrame } from './browser-frames.ts'

// Espelho do codec do Hub (host/src/fork/browser/stream/codec.rs): formato `MXJ1`, tudo em ordem de rede.

const quadro = (): BrowserFrame => ({
  session: 'sessão',
  target: 'aba',
  sequence: 7,
  targetGeneration: 3,
  image: { width: 960, height: 540 },
  viewport: { width: 1280, height: 720 },
  jpeg: Uint8Array.from([0xff, 0xd8, 1, 0xff, 0xd9]),
})

const kind = (fn: () => unknown) => {
  try {
    fn()
  } catch (error) {
    return error instanceof FrameCodecError ? error.kind : 'outro'
  }
  return 'nada'
}

test('o cabeçalho segue o formato do Hub, byte a byte', () => {
  const wire = encodeFrame(quadro())
  const session = new TextEncoder().encode('sessão')
  assert.deepEqual([...wire.subarray(0, 4)], [0x4d, 0x58, 0x4a, 0x31])
  const view = new DataView(wire.buffer)
  assert.equal(view.getUint16(4), session.length)
  assert.equal(view.getUint16(6), 3)
  assert.equal(view.getUint32(8), 5)
  assert.equal(view.getBigUint64(12), 7n)
  assert.equal(view.getBigUint64(20), 3n)
  assert.deepEqual([28, 32, 36, 40].map((at) => view.getUint32(at)), [960, 540, 1280, 720])
  assert.equal(wire.length, FRAME_HEADER + session.length + 3 + 5)
  assert.ok(isFrameBytes(wire))
  assert.deepEqual(decodeFrame(wire), quadro())
})

test('identificadores com U+FEFF no começo voltam iguais: o BOM não é removido', () => {
  const original = { ...quadro(), session: '\uFEFFsessão', target: '\uFEFF' }
  const frame = decodeFrame(encodeFrame(original))
  assert.equal(frame.session, original.session)
  assert.equal(frame.target, original.target)
  assert.equal(decodeFrame(encodeFrame(quadro())).session, 'sessão', 'controle: identificador comum não muda')
})

test('o JPEG decodificado é cópia própria, não uma visão do buffer do canal', () => {
  const wire = encodeFrame(quadro())
  const frame = decodeFrame(wire)
  wire.fill(0)
  assert.deepEqual([...frame.jpeg], [0xff, 0xd8, 1, 0xff, 0xd9])
  assert.equal(frame.jpeg.buffer.byteLength, frame.jpeg.length)
})

test('limites: JPEG de 512 KiB passa, um byte a mais e a mensagem inteira acima do teto são recusados', () => {
  const grande = quadro()
  grande.jpeg = new Uint8Array(FRAME_MAX_JPEG)
  grande.jpeg.set([0xff, 0xd8])
  grande.jpeg.set([0xff, 0xd9], FRAME_MAX_JPEG - 2)
  assert.equal(decodeFrame(encodeFrame(grande)).jpeg.length, FRAME_MAX_JPEG)
  const demais = quadro()
  demais.jpeg = new Uint8Array(FRAME_MAX_JPEG + 1)
  assert.equal(kind(() => encodeFrame(demais)), 'tooLarge')
  assert.equal(kind(() => decodeFrame(new Uint8Array(FRAME_MAX_WIRE + 1))), 'tooLarge')
})

test('recusa truncamento, bytes excedentes, magia errada, UTF-8 inválido, dimensão zero e JPEG sem marcadores', () => {
  const wire = encodeFrame(quadro())
  for (let end = 0; end < wire.length; end += 1) assert.notEqual(kind(() => decodeFrame(wire.subarray(0, end))), 'nada', `truncado em ${end}`)
  assert.equal(kind(() => decodeFrame(Uint8Array.from([...wire, 0]))), 'invalid')
  const magia = wire.slice()
  magia[0] ^= 1
  assert.equal(kind(() => decodeFrame(magia)), 'invalid')
  const utf8 = wire.slice()
  utf8[FRAME_HEADER] = 0xff
  assert.equal(kind(() => decodeFrame(utf8)), 'invalid')
  const zero = wire.slice()
  new DataView(zero.buffer).setUint32(28, 0)
  assert.equal(kind(() => decodeFrame(zero)), 'invalid')
  const fim = wire.slice()
  fim[fim.length - 1] = 0
  assert.equal(kind(() => decodeFrame(fim)), 'invalid')
  const tamanho = wire.slice()
  new DataView(tamanho.buffer).setUint32(8, 0xffffffff)
  assert.equal(kind(() => decodeFrame(tamanho)), 'tooLarge')
  const sequencia = wire.slice()
  new DataView(sequencia.buffer).setBigUint64(12, 2n ** 60n)
  assert.equal(kind(() => decodeFrame(sequencia)), 'invalid')
})

test('o tamanho real do JPEG vem do marcador SOF, antes de decodificar', () => {
  assert.deepEqual(jpegSize(jpegSintetico(960, 540)), { width: 960, height: 540 })
  assert.deepEqual(jpegSize(jpegSintetico(1280, 720, 0xc2)), { width: 1280, height: 720 }, 'progressivo também')
  assert.equal(jpegSize(Uint8Array.from([0xff, 0xd8, 0xff, 0xd9])), null)
  assert.equal(jpegSize(Uint8Array.from([0xff, 0xd8, 0xff, 0xda, 0x00, 0x02, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0, 1, 0, 1, 1, 1, 0x11, 0, 0xff, 0xd9])), null, 'SOF depois dos dados da imagem não vale')
  assert.equal(jpegSize(Uint8Array.from([0xff, 0xd8, 0xff, 0xc0, 0x00, 0x0b, 0x08])), null, 'truncado')
})

test('o teto de pixels vale pelo cabeçalho e pelo próprio JPEG: quadro normal passa, gigante não', () => {
  const normal = { ...quadro(), jpeg: jpegSintetico(960, 540) }
  assert.equal(isDrawable(normal), true)
  assert.equal(isDrawable({ ...normal, image: { width: 8192, height: 2048 }, jpeg: jpegSintetico(8192, 2048) }), true)
  assert.equal(isDrawable({ ...normal, jpeg: jpegSintetico(960, 720) }), false, 'o JPEG tem outra proporção que o cabeçalho: o clique desviaria')
  assert.equal(isDrawable({ ...normal, image: { width: 961, height: 540 } }), false, 'um pixel de diferença também')
  assert.equal(isDrawable({ ...normal, jpeg: jpegSintetico(8192, 8192) }), false, 'pixels demais')
  assert.equal(isDrawable({ ...normal, jpeg: jpegSintetico(FRAME_MAX_SIDE + 1, 10) }), false, 'lado demais')
  assert.equal(isDrawable({ ...normal, image: { width: 960, height: 70_000 } }), false, 'o cabeçalho mente')
  assert.equal(isDrawable({ ...normal, jpeg: Uint8Array.from([0xff, 0xd8, 0, 0, 0xff, 0xd9]) }), false, 'sem SOF')
  assert.equal(withinPixelLimit(4096, 4096), FRAME_MAX_PIXELS === 4096 * 4096)
  assert.equal(withinPixelLimit(0, 10), false)
  assert.equal(withinPixelLimit(10.5, 10), false)
})
