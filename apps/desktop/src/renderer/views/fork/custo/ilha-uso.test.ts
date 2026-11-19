// MX-245 (revisão R2): a ilha mostra a hora em que o limite volta de forma VISÍVEL na linha (nunca só no `title`), cabe
// na coluna nos 6 idiomas, e a frase completa fica no `title` e no nome acessível. Render real de `UsageView` (SSR).
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { APP_LANGUAGES } from '../../../../shared/fork-idioma.ts'
import { voltaCurta } from '../../../lib/limite-volta.ts'

const bundle = await build({
  stdin: {
    contents: `export { UsageView } from './views/island/IslandUsage';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /(^\.\/|\/components\/)Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
  } }, { name: 'mascote', setup(b) {
    // O personagem do provedor (imagens do Vite, `import.meta.glob`) não faz parte do que se confere aqui.
    b.onResolve({ filter: /\/usage\/ProviderMascot$/ }, () => ({ path: 'mascote', namespace: 'mascote' }))
    b.onLoad({ filter: /.*/, namespace: 'mascote' }, () => ({ contents: "export const mascotDataURL = () => ''", loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, location: { search: '' } }
type Modulo = { UsageView: (p: { model: unknown }) => unknown; createElement: typeof import('react').createElement; renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup }
const modulos: Record<string, Modulo> = {}
const carregar = (idioma: string): Modulo => modulos[idioma] ??= (() => {
  const storage = { getItem: (k: string) => (k === 'appLanguage' ? JSON.stringify(idioma) : null), setItem() {}, removeItem() {} }
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask, Intl, Date, document: undefined })
  return module.exports as Modulo
})()
const render = (windows: object[], idioma = 'pt-BR') => {
  const m = carregar(idioma)
  const usage = [{ id: 'claude', name: 'Claude', windows, source: 'x', updatedAt: 1 }]
  return m.renderToStaticMarkup(m.createElement(m.UsageView as never, { model: { summary: { usage } } }))
}
const reset = (html: string) => [...html.matchAll(/<span class="island-usage-reset">([^<]*)<\/span>/g)].map((m) => m[1]!)
const notas = (html: string) => [...html.matchAll(/<div class="island-usage-nota line-clamp(?: island-usage-nota-longa)?">([^<]*)<\/div>/g)].map((m) => m[1]!)
/** Cada janela, na ordem: a coluna estreita da hora curta (ou null) e a frase inteira da linha própria (ou null). */
const janelas = (html: string) => html.split('<div class="island-usage-janela"').slice(1).map((j) => ({ coluna: reset(j)[0] ?? null, nota: notas(j)[0] ?? null }))
const dias = (n: number) => Date.now() + n * 86_400_000

test('ilha: só resetsText, prazo passado e sem dado vão inteiros numa linha própria, sem coluna estreita', () => {
  const html = render([
    { id: 'a', label: '5-hour', percent: 10, resetsText: 'Sep 26 at 12pm (Asia/Taipei)' },
    { id: 'b', label: 'Weekly', percent: 20, resetsAt: Date.now() - 3_600_000 },
    { id: 'c', label: 'Weekly · Opus', percent: 30 },
  ])
  const [texto, passado, nada] = janelas(html)
  assert.deepEqual(texto, { coluna: null, nota: 'volta: Sep 26 at 12pm (Asia/Taipei)' })
  assert.equal(passado!.coluna, null)
  assert.match(passado!.nota!, /^o prazo informado já passou \(.+\)$/, "hora, ou data e hora se virou o dia")
  assert.deepEqual(nada, { coluna: null, nota: 'o provedor não informa quando volta' })
  assert.equal(reset(html).length, 0, 'nenhuma coluna estreita nesses estados')
  // A linha própria quebra (classe de line-clamp, nada de reticências por largura fixa); o texto inteiro está no DOM.
  assert.equal((html.match(/island-usage-nota line-clamp/g) ?? []).length, 3)
  // O title é complemento (frase completa) e há nome acessível com o mesmo texto; nada de tabindex.
  assert.match(html, /title="volta: Sep 26 at 12pm \(Asia\/Taipei\)"/)
  assert.match(html, /role="group" title="[^"]*" aria-label="[^"]*· 10% · volta: Sep 26 at 12pm/)
  assert.doesNotMatch(html, /tabindex/i)
})

test('ilha: a frase longa só vai para a linha própria quando não há hora curta; com hora curta, só a coluna', () => {
  const html = render([
    { id: 'a', label: '5-hour', percent: 10, resetsAt: Date.now() + 40 * 86_400_000 },
    { id: 'b', label: 'Weekly', percent: 20, resetsText: 'quando liberar' },
  ])
  const [futuro, texto] = janelas(html)
  assert.match(futuro!.coluna!, /^\d{2}\/\d{2}, \d{2}:\d{2}$/)
  assert.equal(futuro!.nota, null)
  assert.deepEqual(texto, { coluna: null, nota: 'volta: quando liberar' })
})

test('ilha: instante informado mostra a forma curta na coluna e a frase completa no title', () => {
  const html = render([{ id: 'a', label: '5-hour', percent: 10, resetsAt: dias(30) }])
  const [curto] = reset(html)
  assert.match(curto!, /^\d{2}\/\d{2}, \d{2}:\d{2}$/)
  assert.match(html, /title="volta \d{2}\/\d{2} às \d{2}:\d{2}"/)
})

test('ilha, en: a forma curta mais longa ("tomorrow 09:00 AM") sai inteira, sem reticências no texto', () => {
  const html = render([{ id: 'a', label: '5-hour', percent: 10, resetsAt: Date.now() + 36 * 3_600_000 }], 'en')
  const [curto] = reset(html)
  assert.match(curto!, /^(tomorrow|[A-Z][a-z]{2}) \d{2}:\d{2} [AP]M$|^\d{2}\/\d{2}, \d{2}:\d{2} [AP]M$/)
  assert.ok(!curto!.includes('…'))
  assert.deepEqual(janelas(html).map((j) => j.nota), [null])
})

test('a forma curta cabe na coluna (max 128px de largura) nos 6 idiomas: no máximo 18 caracteres em todos os casos', () => {
  const AGORA = Date.parse('2026-11-11T18:00:00Z')
  const SP = 'America/Sao_Paulo'
  for (const idioma of APP_LANGUAGES) {
    for (const dias of [0, 1, 2, 3, 4, 5, 6, 7, 40]) {
      // Pior caso de largura: 23:59, fim do dia, em cada um dos dias.
      const curta = voltaCurta(idioma, AGORA + dias * 86_400_000 + 5 * 3_600_000, AGORA, SP)!
      assert.ok(curta && [...curta.replace(/[⁦-⁩]/g, '')].length <= 18, `${idioma} +${dias}d: "${curta}"`)
    }
  }
})

test('ilha: o cartão nunca estoura: 1 frase → até 2 linhas; 2 ou mais no cartão → 1 linha cada', () => {
  const texto = (id: string): Record<string, unknown> => ({ id, label: id === 'a' ? '5-hour' : `Weekly · ${id}`, percent: 10, resetsText: 'Sep 26 at 12pm (America/Argentina/Buenos_Aires)' })
  const hora = (id: string): Record<string, unknown> => ({ id, label: `Weekly · ${id}`, percent: 10, resetsAt: Date.now() + 40 * 86_400_000 })
  const longas = (html: string) => (html.match(/island-usage-nota line-clamp island-usage-nota-longa/g) ?? []).length
  const todas = (html: string) => (html.match(/island-usage-nota line-clamp/g) ?? []).length
  // Uma frase: 2 linhas (sobra altura). Duas ou três: 1 linha cada (3 × 1 linha cabem em 150px).
  const uma = render([texto('a'), hora('b'), hora('c')])
  assert.deepEqual([todas(uma), longas(uma)], [1, 1])
  for (const n of [2, 3]) {
    const varias = render([texto('a'), texto('b'), texto('c')].slice(0, n).concat(Array.from({ length: 3 - n }, (_, i) => hora(`h${i}`))))
    assert.deepEqual([todas(varias), longas(varias)], [n, 0], `${n} frases`)
  }
  // O texto completo continua no DOM e no title mesmo cortado em 1 linha.
  const tres = render([texto('a'), texto('b'), texto('c')])
  assert.equal(notas(tres).length, 3)
  assert.ok(notas(tres).every((n) => n === 'volta: Sep 26 at 12pm (America/Argentina/Buenos_Aires)'))
  assert.match(tres, /title="volta: Sep 26 at 12pm \(America\/Argentina\/Buenos_Aires\)"/)
  // A regra vale por cartão (provedor), não pela ilha toda.
  const m = carregar('pt-BR')
  const doisCartoes = m.renderToStaticMarkup(m.createElement(m.UsageView as never, { model: { summary: { usage: [
    { id: 'claude', name: 'Claude', windows: [texto('a'), hora('b')], source: 'x', updatedAt: 1 },
    { id: 'codex', name: 'Codex', windows: [texto('c'), texto('d')], source: 'x', updatedAt: 1 },
  ] } } }))
  assert.deepEqual([todas(doisCartoes), longas(doisCartoes)], [3, 1], 'cartão com 1 frase: 2 linhas; cartão com 2: 1 linha')
})

test('CSS da ilha: coluna da hora curta cresce ao conteúdo; a linha própria da frase quebra (line-clamp), sem truncar por largura', () => {
  const css = readFileSync(new URL('../../island/island-views.css', import.meta.url), 'utf8')
  const bloco = css.match(/\.island-usage-reset \{([^}]*)\}/)![1]!
  assert.doesNotMatch(bloco, /(^|[^-])width:\s*\d/, 'sem width fixa')
  assert.match(bloco, /min-width:/)
  assert.match(bloco, /max-width:\s*128px/)
  assert.match(css.match(/\.island-usage-row \{([^}]*)\}/)![1]!, /white-space:\s*nowrap/)
  const nota = css.match(/\.island-usage-nota \{([^}]*)\}/)![1]!
  assert.match(nota, /white-space:\s*normal/)
  assert.match(nota, /-webkit-line-clamp:\s*1\b/)
  assert.match(css.match(/\.island-usage-nota-longa \{([^}]*)\}/)![1]!, /-webkit-line-clamp:\s*2\b/)
  assert.doesNotMatch(nota, /text-overflow|nowrap|(^|[^-])width:/, 'a frase não trunca por largura')
})
