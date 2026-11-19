import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { Escolha, Provedor } from '../seletor-modelo/regras.ts'
import { chaveDoSeletor, gravacoesDaEscolha, provedoresDoEditor } from './seletor-editor.ts'

const atual: Escolha = { accountId: null, backend: 'claude', model: 'claude-opus-5-5', modelOptions: { effort: 'high' }, fastMode: false }

test('sem conta: a escolha do seletor vira as intenções de antes (model, modelOptions, fastMode)', () => {
  assert.deepEqual(gravacoesDaEscolha(atual, { ...atual, model: 'claude-sonnet-5-5', modelOptions: {} }, false),
    [{ via: 'intencao', control: 'model', patch: { model: 'claude-sonnet-5-5', modelOptions: {} } }])
  assert.deepEqual(gravacoesDaEscolha(atual, { ...atual, modelOptions: { effort: 'max' } }, false),
    [{ via: 'intencao', control: 'modelOptions', patch: { modelOptions: { effort: 'max' } } }])
  assert.deepEqual(gravacoesDaEscolha(atual, { ...atual, fastMode: true }, false),
    [{ via: 'intencao', control: 'fastMode', patch: { fastMode: true } }])
  assert.deepEqual(gravacoesDaEscolha(atual, { ...atual, model: null, modelOptions: {} }, false),
    [{ via: 'intencao', control: 'model', patch: { model: null, modelOptions: {} } }], '"Padrão do motor" = modelo vazio')
  assert.deepEqual(gravacoesDaEscolha(atual, { ...atual }, false), [], 'nada mudou, nada grava')
})

test('com conta: modelo e opções pelo patch versionado do editor; velocidade continua pela intenção', () => {
  const conta = { ...atual, accountId: 'acc-1' }
  assert.deepEqual(gravacoesDaEscolha(conta, { ...conta, model: 'claude-sonnet-5-5', modelOptions: {} }, true),
    [{ via: 'editor', patch: { model: 'claude-sonnet-5-5', modelOptions: {} } }])
  assert.deepEqual(gravacoesDaEscolha(conta, { ...conta, modelOptions: { effort: 'low' } }, true),
    [{ via: 'editor', patch: { modelOptions: { effort: 'low' } } }])
  assert.deepEqual(gravacoesDaEscolha(conta, { ...conta, fastMode: true }, true), [{ via: 'intencao', control: 'fastMode', patch: { fastMode: true } }])
})

test('o editor não troca o motor pelo seletor: escolha de outro motor não grava nada', () => {
  assert.deepEqual(gravacoesDaEscolha(atual, { ...atual, backend: 'codex', model: 'gpt-6.1-sol' }, false), [])
})

test('provedores do editor: só o motor salvo; com conta, só os modelos comprovados e sem o "Default"', () => {
  const provedores: Provedor[] = [
    { id: 'claude', name: 'Claude Code', models: [{ id: 'claude-opus-5-5', name: 'Claude Opus 5.5', contextTokens: 1_000_000 }, { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5' }], padrao: 'default' },
    { id: 'codex', name: 'Codex', models: [] },
  ]
  assert.deepEqual(provedoresDoEditor(provedores, 'claude', null).map((p) => p.id), ['claude'])
  const comConta = provedoresDoEditor(provedores, 'claude', { models: [
    { id: 'default', options: [] }, { id: 'claude-opus-5-5', options: [{ id: 'effort', values: ['high'], default: 'high' }] },
  ] })
  assert.deepEqual(comConta[0]!.models!.map((m) => [m.id, m.name, m.contextTokens]), [['claude-opus-5-5', 'Claude Opus 5.5', 1_000_000]],
    'só o comprovado pela conta (Haiku fica de fora), com nome e contexto do catálogo real')
  const comDefault = provedoresDoEditor([{ id: 'claude', name: 'Claude Code', models: [{ id: 'default', name: 'Default (recommended)' }] }], 'claude',
    { models: [{ id: 'default', options: [] }, { id: 'claude-opus-5-5', options: [] }] })
  assert.deepEqual(comDefault[0]!.models!.map((m) => m.id), ['claude-opus-5-5'], 'o "Default (recommended)" fica escondido')
  assert.equal(comDefault[0]!.padrao, 'default')
})

test('trocar o motor remonta o seletor: a chave muda com o motor e só com ele (rodada 2 da Sol)', () => {
  assert.notEqual(chaveDoSeletor('codex'), chaveDoSeletor('claude'))
  assert.equal(chaveDoSeletor('claude'), chaveDoSeletor('claude'))
  // Depois da troca, a única aba possível é a do motor novo: é nela que o seletor remontado nasce (sem favoritos).
  const depois = provedoresDoEditor([{ id: 'codex', name: 'Codex', models: [] }, { id: 'claude', name: 'Claude Code', models: [] }], 'claude', null)
  assert.deepEqual(depois.map((p) => p.id), ['claude'])
})
