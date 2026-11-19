import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { TimesBot, TimesGraph, TimesMember } from '../../../../shared/fork-times.ts'
import { boardLayout, fitView, revealPan } from './board-layout.ts'
import { boardPrefsKey, readBoardPrefs, writeBoardPrefs } from './board-prefs.ts'
import { focusLost, type FocusTarget } from './focus.ts'
import { ancestorsOf, structureTree, treeIndex } from './structure.ts'

// Times (times-03): o Quadro desenha a estrutura (líder acima, subbots abaixo), arrastar só soma disposição local,
// e a resposta do Hub nunca rouba o foco que a pessoa escolheu durante a espera.

const AREA = '6f1c2b8e-1d2a-4c3b-9e4f-5a6b7c8d9e0f'
const member = (botId: string, extra: Partial<TimesMember> = {}): TimesMember => ({
  botId, teamId: null, parentBotId: null, role: 'executor', reportsTo: { kind: 'owner' }, ownerContact: 'reports', ...extra,
})
const graph: TimesGraph = {
  nodes: [{ id: AREA, name: 'Marketing', parentId: null, managerBotId: 'hype', position: 0 }],
  members: [
    member('donna', { role: 'manager' }),
    member('hype', { role: 'manager', teamId: AREA }),
    member('clipe', { teamId: AREA, reportsTo: { kind: 'bot', botId: 'hype' }, ownerContact: 'on_request' }),
    member('lingo', { teamId: AREA, parentBotId: 'clipe', reportsTo: { kind: 'bot', botId: 'donna' }, ownerContact: 'on_request' }),
    member('ata'),
  ],
  routes: [{ fromBotId: 'clipe', toBotId: 'ata', state: 'proposed' }],
}
const bots: TimesBot[] = ['donna', 'hype', 'clipe', 'lingo', 'ata'].map((id) => ({ id, name: id, spaceId: 'p', title: '', avatarColor: '', avatarShape: '', hidden: false }))

test('a estrutura: Você no topo, a principal e as áreas abaixo, o gerente acima da área, Sem time no fim', () => {
  const root = structureTree(graph, bots, 'donna')
  assert.deepEqual(root.children.map((c) => c.id), ['donna', `area:${AREA}`, 'loose'])
  const area = root.children[1]!
  assert.deepEqual(area.children.map((c) => c.id), ['hype'], 'o gerente fica no topo da área')
  assert.deepEqual(area.children[0]!.children.map((c) => c.id), ['clipe'])
  assert.deepEqual(area.children[0]!.children[0]!.children.map((c) => c.id), ['lingo'], 'subbot abaixo do bot de cima')
  assert.deepEqual(root.children[2]!.children.map((c) => c.id), ['ata'])
  const index = treeIndex(root)
  assert.deepEqual(ancestorsOf(index, 'lingo'), ['clipe', 'hype', `area:${AREA}`, 'owner'])
})

test('o líder fica acima; arrastar soma só disposição; recolher esconde quem está abaixo', () => {
  const root = structureTree(graph, bots, 'donna')
  const plain = boardLayout(root, graph, { collapsed: new Set(), offsets: {}, showRoutes: true })
  const at = (l: typeof plain, key: string) => l.cards.find((c) => c.key === key)!
  assert.ok(at(plain, 'owner').y < at(plain, 'donna').y)
  assert.ok(at(plain, 'hype').y < at(plain, 'clipe').y)
  assert.ok(at(plain, 'clipe').y < at(plain, 'lingo').y)
  assert.equal(plain.reports.length, graph.members.length, 'uma curva de Reporta a por bot')
  assert.equal(plain.reports.find((r) => r.from === 'lingo')!.to, 'donna', 'Reporta a segue o destinatário salvo, não a estrutura')
  assert.equal(plain.routes.length, 1)

  const moved = boardLayout(root, graph, { collapsed: new Set(), offsets: { clipe: { x: 40, y: 10 } }, showRoutes: false })
  assert.equal(at(moved, 'clipe').x, at(plain, 'clipe').x + 40)
  assert.equal(at(moved, 'lingo').x, at(plain, 'lingo').x, 'só o cartão arrastado muda de lugar')
  assert.equal(moved.routes.length, 0)

  const folded = boardLayout(root, graph, { collapsed: new Set(['clipe']), offsets: {}, showRoutes: true })
  assert.equal(folded.cards.some((c) => c.key === 'lingo'), false)
  assert.equal(at(folded, 'clipe').hiddenBelow, 1)
})

test('Ajustar cabe tudo; revelar só desloca quando o item está fora de vista', () => {
  const fit = fitView({ width: 2000, height: 1000 }, { w: 1000, h: 600 })
  assert.ok(fit.zoom <= 0.5 && fit.zoom >= 0.3)
  assert.equal(revealPan({ x: 30, y: 30, w: 100, h: 50 }, 1, { x: 0, y: 0 }, { w: 800, h: 600 }), null)
  assert.deepEqual(revealPan({ x: 900, y: 30, w: 100, h: 50 }, 1, { x: 0, y: 0 }, { w: 800, h: 600 }), { x: -224, y: 0 })
})

test('o foco só volta ao cartão se se perdeu; um controle ou a caixa de texto escolhidos durante a espera ficam', () => {
  const el = (extra: Partial<FocusTarget & { hidden: boolean }> = {}): FocusTarget => ({
    isConnected: true, closest: () => (extra.hidden ? {} : null), getClientRects: () => ({ length: 1 }), ...extra,
  })
  const body = {}
  const html = {}
  assert.equal(focusLost(el(), body, html), false, 'a pessoa está num controle visível')
  assert.equal(focusLost(null, body, html), true)
  assert.equal(focusLost(body as FocusTarget, body, html), true, 'corpo da página')
  assert.equal(focusLost(el({ isConnected: false }), body, html), true, 'elemento removido')
  assert.equal(focusLost(el({ hidden: true }), body, html), true, 'dentro de algo escondido ou inerte')
  assert.equal(focusLost(el({ getClientRects: () => ({ length: 0 }) }), body, html), true, 'sem caixa (recolhido)')
})

test('preferências do quadro são por Hub e space, e um valor inválido volta ao padrão sem perder os outros', () => {
  const data = new Map<string, string>()
  const storage = { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) }
  const a = boardPrefsKey('hub-a', 'p')
  const b = boardPrefsKey('hub-b', 'p')
  writeBoardPrefs(storage, a, { zoom: 0.8, pan: { x: 1, y: 2 }, locked: true, showRoutes: false, collapsed: ['x'], offsets: { clipe: { x: 3, y: 4 } } })
  assert.equal(readBoardPrefs(storage, a).locked, true)
  assert.equal(readBoardPrefs(storage, b).locked, false, 'outro Hub não herda a disposição')
  data.set(a, JSON.stringify({ zoom: 'muito', locked: true, offsets: { clipe: { x: 'a' }, feed: { x: 1, y: 1 } } }))
  const read = readBoardPrefs(storage, a)
  assert.equal(read.zoom, null)
  assert.equal(read.locked, true)
  assert.deepEqual(read.offsets, { feed: { x: 1, y: 1 } })
})
