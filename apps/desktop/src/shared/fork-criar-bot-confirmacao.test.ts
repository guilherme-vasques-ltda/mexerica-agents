import test from 'node:test'
import assert from 'node:assert/strict'
import { confirmarConta, contaConfirmadaPara, reconciliarConfirmacao, type ContextoConta } from './fork-criar-bot-confirmacao.ts'

const choice: ContextoConta = { hubId: 'hub-A', epoch: 1, backend: 'codex', accountId: 'conta-X', spaceId: 'trabalho' }
test('D543: confirmação confere o ID efetivamente enviado, não apenas o estado marcado', () => {
  const confirmed = confirmarConta(choice)
  assert.equal(contaConfirmadaPara(confirmed, choice, 'conta-X'), true)
  assert.equal(contaConfirmadaPara(confirmed, choice, 'conta-Y'), false)
  assert.equal(contaConfirmadaPara(confirmed, choice, null), false)
  assert.equal(confirmarConta({ ...choice, accountId: null }), null)
})
test('D543: motor, Hub, geração, Space e conta invalidam a confirmação; voltar não a recupera', () => {
  for (const change of [{ backend: 'claude' }, { hubId: 'hub-B' }, { epoch: 2 }, { spaceId: 'casa' }, { accountId: 'conta-Y' }]) {
    let confirmed = confirmarConta(choice)
    const changed = { ...choice, ...change }
    assert.equal(contaConfirmadaPara(confirmed, changed, changed.accountId), false)
    confirmed = reconciliarConfirmacao(confirmed, changed)
    assert.equal(confirmed, null)
    assert.equal(reconciliarConfirmacao(confirmed, choice), null)
  }
})
test('D543: releitura que troca o padrão de X para Y não herda confirmação', () => {
  const confirmed = confirmarConta(choice)
  assert.equal(contaConfirmadaPara(confirmed, { ...choice, accountId: 'conta-Y' }, 'conta-Y'), false)
  assert.equal(contaConfirmadaPara(confirmed, { ...choice }, 'conta-X'), true, 'releitura estável conserva a escolha')
})
