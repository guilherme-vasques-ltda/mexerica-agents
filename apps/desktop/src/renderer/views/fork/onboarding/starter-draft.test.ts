import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ComposerDrafts } from '../../../../shared/composer-drafts.ts'
import { applyStarterDraft } from './starter-draft.ts'

test('sugestão troca só o próprio rascunho intacto e não apaga texto nem anexos', () => {
  const drafts = new ComposerDrafts({ getItem: () => null, setItem: () => {} }, 'fixture')
  const first = applyStarterDraft(drafts, 'berga', 'Primeira tarefa', undefined)
  const files = [{ id: 'f', name: 'notas.txt', data: new Uint8Array() }]
  drafts.setFiles('berga', files)
  const second = applyStarterDraft(drafts, 'berga', 'Segunda tarefa', first)
  assert.equal(drafts.get('berga'), 'Segunda tarefa')
  assert.equal(drafts.files('berga'), files)
  assert.equal(second?.segments[0]?.kind, 'generated')

  drafts.edit('berga', 'Segunda tarefa com notas', 'typed')
  assert.equal(applyStarterDraft(drafts, 'berga', 'Terceira tarefa', second), undefined)
  assert.equal(drafts.get('berga'), 'Segunda tarefa com notas')
  drafts.edit('berga', 'Segunda tarefa', 'typed')
  assert.equal(applyStarterDraft(drafts, 'berga', 'Terceira tarefa', second), undefined)
  assert.equal(applyStarterDraft(drafts, 'berga', 'Terceira tarefa', undefined), undefined)
  assert.equal(drafts.get('berga'), 'Segunda tarefa')

  drafts.edit('outro', 'Meu texto', 'typed')
  assert.equal(applyStarterDraft(drafts, 'outro', 'Terceira tarefa', second), undefined)
  assert.equal(drafts.get('outro'), 'Meu texto')
  drafts.retire()
  assert.equal(applyStarterDraft(drafts, 'outro', 'Outra tarefa', drafts.origin('outro')), undefined)
  assert.equal(drafts.get('outro'), 'Meu texto')
})
