import assert from 'node:assert/strict'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'

const bundle = buildSync({ entryPoints: [new URL('./audio-pcm.ts', import.meta.url).pathname], bundle: true, platform: 'node', format: 'cjs', write: false, define: { 'process.env.NODE_ENV': '"production"' } }).outputFiles[0]!.text
function audioReal() {
  const fontes: { buffer: unknown; onended: (() => void) | null; parado: boolean; start(t: number): void; stop(): void; disconnect(): void; connect(): void }[] = []
  let paradas = 0
  let fechado = 0
  const contexto = { currentTime: 0 }
  const module = { exports: {} }
  let captura: ((e: { inputBuffer: { getChannelData(): Float32Array } }) => void) | null = null
  runInNewContext(bundle, {
    module, Uint8Array, Int16Array, Float32Array, DataView, queueMicrotask,
    localStorage: { getItem: () => null, setItem() {} },
    window: { addEventListener() {}, codync: { speech: { available: false } } },
    navigator: { mediaDevices: { getUserMedia: async () => ({ getTracks: () => [{ stop: () => paradas++ }] }) } },
    AudioContext: class {
      sampleRate = 48000
      state = 'running'
      destination = {}
      get currentTime() { return contexto.currentTime }
      createMediaStreamSource() { return { connect() {}, disconnect() {} } }
      createScriptProcessor() { return { set onaudioprocess(f: typeof captura) { captura = f }, connect() {}, disconnect() {} } }
      createBuffer(_c: number, n: number, rate: number) { assert.equal(rate, 24000); const pcm = new Float32Array(n); return { getChannelData: () => pcm } }
      createBufferSource() {
        const f = { buffer: null as unknown, onended: null as (() => void) | null, parado: false, start(_t: number) {}, stop() { this.parado = true }, disconnect() {}, connect() {} }
        fontes.push(f); return f
      }
      async close() { fechado++ }
    },
  })
  const mod = module.exports as { PCM24: typeof import('./audio-pcm').PCM24; AudioPCM: typeof import('./audio-pcm').AudioPCM }
  return { mod, fontes, contexto, paradas: () => paradas, fechado: () => fechado, capturar: () => captura!({ inputBuffer: { getChannelData: () => new Float32Array(480).fill(0.5) } }) }
}

test('PCM24 contínuo: dispositivo 48k/44.1k/24k vira mono PCM16 LE 24k sem cabeçalho', () => {
  const h = audioReal()
  for (const rate of [48000, 44100, 24000]) {
    const conversor = new h.mod.PCM24(rate)
    let bytes = 0
    for (let i = 0; i < 100; i++) {
      const pcm = conversor.converter(new Float32Array(rate / 100).fill(0.5))
      bytes += pcm.length
      assert.equal(new DataView(pcm.buffer, pcm.byteOffset).getInt16(0, true), 16384)
      assert.notEqual(new TextDecoder().decode(pcm.subarray(0, 4)), 'RIFF')
    }
    assert.ok(Math.abs(bytes - 48000) <= 2, `${rate}: ${bytes}`)
  }
})

test('AudioPCM real corta a fila pelo trecho ouvido, limita dez segundos e libera captura/saída uma vez', async () => {
  const h = audioReal()
  const audio = new h.mod.AudioPCM()
  let entrada = 0
  await audio.iniciar(pcm => { entrada += pcm.length })
  h.capturar(); assert.equal(entrada, 480, '480 amostras a 48k viram 240 amostras a 24k')
  audio.tocar(new Uint8Array(24000), 'item', 0) // 0,5 s
  audio.tocar(new Uint8Array(24000), 'item', 0)
  h.contexto.currentTime = 0.75
  assert.equal(audio.cortar()!.ms, 750)
  assert.ok(h.fontes.every(f => f.parado))
  assert.equal(audio.ocupado, false)
  audio.tocar(new Uint8Array(480000), 'longo', 0)
  assert.throws(() => audio.tocar(new Uint8Array(2), 'longo', 0), (e: unknown) => (e as { chave?: string }).chave === 'voz.chamada.falha.audioFila')
  audio.pararEntrada(); assert.equal(h.paradas(), 1)
  audio.terminar(); audio.terminar(); assert.equal(h.fechado(), 1); assert.equal(h.paradas(), 1)
})
