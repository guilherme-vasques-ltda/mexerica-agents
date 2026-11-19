import { test } from 'node:test'
import assert from 'node:assert/strict'
import { delegatedRows, parseDelegatedPermissions } from './fork-torre-permissoes.ts'
import { openTarget, parseTowerSnapshot } from './fork-torre.ts'

// MX-478: o contrato mínimo é fail-closed. Campo ausente ou torto é indisponível; item incoerente some; só vira linha
// o cartão que a própria fotografia ainda lista como pendente do bot que recebeu; Abrir vai à conversa de quem recebeu.
const obs = (value: unknown) => ({ availability: 'available', source: 'delegacao.pausa', coverage: 'space', value })
const item = (extra: Record<string, unknown> = {}) => ({
  key: 'k1', requesterBotId: 'a', targetBotId: 'b', entryId: 'e1', conversation: { conversationId: 'b', revision: 2 }, mode: 'wait', requestedAt: 1000, ...extra,
})
const bot = (botId: string, pending: unknown) => ({ botId, name: botId.toUpperCase(), usage: {}, pending })
const raw = (extra: Record<string, unknown> = {}, bots: unknown[] = []) => ({ version: 1, hubId: 'h', spaceId: 's', snapshotId: 'x', collectedAt: 2000, bots, ...extra })
const expected = { hubId: 'h', spaceId: 's' }
const card = (entryId: string, conversationId = 'b') => ({ entryId, sessionRef: { id: 's1', chatId: conversationId }, kind: 'permission', status: 'pending', conversation: { conversationId, revision: 2 } })

test('permissões delegadas: sem o campo (Hub anterior) ou torto é indisponível, nunca vazio', () => {
  assert.deepEqual(parseTowerSnapshot(raw(), expected).delegatedPermissions, { availability: 'unavailable' })
  for (const value of [null, [], {}, { items: [] }, { items: {}, omitted: 0 }, { items: [], omitted: -1 }, { items: [], omitted: 1.5 }]) {
    assert.deepEqual(parseTowerSnapshot(raw({ delegatedPermissions: obs(value) }), expected).delegatedPermissions, { availability: 'unavailable' })
  }
  assert.deepEqual(parseTowerSnapshot(raw({ delegatedPermissions: { availability: 'unavailable' } }), expected).delegatedPermissions, { availability: 'unavailable' })
})

test('permissões delegadas: só item completo e coerente entra; o resto é ignorado', () => {
  const ruins = [
    { key: '' }, { requesterBotId: null }, { targetBotId: '' }, { entryId: 1 }, { conversation: null }, { conversation: { conversationId: 'b' } },
    { conversation: { revision: 1 } }, { mode: 'novo' }, { requestedAt: 'ontem' }, { requestedAt: Infinity }, { targetBotId: 'a' },
  ]
  const parsed = parseDelegatedPermissions({ items: [item(), ...ruins.map((r) => item(r)), 7, null], omitted: 3 })
  assert.deepEqual(parsed, { items: [item()], omitted: 3 })
  assert.equal(parseDelegatedPermissions({ items: [item({ mode: 'async' })], omitted: 0 })?.items[0]?.mode, 'async')
})

test('permissões delegadas: nenhum texto de pedido, comando ou argumento atravessa o parser', () => {
  const parsed = parseDelegatedPermissions({ items: [item({ text: 'rm -rf', command: 'x', args: ['y'], reason: 'z', digest: 'd' })], omitted: 0 })
  assert.deepEqual(Object.keys(parsed!.items[0]!).sort(), ['conversation', 'entryId', 'key', 'mode', 'requestedAt', 'requesterBotId', 'targetBotId'])
})

test('permissões delegadas: só vira linha o cartão que a fotografia ainda lista como pendente de quem recebeu', () => {
  const snap = (bots: unknown[]) => parseTowerSnapshot(raw({}, bots), expected).bots
  const ok = snap([bot('a', obs([])), bot('b', obs([card('e1')]))])
  assert.equal(delegatedRows([item()] as never, ok).length, 1)
  assert.equal(delegatedRows([item()] as never, ok)[0]!.to.botId, 'b')
  // Cartão já respondido (sumiu da lista), de outra conversa, de outro bot, bot fora da fotografia ou pendências indisponíveis.
  assert.equal(delegatedRows([item()] as never, snap([bot('a', obs([])), bot('b', obs([]))])).length, 0)
  assert.equal(delegatedRows([item()] as never, snap([bot('a', obs([])), bot('b', obs([card('e1', 'outra')]))])).length, 0)
  assert.equal(delegatedRows([item()] as never, snap([bot('a', obs([card('e1')])), bot('b', obs([]))])).length, 0)
  assert.equal(delegatedRows([item()] as never, snap([bot('b', obs([card('e1')]))])).length, 0)
  // Mesmo cartão, mas de outro tipo, já respondido ou de outra revisão da conversa.
  for (const outro of [{ kind: 'question' }, { status: 'answered' }, { conversation: { conversationId: 'b', revision: 9 } }]) {
    assert.equal(delegatedRows([item()] as never, snap([bot('a', obs([])), bot('b', obs([{ ...card('e1'), ...outro }]))])).length, 0)
  }
  assert.equal(delegatedRows([item()] as never, snap([bot('a', obs([])), bot('b', { availability: 'unavailable' })])).length, 0)
})

test('permissões delegadas: Abrir usa o destino de quem recebeu e nunca cai na conversa principal', () => {
  assert.deepEqual(openTarget('b', 'b', 'p', 'p'), { kind: 'personal', botId: 'b' })
  // No Pessoal, conversa que não é a própria do bot não tem caminho.
  assert.equal(openTarget('outra', 'b', 'p', 'p'), null)
  assert.deepEqual(openTarget('c1', 'b', 's', 'p'), { kind: 'space', spaceId: 's', conversationId: 'c1' })
})
