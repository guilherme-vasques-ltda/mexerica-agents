import assert from 'node:assert/strict'
import { test } from 'node:test'
import { outcomeLabel, runOutcomeText } from './run-outcome.ts'

test('cada desfecho tem rótulo, tom e frase simples; ✓ só em sem novidades e com resultado', () => {
  const cases: [string, object | null | undefined, string, string][] = [
    ['succeeded', { version: 1, kind: 'noNews', source: 'declared' }, 'Sem novidades', 'ok'],
    ['succeeded', { version: 1, kind: 'reported', source: 'declared', summary: 'Pedido enviado' }, 'Com resultado', 'ok'],
    ['failed', { version: 1, kind: 'needsOwner', source: 'declared', summary: 'Falta a senha do banco' }, 'Precisa de você', 'attention'],
    ['failed', { version: 1, kind: 'failed', source: 'declared', summary: 'Site fora do ar' }, 'Não conseguiu', 'danger'],
    ['failed', { version: 1, kind: 'failed', source: 'observed', reason: 'timeout' }, 'Falhou', 'danger'],
    ['succeeded', { version: 1, kind: 'unconfirmed', source: 'none', reason: 'noDeclaration', visibleReply: true }, 'Sem confirmação', 'neutral'],
    ['succeeded', { version: 1, kind: 'unconfirmed', source: 'declared', reason: 'conflict' }, 'Sem confirmação', 'neutral'],
    ['interrupted', { version: 1, kind: 'unconfirmed', source: 'observed', reason: 'interrupted' }, 'Interrompida', 'danger'],
    ['cancelled', { version: 1, kind: 'unconfirmed', source: 'observed', reason: 'stopped' }, 'Cancelada', 'neutral'],
    ['succeeded', { kind: 'unknown' }, 'Sem confirmação', 'neutral'],
    ['succeeded', { kind: 'algoNovo' }, 'Sem confirmação', 'neutral'],
    ['succeeded', undefined, 'Terminou', 'neutral'],
    ['failed', null, 'Falhou', 'danger'],
    ['running', undefined, 'Em execução', 'active'],
    ['estranho', undefined, 'Sem confirmação', 'neutral'],
  ]
  for (const [status, outcome, label, tone] of cases) {
    const text = runOutcomeText(status, outcome, 'pt-BR')
    assert.deepEqual([text.label, text.tone], [label, tone], `${status} ${JSON.stringify(outcome)}`)
    assert.doesNotMatch(outcomeLabel(text), /ACP|stopReason|declara|Hub|motor|Concluída/)
  }
  assert.equal(runOutcomeText('succeeded', { version: 1, kind: 'reported', source: 'declared', summary: 'Pedido enviado' }, 'pt-BR').sentence, 'Segundo o bot: Pedido enviado')
  assert.match(runOutcomeText('succeeded', undefined, 'pt-BR').sentence, /resultado não foi registrado/)
})

test('R1: origem ou versão desconhecida nunca dá ✓ nem "precisa de você"', () => {
  for (const kind of ['reported', 'noNews', 'needsOwner']) {
    for (const outcome of [
      { version: 1, kind, source: 'unknown', summary: 'resumo' },
      { version: 1, kind, source: 'observed', summary: 'resumo' },
      { kind, source: 'declared', summary: 'resumo' },
      { version: 2, kind, source: 'declared', summary: 'resumo' },
      { version: 1, kind, source: 'declared', reason: 'unknown', summary: 'resumo' },
      { version: 1, kind, source: 'declared', reason: 'motivoFuturo', summary: 'resumo' },
    ]) {
      const text = runOutcomeText('succeeded', outcome, 'pt-BR')
      assert.deepEqual([text.label, text.tone], ['Sem confirmação', 'neutral'], JSON.stringify(outcome))
    }
  }
  // Falha de origem desconhecida continua falha, sem a atribuição "Segundo o bot".
  const failed = runOutcomeText('failed', { version: 1, kind: 'failed', source: 'unknown', summary: 'x' }, 'pt-BR')
  assert.deepEqual([failed.label, failed.tone], ['Falhou', 'danger'])
  // Controle positivo: declarado na versão conhecida.
  assert.equal(runOutcomeText('succeeded', { version: 1, kind: 'reported', source: 'declared', summary: 'ok' }, 'pt-BR').tone, 'ok')
  assert.equal(runOutcomeText('succeeded', { version: 1, kind: 'noNews', source: 'declared' }, 'pt-BR').tone, 'ok')
})

test('DON-917: o desfecho sai no idioma pedido e marca "sem confirmação" sem depender do texto', () => {
  const en = runOutcomeText('succeeded', { version: 1, kind: 'reported', source: 'declared', summary: 'Pedido enviado' }, 'en')
  assert.deepEqual([en.label, en.sentence], ['With result', 'According to the bot: Pedido enviado'])
  for (const idioma of ['en', 'pt-BR', 'ar-AE'] as const) assert.equal(runOutcomeText('succeeded', { kind: 'unknown' }, idioma).unconfirmed, true)
  assert.equal(runOutcomeText('running', undefined, 'en').label, 'Running')
})
