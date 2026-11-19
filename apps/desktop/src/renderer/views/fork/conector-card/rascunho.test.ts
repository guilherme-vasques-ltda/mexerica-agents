import { test } from 'node:test'
import assert from 'node:assert/strict'
import { lerRascunho, telaRascunho } from './rascunho.ts'

const raw = () => ({ receipt: { draftId: 'draft', state: 'draft', deadline: 90_000 }, capability: { approve: false, send: false },
  presentation: { text: 'Resposta', account: '+5511900000000', replyTo: { messageId: 'M1', sender: 'Contato', excerpt: 'Original', origin: 'external' } } })

test('rascunho só apresenta privado completo, do mesmo ID e antes do prazo', () => {
  assert.equal(lerRascunho(raw(), 'draft', 89_999)?.replyTo.messageId, 'M1')
  assert.equal(lerRascunho(raw(), 'outro', 1), null)
  assert.equal(lerRascunho(raw(), 'draft', 90_000), null)
  assert.equal(lerRascunho({ ...raw(), presentation: null }, 'draft', 1), null)
})

test('encerrado, sem origem externa ou com poder de envio não é este contrato', () => {
  const closed = raw(); closed.receipt.state = 'closed'
  assert.equal(lerRascunho(closed, 'draft', 1), null)
  const altered = raw(); altered.presentation.replyTo.origin = 'human'
  assert.equal(lerRascunho(altered, 'draft', 1), null)
  const enabled = raw(); enabled.capability.send = true
  assert.equal(lerRascunho(enabled, 'draft', 1), null)
})

test('erro com prazo vencido fica indisponível; só closed do Hub confirma fechamento', () => {
  const presentation = lerRascunho(raw(), 'draft', 1)!
  const draft = Object.freeze({ state: 'draft' as const, deadline: 90_000 })
  assert.deepEqual(telaRascunho(draft, true, 90_000, presentation),
    { closed: false, visible: null, failed: true })
  assert.deepEqual(telaRascunho(draft, false, 90_000, presentation),
    { closed: false, visible: null, failed: false })
  assert.deepEqual(telaRascunho({ ...draft, state: 'closed' }, true, 90_000, presentation),
    { closed: true, visible: null, failed: false })
  assert.deepEqual(telaRascunho({ ...draft, state: 'closed' }, false, 1, presentation),
    { closed: true, visible: null, failed: false })
  assert.equal(telaRascunho(draft, false, 89_999, presentation).visible, presentation)
  assert.equal(draft.deadline, 90_000)
})
