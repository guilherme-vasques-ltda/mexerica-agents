import assert from 'node:assert/strict'
import { textoAgora } from '../bot-settings/mensagem.ts'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'

// Leitor do Hub real; só Audio, URL e a fala do provedor são fictícios.
const bundle = buildSync({
  stdin: { contents: `export { LeitorDoHub } from './views/fork/voz/leitores';`, resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false,
  define: { 'process.env.NODE_ENV': '"production"' },
})

class FakeAudio {
  static todos: FakeAudio[] = []
  pausado = false
  tocando = false
  onended: (() => void) | null = null
  onerror: (() => void) | null = null
  readonly src: string
  constructor(src: string) { this.src = src; FakeAudio.todos.push(this) }
  play() { this.tocando = true; return Promise.resolve() }
  pause() { this.pausado = true; this.tocando = false }
}

function carregar() {
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, setTimeout, clearTimeout, Blob, Audio: FakeAudio,
    URL: { createObjectURL: () => 'blob:x', revokeObjectURL() {} },
    console: { error() {}, log() {}, warn() {} },
    localStorage: { getItem: () => null, setItem() {} },
    window: { addEventListener() {}, codync: { appVersion: '2.20.3', platform: 'darwin', speech: { available: true } } },
    navigator: { language: 'pt-BR', languages: ['pt-BR'] },
  })
  return (module.exports as { LeitorDoHub: typeof import('./leitores').LeitorDoHub }).LeitorDoHub
}
const LeitorDoHub = carregar()
const espera = () => new Promise((r) => setTimeout(r, 0))

test('falha do provedor na leitura chega ao motor, para a queda gratuita (achado Codex P2)', async () => {
  const leitor = new LeitorDoHub('gemini', () => Promise.reject(new Error('API key not valid')))
  let fim = 0
  let falha: string | null = null
  leitor.ler('Olá.', () => { fim++ }, (m) => { falha = m })
  await espera(); await espera()
  assert.equal(falha, textoAgora({ chave: 'voz.chamada.falha.chave' }), 'texto leigo (do catálogo), sem o que o provedor disse')
  assert.equal(fim, 0, 'não finge que leu')
})

test('uma leitura nova para o áudio anterior; Parar corta o que toca (achado Codex P2)', async () => {
  FakeAudio.todos = []
  const leitor = new LeitorDoHub('openai', () => Promise.resolve({ audio: new Uint8Array([1]), format: 'aac' }))
  leitor.ler('Primeira resposta.', () => {})
  await espera(); await espera()
  const primeiro = FakeAudio.todos[0]!
  assert.equal(primeiro.tocando, true)
  leitor.ler('Segunda resposta.', () => {})
  assert.equal(primeiro.pausado, true, 'a primeira não continua tocando por baixo')
  await espera(); await espera()
  const segundo = FakeAudio.todos[1]!
  assert.equal(segundo.tocando, true)
  leitor.parar()
  assert.equal(segundo.pausado, true)
  assert.equal(FakeAudio.todos.filter((a) => a.tocando).length, 0)
})
