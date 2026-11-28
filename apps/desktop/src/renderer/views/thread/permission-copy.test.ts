import { strict as assert } from 'node:assert'
import { test } from 'node:test'
import { tradutor } from '../../lib/fork-i18n/index.ts'
import { actionLabel, capturePermissionNote, clickPermissionNote, memoryPermissionNote, permissionDetail, permissionHeadline, permissionTitle } from './permission-copy.ts'

const t = tradutor('pt-BR')
const en = tradutor('en')

test('identifica servidor memory sem afirmar que é interno ou que salvou', () => {
  assert.equal(permissionTitle(t, 'mcp_memory', 'Donna'), 'Donna quer usar uma ferramenta de memória')
  assert.equal(permissionHeadline(t, 'mcp_memory'), 'Quer usar uma ferramenta de memória')
  assert.equal(actionLabel(t, 'mcp_memory'), 'Ferramenta de memória')
  assert.equal(permissionDetail(t, 'mcp_memory', 'Autorizar mcp.memory.remember: memory'), 'Nome técnico: mcp.memory.remember')
  assert.equal(memoryPermissionNote(t, 'mcp_memory'), 'O Codex não confirma ao app se esta é a memória deste bot.')
  assert.equal(permissionTitle(t, 'execute', 'Donna'), 'Donna quer rodar um comando')
  assert.equal(actionLabel(t, 'execute'), 'Comando')
  assert.equal(permissionDetail(t, 'execute', 'Autorizar mcp.memory.remember: memory'), 'Autorizar mcp.memory.remember: memory')
  assert.equal(memoryPermissionNote(t, 'execute'), '')
})

test('a nota de memória vale para bots com outros nomes', () => {
  assert.equal(permissionTitle(t, 'mcp_memory', 'Lia'), 'Lia quer usar uma ferramenta de memória')
  assert.equal(memoryPermissionNote(t, 'mcp_memory'), 'O Codex não confirma ao app se esta é a memória deste bot.')
  assert.equal(memoryPermissionNote(t, undefined), '')
})

test('mostra o nome técnico somente quando o título tratado começa com mcp.', () => {
  assert.equal(permissionDetail(t, 'mcp_memory', 'mcp.memory.remember'), 'Nome técnico: mcp.memory.remember')
  for (const title of ['Consultar lembranças', 'Autorizar Consultar lembranças: memory', 'mcp', 'prefixo mcp.memory.remember', '', undefined]) {
    assert.equal(permissionDetail(t, 'mcp_memory', title), '')
  }
})

test('preserva o título original de outros tipos de ferramenta', () => {
  assert.equal(permissionDetail(t, 'read', 'Consultar lembranças'), 'Consultar lembranças')
  assert.equal(permissionDetail(t, undefined, 'Autorizar mcp.memory.remember: memory'), 'Autorizar mcp.memory.remember: memory')
  assert.equal(permissionDetail(t, 'execute', undefined), '')
})

test('em inglês, a frase com o autor vem inteira do catálogo', () => {
  assert.equal(permissionTitle(en, 'execute', 'Lia'), 'Lia wants to run a command')
  assert.equal(permissionHeadline(en, 'fetch'), 'Wants to access the web')
  assert.equal(actionLabel(en, 'unknown'), 'Tool')
})

test('a captura informa a tela inteira, o motor e o possível envio ao provedor', () => {
  assert.equal(permissionTitle(t, 'mac_local_screen_action', 'Berga'), 'Berga quer capturar a tela principal')
  assert.equal(actionLabel(t, 'mac_local_screen_action'), 'Captura de tela')
  assert.match(capturePermissionNote(t, 'mac_local_screen_action'), /tela principal inteira.*motor do bot.*provedor do modelo/)
  assert.match(capturePermissionNote(en, 'mac_local_screen_action'), /entire main display.*bot’s engine.*model provider/)
  assert.equal(capturePermissionNote(t, 'screen_access'), '')
})

test('o clique mostra a ação e pede voltar à janela aprovada', () => {
  const kind = 'mac_local_screen_click'
  assert.equal(permissionTitle(t, kind, 'Berga'), 'Berga quer acionar um botão')
  assert.equal(actionLabel(t, kind), 'Acionar botão')
  assert.match(clickPermissionNote(t, kind), /volte à janela.*10 segundos/i)
  assert.equal(clickPermissionNote(t, 'mac_local_screen_action'), '')
})
