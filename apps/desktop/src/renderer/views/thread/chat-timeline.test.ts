import { test } from 'node:test'
import { readFileSync } from 'node:fs'
import assert from 'node:assert/strict'
import { hoverScale, TIMELINE_VARIANT, VIEW_SCALE, timelineFocusIndex, timelineMarks, timelineTicks, type TimelineMessage } from './chat-timeline.ts'

// D165 fase E (ChatTimeline): um risco por mensagem; o espaço encolhe de 6 (passo do T3) até 3px e, passado isso,
// vizinhas dividem um risco sem perder nenhuma mensagem nem a marca de não lida.
const messages = (n: number, unreadFrom = n): TimelineMessage[] =>
  Array.from({ length: n }, (_, i) => ({ id: `m${i}`, time: '09:12', text: `mensagem ${i}`, mine: i % 2 === 0, unread: i >= unreadFrom }))

test('poucas mensagens: um risco cada, com o espaço máximo', () => {
  const t = timelineTicks(messages(5), 400)
  assert.equal(t.items.length, 5)
  assert.equal(t.gap, 6)
})

test('muitas mensagens: o espaço não passa de 3px e vizinhas viram um risco, sem perder ids nem não lidas', () => {
  const t = timelineTicks(messages(400, 398), 300)
  assert.equal(t.gap, 3)
  assert.ok(t.items.length < 400 && t.items.length >= 50)
  assert.deepEqual(t.items.flatMap((i) => i.ids), messages(400).map((m) => m.id))
  assert.equal(t.items[t.items.length - 1]!.unread, true)
  assert.equal(t.items[0]!.unread, false)
})

// Captura a regressão E2: usar o índice antigo após reagrupar muda a identidade focada.
test('foco acompanha a mensagem ao reagrupar e usa a atual se a âncora sair dos dados', () => {
  const all = messages(186)
  const tall = timelineTicks(all, 465).items
  const short = timelineTicks(all, 235).items
  assert.equal(timelineFocusIndex(tall, 'm22', 'm80'), 11)
  assert.equal(timelineFocusIndex(short, 'm22', 'm80'), 5)
  assert.ok(short[5]!.ids.includes('m22'))
  assert.equal(timelineFocusIndex(tall, 'm22', 'm80'), 11)
  const removed = timelineTicks(all.filter((m) => m.id !== 'm22'), 235).items
  assert.equal(timelineFocusIndex(removed, 'm22', 'm80'), 19)
  assert.equal(timelineFocusIndex(short, 'removed', 'removed'), 46)
  assert.equal(timelineFocusIndex([], 'm22', 'm80'), -1)
})

// D436 (dono): variante B num ponto só, e as marcas de cada uma. Hover em degradê como no T3 (pedido do dono, 07/11).
test('variante: B (D436) num ponto só; B marca todas, A só as suas com a resposta na prévia', () => {
  assert.equal(TIMELINE_VARIANT, 'b')
  const list = [{ id: 'u1', time: '09:00', text: 'pergunta', mine: true, unread: false }, { id: 'b1', time: '09:01', text: 'resposta', mine: false, unread: true }, { id: 'u2', time: '09:02', text: 'outra', mine: true, unread: false }]
  const b = timelineMarks(list, 'b')
  assert.deepEqual(b.marks.map((m) => [m.id, m.unread]), [['u1', false], ['b1', true], ['u2', false]])
  assert.equal(b.marks[0]!.answer, 'resposta')
  const a = timelineMarks(list, 'a')
  assert.deepEqual(a.marks.map((m) => [m.id, m.unread, m.answer]), [['u1', true, 'resposta'], ['u2', false, null]])
  assert.equal(a.owner.get('b1'), 'u1')
  assert.deepEqual([0, 1, 2, 3].map(hoverScale), [3, 2, 1.25, 1])
  assert.ok(VIEW_SCALE > 1 && VIEW_SCALE < hoverScale(1))
})

// Pedido do dono (16/11): a régua não tem fundo borrado/véu; o contorno nítido das marcas sobre wallpaper e a área de
// clique invisível seguem; o accent das não lidas vem adaptado (`--accent-marca`).
test('régua sem fundo: sem véu, blur nem painel; marcas com contorno nítido e hit area mantida', () => {
  const css = readFileSync(new URL('./chat-timeline.css', import.meta.url), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
  assert.doesNotMatch(css, /chat-timeline-track::before|backdrop-filter|blur\(|glass-raised\);\s*\n\s*--veu|mask-image/)
  assert.match(css, /html\[data-wallpaper\] \.chat-tick,\s*html\[data-wallpaper\] \.chat-tick:focus-visible::after \{\s*box-shadow: 0 0 0 1px var\(--glass-raised\);\s*\}/)
  assert.match(css, /\.chat-tick:focus-visible::after \{[^}]*background: var\(--accent-marca, var\(--accent\)\);/)
  assert.match(css, /\.chat-tick::before \{[^}]*left: -6px;[^}]*right: -8px;/)
  assert.match(css, /\.chat-tick\.is-unread \{\s*background: var\(--accent-marca, var\(--accent\)\);/)
  assert.match(css, /prefers-reduced-motion[\s\S]*\.chat-tick \{ transition: none; \}/)
})
