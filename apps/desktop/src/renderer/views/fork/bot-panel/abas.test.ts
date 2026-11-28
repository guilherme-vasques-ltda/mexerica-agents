import assert from 'node:assert/strict'
import test from 'node:test'
import { abaQueSumiu, currentTab, tabKeyTarget, visibleTabs, type PanelTabDef } from './abas.ts'

// Três abas falsas; a terceira só aparece quando o contexto permite (como uma aba condicional futura).
const tabs: PanelTabDef<{ extra: boolean }>[] = [
  { id: 'a', chave: 'A' },
  { id: 'b', chave: 'B' },
  { id: 'c', chave: 'C', visible: (ctx) => ctx.extra },
]

test('abas do painel: a aba condicional entra e sai pela lista', () => {
  assert.deepEqual(visibleTabs(tabs, { extra: true }).map((t) => t.id), ['a', 'b', 'c'])
  assert.deepEqual(visibleTabs(tabs, { extra: false }).map((t) => t.id), ['a', 'b'])
})

test('abas do painel: a aba guardada que sumiu volta para a primeira', () => {
  assert.equal(currentTab(visibleTabs(tabs, { extra: true }), 'c'), 'c')
  assert.equal(currentTab(visibleTabs(tabs, { extra: false }), 'c'), 'a')
  assert.equal(currentTab(visibleTabs(tabs, { extra: false }), 'b'), 'b')
})

test('abas do painel: setas, Home e End com 3 abas', () => {
  const ids = ['a', 'b', 'c']
  assert.equal(tabKeyTarget(ids, 'a', 'ArrowRight'), 'b')
  assert.equal(tabKeyTarget(ids, 'c', 'ArrowRight'), 'a')
  assert.equal(tabKeyTarget(ids, 'a', 'ArrowLeft'), 'c')
  assert.equal(tabKeyTarget(ids, 'b', 'Home'), 'a')
  assert.equal(tabKeyTarget(ids, 'a', 'End'), 'c')
  assert.equal(tabKeyTarget(ids, 'a', 'Enter'), null)
})

test('abas do painel (A3): a aba que sumiu grava a primeira; quando ela volta, a seleção não troca sozinha', () => {
  let saved = 'c'
  const semExtra = visibleTabs(tabs, { extra: false })
  const volta = abaQueSumiu(semExtra, saved)
  assert.equal(volta, 'a')
  saved = volta!
  assert.equal(abaQueSumiu(visibleTabs(tabs, { extra: true }), saved), null)
  assert.equal(currentTab(visibleTabs(tabs, { extra: true }), saved), 'a')
  assert.equal(abaQueSumiu(visibleTabs(tabs, { extra: true }), 'c'), null)
})

// Dono, 07/11: a aba "Recente" e a seção "Atividade recente" saíram do painel; o bot tem Sessões, Rotinas, Time e WhatsApp.
test('painel do bot: sem aba Recente nem Atividade recente', async () => {
  const { readFileSync } = await import('node:fs')
  const src = readFileSync(new URL('./BotDetailsPanel.tsx', import.meta.url), 'utf8')
  const line = src.split('\n').find((l) => l.startsWith('const BOT_TABS')) ?? ''
  assert.deepEqual([...line.matchAll(/id: '([a-z]+)'/g)].map((m) => m[1]), ['sessions', 'routines', 'time', 'whatsapp'])
  assert.ok(!/Atividade recente|RecentActivity|'recent'/.test(src))
})
