import { test } from 'node:test'
import assert from 'node:assert/strict'
import { tradutor } from '../../lib/fork-i18n/index.ts'
import { routineNoticeText, subscriptionNoticeText } from './notice-label.ts'

const t = tradutor('pt-BR')

test('traduz a recusa de assinatura e não altera outros avisos', () => {
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
    const traduzir = tradutor(idioma)
    assert.equal(subscriptionNoticeText(traduzir, 'personal_subscription_chat_only: detalhe'), traduzir('contas.assinaturaSoChat'))
    assert.equal(subscriptionNoticeText(traduzir, 'personal_subscription_no_fallback: detalhe'), traduzir('contas.assinaturaSemTroca'))
    assert.equal(subscriptionNoticeText(traduzir, 'meta_standard_unverified: detalhe'), traduzir('contas.apiDireta.erro.metaStandard'))
    assert.equal(subscriptionNoticeText(traduzir, 'generic error'), 'generic error')
  }
})

// D169 R5: só o prefixo do app muda; o nome da rotina (conteúdo do usuário) fica igual.
test('aviso de rotina: troca só o prefixo do Hub e preserva o nome', () => {
  assert.equal(routineNoticeText(t, 'Routine · Resumo da semana'), 'Rotina · Resumo da semana')
  assert.equal(routineNoticeText(t, 'Routine · Routine · Daily report'), 'Rotina · Routine · Daily report')
  assert.equal(routineNoticeText(t, 'Rotina · já em português'), 'Rotina · já em português')
  assert.equal(routineNoticeText(t, 'My Routine · notes'), 'My Routine · notes')
  assert.equal(routineNoticeText(t, undefined), 'Rotina')
  assert.equal(routineNoticeText(tradutor('en'), 'Routine · Resumo da semana'), 'Routine · Resumo da semana')
})
