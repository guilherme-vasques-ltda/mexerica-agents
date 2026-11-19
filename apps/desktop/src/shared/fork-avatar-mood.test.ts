import assert from 'node:assert/strict'
import test from 'node:test'
import { avatarMoodEvent, AvatarBotMap, AvatarMoods, IDLE_MOOD } from './fork-avatar-mood.ts'
import { normalizeBot, draftOf, type Entry } from './models.ts'
import { DEFAULT_CONFIG, validAvatar, normalizedAvatar } from './fork-avatar-config.ts'
import { botEditorUpdate } from './bot-autonomy.ts'

test('eventos mapeiam estados e reações sem interromper trabalho pela digitação', () => {
  const listening = avatarMoodEvent(IDLE_MOOD, 'typing', 1)
  assert.equal(listening.base, 'listen')
  const thinking = avatarMoodEvent(listening, 'generating', 2)
  assert.equal(thinking.base, 'think')
  const working = avatarMoodEvent(thinking, 'tool', 3)
  assert.equal(working.base, 'work')
  assert.equal(avatarMoodEvent(working, 'typing', 4), working)
  const done = avatarMoodEvent(working, 'completed', 5)
  assert.equal(done.base, 'idle'); assert.equal(done.action, 'happy')
  assert.equal(avatarMoodEvent(done, 'error', 6).action, 'surprise')
  assert.equal(avatarMoodEvent(done, 'message', 7).action, 'blink')
  assert.equal(avatarMoodEvent(done, 'disabled', 8).base, 'sleep')
  assert.equal(avatarMoodEvent(done, 'enabled', 9).action, 'wake')
})

test('sync atribui ferramentas ao autor do grupo, isola bots e não repete reações no echo', () => {
  const store = new AvatarMoods()
  const otherHub = new AvatarMoods()
  const bot = normalizeBot({ id: 'a', name: 'Amazon', status: 'working' })
  const peer = normalizeBot({ id: 'b' })
  store.bot(bot); store.bot(peer); otherHub.bot(peer)
  const mirror = new AvatarBotMap(store, [{ ...bot, rev: 3 }])
  mirror.set(bot.id, { ...bot, rev: 2, name: 'Hub restaurado', unread: 3 })
  assert.equal(mirror.get(bot.id)?.rev, 2)
  assert.equal(mirror.get(bot.id)?.name, 'Hub restaurado')
  assert.equal(mirror.get(bot.id)?.unread, 3)
  const tool: Entry = { id: 't', botId: 'group', seq: 1, rev: 1, turn: 1, kind: 'tool', createdAt: Date.now(), updatedAt: Date.now(), data: { author: 'a', status: 'in_progress' } }
  store.entry({ ...tool, createdAt: Date.now() - 60_000, updatedAt: Date.now() - 60_000 })
  assert.equal(store.get('a').base, 'work'); assert.equal(store.get('b').base, 'idle')
  assert.equal(otherHub.get('a').base, 'idle')
  const cached = new AvatarMoods()
  cached.bot(bot)
  cached.restoreEntries(new Map([['group', [{ ...tool, createdAt: Date.now() - 60_000, updatedAt: Date.now() - 60_000 }]]]))
  assert.equal(cached.get('a').base, 'work')
  assert.equal(cached.get('a').action, null)
  cached.retire()
  store.entry({ ...tool, rev: 2, data: { ...tool.data, status: 'completed' } })
  assert.equal(store.get('a').base, 'think')
  store.bot({ ...bot, status: 'idle' })
  assert.equal(store.get('a').action, 'happy')
  const seq = store.get('a').sequence
  store.bot({ ...bot, status: 'idle' })
  assert.equal(store.get('a').sequence, seq)
  store.connection(false); assert.equal(store.get('a').base, 'sleep')
  store.connection(true); assert.equal(store.get('a').action, 'wake')
  store.retire(); otherHub.retire()
})

test('digitação só emite as bordas de ouvir e pausa, sem notificação por tecla', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const moods = new AvatarMoods()
  const bot = normalizeBot({ id: 'a', status: 'idle' })
  moods.bot(bot)
  let notifications = 0
  moods.subscribe(() => { notifications++ })
  for (let i = 0; i < 10; i++) { moods.typing('a'); t.mock.timers.tick(100) }
  assert.equal(moods.get('a').base, 'listen')
  assert.equal(notifications, 1)
  t.mock.timers.tick(1801)
  assert.equal(moods.get('a').base, 'idle')
  assert.equal(notifications, 2)
  moods.bot({ ...bot, status: 'needsInput' })
  notifications = 0
  for (let i = 0; i < 10; i++) moods.typing('a')
  t.mock.timers.tick(1801)
  assert.equal(moods.get('a').base, 'listen')
  assert.equal(notifications, 0)
  moods.retire()
})

test('cache inválido não chega a estilo e o save geral não sobrescreve o avatar', () => {
  for (const color of ['#123456;display:none', '#123456\n', 'red']) assert.equal(validAvatar({ ...DEFAULT_CONFIG, color }), false)
  assert.equal(validAvatar({ ...DEFAULT_CONFIG, eye: 'desconhecido' }), false)
  const bot = normalizeBot({ id: 'a', avatarConfig: { ...DEFAULT_CONFIG, color: '#aB09Ff' } })
  assert.equal(normalizedAvatar(bot.avatarConfig).color, '#aB09Ff')
  const profile = botEditorUpdate({ name: 'A', description: '', backend: 'codex', cwd: '', permission: 'ask', avatarColor: 'blue', avatarShape: 'blob', avatarConfig: bot.avatarConfig })
  assert.equal('avatarConfig' in profile, false)
  assert.equal('mexeriquinha' in botEditorUpdate({...draftOf(bot),mexeriquinha:{v:2,estilo:'3d',conjuntoId:'base'}}),false)
})

test('aprovação mantém listen quando a pausa de digitação anterior termina', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const store = new AvatarMoods()
  const bot = normalizeBot({ id: 'a' })
  store.bot(bot); store.typing(bot.id)
  store.bot({ ...bot, status: 'needsInput' })
  t.mock.timers.tick(1801)
  assert.equal(store.get(bot.id).base, 'listen')
  store.typing(bot.id)
  t.mock.timers.tick(1801)
  assert.equal(store.get(bot.id).base, 'listen')
  store.retire()
})


test('2a: perguntas pendentes chegam à ilha sem inventar trabalho ou sono na desconexão', () => {
  const moods = new AvatarMoods()
  const bot = normalizeBot({ id: 'pergunta', status: 'idle', lastAt: 1000,
    pendingQuestions: [{ entryId: 'q', chatId: 'pergunta', text: 'Qual?', askedAt: 1000 }] })
  moods.bot(bot)
  assert.equal(moods.get(bot.id).precisaResposta, true)
  assert.equal(moods.get(bot.id).runtimeStatus, 'idle')
  moods.connection(false)
  assert.equal(moods.get(bot.id).indisponivel, true)
  assert.equal(moods.get(bot.id).ultimaAtividade, 1000)
  moods.connection(true)
  assert.equal(moods.get(bot.id).indisponivel, false)
  assert.equal(moods.get(bot.id).ultimaAtividade, 1000)
  moods.retire()
})
