import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { EstadoWhatsapp } from './estado.ts'
import type { LeituraWhatsapp } from './regras.ts'

// D70 (R363-1): fechar a folha "Conectar número" real com o primeiro QR do Hub retido. O componente roda
// como função, com os ganchos do React trocados por um executor mínimo; os botões são os do JSX real.
const bundle = await build({
  stdin: {
    contents: `export { Pareamento } from './views/fork/whatsapp/Pareamento'; export { Sheet } from './components/Overlay';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'ganchos', setup(b) {
    // Só o Pareamento recebe os ganchos de teste; o resto usa o React de verdade.
    b.onResolve({ filter: /^react$/ }, (a) => a.importer.endsWith('Pareamento.tsx') ? { path: 'ganchos', namespace: 'ganchos' } : undefined)
    b.onResolve({ filter: /^react-real$/ }, (a) => b.resolve('react', { resolveDir: a.resolveDir, kind: 'import-statement' }))
    b.onLoad({ filter: /.*/, namespace: 'ganchos' }, () => ({
      resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'js',
      contents: `export * from 'react-real'
        export const useState = (v) => ganchos.useState(v)
        export const useEffect = () => {}
        export const useRef = (v) => ({ current: v ?? null })`,
    }))
    // Fatia 6 (D454): o idioma do app vem do estado do app; aqui, fixo em pt-BR (o teste procura os botões pelo texto).
    b.onResolve({ filter: /\/fork-i18n\/estado$/ }, () => ({ path: 'idioma', namespace: 'idioma' }))
    b.onLoad({ filter: /.*/, namespace: 'idioma' }, () => ({
      resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'js',
      contents: `import { tradutor, superficieIdioma } from './lib/fork-i18n/index.ts'
        export { superficieIdioma }
        export const useIdioma = () => 'pt-BR'
        export const useT = () => tradutor('pt-BR')`,
    }))
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
const estados: unknown[] = []
let indice = 0
const ganchos = {
  useState(inicial: unknown) {
    const k = indice++
    if (!(k in estados)) estados[k] = typeof inicial === 'function' ? (inicial as () => unknown)() : inicial
    return [estados[k], (v: unknown) => { estados[k] = typeof v === 'function' ? (v as (a: unknown) => unknown)(estados[k]) : v }]
  },
}
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 }
const storage = { getItem: () => null, setItem() {}, removeItem() {} }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), ganchos, window, matchMedia: media, localStorage: storage, TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask })
const m = module.exports as { Pareamento: typeof import('./Pareamento').Pareamento; Sheet: unknown }

type El = { type: unknown; props: Record<string, unknown> & { children?: unknown } }
function achar(no: unknown, ok: (e: El) => boolean): El | undefined {
  if (Array.isArray(no)) { for (const f of no) { const r = achar(f, ok); if (r) return r } return undefined }
  if (!no || typeof no !== 'object' || !('props' in no)) return undefined
  const e = no as El
  return ok(e) ? e : achar(e.props.children, ok)
}
const botao = (arvore: unknown, texto: string) => achar(arvore, (e) => e.props.children === texto && typeof e.props.onClick === 'function')!.props.onClick as () => void

const ajustes = { readChats: false, replyChats: false, readGroups: false, replyGroups: false, replyMode: 'approval' as const, delaySeconds: 8, hourlyLimit: 30, transcribeAudio: true }
const nada = { readChats: false, replyChats: false, readGroups: false, replyGroups: false }
const leitura = (number: LeituraWhatsapp['bots'][number]['number']): LeituraWhatsapp => ({
  plugin: { enabled: true, mode: 'donna_only' },
  transcription: { provider: null, model: null, source: 'none', keyOnHub: false },
  bots: [{ botId: 'donna', isDonna: true, paused: false, settings: ajustes, number, botCan: nada, sending: { state: 'none' } }],
})
const qr = { state: 'pairing', pairing: { method: 'qr' as const, result: 'waiting' as const, qr: 'QR-FALSO' } }

/** Hub falso: o pedido de pareamento fica retido até `soltar`; o resto responde na hora. */
function folha(numero: LeituraWhatsapp['bots'][number]['number'] = { state: 'none' }) {
  const pedidos: string[] = []
  let soltar: (v: unknown) => void = () => {}
  const estado = new EstadoWhatsapp(() => ({ call: async <T>(metodo: string) => {
    pedidos.push(metodo)
    if (metodo === 'donnaWhatsappPair') return new Promise<T>((r) => { soltar = r as (v: unknown) => void })
    return leitura({ state: 'none' }) as T
  } }))
  estado.leitura = leitura(numero)
  let fechou = 0
  estados.length = 0
  const desenhar = () => {
    indice = 0
    return m.Pareamento({ store: { bots: new Map(), hostName: '' } as never, estado, bot: estado.leitura!.bots[0]!, nome: 'Donna', aberto: true, aoFechar: () => { fechou++ } }) as unknown
  }
  const fechar = () => (achar(desenhar(), (e) => e.type === m.Sheet)!.props.onClose as () => void)()
  const continuar = () => {
    (achar(desenhar(), (e) => typeof e.props.setEntendi === 'function')!.props.setEntendi as (v: boolean) => void)(true)
    botao(desenhar(), 'Continuar')()
  }
  return { estado, pedidos, desenhar, fechar, continuar, soltar: (v: unknown) => soltar(v), fechou: () => fechou }
}
const cancelamentos = (p: string[]) => p.filter((x) => x === 'donnaWhatsappPairCancel').length
const espera = () => new Promise((r) => setTimeout(r, 0))

test('fechar antes do primeiro QR, com o pedido disparado, cancela no Hub', async () => {
  const f = folha()
  f.continuar()
  assert.deepEqual(f.pedidos, ['donnaWhatsappPair'])
  f.fechar()
  assert.equal(f.fechou(), 1)
  assert.equal(cancelamentos(f.pedidos), 1)
  await espera()
  // (d) A resposta tardia do primeiro QR, depois do cancelamento, não volta para a tela.
  f.soltar(leitura(qr))
  await espera()
  assert.equal(f.estado.leitura!.bots[0]!.number.pairing, undefined)
  assert.equal(f.estado.salvando, false)
})

test('fechar na etapa Riscos só fecha', () => {
  const f = folha()
  f.fechar()
  botao(f.desenhar(), 'Cancelar')()
  assert.equal(f.fechou(), 2)
  assert.deepEqual(f.pedidos, [])
})

test('fechar com o QR na tela cancela uma vez', () => {
  const f = folha(qr)
  f.fechar()
  assert.equal(f.fechou(), 1)
  assert.deepEqual(f.pedidos, ['donnaWhatsappPairCancel'])
})
