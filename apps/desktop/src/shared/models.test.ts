import { cachedBots, isCurrentMirrorSchema, MIRROR_SCHEMA, mirrorCacheStamp } from './fork-mirror-cache.ts'
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { canKeepConversation, draftOf, isChat, isReplyable, memoryUndoable, normalizeBot, quoteOf, type Entry } from './models.ts'

test('aviso de pasta sobrevive à normalização e ao cache sem entrar no rascunho', () => {
  const workspaceIssue = 'Revise a pasta. Escolha outra pasta nas configurações do bot.'
  const bot = normalizeBot({ id: 'legado', cwd: '/', workspaceIssue })
  assert.equal(normalizeBot(JSON.parse(JSON.stringify(bot))).workspaceIssue, workspaceIssue)
  assert.equal(Object.hasOwn(draftOf(bot), 'workspaceIssue'), false)
  assert.equal(normalizeBot({ id: 'antigo' }).workspaceIssue, null)
})

test('a bot keeps its Fast mode through the settings draft', () => {
  const bot = normalizeBot({ id: 'b', backend: 'codex', fastMode: true })
  assert.equal(bot.fastMode, true)
  assert.equal(draftOf(bot).fastMode, true)
})

test('a host without Fast mode reads as off', () => {
  assert.equal(normalizeBot({ id: 'b' }).fastMode, false)
})

test('salvar o perfil não reenvia uma permissão de gestão anterior à troca de principal', () => {
  const bot = normalizeBot({ id: 'b', manageTeam: true })
  assert.equal(bot.manageTeam, true)
  assert.equal(Object.hasOwn(draftOf(bot), 'manageTeam'), false)
})

test('the composer offers Fast mode until the agent says it has none, and while it is on', async () => {
  const { fastModeShown, fastModeState, fastModeDetail } = await import('./fast-mode.ts')
  assert.equal(fastModeShown(normalizeBot({ id: 'b' })), true)
  assert.equal(fastModeShown(normalizeBot({ id: 'b', fastModeSupport: 'unsupported' })), false)
  assert.equal(fastModeShown(normalizeBot({ id: 'b', fastModeSupport: 'offered' })), true)
  assert.equal(fastModeShown(normalizeBot({ id: 'b', fastMode: true, fastModeSupport: 'unsupported' })), true)
  assert.equal(fastModeShown(normalizeBot({ id: 'g', kind: 'group', fastModeSupport: 'offered' })), false)
  const refused = normalizeBot({ id: 'b', fastMode: true, fastModeSupport: 'refused', fastModeIssue: 'requires extra usage' })
  assert.equal(fastModeState(refused), 'refused')
  const { tradutor } = await import('../renderer/lib/fork-i18n/index.ts')
  assert.match(fastModeDetail(tradutor('en'), refused), /requires extra usage/)
})

test('a bot memory write shows with its undo; note lines and summaries stay out of the chat', () => {
  const notice = (data: Entry['data']) => ({ id: 'n', botId: 'b', kind: 'notice', rev: 1, turn: 1, createdAt: 0, data }) as Entry
  assert.equal(isChat(notice({ text: 'Lembrei: gosta de chá', memory: { status: 'done', rev: 7 } })), true)
  assert.equal(memoryUndoable({ status: 'done', rev: 7 }), true)
  assert.equal(memoryUndoable({ status: 'changed', rev: 7 }), false)
  assert.equal(memoryUndoable({ status: 'undone', rev: 7 }), false)
  assert.equal(memoryUndoable(undefined), false)
  // Lines and pending cards from before Mexerica carry no revision: they stay hidden.
  assert.equal(isChat(notice({ text: 'Remembered: likes tea', memory: { status: 'done' } })), false)
  assert.equal(isChat(notice({ text: 'Wants to remember: tea', memory: { status: 'pending' } })), false)
  assert.equal(isChat(notice({ text: 'Updated people.md', memoryFile: { status: 'done' } })), false)
  assert.equal(isChat(notice({ text: 'Earlier context was summarized to make room.', compaction: { byHost: true } })), false)
  assert.equal(isChat(notice({ text: 'Stopped.' })), true)
})

test('o marcador de sessão fica para apps antigos; este app o esconde', () => {
  const notice = (data: Entry['data']) => ({ id: 'n', botId: 'b', kind: 'notice', rev: 1, turn: 1, createdAt: 0, data }) as Entry
  assert.equal(isChat(notice({ text: 'Sessão “Plano”', sessionMarker: { sessionId: 's1' } })), false)
  assert.equal(isChat(notice({ text: 'Sessão principal', sessionMarker: { sessionId: 'b' } })), false)
})

test('a kept handoff leaves no line; a fresh conversation does', () => {
  const notice = (status: string) => ({ id: 'n', botId: 'b', kind: 'notice', rev: 1, turn: 1, createdAt: 0, data: { text: '…', handoff: { status } } }) as Entry
  assert.equal(isChat(notice('kept')), false)
  assert.equal(isChat(notice('sent')), true)
  assert.equal(isChat(notice('draft')), true)
})

const message = (kind: string, data: Entry['data'], id = 'e1'): Entry => ({ id, seq: 1, botId: 'b', rev: 1, kind, turn: 1, createdAt: 0, updatedAt: 0, data })

test('the user and final bot replies can be quoted, once the computer has them', () => {
  assert.equal(isReplyable(message('user', { text: 'hi' })), true)
  assert.equal(isReplyable(message('agent', { text: 'done', final: true })), true)
  assert.equal(isReplyable(message('agent', { text: 'narration' })), false)
  assert.equal(isReplyable(message('notice', { text: 'Stopped.' })), false)
  assert.equal(isReplyable(message('permission', { title: 'Run?' })), false)
  assert.equal(isReplyable(message('user', { text: 'sending' }, 'local-n')), false)
})

test('a quote names who wrote it and falls back to file names', () => {
  assert.deepEqual(quoteOf(message('user', { text: 'hi' })), { id: 'e1', author: 'user', text: 'hi' })
  assert.equal(quoteOf(message('agent', { text: 'yo', final: true, author: 'ann' })).author, 'ann')
  assert.equal(quoteOf(message('agent', { text: 'yo', final: true })).author, 'b')
  assert.equal(quoteOf(message('user', { text: '', attachments: [{ id: 'a', name: 'a.png', size: 1 }, { id: 'c', name: 'c.pdf', size: 1 }] })).text, 'a.png, c.pdf')
  assert.equal(quoteOf(message('user', { text: 'x'.repeat(900) })).text.length, 500)
})

test('Sessões PR5: continuação e recomeço não oferecem "Manter esta conversa", como a troca de motor', () => {
  for (const reason of ['cold', 'away', 'full', undefined]) assert.equal(canKeepConversation({ status: 'draft', reason }), true)
  for (const reason of ['engineSwitch', 'continuation', 'restart', 'outroMotivo']) {
    assert.equal(canKeepConversation({ status: 'draft', reason }), false, reason)
  }
})

test('D510: chave por computador atravessa o cache, não o rascunho; Hub antigo não libera', async () => {
  const { isCurrentMirrorSchema, mirrorCacheStamp } = await import('./fork-mirror-cache.ts')
  assert.equal(isCurrentMirrorSchema('donnabot-mirror-v27/2.20.3/2.20.3'), false)
  assert.equal(isCurrentMirrorSchema(mirrorCacheStamp('2.20.3', '2.20.3')), true)
  const screenAccess = [{ target: 'host:fixture', name: 'Host', available: true, enabled: false, canEndTemporary: true, notice: 'aviso' }]
  const bot = normalizeBot({ id: 'b', computer: true, screenAccess })
  assert.deepEqual(normalizeBot(JSON.parse(JSON.stringify(bot))).screenAccess, screenAccess)
  assert.equal(Object.hasOwn(draftOf(bot), 'screenAccess'), false)
  assert.equal(normalizeBot({ id: 'b', computer: true }).screenAccess, null)
})

test('preferência de navegador antiga assume integrado e PC persiste em normalize e draft sem permissão', () => {
  assert.equal(normalizeBot({ id: 'legado' }).browserPreference, 'integrated')
  const bot = normalizeBot({ id: 'pc', browserPreference: 'computer', computer: false })
  assert.equal(normalizeBot(JSON.parse(JSON.stringify(bot))).browserPreference, 'computer')
  const off = normalizeBot({ id: 'off', browserPreference: 'off', computer: false })
  assert.equal(normalizeBot(JSON.parse(JSON.stringify(off))).browserPreference, 'off', 'off sobrevive ao round-trip do mirror')
  assert.equal(draftOf(off).browserPreference, 'off')
  assert.equal(draftOf(bot).browserPreference, 'computer')
  assert.equal(draftOf(bot).computer, false)
  assert.equal(bot.screenAccess, null)
})

test('o schema atual invalida caches 29 até o anterior e conserva preferência sem transformar cache em autorização', () => {
  const stamp = mirrorCacheStamp('fixture-app', 'fixture-hub')
  assert.equal(stamp, 'donnabot-mirror-v53/fixture-app/fixture-hub')
  // MEX-1391: o cache do schema 51 não tem `contaTroca` nem os códigos novos de falha de preparo.
  assert.equal(isCurrentMirrorSchema('donnabot-mirror-v51/fixture-app/fixture-hub'), false)
  assert.equal(isCurrentMirrorSchema('donnabot-mirror-v50/fixture-app/fixture-hub'), false)
  assert.equal(isCurrentMirrorSchema('donnabot-mirror-v49/fixture-app/fixture-hub'), false)
  assert.equal(isCurrentMirrorSchema('donnabot-mirror-v48/fixture-app/fixture-hub'), false)
  assert.equal(stamp, `donnabot-mirror-v${MIRROR_SCHEMA}/fixture-app/fixture-hub`)
  for (let schema = 29; schema < MIRROR_SCHEMA; schema++) {
    assert.equal(isCurrentMirrorSchema(`donnabot-mirror-v${schema}/fixture-app/fixture-hub`), false)
  }
  const old = 'donnabot-mirror-v30/fixture-app/fixture-hub'
  const pc = normalizeBot({ id: 'pc', browserPreference: 'computer', computer: false })
  assert.equal(cachedBots(stamp, [pc])[0]!.browserPreference, 'computer')
  const legacy = normalizeBot({ id: 'legado' })
  const cached = cachedBots(old, [legacy])[0]!
  assert.equal(cached.browserPreference, 'integrated')
  assert.equal(cached.status, 'unavailable')
  assert.equal(cached.computer, false)
})

test('continuações internas do Hub não aparecem como falas da pessoa', () => {
  const interna = message('user', { text: '[Login “Conta” for exemplo is on for you as `id`. Type it with the computer tool type_login.]', from: 'host' })
  assert.equal(isChat(interna), false)
  assert.equal(isReplyable(interna), false)
  assert.equal(isChat(message('user', { text: interna.data.text })), true)
  assert.equal(isChat(message('notice', { text: 'Aviso', from: 'host' })), true)
})
