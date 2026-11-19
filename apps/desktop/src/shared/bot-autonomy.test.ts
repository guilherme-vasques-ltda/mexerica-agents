import assert from 'node:assert/strict'
import test from 'node:test'
import { botEditorUpdate, chooseNewBotPermission, inheritNewBotPermission } from './bot-autonomy.ts'
import { draftOf, normalizeBot } from './models.ts'

test('editar o perfil não transforma autonomia herdada em escolha do usuário', () => {
  for (const permission of ['ask', 'auto']) {
    const draft = draftOf(normalizeBot({ id: 'child', permission, createdBy: 'donna' }))
    const payload = botEditorUpdate({ ...draft, description: 'Novas instruções' })
    assert.equal('permission' in payload, false)
    assert.equal(payload.id, 'child')
    assert.equal(payload.description, 'Novas instruções')
    assert.equal(payload.model, null)
  }
})

test('novo bot segue a geral sem sinal; mexer no controle marca inclusive ao voltar', () => {
  for (const general of ['ask', 'auto']) {
    const base = draftOf(normalizeBot({ id: 'unused', permission: 'auto' }))
    delete base.id
    const draft = inheritNewBotPermission(base, general)
    assert.equal(draft.permission, general)
    assert.equal(draft.permissionChosenByUser, undefined)
    const selected = chooseNewBotPermission(chooseNewBotPermission(draft, general === 'ask' ? 'auto' : 'ask'), general)
    assert.equal(selected.permission, general)
    assert.equal(selected.permissionChosenByUser, true)
    assert.equal(inheritNewBotPermission(selected, 'other').permission, general)
  }
})
