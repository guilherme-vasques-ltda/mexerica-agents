import assert from 'node:assert/strict'
import { test } from 'node:test'
import { encodeTemplate } from './bot-template.ts'
import { draftOf, normalizeBot } from './models.ts'

test('exportar um modelo preserva o perfil sem carregar contas ou escolhas de motor/modelo', () => {
  const draft = draftOf(normalizeBot({
    id: 'source', name: 'Scout', description: 'Pesquisar', backend: 'custom', command: 'custom-cli',
    model: 'source-model', modelOptions: { effort: 'high' }, fastMode: true,
    connectors: ['mcp', 'ferramenta-mail'], logins: ['account'], skills: ['research'],
    pinned: true, hidden: false,
  }))
  const before = structuredClone(draft)
  const template = JSON.parse(encodeTemplate(draft))
  for (const key of ['id', 'pinned', 'hidden', 'connectors', 'logins', 'backend', 'command', 'model', 'modelOptions', 'fastMode']) {
    assert.equal(key in template, false, key)
  }
  assert.equal(template.name, 'Scout')
  assert.equal(template.description, 'Pesquisar')
  assert.deepEqual(template.skills, ['research'])
  assert.deepEqual(draft, before, 'a exportação não altera o bot de origem')
})
