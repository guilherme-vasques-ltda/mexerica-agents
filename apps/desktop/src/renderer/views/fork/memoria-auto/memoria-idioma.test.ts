import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { tradutor, traduzirPlural } from '../../../lib/fork-i18n/index.ts'

// Fatia 7 (D454, D507, D70): a memória guarda dado (chave ou texto cru do Hub), nunca frase pronta; autor, versão e
// natureza são frases inteiras do catálogo com o nome do bot isolado (`<bdi dir="auto">`).
const bundle = await build({
  stdin: {
    contents: `export { MemoryStore, authorLabel, versionLabel } from './store/memory-store'
      export { textoDaMensagem } from './views/fork/bot-settings/mensagem'
      export { Autor, MemoryNature, MemoryVersionMeta } from './views/fork/memoria-auto/MemoryNature'
      export { createElement } from 'react'
      export { renderToStaticMarkup } from 'react-dom/server'`,
    resolveDir: new URL('../../..', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', loader: { '.css': 'empty', '.jpg': 'empty', '.webp': 'empty', '.png': 'empty', '.svg': 'empty', '.woff2': 'empty', '.mp3': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, location: { search: '' } }
// O texto conferido é o pt-BR: idioma escolhido pt-BR (sem escolha, o padrão é o inglês, D454/D495).
const storage = { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} }
const module = { exports: {} as Record<string, never> }
// `Error` do teste: o erro do Hub simulado nasce aqui e o `instanceof` do bundle o reconhece.
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, console, setTimeout, clearTimeout, queueMicrotask, TextEncoder, URL, URLSearchParams, Intl, Error })
const m = module.exports as Record<string, any>
const pt = tradutor('pt-BR')
const en = tradutor('en')
const texto = (t: typeof pt, chave: string, params?: Record<string, string>) => (t as unknown as (c: string, p?: Record<string, string>) => string)(chave, params)

test('erro do Hub antigo fica como chave e troca de idioma; erro cru do Hub fica como veio', async () => {
  const antigo = new m.MemoryStore('bot', async () => ({ call: async () => ({}) }))
  await antigo.load()
  assert.equal(JSON.stringify(antigo.error), JSON.stringify({ chave: 'marca.memoria.hubAntigo' }))
  assert.notEqual(m.textoDaMensagem(en, antigo.error), m.textoDaMensagem(pt, antigo.error))
  const cru = new m.MemoryStore('bot', async () => ({ call: async () => { throw new Error('quota exceeded') } }))
  await cru.load()
  assert.equal(JSON.stringify(cru.error), JSON.stringify({ texto: 'quota exceeded' }))
})

test('autor e versão de nota: uma frase por autor e causa, com o nome do bot à parte e isolado', () => {
  const nome = (id: string | null) => (id === 'b1' ? 'Lia' : '?')
  assert.equal(JSON.stringify(m.authorLabel({ author: 'bot', authorBot: 'b1' }, nome)), JSON.stringify({ chave: 'memoria.painel.autor.bot', bot: 'Lia' }))
  assert.equal(texto(en, m.authorLabel({ author: 'user' }, nome).chave), 'You wrote')
  for (const author of ['user', 'bot', 'dream', 'computer']) {
    for (const cause of ['delete', 'restore', 'undo', 'edit', 'dream']) {
      const r = m.versionLabel({ author, authorBot: 'b1', cause }, nome)
      const em = texto(en, r.chave, r.bot ? { bot: r.bot } : undefined)
      assert.notEqual(em, r.chave, `${author}/${cause}`)
      assert.notEqual(em, texto(pt, r.chave, r.bot ? { bot: r.bot } : undefined))
      assert.equal(r.bot === undefined, author !== 'bot')
    }
  }
  const html = m.renderToStaticMarkup(m.createElement(m.Autor, { rotulo: m.versionLabel({ author: 'bot', authorBot: 'b1', cause: 'restore' }, nome) }))
  assert.equal(html, '<bdi dir="auto">Lia</bdi> restaurou uma versão')
  const natureza = m.renderToStaticMarkup(m.createElement(m.MemoryNature, { nature: 'inferred', bot: 'b1', name: () => 'Lia' }))
  assert.match(natureza, /<bdi dir="auto">Lia<\/bdi> inferiu · hipótese/)
  assert.match(m.renderToStaticMarkup(m.createElement(m.MemoryVersionMeta, { at: 1793718000000, rev: 3 })), /<bdi>[^<]+<\/bdi> · Versão \u20663\u2069/)
})

test('contagens da memória: zero natural nos 6 idiomas e dual árabe próprio (D507)', () => {
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
    for (const base of ['memoria.painel.camada.voce', 'memoria.painel.camada.bot'] as const) {
      const zero = traduzirPlural(idioma, base, 0, { limite: 400 })
      assert.doesNotMatch(zero, /\b0\b/, `${idioma} ${base}: ${zero}`)
      assert.notEqual(zero, traduzirPlural(idioma, base, 5, { limite: 400 }))
    }
    assert.doesNotMatch(traduzirPlural(idioma, 'memoria.painel.notas.tokens', 0), /\b0\b/, idioma)
  }
  for (const base of ['memoria.painel.camada.bot', 'memoria.painel.notas.acima', 'memoria.painel.notas.tokens'] as const) {
    const params = { limite: 30 } as never
    assert.notEqual(traduzirPlural('ar-AE', base, 2, params), traduzirPlural('ar-AE', base, 1, params), base)
  }
})
