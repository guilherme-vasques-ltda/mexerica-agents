import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { Entry, MemoryOrigin } from '../../../../shared/models.ts'
import { isChat, memoryShown, memoryUndoable, migratedMemoryNote } from '../../../../shared/models.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import { automaticText, natureLabel, noticeKeys, noticeText, originLabel } from './presentation.ts'

const pt = tradutor('pt-BR')
const en = tradutor('en')
import { MemoryUndoState } from './undo-state.ts'

const origin = (kind: MemoryOrigin['kind'], external = kind === 'external'): MemoryOrigin => ({ schema: 1, kind, external, sources: [] })
const notice = (status = 'done', rev = 1): Entry => ({ id: 'memory-auto-job-op', botId: 'bot', seq: 1, rev, kind: 'notice', turn: 1, createdAt: 0, updatedAt: 0, data: { memory: { status, rev: 7, origin: origin('user') } } })

test('nota migrada tem só diagnóstico, origem do Hub e nenhuma ação/conteúdo; nota antiga continua oculta', () => {
  for (const availability of ['full', 'expired'] as const) {
    const data = { memoryFile: { status: 'pending', confirmation: true, jobId: 'legacy-note', availability, text: 'conteúdo da nota', content: 'conteúdo completo', origin: origin('external') } }
    const memory = migratedMemoryNote(data)!
    assert.equal(isChat({ ...notice(), data }), true)
    assert.equal(memory.action, 'note')
    assert.equal(memory.confirmation, false)
    assert.equal(memoryUndoable(memory), false)
    assert.equal(memory.text, undefined)
    assert.equal(Object.hasOwn(memory, 'content'), false)
    assert.equal(originLabel(pt, memory.origin), 'De fora')
  }
  const legacy = { memoryFile: { status: 'pending', confirmation: true, text: 'legado' } }
  assert.equal(migratedMemoryNote(legacy), undefined)
  assert.equal(isChat({ ...notice(), data: legacy }), false)
  assert.equal(originLabel(pt, migratedMemoryNote({ memoryFile: { status: 'expired' } })!.origin), 'Origem não registrada')
})

test('origem é independente do autor e nunca inventa detalhe externo nem fala do dono', () => {
  assert.equal(originLabel(pt, undefined), 'Origem não registrada')
  assert.equal(originLabel(pt, origin('unknown')), 'Origem não registrada')
  assert.equal(originLabel(pt, origin('user')), 'Dito por você')
  assert.equal(originLabel(pt, origin('conversation')), 'Da conversa')
  assert.equal(originLabel(pt, origin('external')), 'De fora')
  assert.equal(originLabel(pt, { ...origin('user', true), sources: [{ messageId: 'email', kind: 'external', detail: 'e-mail' }] }), 'De fora · e-mail')
})

test('Origin::default real do Hub conserva origem desconhecida e a marca externa conservadora', () => {
  const legacy: MemoryOrigin = { schema: 1, kind: 'unknown', sources: [], external: true }
  const before = structuredClone(legacy)
  assert.equal(originLabel(pt, legacy), 'Origem não registrada')
  assert.deepEqual(legacy, before)
  assert.equal(legacy.external, true)
})

test('natureza é só dado do Hub: inferência sempre hipótese, conflito sem vencedor e legado desconhecido', () => {
  assert.equal(natureLabel(pt, 'said'), 'Você disse')
  assert.equal(natureLabel(pt, 'inferred', 'Berga'), 'O Berga inferiu · hipótese')
  assert.equal(natureLabel(pt, 'inferred', 'Donna'), 'Donna inferiu · hipótese') // D444: um bot comum chamado Donna é só um nome
  assert.equal(natureLabel(pt, 'inferred', 'Lia'), 'Lia inferiu · hipótese')
  assert.equal(natureLabel(pt, 'inferred'), 'Inferência · bot não informado · hipótese')
  assert.equal(natureLabel(pt, 'conflict', 'Donna'), 'Conflito')
  assert.equal(natureLabel(pt, 'unknown', 'Donna'), 'Natureza não registrada')
  assert.equal(natureLabel(pt, undefined, 'Donna'), 'Natureza não registrada')
})

test('pendência legada e falta de seleção não viram confirmação nem sucesso; cheia/expirada não prometem retry', () => {
  assert.equal(noticeText(pt, { status: 'pending', confirmation: true }).title, 'Ainda não salvo na memória')
  assert.equal(memoryShown({ status: 'waiting', jobId: 'job', availability: 'noSelection' }), true)
  assert.equal(memoryUndoable({ status: 'pending', confirmation: true }), false)
  assert.equal(memoryShown({ status: 'noChange', jobId: 'duplicate' }), false)
  assert.equal(automaticText(pt, { status: 'applied', jobId: 'job', availability: null }), null)
  assert.equal(automaticText(pt, { status: 'noChange', jobId: 'job' }), null)
  assert.equal(automaticText(pt, { status: 'reserved', availability: 'noSelection' })!.title, 'Memória em processamento')
  assert.match(noticeText(pt, { status: 'waiting', availability: 'noSelection' }).detail!, /configuração compatível/)
  for (const availability of ['full', 'expired'] as const) {
    const text = noticeText(pt, { status: 'notQueued', availability })
    assert.equal(text.title, 'Não salvo na memória')
    assert.match(text.detail!, /não será reenviada automaticamente/)
  }
})

test('fatia 7 (D454): rótulos e recibos saem no idioma do tradutor, com chaves em todo status', () => {
  assert.equal(originLabel(en, origin('user')), 'Said by you')
  assert.equal(originLabel(en, { ...origin('user', true), sources: [{ messageId: 'email', kind: 'external', detail: 'e-mail' }] }), 'From outside · e-mail')
  assert.equal(natureLabel(en, 'inferred', 'Berga'), 'Berga inferred it · hypothesis')
  assert.equal(noticeText(en, { status: 'notQueued', availability: 'full' }).title, 'Not saved to memory')
  for (const status of ['done', 'undone', 'changed', 'declined', 'noChange', 'reserved', 'pending', 'failed']) {
    const keys = noticeKeys({ status })
    assert.match(keys.titulo, /^memoria\.auto\.aviso\./)
    assert.notEqual(en(keys.titulo as never), keys.titulo)
  }
})

test('timeout do undo é desconhecido; retry usa o mesmo recibo e aceita origem restaurada só do Hub', async () => {
  const ids: string[] = []
  const restored = notice('undone', 2)
  restored.data.memory!.origin = origin('external')
  restored.data.memory!.nature = 'said'
  restored.data.memory!.natureBot = null
  const state = new MemoryUndoState(async (id) => {
    ids.push(id)
    if (ids.length === 1) throw new Error('timeout')
    return { entry: restored }
  }, () => {})
  const entry = notice()
  await state.undo(entry)
  assert.equal(state.state(entry).unknown, true)
  assert.equal(state.state(entry).entry.data.memory!.status, 'done')
  await state.undo(entry)
  assert.deepEqual(ids, [entry.id, entry.id])
  assert.equal(state.state(entry).unknown, false)
  assert.equal(state.state(entry).entry, restored)
  assert.equal(state.state(entry).entry.data.memory!.origin!.kind, 'external')
  assert.equal(state.state(entry).entry.data.memory!.nature, 'said')
})

test('resposta vazia ou de outro recibo não confirma; stream terminal e edição posterior prevalecem', async () => {
  const entry = notice()
  const empty = new MemoryUndoState(async () => ({}), () => {})
  await empty.undo(entry)
  assert.equal(empty.state(entry).unknown, true)
  const state = new MemoryUndoState(async () => ({ entry: { ...notice('undone', 2), id: 'outro' } }), () => {})
  await state.undo(entry)
  assert.equal(state.state(entry).unknown, true)
  const changed = notice('changed', 3)
  assert.equal(state.state(changed).unknown, false)
  assert.equal(state.state(changed).entry, changed)
  assert.equal(memoryUndoable(changed.data.memory), false)
})

test('duplo clique não envia dois undos concorrentes', async () => {
  let resolve!: (value: { entry: Entry }) => void
  let calls = 0
  const state = new MemoryUndoState(() => { calls++; return new Promise((done) => { resolve = done }) }, () => {})
  const entry = notice()
  const pending = state.undo(entry)
  await state.undo(entry)
  assert.equal(calls, 1)
  assert.equal(state.state(entry).busy, true)
  resolve({ entry: notice('changed', 2) })
  await pending
  assert.equal(state.state(entry).busy, false)
  assert.equal(state.state(entry).entry.data.memory!.status, 'changed')
})

test('recibo desfeito conserva snapshot histórico; cliente não copia previousNature/origin para o atual', async () => {
  const entry = notice()
  entry.data.memory = { ...entry.data.memory, nature: 'conflict', previousNature: 'said', origin: origin('external'), previousOrigin: origin('user') }
  const undone = { ...entry, rev: 2, data: { memory: { ...entry.data.memory, status: 'undone' } } }
  const state = new MemoryUndoState(async () => ({ entry: undone }), () => {})
  await state.undo(entry)
  assert.equal(state.state(entry).entry.data.memory!.nature, 'conflict')
  assert.equal(state.state(entry).entry.data.memory!.origin!.kind, 'external')
})
