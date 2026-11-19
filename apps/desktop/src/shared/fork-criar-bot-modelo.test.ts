import assert from 'node:assert/strict'
import { test } from 'node:test'
import { corpoDaCriacao } from './fork-criar-bot-destino.ts'
import { aplicarEscolha, modelosDaConta, valorDoSeletor } from './fork-criar-bot-modelo.ts'

const draft = { name: 'Crivo', backend: 'claude', model: null as string | null, modelOptions: {}, fastMode: false }

test('a escolha do seletor vai ao rascunho e o createBot grava modelo, opções e fastMode booleano', () => {
  const r = aplicarEscolha(draft, { conta: 'acc-1', spaceId: null }, 'claude',
    { accountId: 'acc-1', backend: 'claude', model: 'claude-sonnet-5-5', modelOptions: { effort: 'high' }, fastMode: false })
  assert.equal(r.motorTrocado, false)
  assert.deepEqual(r.destino, { conta: 'acc-1', spaceId: null }, 'mesmo motor: a conta fica')
  const corpo = corpoDaCriacao(r.draft, r.destino, 'personal')
  assert.equal(corpo.model, 'claude-sonnet-5-5')
  assert.deepEqual(corpo.modelOptions, { effort: 'high' })
  assert.equal(corpo.fastMode, false)
  assert.equal('fast' in corpo.modelOptions, false, 'nada de modelOptions.fast')
  assert.deepEqual(corpo.forkEditor, { engineAccountId: 'acc-1' })
  assert.deepEqual(valorDoSeletor(r.draft, 'claude', 'acc-1'),
    { accountId: 'acc-1', backend: 'claude', model: 'claude-sonnet-5-5', modelOptions: { effort: 'high' }, fastMode: false })
})

test('modelo de outro motor (outra aba do seletor) troca o motor e tira a conta, como o passo', () => {
  const r = aplicarEscolha(draft, { conta: 'acc-1', spaceId: 'loja' }, 'claude',
    { accountId: null, backend: 'codex', model: 'gpt-6.1-sol', modelOptions: {}, fastMode: true })
  assert.equal(r.motorTrocado, true)
  assert.deepEqual(r.destino, { conta: null, spaceId: 'loja' })
  assert.deepEqual({ backend: r.draft.backend, model: r.draft.model, fastMode: r.draft.fastMode }, { backend: 'codex', model: 'gpt-6.1-sol', fastMode: true })
})

test('com conta, só os modelos comprovados; contexto do catálogo real ou nenhum número', () => {
  const lista = modelosDaConta(
    [{ id: 'claude-opus-5-5', options: [{ id: 'effort', values: ['high'], default: 'high' }] }, { id: 'so-na-conta', options: [] }],
    [{ id: 'claude-opus-5-5', name: 'Claude Opus 5.5', contextTokens: 1_000_000 }, { id: 'claude-haiku-5-5', name: 'Haiku', contextTokens: 200_000 }],
  )
  assert.deepEqual(lista.map((m) => [m.id, m.name, m.contextTokens]), [['claude-opus-5-5', 'Claude Opus 5.5', 1_000_000], ['so-na-conta', 'so-na-conta', null]])
  assert.deepEqual(lista[0]!.options?.[0]?.values, [{ id: 'high', name: 'high' }])
})
