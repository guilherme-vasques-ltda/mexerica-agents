import { test } from 'node:test'
import assert from 'node:assert/strict'
import { changedModel, receiptAnchors, receiptView, showsIcon, usoDoTurno } from './presentation.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import type { ReceiptLoad } from '../../../store/fork-recibo.ts'
import type { RoutingReceipt } from '../../../../shared/fork-roteamento.ts'
import type { Entry } from '../../../../shared/models.ts'

const t = tradutor('pt-BR')

const receipt = (reason: RoutingReceipt['reason'], model = 'gpt-6.1-sol', accountId = 'ac_pessoal', session = 'byte'): ReceiptLoad => ({
  status: 'loaded',
  response: { hubId: 'hub', state: 'routed', receipt: {
    schemaVersion: 1, destination: { botId: 'byte', spaceId: 'p', session: { id: session, chatId: 'byte' } },
    requested: { accountId, selection: { backend: 'codex', model, modelOptions: { reasoning_effort: 'high' } } },
    admitted: { accountId, selection: { backend: 'codex', model, modelOptions: { reasoning_effort: 'high' } } },
    decidedBy: 'fixo', reason,
    requiredFacts: { coverageBefore: 'requiredFactNotAttested', coverageAfter: 'requiredFactNotAttested', accountIdentityGenerationCatalog: 'requiredFactNotAttested' },
  } },
})
const accounts = [{ id: 'ac_pessoal', label: 'Codex pessoal' }]
const state = (s: 'pending' | 'notRouted'): ReceiptLoad => ({ status: 'loaded', response: { hubId: 'hub', state: s, receipt: null } })

test('recibo roteado: cada motivo com sua etiqueta, texto e link só no Fixo', () => {
  const fixo = receiptView(t, receipt('fixedSelection'), accounts, 'Byte')
  assert.ok(fixo.kind === 'routed')
  assert.deepEqual([fixo.model, fixo.account, fixo.engine, fixo.effort, fixo.tag.text, fixo.motorLink], ['gpt-6.1-sol', 'Codex pessoal', 'Codex', 'Alto', 'Fixo do bot', 'Abrir Motor de Byte'])
  assert.equal(fixo.why, 'Byte usa sempre a combinação de Configurações → Motor.')
  assert.equal(fixo.label, 'Respondido com gpt-6.1-sol, conta Codex pessoal, esforço alto. Fixo do bot. Abrir recibo')
  const conversa = receiptView(t, receipt('explicitSelection'), accounts, 'Byte')
  assert.ok(conversa.kind === 'routed')
  assert.deepEqual([conversa.tag.text, conversa.motorLink], ['Escolha desta conversa', null])
  const sessao = receiptView(t, receipt('frozenSessionSelection'), accounts, 'Byte')
  assert.ok(sessao.kind === 'routed')
  assert.deepEqual([sessao.tag.text, sessao.motorLink], ['Fixada nesta sessão', null])
  assert.equal(sessao.why, 'A sessão guardou esta combinação ao começar. Mudar o Fixo de Byte não muda esta sessão.')
})

test('sessão extra: recibo Fixo do 1º turno vira "Fixada nesta sessão", sem o atalho do Motor', () => {
  const extra = receiptView(t, receipt('fixedSelection', 'gpt-6.1-sol', 'ac_pessoal', 'byte-extra'), accounts, 'Byte')
  assert.ok(extra.kind === 'routed')
  assert.deepEqual([extra.tag.text, extra.motorLink], ['Fixada nesta sessão', null])
  assert.ok(extra.label.includes('Fixada nesta sessão'))
})

test('conta fora da lista do app: "Conta removida", sem o id; lista que falhou: "Conta indisponível"', () => {
  const view = receiptView(t, receipt('fixedSelection', 'gpt-6.1-sol', 'ac_apagada'), accounts, 'Byte')
  assert.ok(view.kind === 'routed')
  assert.deepEqual([view.account, view.accountGone], ['Conta removida', true])
  assert.ok(!view.label.includes('ac_apagada'))
  const failed = receiptView(t, receipt('fixedSelection'), 'failed', 'Byte')
  assert.ok(failed.kind === 'routed')
  assert.deepEqual([failed.account, failed.accountGone], ['Conta indisponível', false], 'lista não lida não vira "Conta removida"')
  const loading = receiptView(t, receipt('fixedSelection'), null, 'Byte')
  assert.ok(loading.kind === 'routed')
  assert.equal(loading.account, '…')
})

test('sem recibo, na fila e erro: textos aprovados; ícone só na fila e no erro', () => {
  const none = receiptView(t, state('notRouted'), accounts, 'Byte')
  assert.ok(none.kind === 'plain')
  assert.equal(none.title, 'Sem recibo de roteamento')
  assert.equal(none.note, 'Não é um erro. Respostas novas mostram o ícone de recibo ao lado da hora.')
  assert.equal(receiptView(t, undefined, accounts, 'Byte', false).kind, 'plain', 'turno sem pedido humano não consulta o Hub')
  const fila = receiptView(t, state('pending'), accounts, 'Byte')
  assert.ok(fila.kind === 'plain')
  assert.equal(fila.title, 'Ainda não enviada')
  const erro = receiptView(t, { status: 'error', code: 'routing_receipt_unavailable' }, accounts, 'Byte')
  assert.ok(erro.kind === 'plain')
  assert.deepEqual([erro.title, erro.retry, erro.warn], ['Não consegui ler o recibo agora', true, true])
  assert.equal(erro.note, 'Hub: não foi possível ler o recibo.')
  assert.deepEqual([showsIcon(receipt('fixedSelection')), showsIcon(state('pending')), showsIcon({ status: 'error', code: 'x' }), showsIcon(state('notRouted')), showsIcon({ status: 'loading' }), showsIcon(undefined)], [true, true, true, false, false, false])
})

test('modelo ao lado do ícone só quando muda em relação ao turno anterior', () => {
  assert.equal(changedModel(t, receipt('fixedSelection', 'gpt-6.1-sol'), receipt('fixedSelection', 'claude-opus-5-5')), 'gpt-6.1-sol')
  assert.equal(changedModel(t, receipt('fixedSelection', 'gpt-6.1-sol'), receipt('fixedSelection', 'gpt-6.1-sol')), null)
  assert.equal(changedModel(t, receipt('fixedSelection'), undefined), null, 'sem turno anterior lido, só o ícone')
  assert.equal(changedModel(t, receipt('fixedSelection'), state('notRouted')), null)
})

test('a resposta que abre o turno aponta a entrada do usuário do mesmo turno e o turno anterior', () => {
  const e = (id: string, kind: string, turn: number, data: Entry['data'] = {}): Entry => ({ id, seq: 0, botId: 'byte', rev: 0, kind, turn, data, createdAt: 0, updatedAt: 0 })
  const anchors = receiptAnchors([
    e('u1', 'user', 1), e('a1', 'agent', 1, { final: true }), e('a1b', 'agent', 1, { final: true }),
    e('u2', 'user', 2), e('u3', 'user', 3), e('a3', 'agent', 3, { final: true }),
    e('rotina', 'agent', 4, { final: true }),
  ])
  assert.deepEqual(anchors.get('a1'), { request: { botId: 'byte', conversationId: 'byte', sessionId: 'byte', entryId: 'u1' }, previous: null })
  assert.equal(anchors.has('a1b'), false, 'só a primeira resposta do turno')
  assert.deepEqual(anchors.get('a3'), { request: { botId: 'byte', conversationId: 'byte', sessionId: 'byte', entryId: 'u3' }, previous: { botId: 'byte', conversationId: 'byte', sessionId: 'byte', entryId: 'u1' } })
  assert.equal(anchors.get('rotina')?.request, null)
})

test('reconectar (hello, nova época): o mesmo pedido volta a ler e o recibo volta a "routed", sem chamada duplicada', async () => {
  const { ReceiptCache } = await import('../../../store/fork-recibo.ts')
  const calls: Record<string, unknown>[] = []
  let notify = () => {}
  const loaded = receipt('fixedSelection')
  const source = {
    connectedClient: { call<T>(_: string, body: Record<string, unknown>) { calls.push(body); return Promise.resolve((loaded as { response: unknown }).response as T) } },
    hello: { hostId: 'hub' }, principalEpoch: 1, bots: new Map(),
    subscribe: (l: () => void) => { notify = l; return () => {} },
  }
  const cache = new ReceiptCache(source)
  const request = { botId: 'byte', conversationId: 'byte', sessionId: 'byte', entryId: 'u1' }
  const settle = () => new Promise((r) => setTimeout(r, 0))
  cache.prefetch(request); cache.prefetch(request)
  await settle()
  assert.equal(calls.length, 1)
  assert.equal(showsIcon(cache.get(request)), true)
  const epoch = cache.epoch
  source.principalEpoch = 2; notify()
  assert.equal(cache.epoch, epoch + 1, 'a view relê porque a época mudou')
  assert.equal(cache.get(request), undefined)
  cache.prefetch(request) // o efeito do ícone, refeito pela época
  cache.open(request) // o popover aberto: não pede de novo nem fica preso em "Lendo"
  await settle()
  assert.equal(calls.length, 2)
  const view = receiptView(t, cache.get(request), accounts, 'Byte')
  assert.equal(view.kind, 'routed')
})

test('recibo em inglês: frase acessível inteira e etiqueta traduzida', () => {
  const en = tradutor('en')
  const view = receiptView(en, receipt('fixedSelection'), accounts, 'Byte')
  assert.ok(view.kind === 'routed')
  assert.equal(view.label, 'Answered with gpt-6.1-sol, account Codex pessoal, high effort. Bot default. Open receipt')
  assert.equal(view.motorLink, "Open Byte's Engine")
})

const comUso = (state: 'routed' | 'notRouted' | 'pending', usage: unknown): ReceiptLoad => ({ status: 'loaded', response: { hubId: 'hub', state, receipt: null, usage } as never })

test('uso do turno: só o que o motor informou; o que falta é "não informado", nunca zero', () => {
  const uso = usoDoTurno('pt-BR', comUso('notRouted', { input: 12_345, cacheRead: 20, cacheWrite: null, output: 40 }))
  assert.ok(uso?.kind === 'informado')
  assert.deepEqual(uso.linhas.map((l) => [l.id, l.valor?.replace(/[\u00a0\u202f]/g, ' ') ?? null]), [['entrada', '12.345'], ['cacheLido', '20'], ['cacheGravado', null], ['saida', '40']])
  assert.deepEqual(usoDoTurno('pt-BR', comUso('routed', { input: null, cacheRead: null, cacheWrite: null, output: null })), { kind: 'naoInformado' })
  assert.deepEqual(usoDoTurno('pt-BR', comUso('routed', null)), { kind: 'semMedida' })
  // Número inválido do Hub (negativo, fracionado, texto) não vira número na tela.
  assert.deepEqual(usoDoTurno('pt-BR', comUso('routed', { input: -1, cacheRead: 1.5, cacheWrite: '7', output: Number.NaN })), { kind: 'naoInformado' })
})

test('uso do turno: Hub antigo (sem usage), na fila, em leitura ou com erro não mostram bloco', () => {
  assert.equal(usoDoTurno('pt-BR', state('notRouted')), null)
  assert.equal(usoDoTurno('pt-BR', comUso('pending', { input: 1, cacheRead: 1, cacheWrite: 1, output: 1 })), null)
  assert.equal(usoDoTurno('pt-BR', { status: 'loading' }), null)
  assert.equal(usoDoTurno('pt-BR', { status: 'error', code: 'x' }), null)
  assert.equal(usoDoTurno('pt-BR', undefined), null)
})

test('ícone aparece em turno não roteado quando há uso medido, e só então', () => {
  assert.equal(showsIcon(comUso('notRouted', { input: 1, cacheRead: null, cacheWrite: null, output: 2 })), true)
  assert.equal(showsIcon(comUso('notRouted', null)), false)
  assert.equal(showsIcon(state('notRouted')), false)
})

test('ícone não aparece em turno não roteado quando o motor não informou nenhuma contagem', () => {
  assert.equal(showsIcon(comUso('notRouted', { input: null, cacheRead: null, cacheWrite: null, output: null })), false)
  assert.equal(showsIcon(comUso('notRouted', { input: -1, cacheRead: 1.5, cacheWrite: 'x', output: null })), false)
  assert.equal(showsIcon(comUso('routed', { input: null, cacheRead: null, cacheWrite: null, output: null })), true, 'roteado sempre tem recibo')
})
