import assert from 'node:assert/strict'
import { test } from 'node:test'
import { seloDoModelo } from './selo.ts'

test('modelo do motor Mexerica roda pela chave do fornecedor do id', () => {
  assert.deepEqual(seloDoModelo({ backend: 'mexerica', modelo: 'anthropic/claude-x', nomeDoMotor: 'Mexerica' }), { grupo: 'chaves', nome: 'Anthropic' })
  assert.deepEqual(seloDoModelo({ backend: 'mexerica', modelo: 'xai/grok-x', nomeDoMotor: 'Mexerica' }), { grupo: 'chaves', nome: 'xAI' })
  assert.deepEqual(seloDoModelo({ backend: 'mexerica', modelo: 'openrouter/vendor/model', nomeDoMotor: 'Mexerica' }), { grupo: 'chaves', nome: 'OpenRouter' })
})

test('fornecedor desconhecido, id sem prefixo e nomes herdados do objeto não ganham selo', () => {
  assert.equal(seloDoModelo({ backend: 'mexerica', modelo: 'desconhecido/x', nomeDoMotor: 'Mexerica' }), null)
  assert.equal(seloDoModelo({ backend: 'mexerica', modelo: 'sem-prefixo', nomeDoMotor: 'Mexerica' }), null)
  assert.equal(seloDoModelo({ backend: 'mexerica', modelo: 'constructor/x', nomeDoMotor: 'Mexerica' }), null)
  assert.equal(seloDoModelo({ backend: 'mexerica', modelo: '__proto__/x', nomeDoMotor: 'Mexerica' }), null)
})

test('Codex e Claude: plano na assinatura, chave na conta paga por uso, só o nome quando a cobrança não é conhecida', () => {
  assert.deepEqual(seloDoModelo({ backend: 'codex', modelo: 'm', nomeDoMotor: 'Codex', cobranca: 'subscription' }), { grupo: 'planos', nome: 'Codex' })
  assert.deepEqual(seloDoModelo({ backend: 'claude', modelo: 'm', nomeDoMotor: 'Claude Code', cobranca: 'paid_api' }), { grupo: 'chaves', nome: 'Claude Code' })
  assert.deepEqual(seloDoModelo({ backend: 'codex', modelo: 'm', nomeDoMotor: 'Codex', cobranca: 'unverified' }), { grupo: null, nome: 'Codex' })
  assert.deepEqual(seloDoModelo({ backend: 'codex', modelo: 'm', nomeDoMotor: 'Codex' }), { grupo: null, nome: 'Codex' })
})

test('outros motores não têm selo de conexão', () => {
  assert.equal(seloDoModelo({ backend: 'copilot', modelo: 'm', nomeDoMotor: 'Copilot' }), null)
})
