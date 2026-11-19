import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { readFileSync } from 'node:fs'
import { build, type Plugin } from 'esbuild'
import { avisoDoStore, proximaFase, type AvisoDesatualizado } from './fork-aviso-desatualizado.ts'
import { CATALOGOS, tradutor } from './fork-i18n/index.ts'
import { APP_LANGUAGES, type AppLanguage } from '../../shared/fork-idioma.ts'

const computador: AvisoDesatualizado = { alvo: 'computador', encontrada: '2.19.0', minima: '2.20.1', verificando: false }

test('avisoDoStore: computador antigo vira aviso do computador; app antigo, aviso do app; sem divergência, nada', () => {
  assert.equal(avisoDoStore({ desatualizado: null }), null)
  assert.deepEqual(avisoDoStore({ desatualizado: { alvo: 'hub', encontrada: '2.19.0', minimo: '2.20.1' } }), computador)
  assert.deepEqual(avisoDoStore({ desatualizado: { alvo: 'app', encontrada: '0.4.0', minimo: '0.3.0' } }, true),
    { alvo: 'app', encontrada: '0.4.0', minima: '0.3.0', verificando: true })
})

test('avisoDoStore: versão ilegível do computador chega como encontrada null, nunca como texto inválido', () => {
  assert.deepEqual(avisoDoStore({ desatualizado: { alvo: 'hub', encontrada: null, minimo: '2.20.1' } }),
    { alvo: 'computador', encontrada: null, minima: '2.20.1', verificando: false })
})

test('fases: avisa, confirma que voltou e some; queda de conexão não anuncia atualização', () => {
  assert.equal(proximaFase('oculto', null, true), 'oculto')
  assert.equal(proximaFase('oculto', computador, true), 'desatualizado')
  assert.equal(proximaFase('desatualizado', computador, true), 'desatualizado')
  assert.equal(proximaFase('desatualizado', null, true), 'voltou')
  assert.equal(proximaFase('desatualizado', null, false), 'oculto')
  assert.equal(proximaFase('voltou', null, true), 'voltou')
  assert.equal(proximaFase('voltou', computador, true), 'desatualizado')
})

test('catálogos: o aviso fala em linguagem leiga, nos 6 idiomas, sem nome de fornecedor nem jargão de infraestrutura', () => {
  const proibido = /\b(hub|relay|cloudflare|clerk|servidor|server|tunnel|túnel|ssh)\b/i
  for (const [idioma, catalogo] of Object.entries(CATALOGOS)) {
    const chaves = Object.keys(catalogo).filter((c) => c.startsWith('navegacao.janela.desatualizado.'))
    assert.equal(chaves.length, 17, idioma)
    for (const chave of chaves) assert.doesNotMatch(String(catalogo[chave as keyof typeof catalogo]), proibido, `${idioma} ${chave}`)
  }
})

test('CSS do aviso: calmo, só tokens, sem vermelho de erro', () => {
  const css = readFileSync(new URL('../views/aviso-computador.css', import.meta.url), 'utf8')
  assert.doesNotMatch(css, /danger|error|red\b|#[0-9a-f]{3,8}\b|rgb\(/i)
  assert.match(css, /var\(--ink-2\)/)
})

// Renderiza o cartão real no servidor (mesma técnica de ajustes-bidi.test.ts: sem DOM no Node).
const reactNoServidor: Plugin = {
  name: 'react-no-servidor',
  setup(b) {
    b.onResolve({ filter: /^react$/ }, (args) => (args.namespace === 'react-servidor' ? undefined : { path: 'react', namespace: 'react-servidor' }))
    b.onLoad({ filter: /.*/, namespace: 'react-servidor' }, () => ({
      contents: `const R = require(${JSON.stringify(createRequire(import.meta.url).resolve('react'))});
        module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
  },
}
const bundle = await build({
  stdin: {
    contents: `export { AvisoComputador } from './views/AvisoComputador';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent', plugins: [reactNoServidor],
  alias: { '@shared': new URL('../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.png': 'empty', '.jpg': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp3': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const storage = { getItem: () => null, setItem() {}, removeItem() {} }
const window = { codync: { platform: 'darwin' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage, innerWidth: 1200, innerHeight: 800 }
const document = { documentElement: { lang: 'pt-BR', dir: 'ltr', dataset: {}, style: { setProperty() {} } }, body: { setAttribute() {} }, addEventListener() {}, removeEventListener() {}, hidden: false }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage,
  navigator: { platform: 'MacIntel', language: 'pt-BR' }, fetch: () => new Promise(() => {}), TextEncoder, URLSearchParams, URL, Intl, console, setTimeout, clearTimeout, queueMicrotask,
  requestAnimationFrame: () => 0, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const m = module.exports as {
  AvisoComputador: typeof import('../views/AvisoComputador').AvisoComputador
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}
// Nome genérico de computador: nada real nesta prova (docs/propostas e CI de vazamento).
const NOME = 'Computador de teste'
const render = (props: Partial<import('../views/AvisoComputador').AvisoComputadorProps> & { idioma?: AppLanguage } = {}) => {
  const { idioma = 'pt-BR', ...resto } = props
  return m.renderToStaticMarkup(m.createElement(m.AvisoComputador, {
    t: tradutor(idioma), fase: 'desatualizado', host: m.createElement('bdi', { dir: 'auto' }, NOME), aviso: computador, como: 'Abra a Mexerica naquele Mac e escolha Procurar atualizações.', ...resto,
  }))
}

test('desatualizado: diz o que houve, o que segue, o que para e o que fazer, em role=status e sem vermelho', () => {
  const html = render()
  assert.match(html, /role="status"/)
  assert.match(html, /aria-labelledby="/)
  assert.match(html, /O computador <bdi dir="auto">Computador de teste<\/bdi> precisa ser atualizado para esta versão da Mexerica/)
  assert.match(html, /Continua funcionando.*ler o que já foi carregado/)
  assert.match(html, /Indisponível por enquanto.*Enviar mensagens/)
  assert.match(html, /O que fazer.*Procurar atualizações/)
  assert.match(html, /Reconecta sozinho assim que for atualizado\./)
  assert.match(html, /Versão no computador: 2\.19\.0 · necessária: 2\.20\.1/)
  assert.doesNotMatch(html, /Verificando|danger|var\(--warning\)/)
  assert.doesNotMatch(html, /role="alert"/)
})

test('verificando: a linha discreta troca o texto e mostra o indicador; a leitura em voz alta não repete', () => {
  const html = render({ aviso: { ...computador, verificando: true } })
  assert.match(html, /aria-live="off"[^>]*>.*Verificando de novo…/)
  assert.doesNotMatch(html, /Reconecta sozinho/)
})

// Texto que o usuário lê: sem tags, com as entidades que o React escapa (ex.: apóstrofo em en).
const textoVisivel = (html: string) => html.replace(/<[^>]+>/g, ' ')
  .replace(/&#x27;/g, "'").replace(/&quot;/g, '"').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&')

test('versão ilegível: nos 6 idiomas, a frase traduzida aparece sem chave crua, undefined ou null', () => {
  assert.equal(APP_LANGUAGES.length, 6)
  for (const idioma of APP_LANGUAGES) {
    const html = render({ idioma, aviso: { ...computador, encontrada: null } })
    const visivel = textoVisivel(html)
    const esperado = tradutor(idioma)('navegacao.janela.desatualizado.versaoComputadorIlegivel', { minimo: '2.20.1' })
    assert.ok(visivel.includes(esperado), `${idioma}: frase da versão ilegível ausente`)
    assert.doesNotMatch(visivel, /navegacao\.|\bundefined\b|\bnull\b/, `${idioma}: chave crua, undefined ou null`)
    assert.doesNotMatch(html, /undefined|\bnull\b/, `${idioma}: undefined ou null no HTML`)
  }
})

test('aviso do app: título e texto próprios, sem versão do computador, com o botão que o conector passar', () => {
  const html = render({ aviso: { alvo: 'app', encontrada: '0.4.0', minima: '0.3.0', verificando: false }, como: 'Instale a atualização deste app.', acao: m.createElement('button', null, 'Atualizar') })
  assert.match(html, /Atualize este app para voltar a usar <bdi dir="auto">Computador de teste<\/bdi>/)
  assert.match(html, /Este app precisa da versão 0\.3\.0 ou mais nova\./)
  assert.match(html, /aviso-computador-acao"><button>Atualizar<\/button>/)
  assert.doesNotMatch(html, /Versão no computador/)
})

test('voltou ao normal: só a confirmação curta, em role=status', () => {
  const html = render({ fase: 'voltou', aviso: null })
  assert.match(html, /aviso-computador-voltou/)
  assert.match(html, /<bdi dir="auto">Computador de teste<\/bdi> foi atualizado e já está funcionando\./)
  assert.doesNotMatch(html, /<dl|button/)
})

test('árabe: o nome do computador fica isolado na frase da direita para a esquerda', () => {
  const html = render({ idioma: 'ar-AE' })
  assert.match(html, /يجب تحديث الحاسوب ⁨<bdi dir="auto">Computador de teste<\/bdi>⁩/)
})
