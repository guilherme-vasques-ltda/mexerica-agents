import assert from 'node:assert/strict'
import test from 'node:test'
import { sessionMenuCurrent, sessionMenuKey, sessionMenuRow, type SessionMenuScope } from './menu.ts'
import { CATALOGOS, superficieIdioma, tradutor } from '../../../lib/fork-i18n/index.ts'

test('D545: uma intenção de menu não atravessa store, Space, conversa, link ou reconexão', () => {
  const scope: SessionMenuScope = { store: {}, client: {}, spaceId: 'space-a', hubId: 'hub', chatId: 'bot', epoch: 1 }
  assert.equal(sessionMenuCurrent(scope, { ...scope }), true)
  assert.equal(sessionMenuCurrent(null, scope), false)
  assert.equal(sessionMenuCurrent({ ...scope, spaceId: undefined }, { ...scope, spaceId: undefined }), false)
  assert.equal(sessionMenuCurrent({ ...scope, spaceId: '' }, { ...scope, spaceId: '' }), false)
  for (const change of [{ store: {} }, { spaceId: 'space-b' }, { chatId: 'outro' }, { client: {} }, { client: null }, { epoch: 2 }, { spaceId: undefined }, { spaceId: '' }]) {
    assert.equal(sessionMenuCurrent(scope, { ...scope, ...change }), false, JSON.stringify(Object.keys(change)))
  }
})

test('D545: Shift+F10 e tecla de menu; F10 e outras teclas preservam a ação original', () => {
  for (const [key, shiftKey, expected] of [['F10', true, true], ['ContextMenu', false, true], ['F10', false, false], ['Enter', true, false]] as const) {
    let opened = 0, prevented = 0
    assert.equal(sessionMenuKey({ key, shiftKey, preventDefault: () => { prevented++ } }, () => { opened++ }), expected)
    assert.equal(opened, Number(expected))
    assert.equal(prevented, Number(expected))
  }
})

test('D545: seis idiomas completos, com direção RTL no portal árabe', () => {
  const keys = ['copyId', 'copyName', 'copySessionName', 'archive', 'delete', 'deleteUnavailable', 'label', 'copied', 'copyFailed'] as const
  for (const language of Object.keys(CATALOGOS) as (keyof typeof CATALOGOS)[]) {
    for (const key of keys) assert.ok(CATALOGOS[language][`sessions.menu.${key}`]?.trim())
    assert.equal(superficieIdioma(language).dir, language === 'ar-AE' ? 'rtl' : 'ltr')
  }
  assert.equal(tradutor('pt-BR')('sessions.menu.archive'), 'Arquivar')
})


test('D545: copiar a principal antes do catálogo conserva nome e caixa; extra desconhecida não é inventada', () => {
  assert.deepEqual(sessionMenuRow(null, { id: 'canal', name: 'Lançamento' }, 'canal'),
    { id: 'canal', principal: true, state: 'open', title: 'Lançamento' })
  assert.equal(sessionMenuRow(null, { id: 'canal', name: 'Lançamento' }, 'extra'), null)
  assert.equal(sessionMenuRow(null, null, ''), null)
  for (const language of Object.keys(CATALOGOS) as (keyof typeof CATALOGOS)[]) {
    assert.ok(CATALOGOS[language]['sessions.untitled'].trim())
    assert.ok(CATALOGOS[language]['conversa.arquivar.encerrar.titulo'].includes('{titulo}'))
    assert.equal('conversa.arquivar.encerrar.menu' in CATALOGOS[language], false)
    assert.equal('conversa.arquivar.encerrar.confirmar' in CATALOGOS[language], false)
  }
  assert.equal(tradutor('pt-BR')('sessions.untitled'), 'Sem título')
  assert.equal(tradutor('pt-BR')('conversa.arquivar.encerrar.titulo', { titulo: 'Teste' }), 'Arquivar “Teste”?')
})
