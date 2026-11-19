import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build, type Plugin } from 'esbuild'
import { CATALOGOS, traduzir } from './fork-i18n/index.ts'
import { cru, textoDe, ehFrase, ErroFrase, frase, plural } from './fork-i18n/frase.ts'
import type { Mensagem } from '../views/fork/bot-settings/mensagem.ts'
import { CatalogoStore } from '../views/fork/catalogo-skills/catalogo-store.ts'
import { frases } from '../views/fork/catalogo-skills/textos.ts'
import { tela } from '../views/fork/conector-card/modelo.ts'

// D454 (fatia 8b): catálogo de skills e cartão de conectores no idioma escolhido, com as lições das R1 do #565, #568,
// #572 e #577: estado guardado como frase (chave + parâmetros) e traduzido ao desenhar; código LTR isolado; lang/dir
// na raiz; conteúdo do usuário em dir="auto"; fechar e cancelar traduzidos; duais do árabe próprios.

test('frase: traduz no idioma pedido; texto de parâmetro (dado) entra isolado; ErroFrase guarda a frase', () => {
  const f = frase('skills.loja.instalada', { nome: 'Design' })
  assert.equal(textoDe(f, 'en'), '⁨Design⁩ installed.')
  assert.equal(textoDe(f, 'pt-BR'), '⁨Design⁩ instalada.')
  assert.match(textoDe(f, 'ar-AE'), /ثُبّتت/)
  const e = new ErroFrase(frase('skills.loja.semConexao'))
  assert.equal(e.message, 'No connection to the Hub right now. Try again when it’s back.')
  assert.equal(textoDe(e.frase, 'fr'), 'Pas de connexion au Hub pour l’instant. Réessayez quand il sera de retour.')
  assert.equal(textoDe(plural('skills.contagem', 2), 'ar-AE'), 'مهارتان')
})

test('D507: a frase é dado (chave tipada + parâmetros): clona, serializa e compara; chave e parâmetros pelo tipo', () => {
  const f = frase('skills.loja.falha.instalar', { nome: 'Alpha', motivo: frase('skills.loja.semConexao') })
  assert.deepEqual(f, { chave: 'skills.loja.falha.instalar', params: { nome: 'Alpha', motivo: { chave: 'skills.loja.semConexao' } } })
  assert.deepEqual(structuredClone(f), f)
  const volta = JSON.parse(JSON.stringify(f)) as unknown
  assert.ok(ehFrase(volta))
  assert.equal(textoDe(volta, 'en'), textoDe(f, 'en'))
  assert.deepEqual(plural('skills.contagem', 2), { plural: 'skills.contagem', n: 2 })
  assert.equal(ehFrase({ chave: 'x', outro: 1 }), false)
  // Conferido pelo typecheck: sem o erro de tipo, o `@ts-expect-error` sobra e o tsc falha.
  // @ts-expect-error chave fora do catálogo
  frase('skills.naoExiste')
  // @ts-expect-error falta o parâmetro {nome}
  frase('skills.loja.instalada')
  // @ts-expect-error parâmetro que a chave não tem
  frase('skills.loja.instalada', { outro: 'x' })
})

test('D507 e Kodus do #572: a Mensagem do #572 já é Frase; a recusa da Base central fica como dado', () => {
  const recusa: Mensagem = { chave: 'skills.semConexao' }
  const doHub: Mensagem = { texto: 'Hub says no' }
  assert.ok(ehFrase(recusa) && ehFrase(doHub))
  assert.equal(textoDe(recusa, 'en'), 'No connection to the Hub')
  assert.equal(textoDe(doHub, 'ar-AE'), 'Hub says no')
  assert.deepEqual(cru('x'), { texto: 'x' })
  const fonte = readFileSync(new URL('../views/fork/catalogo-skills/BaseCentral.tsx', import.meta.url), 'utf8')
  assert.match(fonte, /marcas\.recusarNoGesto\(bot, skill\.id, escolha\.refused\)/)
  assert.doesNotMatch(fonte, /textoAgora/)
})

test('R1 do #572: a loja guarda a frase; a mesma falha sai em inglês e em árabe sem reler nada', async () => {
  const loja = new CatalogoStore(async () => { throw new Error('não deve chamar') }, () => {}, () => ({ hub: 'h', client: null, geracao: 0 }))
  loja.sincronizar('h', { adiarCarga: true })
  loja.itens = [{ id: 'alpha', name: 'Alpha', installed: false, installable: true, digest: 'd' } as never]
  await loja.instalar(loja.itens[0]!, loja.capturarIntencao())
  const falha = loja.falhas.get('alpha')!
  const aviso = loja.aviso!
  assert.deepEqual(structuredClone(loja.falhas), loja.falhas, 'o que a loja guarda é dado, não closure')
  assert.equal(textoDe(falha, 'en'), 'No connection to the Hub right now. Try again when it’s back.')
  assert.match(textoDe(falha, 'ar-AE'), /لا يوجد اتصال بالـ Hub الآن/)
  assert.equal(textoDe(aviso, 'en'), 'Couldn’t install ⁨Alpha⁩: No connection to the Hub right now. Try again when it’s back.')
  assert.match(textoDe(aviso, 'ar-AE'), /تعذّر تثبيت/)
})

test('cartão de conector: selo e nota são frases; a recusa do Hub entra como dado, isolada', () => {
  const aberto = { suportado: true, apresentacao: true, conta: true, vencido: false, substituido: false }
  const t = tela('incerta', aberto)
  assert.equal(textoDe(t.selo, 'en'), 'Unconfirmed')
  assert.match(textoDe(t.nota!, 'ar-AE'), /لا نعرف إن كان قد خرج/)
  const recusa = tela('aguardando', { ...aberto, apresentacao: false, recusaLeitura: frase('conectores.cartao.nota.nadaEnviado') })
  assert.match(textoDe(recusa.nota!, 'en'), /^The Hub didn’t open this request in this app: Nothing was sent\./)
})

test('D507 (#580): contagens de itens da 8b têm o zero natural nos 6 idiomas; 1 e 2 não mudam', () => {
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
    for (const base of ['skills.contagem', 'skills.matriz.bots'] as const) {
      assert.doesNotMatch(textoDe(plural(base, 0), idioma), /0/, `${idioma}: ${base}`)
    }
  }
  assert.equal(textoDe(plural('skills.contagem', 0), 'pt-BR'), 'nenhuma skill')
  assert.equal(textoDe(plural('skills.matriz.bots', 0), 'en'), 'no bots')
  assert.equal(textoDe(plural('skills.contagem', 1), 'pt-BR'), '1 skill')
  assert.equal(textoDe(plural('skills.matriz.bots', 2), 'en'), '2 bots')
})

test('duais do árabe (lição da R1 do #570): a forma two não repete o singular nem usa "2 + singular"', () => {
  const ar = CATALOGOS['ar-AE'] as Record<string, string>
  const bases = Object.keys(ar).filter((k) => (k.startsWith('skills.') || k.startsWith('conectores.')) && k.endsWith('.two')).map((k) => k.slice(0, -4))
  assert.ok(bases.length >= 6)
  for (const b of bases) {
    assert.notEqual(ar[`${b}.two`], ar[`${b}.one`], b)
    assert.doesNotMatch(ar[`${b}.two`]!, /(^|\s)2\s/, b)
  }
})

test('código LTR e isolado, fechar traduzido e conteúdo do usuário em dir="auto" (fonte)', () => {
  const fonte = (a: string) => readFileSync(new URL(`../views/${a}`, import.meta.url), 'utf8')
  // Junção com a 8a: a página Skills e as instaladas saem do limite legado (a raiz delas declara lang/dir).
  assert.doesNotMatch(readFileSync(new URL('../views/marketplace/MarketplaceView.tsx', import.meta.url), 'utf8'), /LIMITE_LEGADO/)
  // Diff é código (R1 do #565).
  assert.match(fonte('fork/catalogo-skills/ComparaRascunho.tsx'), /className="cs-rev-diff selectable bidi-isolado" dir="ltr"/)
  // Todo ModalHeader da fatia passa o fechar do catálogo (R1-01 do #577).
  for (const a of ['fork/catalogo-skills/ComparaRascunho.tsx', 'fork/catalogo-skills/EscreverSkill.tsx', 'fork/catalogo-skills/DetalhesSkill.tsx', 'marketplace/SkillSheet.tsx']) {
    const s = fonte(a)
    assert.equal(s.split('<ModalHeader').length - 1, s.split('closeTitle=').length - 1, a)
  }
  // Cartão de aprovação: conta de saída LTR isolada; campos e rótulo da conta na direção do próprio dado.
  const cartao = fonte('fork/conector-card/ConectorCard.tsx')
  assert.match(cartao, /<bdi className="selectable conector-card-conta" dir="ltr">/)
  assert.match(cartao, /<bdi className="selectable conector-card-valor" dir="auto">/)
  assert.match(cartao, /<textarea className="conector-card-campo" dir="auto"/)
})

// O cartão real, renderizado no servidor em árabe (sem DOM no Node: hooks com snapshot de servidor só neste bundle).
const reactNoServidor: Plugin = {
  name: 'react-no-servidor',
  setup(b) {
    const real = (m: string) => JSON.stringify(createRequire(import.meta.url).resolve(m))
    b.onResolve({ filter: /^react$/ }, (args) => (args.namespace === 'shim' ? undefined : { path: 'react', namespace: 'shim' }))
    b.onResolve({ filter: /^react-dom$/ }, (args) => (args.namespace === 'shim' ? undefined : { path: 'react-dom', namespace: 'shim' }))
    b.onLoad({ filter: /^react-dom$/, namespace: 'shim' }, () => ({
      contents: `const D = require(${real('react-dom')}); module.exports = { ...D, createPortal: (filhos) => filhos };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
    b.onLoad({ filter: /^react$/, namespace: 'shim' }, () => ({
      contents: `const R = require(${real('react')}); module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
  },
}
const bundle = await build({
  stdin: {
    contents: `export { ConectorCard } from './views/fork/conector-card/ConectorCard';
      export { StoreContext } from './store/context';
      export { Popover } from './views/fork/catalogo-skills/BaseCentral';
      export { TextoPassivo } from './views/fork/catalogo-skills/TextoPassivo';
      export { idiomaPref } from './lib/fork-i18n/estado';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent', plugins: [reactNoServidor],
  alias: { '@shared': new URL('../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.png': 'empty', '.jpg': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp3': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const salvo = new Map([['appLanguage', JSON.stringify('ar-AE')]])
const storage = { getItem: (k: string) => salvo.get(k) ?? null, setItem: (k: string, v: string) => { salvo.set(k, v) }, removeItem() {} }
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'darwin' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage, innerWidth: 1200, innerHeight: 800 }
const document = { documentElement: { lang: 'pt-BR', dir: 'ltr', dataset: {}, style: { setProperty() {} } }, body: { setAttribute() {} }, addEventListener() {}, removeEventListener() {}, hidden: false }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage,
  navigator: { platform: 'MacIntel', language: 'ar-AE' }, fetch: () => new Promise(() => {}), TextEncoder, URLSearchParams, URL, Intl, console, setTimeout, clearTimeout, queueMicrotask,
  requestAnimationFrame: () => 0, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const m = module.exports as {
  ConectorCard: typeof import('../views/fork/conector-card/ConectorCard').ConectorCard
  StoreContext: typeof import('../store/context').StoreContext
  Popover: typeof import('../views/fork/catalogo-skills/BaseCentral').Popover
  TextoPassivo: typeof import('../views/fork/catalogo-skills/TextoPassivo').TextoPassivo
  idiomaPref: typeof import('./fork-i18n/estado').idiomaPref
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

test('cartão de aprovação de conector em árabe: lang/dir na raiz, título traduzido com o nome do bot isolado', () => {
  const store = { subscribe: () => () => {}, getVersion: () => 0, bots: new Map(), authorName: () => 'سارة', ready: () => new Promise(() => {}) }
  const entry = { id: 'e', rev: 1, data: { author: 'b', conectorOperation: { tentativaId: 't', cardId: 'c', state: 'aguardando', decisionProtocol: 'outro' } } }
  const html = m.renderToStaticMarkup(m.createElement(m.StoreContext.Provider, { value: store as never }, m.createElement(m.ConectorCard, { entry: entry as never })))
  assert.match(html, /<section class="approval-card conector-card idioma-superficie" lang="ar-AE" dir="rtl"/)
  assert.match(html, /aria-label="طلب [⁨]+سارة[⁩]+ إجراء موصل"/)
  // Protocolo desconhecido: não decide aqui, e o motivo vem do catálogo no idioma.
  assert.ok(html.includes('لا يمكن القرار هنا'))
  assert.doesNotMatch(html, /Não dá para decidir|Esperando você|Pedido de/)
  assert.ok(textoDe(frases.semConexao, 'ar-AE').length > 0)
})

test('R1-02 do #579: a dica da Base central guarda frases; trocar o idioma com ela aberta troca título, motivo e ação', () => {
  const alvo = { getBoundingClientRect: () => ({ top: 0, left: 0, bottom: 20, right: 20, width: 20, height: 20 }), focus() {} }
  const dica = {
    alvo: alvo as never,
    titulo: frase('skills.matriz.naoMudouEm', { onde: frase('skills.matriz.onde', { skill: 'Design', bot: 'Ana' }) }),
    texto: frase('skills.semConexao'),
    acao: { rotulo: frase('skills.matriz.lerDeNovo'), fazer: () => {} },
  }
  // A dica é dado: nada de texto pronto guardado.
  assert.deepEqual(structuredClone({ ...dica, alvo: null, acao: { ...dica.acao, fazer: null } }), { ...dica, alvo: null, acao: { ...dica.acao, fazer: null } })
  const desenhar = () => m.renderToStaticMarkup(m.createElement(m.Popover, { dica, onClose: () => {} }))
  m.idiomaPref.set('ar-AE')
  const ar = desenhar()
  m.idiomaPref.set('en')
  const en = desenhar()
  m.idiomaPref.set('ar-AE')
  assert.match(ar, /lang="ar-AE" dir="rtl"/)
  assert.ok(ar.includes(textoDe(dica.texto, 'ar-AE')) && ar.includes(textoDe(dica.acao.rotulo, 'ar-AE')))
  assert.match(en, /lang="en" dir="ltr"/)
  assert.ok(en.includes('No connection to the Hub') && en.includes(textoDe(dica.acao.rotulo, 'en')))
  assert.notEqual(textoDe(dica.acao.rotulo, 'en'), textoDe(dica.acao.rotulo, 'ar-AE'))
})

test('R1-03 do #579: instruções da skill em dir="auto"; bloco e trecho de código LTR, mesmo começando em árabe', () => {
  const md = 'Use this skill carefully.\n\nشغّل `أداة --x` assim:\n\n```\n./أداة --port 3000\n```'
  const html = m.renderToStaticMarkup(m.createElement(m.TextoPassivo, { texto: md }))
  assert.match(html, /<div class="cs-texto-passivo" dir="auto"/)
  assert.match(html, /<pre class="md-code-text" dir="ltr">\.\/أداة --port 3000<\/pre>/)
  assert.match(html, /<code class="md-code-span" dir="ltr">أداة --x<\/code>/)
})

test('R1-01 do #579: a explicação da skill adaptada segue o idioma da tela (nada forçado em pt-BR na fatia)', () => {
  for (const a of ['fork/catalogo-skills/DetalhesSkill.tsx', 'fork/catalogo-skills/BaseCentral.tsx', 'fork/catalogo-skills/CatalogoSkills.tsx', 'fork/catalogo-skills/SkillsInstaladas.tsx', 'fork/catalogo-skills/partes.tsx', 'marketplace/SkillSheet.tsx']) {
    assert.doesNotMatch(readFileSync(new URL(`../views/${a}`, import.meta.url), 'utf8'), /traduzir\('pt-BR'/, a)
  }
  assert.match(readFileSync(new URL('../views/fork/catalogo-skills/DetalhesSkill.tsx', import.meta.url), 'utf8'), /traduzir\(idioma, 'marca\.skills\.adaptada'\)/)
  assert.notEqual(traduzir('en', 'marca.skills.adaptada'), traduzir('pt-BR', 'marca.skills.adaptada'))
  assert.notEqual(traduzir('ar-AE', 'marca.skills.adaptada'), traduzir('en', 'marca.skills.adaptada'))
})
