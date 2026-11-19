import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { ESCONDER_APOS_MS, finalDoTelefone, mascararTelefone, numeroAMostra, revelar, vigiarRevelado, type Revelacao } from './numero-bot.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import { estadoNumero } from './caixa.ts'
import type { BotWhatsapp } from './regras.ts'

// D374 (D70): o número do bot escondido por padrão, sem o número real no DOM nem no leitor de tela; revelar volta
// sozinho aos 10 s, ao perder o foco ou ao ocultar a página; nada é salvo. Número fictício.
const FICTICIO = '+5511900000150'

test('máscara: só os 2 últimos dígitos; leitor de tela ouve "Número escondido, termina em 50"', () => {
  assert.equal(mascararTelefone(FICTICIO), '+55 •• ••••-••50')
  assert.equal(mascararTelefone('+12025550199'), '+•• •••• ••99')
  assert.equal(finalDoTelefone(FICTICIO), '50')
  assert.equal(traduzir('pt-BR', 'whatsapp.numero.escondido', { fim: finalDoTelefone(FICTICIO) }), 'Número escondido, termina em 50')
  assert.equal(traduzir('en', 'whatsapp.numero.escondido', { fim: '50' }), 'Number hidden, ends in 50')
})

/** Janela, documento e relógio de mentira, para ver o que esconde. */
function palco() {
  const ouvintes = new Map<string, () => void>()
  const alvo = { addEventListener: (t: string, f: () => void) => { ouvintes.set(t, f) }, removeEventListener: (t: string) => { ouvintes.delete(t) } }
  const doc = { ...alvo, visibilityState: 'visible' }
  let agendado: { fn: () => void; ms: number } | null = null
  const relogio = { setTimeout: (fn: () => void, ms: number) => { agendado = { fn, ms }; return 1 }, clearTimeout: () => { agendado = null } }
  return { ouvintes, alvo, doc, relogio, passar: () => agendado?.fn(), agendado: () => agendado }
}

test('revelado volta a esconder aos 10 s, ao perder o foco e ao ocultar a página; a limpeza tira tudo', () => {
  let escondeu = 0
  const p = palco()
  const limpar = vigiarRevelado(() => escondeu++, p.alvo, p.doc, p.relogio)
  assert.equal(p.agendado()?.ms, ESCONDER_APOS_MS)
  assert.equal(ESCONDER_APOS_MS, 10_000)
  p.passar()
  assert.equal(escondeu, 1, '10 s')
  p.ouvintes.get('blur')!()
  assert.equal(escondeu, 2, 'janela perdeu o foco')
  p.ouvintes.get('visibilitychange')!()
  assert.equal(escondeu, 2, 'página ainda visível')
  p.doc.visibilityState = 'hidden'
  p.ouvintes.get('visibilitychange')!()
  assert.equal(escondeu, 3, 'página oculta')
  limpar()
  assert.equal(p.ouvintes.size, 0)
  assert.equal(p.agendado(), null)
})

const bot = (phone: string): BotWhatsapp => ({ botId: 'b', isDonna: true, paused: false, number: { state: 'connected', phone },
  settings: { readChats: true, replyChats: false, readGroups: false, replyGroups: false, replyMode: 'approval', delaySeconds: 15, hourlyLimit: 30, transcribeAudio: true },
  botCan: { readChats: true, replyChats: false, readGroups: false, replyGroups: false }, sending: { state: 'none' } })

test('o texto do estado nunca leva o número; aba, tela grande e aviso de vazio só mostram o número pelo NumeroBot', () => {
  const e = estadoNumero(bot(FICTICIO))
  assert.equal(e.texto, 'whatsapp.estados.numero.connected')
  assert.equal(e.phone, FICTICIO)
  const fonte = (f: string) => readFileSync(new URL(f, import.meta.url), 'utf8')
  for (const f of ['./MiniWhatsapp.tsx', './TelaGrande.tsx']) {
    const s = fonte(f)
    assert.doesNotMatch(s, /formatarTelefone\(/, `${f} não formata o número do bot por conta própria`)
    assert.match(s, /<NumeroBot phone=\{numero\.phone\}/, `${f} usa o NumeroBot no estado`)
  }
  assert.match(fonte('./MiniWhatsapp.tsx'), /<NumeroBot phone=\{wa\.number\.phone\}/, 'aviso de lista vazia')
  // Nada salvo como "sempre visível".
  for (const f of ['./NumeroBot.tsx', './numero-bot.ts']) assert.doesNotMatch(fonte(f), /localStorage|sessionStorage|indexedDB|persist|setPreference/)
})

test('render real do NumeroBot: escondido por padrão, sem o número real no HTML nem no aria-label', async () => {
  const bundle = await build({
    stdin: {
      contents: `export { NumeroBot } from './views/fork/whatsapp/NumeroBot';
        export { createElement } from 'react';
        export { renderToStaticMarkup } from 'react-dom/server';`,
      resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
    },
    bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
    plugins: [{ name: 'icone', setup(b) {
      b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
      b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
    } }, { name: 'idioma', setup(b) {
      // useT real usa useSyncExternalStore, que não renderiza no servidor: aqui, o mesmo traduzir em pt-BR.
      b.onResolve({ filter: /fork-i18n\/estado$/ }, () => ({ path: 'idioma', namespace: 'idioma' }))
      b.onLoad({ filter: /.*/, namespace: 'idioma' }, () => ({
        contents: `import { traduzir } from './lib/fork-i18n/index.ts'; export function useT() { return (k, p) => traduzir('pt-BR', k, p) }`,
        loader: 'ts', resolveDir: new URL('../../../', import.meta.url).pathname,
      }))
    } }],
    loader: { '.css': 'empty' },
    define: { 'process.env.NODE_ENV': '"production"' },
  })
  // Janela mínima, como em genui/choice-card.test.ts: o idioma (useT) lê a preferência ao carregar; sem escolha, pt-BR.
  const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
  const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 }
  const storage = { getItem: () => null, setItem() {}, removeItem() {} }
  const document = { documentElement: { lang: '', dir: '' }, body: { setAttribute() {} }, addEventListener() {}, removeEventListener() {} }
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, document, console, setTimeout, clearTimeout, TextEncoder, URLSearchParams, queueMicrotask })
  const m = module.exports as { NumeroBot: (p: { phone: string }) => unknown; createElement: typeof import('react').createElement; renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup }
  const html = m.renderToStaticMarkup(m.createElement(m.NumeroBot as never, { phone: FICTICIO, ativo: true }))
  assert.match(html, /\+55 •• ••••-••50/)
  assert.match(html, /aria-label="Número escondido, termina em 50"/)
  assert.match(html, /aria-label="Mostrar número"/)
  assert.match(html, /aria-pressed="false"/)
  // Nenhum pedaço do número além dos 2 últimos dígitos: nem cru, nem formatado, nem em atributo.
  for (const vaza of ['5511900000150', '11 90000', '90000-0150', '900000150', '0150']) assert.ok(!html.includes(vaza), vaza)
  // Superfície fora de vista (painel escondido por CSS): também só a máscara.
  const oculto = m.renderToStaticMarkup(m.createElement(m.NumeroBot as never, { phone: FICTICIO, ativo: false }))
  assert.match(oculto, /\+55 •• ••••-••50/)
  for (const vaza of ['5511900000150', '90000-0150', '0150']) assert.ok(!oculto.includes(vaza), vaza)
})

test('D374-SOL-01: revelar no painel → Expandir → Recolher antes dos 10 s volta escondido; fora de vista, nada revelado', () => {
  // Painel à vista = aberto e sem a tela grande (TelaGrande.tsx, `painelVisivel`).
  const painelVisivel = (aberto: boolean, expandida: boolean) => aberto && !expandida
  let r: Revelacao = { pedido: false }
  let ativo = painelVisivel(true, false)
  assert.equal(numeroAMostra(r, ativo), false, 'escondido por padrão')
  r = revelar(r, { tipo: 'olhinho' })
  assert.equal(numeroAMostra(r, ativo), true, 'olhinho revela')
  // Expandir: o painel continua montado, escondido por CSS. Nada revelado lá, e o pedido acaba.
  ativo = painelVisivel(true, true)
  assert.equal(numeroAMostra(r, ativo), false, 'escondido já no render fora de vista')
  r = revelar(r, { tipo: 'superficie', ativo })
  assert.equal(r.pedido, false)
  // Recolher antes dos 10 s: volta à vista, escondido.
  ativo = painelVisivel(true, false)
  r = revelar(r, { tipo: 'superficie', ativo })
  assert.equal(numeroAMostra(r, ativo), false, 'recolher não traz o número de volta')
  // Painel solto que fecha (fica montado, inerte): o mesmo.
  r = revelar(r, { tipo: 'olhinho' })
  assert.equal(numeroAMostra(r, painelVisivel(false, false)), false)
  r = revelar(r, { tipo: 'superficie', ativo: false })
  assert.equal(numeroAMostra(r, painelVisivel(true, false)), false)
  // O tempo/foco/página também cancelam; à vista de novo, o olhinho revela outra vez.
  r = revelar(revelar(r, { tipo: 'olhinho' }), { tipo: 'esconder' })
  assert.equal(r.pedido, false)
})

test('D374-SOL-01: a ligação — o painel passa "à vista" ao NumeroBot e a tela grande, a expansão', () => {
  const fonte = (f: string) => readFileSync(new URL(f, import.meta.url), 'utf8')
  assert.match(fonte('./TelaGrande.tsx'), /const painelVisivel = panel\.open && !expandida/)
  assert.match(fonte('./TelaGrande.tsx'), /<NumeroBot phone=\{numero\.phone\} ativo=\{controle\.expandida\} \/>/)
  assert.match(fonte('./MiniWhatsapp.tsx'), /<NumeroBot phone=\{numero\.phone\} ativo=\{controle\.painelVisivel\} \/>/)
  assert.match(fonte('./MiniWhatsapp.tsx'), /<NumeroBot phone=\{wa\.number\.phone\} ativo=\{grande \|\| controle\.painelVisivel\} \/>/)
  // O componente desenha o número só com `numeroAMostra` (pedido E à vista).
  assert.match(fonte('./NumeroBot.tsx'), /const visivel = numeroAMostra\(r, ativo\)/)
})
