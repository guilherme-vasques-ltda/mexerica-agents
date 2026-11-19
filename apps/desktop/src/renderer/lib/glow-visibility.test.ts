import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { ATRIBUTO_OCIOSA, ATRIBUTO_OCULTA, EVENTOS_INTERACAO, OCIOSIDADE_MS, acompanharJanelaOculta } from './glow-visibility.ts'

// MX-1287 (D2): halo do header. Atributo no root pela visibilidade; CSS só com tokens, só opacity, parado com movimento reduzido.
function docFalso(estado: string, foco = true) {
  const attrs = new Set<string>()
  const ouvintes = new Set<() => void>()
  const d = {
    attrs, ouvintes,
    visibilityState: estado,
    foco,
    hasFocus: () => d.foco,
    documentElement: { setAttribute: (n: string) => void attrs.add(n), removeAttribute: (n: string) => void attrs.delete(n) },
    addEventListener: (_t: 'visibilitychange', cb: () => void) => void ouvintes.add(cb),
    removeEventListener: (_t: 'visibilitychange', cb: () => void) => void ouvintes.delete(cb),
  }
  return d
}
function janelaFalsa() {
  const ouvintes = new Map<string, Set<() => void>>()
  return {
    ouvintes,
    total: () => [...ouvintes.values()].reduce((n, c) => n + c.size, 0),
    disparar: (t: string) => ouvintes.get(t)?.forEach((cb) => cb()),
    addEventListener: (t: string, cb: () => void, _o?: unknown) => void (ouvintes.get(t) ?? ouvintes.set(t, new Set()).get(t)!).add(cb),
    removeEventListener: (t: string, cb: () => void, _o?: unknown) => void ouvintes.get(t)?.delete(cb),
  }
}

test('o atributo segue a visibilidade e some ao desligar', () => {
  const d = docFalso('visible')
  const j = janelaFalsa()
  const parar = acompanharJanelaOculta(d, j)
  assert.equal(d.attrs.has(ATRIBUTO_OCULTA), false)
  d.visibilityState = 'hidden'
  d.ouvintes.forEach((cb) => cb())
  assert.equal(d.attrs.has(ATRIBUTO_OCULTA), true)
  d.visibilityState = 'visible'
  d.ouvintes.forEach((cb) => cb())
  assert.equal(d.attrs.has(ATRIBUTO_OCULTA), false)
  d.visibilityState = 'hidden'
  d.ouvintes.forEach((cb) => cb())
  parar()
  assert.equal(d.ouvintes.size, 0)
  assert.equal(j.total(), 0)
  assert.equal(d.attrs.has(ATRIBUTO_OCULTA), false)
})

test('perder o foco (janela ainda visível) pausa e voltar ao foco retoma; listeners somem ao desligar', () => {
  const d = docFalso('visible')
  const j = janelaFalsa()
  const parar = acompanharJanelaOculta(d, j)
  d.foco = false
  j.disparar('blur')
  assert.equal(d.attrs.has(ATRIBUTO_OCULTA), true)
  d.foco = true
  j.disparar('focus')
  assert.equal(d.attrs.has(ATRIBUTO_OCULTA), false)
  d.foco = false
  j.disparar('blur')
  parar()
  assert.equal(j.total(), 0)
  assert.equal(d.attrs.has(ATRIBUTO_OCULTA), false)
})

test('começa oculto quando a janela já nasce em segundo plano', () => {
  const d = docFalso('hidden')
  const parar = acompanharJanelaOculta(d, janelaFalsa(), relogioFalso())
  assert.equal(d.attrs.has(ATRIBUTO_OCULTA), true)
  parar()
})

const css = readFileSync(new URL('../views/shell/janela-header.css', import.meta.url), 'utf8')
const glow = css.slice(css.indexOf('.janela-glow {'))
const tokens = readFileSync(new URL('../styles/tokens.css', import.meta.url), 'utf8')

test('tokens do glow existem nos dois temas e o respiro é de 9 s', () => {
  assert.match(tokens, /--glow-respira: 9s;/)
  for (const t of ['pico', 'vale', 'parado']) assert.equal(tokens.match(new RegExp(`--glow-halo-${t}:`, 'g'))?.length, 2, t)
  const claro = Number(/\[data-theme="light"\] \{[^}]*--glow-halo-pico: ([\d.]+)/s.exec(tokens)![1])
  const escuro = Number(/\[data-theme="dark"\] \{[^}]*--glow-halo-pico: ([\d.]+)/s.exec(tokens)![1])
  assert.ok(claro < escuro, 'halo mais fraco no claro')
})

test('CSS do glow: só opacity anima, valores por token, parado com movimento reduzido e em segundo plano', () => {
  const frames = glow.slice(glow.indexOf('@keyframes'), glow.indexOf('/* Janela em segundo plano'))
  assert.deepEqual([...new Set([...frames.matchAll(/([\w-]+):\s*[^;]+;/g)].map((m) => m[1]))], ['opacity'])
  assert.doesNotMatch(glow, /filter|box-shadow|transform/)
  assert.match(glow, /animation: janela-glow-respira var\(--glow-respira\) var\(--glow-quadros\) infinite/)
  assert.match(glow, /html\[data-janela-oculta\] \.janela-glow-halo,\s*html\[data-janela-ociosa\] \.janela-glow-halo \{\s*animation-play-state: paused/)
  assert.match(glow, /prefers-reduced-motion: reduce\) \{\s*\.janela-glow-halo \{\s*animation: none/)
  const semComentarios = glow.replace(/\/\*[\s\S]*?\*\//g, '')
  assert.doesNotMatch(semComentarios, /#[0-9a-f]{3,8}\b/i, 'sem cor fixa')
  assert.doesNotMatch(semComentarios, /(?<![\w-])\d+(\.\d+)?m?s\b/, 'sem duração fixa')
})

test('a faixa fica intacta: nada acima da linha nem atrás da logo', () => {
  assert.match(glow, /\.janela-glow \{[^}]*top: var\(--janela-header-h\)/)
})

test('o glow sai do .split (portal no body, acima das telas z-40) e a pausa vale para segundo plano', () => {
  assert.match(glow, /\.janela-glow \{[^}]*position: fixed[^}]*z-index: 45/s)
  const tsx = readFileSync(new URL('../views/shell/JanelaHeader.tsx', import.meta.url), 'utf8')
  assert.match(tsx, /createPortal\([\s\S]*document\.body/)
})

// Relógio falso: o tempo só anda quando o teste manda.
function relogioFalso() {
  let t = 0
  let id = 0
  const timers = new Map<number, { em: number; cb: () => void }>()
  return {
    agora: () => t,
    setTimeout: (cb: () => void, ms: number) => { timers.set(++id, { em: t + ms, cb }); return id },
    clearTimeout: (i: unknown) => void timers.delete(i as number),
    pendentes: () => timers.size,
    avancar(ms: number) {
      const fim = t + ms
      for (;;) {
        const prox = [...timers.entries()].filter(([, x]) => x.em <= fim).sort((a, b) => a[1].em - b[1].em)[0]
        if (!prox) break
        timers.delete(prox[0]); t = prox[1].em; prox[1].cb()
      }
      t = fim
    },
  }
}

test('60 s sem interação pausa por ociosidade; qualquer interação retoma e rearma', () => {
  const d = docFalso('visible')
  const j = janelaFalsa()
  const r = relogioFalso()
  const parar = acompanharJanelaOculta(d, j, r)
  r.avancar(OCIOSIDADE_MS - 1)
  assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), false)
  r.avancar(1)
  assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), true)
  assert.equal(d.attrs.has(ATRIBUTO_OCULTA), false, 'ociosa não é segundo plano')
  j.disparar('pointermove')
  assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), false)
  r.avancar(OCIOSIDADE_MS - 1)
  assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), false)
  r.avancar(1)
  assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), true)
  parar()
  assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), false)
})

test('interagir adia a pausa sem criar um timer por evento', () => {
  const d = docFalso('visible')
  const j = janelaFalsa()
  const r = relogioFalso()
  const parar = acompanharJanelaOculta(d, j, r)
  for (let i = 0; i < 50; i++) { r.avancar(1_000); j.disparar('pointermove'); j.disparar('keydown') }
  assert.equal(r.pendentes(), 1, 'um timer só, por mais eventos que chegarem')
  r.avancar(OCIOSIDADE_MS - 1_000)
  assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), false, 'a última interação foi há 59 s')
  r.avancar(1_000)
  assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), true)
  parar()
  assert.equal(r.pendentes(), 0, 'desligar cancela o timer')
})

test('cada entrada conta como interação, o foco também, e os ouvintes saem ao desligar', () => {
  for (const evento of [...EVENTOS_INTERACAO, 'focus']) {
    const d = docFalso('visible')
    const j = janelaFalsa()
    const r = relogioFalso()
    const parar = acompanharJanelaOculta(d, j, r)
    r.avancar(OCIOSIDADE_MS)
    assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), true, evento)
    j.disparar(evento)
    assert.equal(d.attrs.has(ATRIBUTO_OCIOSA), false, evento)
    parar()
    assert.equal(j.total(), 0, evento)
  }
})

test('os ouvintes de interação são passivos e de captura', () => {
  const opcoes: unknown[] = []
  const j = { ...janelaFalsa(), addEventListener: (t: string, _cb: () => void, o?: unknown) => { if ((EVENTOS_INTERACAO as readonly string[]).includes(t)) opcoes.push(o) } }
  acompanharJanelaOculta(docFalso('visible'), j, relogioFalso())
  assert.equal(opcoes.length, EVENTOS_INTERACAO.length)
  for (const o of opcoes) assert.deepEqual(o, { passive: true, capture: true })
})

test('loops ambientes: o CSS pausa só o que é enfeite quando ocioso (bot dormindo, Mexeriquinha livre/dormindo)', () => {
  const avatar = readFileSync(new URL('../components/avatar.css', import.meta.url), 'utf8')
  assert.match(avatar, /html\[data-janela-ociosa\] \.face-state\[data-state="sleep"\] > \.donnabot-avatar \{ animation: none/)
  assert.doesNotMatch(avatar, /data-janela-ociosa\][^{]*data-state="(work|listen)"/, 'trabalhando/aguardando seguem animando')
  const mxq = readFileSync(new URL('./mexeriquinhas/mexeriquinhas.css', import.meta.url), 'utf8')
  const ocioso = mxq.slice(mxq.indexOf('html[data-janela-ociosa]'))
  assert.doesNotMatch(ocioso, /trabalhando|precisa-resposta|erro/, 'só livre e dormindo')
  assert.match(ocioso, /mxq--livre[^{]*\{|mxq--dormindo/)
})

test('o glow usa degraus (tokens), camada própria contida e o halo promove só opacity', () => {
  assert.match(tokens, /--glow-quadros: steps\(\d+\);/)
  assert.doesNotMatch(tokens, /--glow-ease/)
  assert.match(glow, /\.janela-glow \{[^}]*contain: layout paint style/s)
  assert.match(glow, /\.janela-glow-halo \{[^}]*will-change: opacity/s)
})
