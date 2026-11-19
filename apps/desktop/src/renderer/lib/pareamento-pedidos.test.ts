import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// D70 (R1-01 do #575): cada pedido de pareamento gera um código novo no Hub. Atualizações do host e troca de
// idioma desenham de novo a janela, mas não podem pedir outro código. A janela e o PairingCode reais rodam
// como funções, com os ganchos do React trocados por um executor mínimo que respeita as dependências.
const raiz = new URL('../../', import.meta.url).pathname
const bundle = await build({
  stdin: { contents: `export { PairingWindow } from './renderer/views/PairingWindow'`, resolveDir: raiz, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'ganchos', setup(b) {
    const falso = (filter: RegExp, contents: string) => {
      const ns = `falso-${filter.source}`
      b.onResolve({ filter }, () => ({ path: 'x', namespace: ns }))
      b.onLoad({ filter: /.*/, namespace: ns }, () => ({ contents, resolveDir: raiz, loader: 'js' }))
    }
    b.onResolve({ filter: /^react$/ }, (a) => /Pairing(Window|Code)\.tsx$/.test(a.importer) ? { path: 'ganchos', namespace: 'ganchos' } : undefined)
    b.onResolve({ filter: /^react-real$/ }, (a) => b.resolve('react', { resolveDir: a.resolveDir, kind: 'import-statement' }))
    b.onLoad({ filter: /.*/, namespace: 'ganchos' }, () => ({ resolveDir: raiz, loader: 'js', contents: `export * from 'react-real'
      export const useState = (v) => ganchos.useState(v)
      export const useRef = (v) => ganchos.useRef(v)
      export const useCallback = (f, d) => ganchos.useCallback(f, d)
      export const useEffect = (f, d) => ganchos.useEffect(f, d)` }))
    falso(/\/sob-demanda\/telas$/, `export { PairingCode } from './renderer/views/PairingCode'`)
    falso(/\/store\/context$/, 'export const useApp = () => ganchos.app')
    falso(/\/store\/analytics$/, 'export const useScreenViewed = () => {}')
    falso(/\/client\/host-client$/, 'export class LoopbackTransport { call(metodo) { return ganchos.chamar(metodo) } }')
    falso(/^qrcode$/, 'export default { toDataURL: async () => "data:qr" }')
    falso(/\/fork-i18n\/estado$/, `import { tradutor } from './renderer/lib/fork-i18n/index.ts'
      export { superficieIdioma } from './renderer/lib/fork-i18n/index.ts'
      export const useIdioma = () => ganchos.idioma
      export const useT = () => tradutor(ganchos.idioma)`)
    falso(/\/components\/Icon$/, 'export function Icon() { return null }')
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})

type Slot = { v?: unknown; current?: unknown; f?: unknown; d?: readonly unknown[] }
const mesmas = (a: readonly unknown[] | undefined, b: readonly unknown[] | undefined) => !!a && !!b && a.length === b.length && a.every((x, i) => Object.is(x, b[i]))
const ganchos = {
  idioma: 'pt-BR',
  app: null as unknown,
  pedidos: [] as string[],
  slots: new Map<string, Slot[]>(),
  atual: [] as Slot[],
  i: 0,
  efeitos: [] as (() => unknown)[],
  comecar(nome: string) { if (!this.slots.has(nome)) this.slots.set(nome, []); this.atual = this.slots.get(nome)!; this.i = 0 },
  slot() { const k = this.i++; return (this.atual[k] ??= {}) },
  useState(inicial: unknown) {
    const s = this.slot()
    if (!('v' in s)) s.v = typeof inicial === 'function' ? (inicial as () => unknown)() : inicial
    return [s.v, (v: unknown) => { s.v = typeof v === 'function' ? (v as (a: unknown) => unknown)(s.v) : v }]
  },
  useRef(inicial: unknown) { const s = this.slot(); if (!('current' in s)) s.current = inicial; return s },
  useCallback(f: unknown, d: readonly unknown[]) { const s = this.slot(); if (!mesmas(s.d, d)) { s.f = f; s.d = d } return s.f },
  useEffect(f: () => unknown, d?: readonly unknown[]) { const s = this.slot(); if (!d || !mesmas(s.d, d)) { s.d = d; this.efeitos.push(f) } },
  async chamar(metodo: string) {
    this.pedidos.push(metodo)
    if (metodo === 'pairing') return { pairingUrl: 'codync://parear', expiresAt: Date.UTC(2999, 0, 1), urls: ['http://exemplo'] }
    if (metodo === 'hello') return { name: 'Mac da Sara' }
    return { enabled: false }
  },
}
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux', app: { onCommand: () => () => {}, copy() {} } }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 }
const localStorage = { getItem: () => null, setItem() {}, removeItem() {} }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), ganchos, window, matchMedia: media, localStorage, TextEncoder, URLSearchParams, console, setTimeout: () => 0, clearTimeout() {}, queueMicrotask, Intl, Date })
const { PairingWindow } = module.exports as { PairingWindow: () => { type: (p: unknown) => unknown; props: unknown } }

type El = { props: Record<string, unknown> & { children?: unknown } }
function achar(no: unknown, ok: (e: El) => boolean): El | undefined {
  if (Array.isArray(no)) { for (const f of no) { const r = achar(f, ok); if (r) return r } return undefined }
  if (!no || typeof no !== 'object' || !('props' in no)) return undefined
  const e = no as El
  return ok(e) ? e : achar(e.props.children, ok)
}

/** Um desenho da janela: roda os ganchos dela e do PairingCode, depois os efeitos cujas dependências mudaram. */
async function desenhar() {
  ganchos.comecar('janela')
  const janela = PairingWindow()
  ganchos.comecar('codigo')
  const arvore = janela.type(janela.props)
  for (const f of ganchos.efeitos.splice(0)) f()
  for (let i = 0; i < 5; i++) await new Promise((r) => setImmediate(r))
  return arvore
}
const host = () => ({ host: { local: { baseURL: 'http://127.0.0.1:1', token: 'tk' } } })

test('um pedido de código apesar de atualizações do host e de troca de idioma (R1-01 do #575)', async () => {
  ganchos.slots.clear(); ganchos.pedidos.length = 0; ganchos.idioma = 'pt-BR'
  ganchos.app = host()
  await desenhar()
  // O snapshot do host é um objeto novo a cada mudança; o desenho seguinte vem do setState do próprio pedido.
  for (let i = 0; i < 3; i++) { ganchos.app = host(); await desenhar() }
  ganchos.idioma = 'en'; await desenhar()
  ganchos.idioma = 'ar-AE'; ganchos.app = host(); await desenhar()
  assert.equal(ganchos.pedidos.filter((m) => m === 'pairing').length, 1)
})

test('sem host, o erro troca de idioma com a mensagem aberta, sem novo pedido', async () => {
  ganchos.slots.clear(); ganchos.pedidos.length = 0; ganchos.idioma = 'pt-BR'
  ganchos.app = { host: { local: null } }
  await desenhar()
  const erro = (arvore: unknown) => achar(arvore, (e) => e.props.className === 'pairing-note fade-in')?.props.children
  assert.equal(erro(await desenhar()), 'O host deste computador não está conectado.')
  ganchos.idioma = 'en'
  assert.notEqual(erro(await desenhar()), 'O host deste computador não está conectado.')
  assert.equal(ganchos.pedidos.length, 0)
})
