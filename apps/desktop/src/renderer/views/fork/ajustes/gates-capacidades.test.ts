import assert from 'node:assert/strict'
import { test } from 'node:test'
import { capacidades } from '../../../../shared/fork-capacidades.ts'
import { createWebBridge } from '../../../../web/platform.ts'
import { abasVisiveis, mostraPerfilConta, paginaDisponivel } from './gates-capacidades.ts'

const entrou = { isSignedIn: true, isExitUnresolved: false, isSigningOut: false }

test('Web: com a conta entrada, o bloco de perfil não renderiza (nem chama a ponte que devolve webUnavailable)', async () => {
  const bridge = createWebBridge()
  const cap = capacidades(bridge)
  assert.equal(mostraPerfilConta(cap, entrou), false)
  // Se o bloco renderizasse, esta seria a mensagem crua que o usuário veria.
  const r = await bridge.accountProfile.load()
  assert.equal(r.ok, false)
  assert.equal(!r.ok && r.message, 'webUnavailable')
})

test('Desktop: o bloco de perfil só aparece com a conta confirmada', () => {
  const cap = capacidades({ platform: 'darwin' })
  assert.equal(mostraPerfilConta(cap, entrou), true)
  assert.equal(mostraPerfilConta(cap, { ...entrou, isSignedIn: false }), false)
  assert.equal(mostraPerfilConta(cap, { ...entrou, isExitUnresolved: true }), false)
  assert.equal(mostraPerfilConta(cap, { ...entrou, isSigningOut: true }), false)
})

test('a página Ícone do app só existe no Desktop', () => {
  assert.equal(paginaDisponivel('app-icon', capacidades(createWebBridge())), false)
  assert.equal(paginaDisponivel('app-icon', capacidades({ platform: 'win32' })), true)
  assert.equal(paginaDisponivel('general', capacidades(createWebBridge())), true)
})

test('Voz: a aba Chaves some quando não há cofre do computador nem Hub', () => {
  const abas = [{ id: 'ouvir' }, { id: 'vivo' }, { id: 'chaves' }]
  assert.deepEqual(abasVisiveis('voice', abas, capacidades(createWebBridge()), 0)?.map((a) => a.id), ['ouvir', 'vivo'])
  assert.equal(abasVisiveis('voice', abas, capacidades(createWebBridge()), 1), abas)
  assert.equal(abasVisiveis('voice', abas, capacidades({ platform: 'linux' }), 0), abas)
  assert.equal(abasVisiveis('usage', abas, capacidades(createWebBridge()), 0), abas)
})
