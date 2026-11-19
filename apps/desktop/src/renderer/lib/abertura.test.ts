// MX-93: a abertura sorteada do index.html (Pingo ou Sesta). Confere o que o navegador não avisa: CSP, sorteio,
// textos iguais aos catálogos, fundo igual ao token e entrada dentro da meta de 700 ms (MX-94).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { IDIOMAS_CATALOGO } from './fork-i18n/registro.ts'
import { sairDaAbertura, criarSaidaDaAbertura, esperaRestanteMs, MIN_SPLASH_MS } from './abertura.ts'

const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8')
const script = html.slice(html.indexOf('<script>') + '<script>'.length, html.indexOf('</script>'))
const css = html.slice(html.indexOf('<style>'), html.indexOf('</style>'))
const META = 700

test('a CSP libera só o script da abertura, pelo hash', () => {
  const hash = createHash('sha256').update(script, 'utf8').digest('base64')
  const csp = /Content-Security-Policy" content="([^"]+)"/.exec(html)?.[1] ?? ''
  assert.match(csp, new RegExp(`script-src 'self' 'sha256-${hash.replace(/[+/]/g, '\\$&')}';`))
  assert.doesNotMatch(/script-src[^;]*/.exec(csp)![0], /unsafe/, 'script-src sem unsafe-inline nem unsafe-eval')
  assert.equal(html.match(/<script>/g)?.length, 1, 'um único script inline')
})

/** Roda o script do index.html com um localStorage e uma URL de mentira; devolve o <html> e o que ficou salvo. */
function abrir(salvo: Record<string, string>, janela = 'chat', sorte = 0.2) {
  const store = new Map(Object.entries(salvo))
  const dataset: Record<string, string> = {}
  const localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) }
  const Math_ = Object.create(Math, { random: { value: () => sorte } })
  runInNewContext(script, { document: { documentElement: { dataset } }, window: { localStorage }, localStorage, location: { search: janela ? `?window=${janela}` : '' }, URLSearchParams, JSON, Math: Math_ })
  return { dataset, store }
}

test('sorteia na primeira abertura e nunca repete a mesma em seguida', () => {
  assert.equal(abrir({}, 'chat', 0.2).dataset.mxAbertura, 'pingo')
  assert.equal(abrir({}, 'chat', 0.8).dataset.mxAbertura, 'sesta')
  let salvo: Record<string, string> = {}
  const vistas: string[] = []
  for (let i = 0; i < 6; i++) {
    const { dataset, store } = abrir(salvo, 'chat', Math.random())
    vistas.push(dataset.mxAbertura!)
    salvo = Object.fromEntries(store)
  }
  for (let i = 1; i < vistas.length; i++) assert.notEqual(vistas[i], vistas[i - 1])
  assert.equal(abrir({ aberturaUltima: '"lixo"' }, 'chat', 0.9).dataset.mxAbertura, 'sesta')
  assert.equal(abrir({ aberturaUltima: '{' }, 'chat', 0.1).dataset.mxAbertura, 'pingo')
})

test('só a janela principal tem abertura; sem ?window é a principal', () => {
  assert.equal(abrir({}, 'island').dataset.mxAbertura, 'nenhuma')
  assert.equal(abrir({}, 'pairing').dataset.mxAbertura, 'nenhuma')
  assert.equal(abrir({}, 'island').store.has('aberturaUltima'), false)
  assert.match(abrir({}, '').dataset.mxAbertura!, /^(pingo|sesta)$/)
})

test('tema e idioma salvos chegam antes da primeira pintura', () => {
  assert.equal(abrir({ appearanceTheme: '"dark"' }).dataset.theme, 'dark')
  assert.equal(abrir({ appearanceTheme: '"light"' }).dataset.theme, 'light')
  assert.equal(abrir({ appearanceTheme: '"system"' }).dataset.theme, undefined)
  assert.equal(abrir({}).dataset.mxIdioma, 'en')
  assert.equal(abrir({ appLanguage: '"zh-CN"' }).dataset.mxIdioma, 'zh-CN')
  assert.equal(abrir({ appLanguage: '"xx"' }).dataset.mxIdioma, 'en')
})

test('frases e status do index.html são os dos catálogos, nos seis idiomas', async () => {
  for (const idioma of IDIOMAS_CATALOGO) {
    const { textos } = (await import(`./fork-i18n/${idioma}/marca.ts`)) as { textos: Record<string, string> }
    const p = (classe: string) => [...html.matchAll(new RegExp(`<p class="${classe} mx-txt" lang="${idioma}"[^>]*>([^<]*)</p>`, 'g'))].map((m) => m[1])
    assert.deepEqual(p('mxa-frase'), [textos['marca.abertura.pingo']], `${idioma}: Pingo`)
    assert.deepEqual(p('mxb-frase'), [textos['marca.abertura.sesta']], `${idioma}: Sesta`)
    assert.deepEqual(p('mx-sr'), [textos['marca.abertura.status']], `${idioma}: status`)
    assert.ok(css.includes(`[data-mx-idioma=${idioma}] #mx-abertura .mx-txt[lang=${idioma}]`), `${idioma}: regra de exibição`)
  }
})

test('o fundo é o --canvas do app, com o mesmo valor antes de o CSS do app carregar', () => {
  const tokens = readFileSync(new URL('../styles/tokens.css', import.meta.url), 'utf8')
  const canvas = (bloco: string) => new RegExp(`${bloco}[^}]*?--canvas:\\s*(#[0-9a-f]+)`, 'i').exec(tokens)?.[1]
  const claro = canvas(':root, \\[data-theme="light"\\] \\{')
  const escuro = canvas('\\[data-theme="dark"\\] \\{')
  for (const cena of ['mxa', 'mxb']) {
    assert.ok(css.includes(`.${cena}{--fundo:var(--canvas,${claro})`), `${cena} claro`)
    assert.ok(css.includes(`@media (prefers-color-scheme:dark){.${cena}{--fundo:var(--canvas,${escuro})`), `${cena} escuro`)
    assert.ok(css.includes(`[data-theme=dark] .${cena}{--fundo:var(--canvas,${escuro})`), `${cena} escuro forçado`)
  }
})

/** Divide por vírgulas fora de parênteses (cubic-bezier). */
const partes = (valor: string) => valor.split(/,(?![^(]*\))/)
const ms = (t: string) => (t.endsWith('ms') ? Number(t.slice(0, -2)) : Number(t.slice(0, -1)) * 1000)

test(`a entrada das duas cenas termina em até ${META} ms; depois disso só o ambiente em loop`, () => {
  const fora: string[] = []
  for (const [, valor] of css.matchAll(/animation:([^;}]+)/g)) {
    for (const parte of partes(valor)) {
      const tokens = parte.trim().split(/\s+/)
      if (tokens.includes('infinite') || tokens[0] === 'none') continue
      // A folha que se solta no tranco da A (cai devagar, atrás da logo) é ambiente, fora da entrada.
      if (tokens[0] === 'mxa-desce') continue
      const tempos = tokens.filter((t) => /^-?[\d.]+m?s$/.test(t)).map(ms)
      const fim = (tempos[0] ?? 0) + (tempos[1] ?? 0)
      if (fim > META) fora.push(`${parte.trim()} → ${fim} ms`)
    }
  }
  for (const [, duracao, atraso] of css.matchAll(/animation-duration:([\d.]+m?s);animation-delay:([\d.]+m?s)/g)) {
    if (ms(duracao) + ms(atraso) > META) fora.push(`${duracao} + ${atraso}`)
  }
  assert.deepEqual(fora, [])
})

test('movimento reduzido: nenhuma animação nas duas cenas, só o quadro final', () => {
  for (const cena of ['mxa', 'mxb']) assert.ok(css.includes(`@media (prefers-reduced-motion:reduce){\n .${cena} *{animation:none!important}`), cena)
})

// --- Saída (lib/abertura.ts). ---

test('a abertura sai do DOM na hora (sem fade), e chamar de novo não quebra', () => {
  let removida = 0
  const raiz = { remove: () => void removida++ }
  const doc = { getElementById: (id: string) => (id === 'mx-abertura' && removida === 0 ? raiz : null) } as unknown as Document
  sairDaAbertura(doc)
  sairDaAbertura(doc)
  assert.equal(removida, 1)
})

test('sem CSS de saída: nada de fade que atrase a primeira pintura do app', () => {
  assert.doesNotMatch(css, /saindo|mx[ab]-sai/)
})

// --- Tempo mínimo (decisão de UX do dono, 16/11/2026): só a exibição; o tempo real de pronto segue medido à parte. ---

/** Relógio de mentira: `avancar` roda os timers vencidos. */
function relogio() {
  let t = 0
  let id = 0
  const timers = new Map<number, { em: number; fn: () => void }>()
  const eventos: string[] = []
  const registros: Array<[number, number]> = []
  const r = {
    now: () => t,
    setTimer: (fn: () => void, ms: number) => (timers.set(++id, { em: t + ms, fn }), id),
    clearTimer: (i: unknown) => void timers.delete(i as number),
    remover: () => void eventos.push(`remove@${t}`),
    registrar: (pronto: number, exibido: number) => void registros.push([pronto, exibido]),
  }
  const avancar = (ate: number) => {
    for (const [i, x] of [...timers].sort((a, b) => a[1].em - b[1].em)) {
      if (x.em <= ate) { t = x.em; timers.delete(i); x.fn() }
    }
    t = ate
  }
  return { r, avancar, eventos, registros, timers, em: (ms: number) => void (t = ms) }
}

test('MIN_SPLASH_MS é 2000 ms', () => assert.equal(MIN_SPLASH_MS, 2000))

test('pronto em 300 ms: a abertura espera até 2000 ms', () => {
  const c = relogio()
  const saida = criarSaidaDaAbertura(c.r)
  c.em(300)
  saida.pronta()
  assert.deepEqual(c.eventos, [])
  c.avancar(1999)
  assert.deepEqual(c.eventos, [])
  c.avancar(2000)
  assert.deepEqual(c.eventos, ['remove@2000'])
  assert.deepEqual(c.registros, [[300, 2000]], 'tempo real de pronto (300) separado do exibido (2000)')
})

test('pronto em 3500 ms: sai na hora, sem acrescentar atraso', () => {
  const c = relogio()
  const saida = criarSaidaDaAbertura(c.r)
  c.em(3500)
  saida.pronta()
  assert.deepEqual(c.eventos, ['remove@3500'])
  assert.equal(c.timers.size, 0)
  assert.deepEqual(c.registros, [[3500, 3500]])
  assert.equal(esperaRestanteMs(3500), 0)
  assert.equal(esperaRestanteMs(2000), 0)
  assert.equal(esperaRestanteMs(300), 1700)
})

test('movimento reduzido: mesma regra de tempo, com a cena estática por CSS', () => {
  // A cena estática é só o CSS (nenhuma animação); o relógio não distingue, então o mínimo vale igual.
  for (const cena of ['mxa', 'mxb']) assert.ok(css.includes(`@media (prefers-reduced-motion:reduce){\n .${cena} *{animation:none!important}`), cena)
  const c = relogio()
  const saida = criarSaidaDaAbertura(c.r)
  c.em(300)
  saida.pronta()
  c.avancar(2000)
  assert.deepEqual(c.eventos, ['remove@2000'])
})

test('erro de carga sai na hora e cancela a espera dos 2 s', () => {
  const c = relogio()
  const saida = criarSaidaDaAbertura(c.r)
  c.em(300)
  saida.pronta()
  c.avancar(500)
  saida.erro()
  assert.deepEqual(c.eventos, ['remove@500'])
  c.avancar(2500)
  assert.deepEqual(c.eventos, ['remove@500'], 'o timer cancelado não remove de novo')
  const d = relogio()
  const semPronta = criarSaidaDaAbertura(d.r)
  d.em(120)
  semPronta.erro()
  assert.deepEqual(d.eventos, ['remove@120'], 'erro antes de ficar pronto não espera o mínimo')
})
