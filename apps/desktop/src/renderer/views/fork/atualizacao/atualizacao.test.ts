import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build, type Plugin } from 'esbuild'
import { ltr } from '../../../../shared/bidi.ts'
import { RELEASE_TAG_URL_PREFIX, type ReleaseNoticeState } from '../../../../shared/fork-release-notice.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'

// Nível 1 de atualizações: renderização no servidor das duas views puras (a mesma técnica de
// ajustes-bidi.test.ts — sem DOM no Node, os hooks de estado do app usam o próprio getSnapshot).

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
    contents: `export { AvisoAtualizacaoView } from './fork/atualizacao/AvisoAtualizacao';
      export { AtualizacaoAjusteView } from './fork/atualizacao/AtualizacaoAjuste';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent', plugins: [reactNoServidor],
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.png': 'empty', '.jpg': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp3': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})

const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const storage = { getItem: (chave: string) => (chave === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} }
const windowStub = { addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage, innerWidth: 1200, innerHeight: 800 }
const documentStub = { documentElement: { lang: 'pt-BR', dir: 'ltr', dataset: {}, style: { setProperty() {} } }, body: { setAttribute() {} }, addEventListener() {}, removeEventListener() {}, hidden: false }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module, exports: module.exports, require: createRequire(import.meta.url), window: windowStub, document: documentStub, matchMedia: media, localStorage: storage,
  navigator: { platform: 'MacIntel', language: 'pt-BR' }, fetch: () => new Promise(() => {}), TextEncoder, URLSearchParams, URL, Intl, console, setTimeout, clearTimeout, queueMicrotask,
  requestAnimationFrame: () => 0, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const m = module.exports as {
  AvisoAtualizacaoView: import('react').ComponentType<{ state: ReleaseNoticeState | null; onOpenDownload: () => void; onDismiss: () => void }>
  AtualizacaoAjusteView: import('react').ComponentType<{ state: ReleaseNoticeState; onCheck: () => void; onOpenDownload: () => void }>
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}
const t = tradutor('pt-BR')

const TAG_URL = `${RELEASE_TAG_URL_PREFIX}v0.2.0`
const estado = (mudanca: Partial<ReleaseNoticeState> = {}): ReleaseNoticeState => ({
  supported: true, currentVersion: '0.1.0', status: 'available', availableVersion: '0.2.0', availableTag: 'v0.2.0',
  releaseUrl: TAG_URL, dismissedVersion: null, lastCheckAt: null, ...mudanca,
})
const aviso = (state: ReleaseNoticeState | null) =>
  m.renderToStaticMarkup(m.createElement(m.AvisoAtualizacaoView, { state, onOpenDownload: () => {}, onDismiss: () => {} }))
const ajuste = (state: ReleaseNoticeState) =>
  m.renderToStaticMarkup(m.createElement(m.AtualizacaoAjusteView, { state, onCheck: () => {}, onOpenDownload: () => {} }))

// Texto que a pessoa lê: sem tags, com as entidades que o React escapa.
const textoVisivel = (html: string) => html.replace(/<[^>]+>/g, ' ')
  .replace(/&#x27;/g, "'").replace(/&quot;/g, '"').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&')

test('aviso: com versão nova, a pílula diz a versão isolada e traz os dois botões', () => {
  const html = aviso(estado())
  const frase = t('ajustes.atualizacoes.novaVersao', { versao: ltr('v0.2.0') })
  assert.ok(textoVisivel(html).includes(frase), 'a frase da versão nova não apareceu')
  assert.ok(html.includes(t('ajustes.atualizacoes.verBaixar')))
  assert.ok(html.includes(t('ajustes.atualizacoes.dispensar')))
  assert.match(html, /role="status"/)
})

test('aviso: dispensada, sem suporte ou sem versão, nada renderiza', () => {
  assert.equal(aviso(estado({ dismissedVersion: '0.2.0' })), '')
  assert.equal(aviso(estado({ supported: false })), '')
  assert.equal(aviso(null), '')
  assert.equal(aviso(estado({ availableVersion: null, availableTag: null, releaseUrl: null })), '')
})

test('ajuste: mostra a versão atual isolada e o botão de procurar', () => {
  const html = ajuste(estado({ status: 'idle', availableVersion: null, availableTag: null, releaseUrl: null }))
  const visivel = textoVisivel(html)
  assert.ok(html.includes('dir="ltr"'), 'a versão fica da esquerda para a direita, inclusive em árabe')
  assert.ok(visivel.includes('v0.1.0'))
  assert.ok(html.includes(t('ajustes.atualizacoes.titulo')))
  assert.ok(html.includes(t('ajustes.atualizacoes.versaoAtual')))
  assert.ok(html.includes(t('ajustes.atualizacoes.procurar')))
  assert.doesNotMatch(html, /disabled/)
})

test('ajuste: procurando desabilita o botão e troca o rótulo', () => {
  const html = ajuste(estado({ status: 'checking', availableVersion: null, availableTag: null, releaseUrl: null }))
  assert.ok(html.includes(t('ajustes.atualizacoes.procurando')))
  assert.ok(html.includes('disabled'))
  assert.ok(!html.includes(t('ajustes.atualizacoes.procurar')), 'o rótulo de parado não aparece enquanto procura')
})

test('ajuste: versão nova aparece com o botão de ver e baixar', () => {
  const html = ajuste(estado())
  const visivel = textoVisivel(html)
  assert.ok(visivel.includes(t('ajustes.atualizacoes.novaVersao', { versao: ltr('v0.2.0') })))
  assert.ok(html.includes(t('ajustes.atualizacoes.verBaixar')))
  assert.ok(!visivel.includes(t('ajustes.atualizacoes.atualizado')))
})

test('ajuste: na versão mais recente, a frase calma; no erro, a frase de atenção', () => {
  const atualizado = ajuste(estado({ status: 'up-to-date', availableVersion: null, availableTag: null, releaseUrl: null }))
  assert.ok(textoVisivel(atualizado).includes(t('ajustes.atualizacoes.atualizado')))
  assert.ok(!atualizado.includes(t('ajustes.atualizacoes.verBaixar')))

  const erro = ajuste(estado({ status: 'error', availableVersion: null, availableTag: null, releaseUrl: null }))
  assert.ok(textoVisivel(erro).includes(t('ajustes.atualizacoes.erro')))
  assert.match(erro, /var\(--text-attention, var\(--attention\)\)/, 'erro em tom de atenção, não de perigo')
  assert.doesNotMatch(erro, /danger/)
})

test('ajuste: versão atual que já vem com v não dobra o prefixo', () => {
  const html = ajuste(estado({ status: 'idle', availableVersion: null, availableTag: null, releaseUrl: null, currentVersion: 'v0.1.0' }))
  const visivel = textoVisivel(html)
  assert.ok(visivel.includes('v0.1.0'))
  assert.doesNotMatch(visivel, /vv/)
})

test('ajuste: erro da checagem manual aparece mesmo com o cache de versão nova na tela', () => {
  const html = ajuste(estado({ status: 'error' }))
  const visivel = textoVisivel(html)
  assert.ok(visivel.includes(t('ajustes.atualizacoes.novaVersao', { versao: ltr('v0.2.0') })))
  assert.ok(visivel.includes(t('ajustes.atualizacoes.erro')))
})

test('css: o botão de dispensar tem anel de foco visível pelo teclado', () => {
  const css = readFileSync(new URL('./aviso-atualizacao.css', import.meta.url), 'utf8')
  assert.match(css, /\.aviso-atualizacao-dispensar:focus-visible/)
  assert.match(css, /var\(--focus-ring, var\(--accent\)\)/)
})
