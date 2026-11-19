import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'
import { detalheTroca, linhaTroca } from './detalhe.ts'

// `useT` lê o documento; aqui o mesmo `traduzir` sem DOM.
const em = (idioma: AppLanguage) => ((chave: Parameters<typeof traduzir>[1], params?: Record<string, string>) => (traduzir as (i: AppLanguage, c: string, p?: Record<string, string>) => string)(idioma, chave, params)) as unknown as Tradutor
const pt = em('pt-BR')
const en = em('en')

test('o balão diz como a conversa passou e quanto foi relido, no idioma do app', () => {
  assert.equal(detalheTroca(pt, 'pt-BR', { carried: 'session', resentTokens: 84_200 }), 'O modelo novo retomou a conversa inteira. 84\u00a0mil tokens reenviados, sem aproveitar o que já estava pronto.')
  assert.equal(detalheTroca(en, 'en', { carried: 'handoff', resentTokens: 900 }), 'The new model got the summary and the latest requests, not the whole conversation. 900 tokens sent, without reusing what was already prepared.')
})

test('antes do primeiro uso do motor novo, sem número inventado', () => {
  assert.equal(detalheTroca(pt, 'pt-BR', { carried: 'handoff', resentTokens: null }), 'O modelo novo recebeu o resumo e os últimos pedidos, não a conversa inteira. Enviando…')
})

test('a linha usa os nomes do Hub; sem eles, o texto gravado', () => {
  const s = { from: { label: 'Claude Code opus' }, to: { label: 'Codex gpt-6.1-sol' } }
  assert.equal(linhaTroca(en, s, 'x'), 'Model changed from Claude Code opus to Codex gpt-6.1-sol. The conversation was passed to the new model.')
  assert.equal(linhaTroca(pt, {}, 'Texto do Hub'), 'Texto do Hub')
})
