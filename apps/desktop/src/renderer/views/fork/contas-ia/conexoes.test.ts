import assert from 'node:assert/strict'
import { test } from 'node:test'
import { agrupar, buscar, chavesDaTela, chavesDisponiveis, ehOpen, planosDaTela, type ContextoDaTela } from './conexoes.ts'

const nome = (engine: string) => (engine === 'codex' ? 'Codex' : engine === 'claude' ? 'Claude Code' : 'Grok Build')
const fornecedores = [{ id: 'anthropic', nome: 'Anthropic' }, { id: 'openai', nome: 'OpenAI' }, { id: 'xai', nome: 'xAI' }]
const completa: ContextoDaTela = { open: false, isLoopback: true, backends: [{ id: 'codex' }, { id: 'mexerica' }] }
const itens = (ctx: ContextoDaTela) => [...planosDaTela(ctx.open, nome as never), ...chavesDaTela(ctx, fornecedores)]

test('edição completa: Planos com Codex e Claude, e Chaves de API com o que o Hub lista', () => {
  const grupos = agrupar(itens(completa))
  assert.deepEqual(grupos.map((g) => g.grupo), ['planos', 'chaves'])
  assert.deepEqual(grupos[0]!.itens.map((i) => i.id), ['plano:codex', 'plano:claude', 'plano:grok'])
  assert.deepEqual(grupos[1]!.itens.map((i) => i.id), ['chave:anthropic', 'chave:openai', 'chave:xai'])
})

test('Open: só o Codex, nunca chaves, mesmo com o motor Mexerica e o Hub local', () => {
  const open = { ...completa, open: true }
  assert.equal(chavesDisponiveis(open), false)
  assert.deepEqual(itens(open).map((i) => i.id), ['plano:codex'])
  assert.deepEqual(agrupar(itens(open)).map((g) => g.grupo), ['planos'])
})

test('a edição vem só da ponte do app; sem ponte, é a completa', () => {
  assert.equal(ehOpen({ appScheme: 'mexerica-open' }), true)
  assert.equal(ehOpen({ appScheme: 'donnabot' }), false)
  assert.equal(ehOpen(undefined), false)
})

test('chaves só com o motor ligado e no Hub deste computador', () => {
  assert.deepEqual(chavesDaTela({ ...completa, backends: [{ id: 'codex' }] }, fornecedores), [])
  assert.deepEqual(chavesDaTela({ ...completa, isLoopback: false }, fornecedores), [])
})

test('busca ignora acento e caixa, aceita pedaços e vazia devolve tudo', () => {
  const todos = itens(completa)
  assert.deepEqual(buscar(todos, 'CLAUDE cod').map((i) => i.id), ['plano:claude'])
  assert.deepEqual(buscar(todos, 'open').map((i) => i.id), ['chave:openai'])
  assert.deepEqual(buscar(todos, '  ').length, todos.length)
  assert.deepEqual(buscar(todos, 'zzz'), [])
  assert.deepEqual(agrupar(buscar(todos, 'xai')).map((g) => g.grupo), ['chaves'])
})
