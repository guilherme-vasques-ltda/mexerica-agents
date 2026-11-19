import assert from 'node:assert/strict'
import test from 'node:test'
import { abaAlterada, abaDaIntencao, abaPeloTeclado, abaHorizontalPeloTeclado, abasComErro } from './abas.ts'
import type { BotEditorProfile } from '../../../../shared/fork-bot-editor.ts'

const saved: BotEditorProfile = { name: 'Byte', title: '', description: '', avatarConfig: null, backend: 'codex', model: null, modelOptions: {}, fastMode: false, forkEditor: { engineAccountId: null } }

test('teclado percorre só as abas visíveis, com volta, Home/End e sem capturar Tab', () => {
  const visible = ['identity', 'engine', 'tools', 'danger'] as const
  assert.equal(abaPeloTeclado(visible, 'identity', 'ArrowDown'), 'engine')
  assert.equal(abaPeloTeclado(visible, 'tools', 'ArrowUp'), 'engine')
  assert.equal(abaPeloTeclado(visible, 'identity', 'ArrowUp'), 'danger')
  assert.equal(abaPeloTeclado(visible, 'danger', 'ArrowDown'), 'identity')
  assert.equal(abaPeloTeclado(visible, 'engine', 'Home'), 'identity')
  assert.equal(abaPeloTeclado(visible, 'engine', 'End'), 'danger')
  assert.equal(abaPeloTeclado(visible, 'engine', 'Tab'), null)
  assert.equal(abaPeloTeclado([], 'engine', 'ArrowDown'), null)
})

test('abas horizontais: esquerda/direita acompanham LTR/RTL e Home/End conservam a ordem', () => {
  const abas = ['vivo', 'ouvir', 'chaves'] as const
  for (const dir of ['ltr', 'rtl'] as const) {
    const avanca = dir === 'ltr' ? 'ArrowRight' : 'ArrowLeft'
    const volta = dir === 'ltr' ? 'ArrowLeft' : 'ArrowRight'
    assert.equal(abaHorizontalPeloTeclado(abas, 'vivo', avanca, dir), 'ouvir')
    assert.equal(abaHorizontalPeloTeclado(abas, 'chaves', avanca, dir), 'vivo')
    assert.equal(abaHorizontalPeloTeclado(abas, 'vivo', volta, dir), 'chaves')
    assert.equal(abaHorizontalPeloTeclado(abas, 'ouvir', 'Home', dir), 'vivo')
    assert.equal(abaHorizontalPeloTeclado(abas, 'ouvir', 'End', dir), 'chaves')
    assert.equal(abaHorizontalPeloTeclado([], 'vivo', avanca, dir), null)
  }
})

test('abas horizontais não capturam cima/baixo, Tab, Espaço ou Enter', () => {
  for (const dir of ['ltr', 'rtl'] as const) for (const tecla of ['ArrowUp', 'ArrowDown', 'Tab', ' ', 'Enter']) {
    assert.equal(abaHorizontalPeloTeclado(['vivo', 'ouvir', 'chaves'], 'vivo', tecla, dir), null)
  }
})

test('pendências do editor pertencem à aba aberta, sem interferir nas outras', () => {
  const draft = { ...saved, description: 'Rascunho não salvo' }
  assert.equal(abaAlterada('identity', draft, saved), true)
  assert.equal(abaAlterada('engine', draft, saved), false)
  assert.equal(abaAlterada('tools', draft, saved), false)
  const engine = { ...saved, forkEditor: { engineAccountId: 'outra-conta' }, modelOptions: { effort: 'high' } }
  assert.equal(abaAlterada('engine', engine, saved), true)
  assert.equal(abaAlterada('identity', engine, saved), false)
  assert.equal(abaAlterada('engine', engine, engine), false)
  assert.equal(abaAlterada('identity', null, saved), false)
})

test('operação e recusa mantêm a aba de origem após navegar', () => {
  assert.equal(abaDaIntencao('ownerPreferences'), 'identity')
  for (const key of ['backend', 'command', 'model', 'modelOptions', 'fastMode'] as const) assert.equal(abaDaIntencao(key), 'engine')
  for (const key of ['cwd', 'computer', 'permission'] as const) assert.equal(abaDaIntencao(key), 'behaviour')
  for (const key of ['skills', 'connectors', 'logins'] as const) assert.equal(abaDaIntencao(key), 'tools')
  assert.equal(abaDaIntencao('notify'), 'notifications')
})


test('Salvo segue a confirmação do editor: espaços preservados no campo, ordem das opções e padrões equivalentes', () => {
  assert.equal(abaAlterada('identity', { ...saved, description: '  texto  ' }, { ...saved, description: 'texto' }), false)
  assert.equal(abaAlterada('engine', { ...saved, fastMode: undefined, modelOptions: { a: '1', b: '2' } }, { ...saved, modelOptions: { b: '2', a: '1' } }), false)
})


test('erro e conflito marcam a aba de origem mesmo sem campo pendente; recusa de intenção mantém sua aba', () => {
  assert.deepEqual([...abasComErro(false, 'identity', saved, saved, {})], [])
  assert.deepEqual([...abasComErro(true, 'engine', saved, saved, {})], ['engine'])
  assert.deepEqual([...abasComErro(true, 'identity', { ...saved, description: 'pendente' }, saved, { skills: 'recusada' })], ['identity', 'tools'])
  assert.deepEqual([...abasComErro(false, 'identity', saved, saved, { notify: 'recusada' })], ['notifications'])
})
