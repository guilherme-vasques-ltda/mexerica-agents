import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { BotEditor, BotEditorSave } from '../../../../shared/fork-bot-editor.ts'
import { DEFAULT_CONFIG, projetarAvatarLegado, type MexeriquinhaConfig } from '../../../../shared/fork-avatar-config.ts'
import { BotSettingsStore } from './bot-settings-store.ts'

;(globalThis as { document?: unknown }).document ??= { documentElement: { lang: 'pt-BR' } }

const v2: MexeriquinhaConfig = { v: 2, estilo: '3d', modeloId: 'pomarca', conjuntoId: 'pomarca' }

function montar(editableFields: BotEditor['capabilities']['editableFields']) {
  let remote: BotEditor = {
    botId: 'a', settingsVersion: 'v1', profile: { name: 'Ana', description: '', avatarConfig: { ...DEFAULT_CONFIG } },
    capabilities: { editableFields, unavailableSections: ['engine'] }, permissions: { canEdit: true, canDelete: true },
    costInfo: null, speedStatus: null, engineAccounts: [], engineRuntime: null, application: 'next_turn', workspaceIssue: null, message: '',
  }
  const pedidos: BotEditorSave[] = []
  const model = new BotSettingsStore({
    read: async () => remote,
    save: async (pedido) => {
      pedidos.push(pedido)
      const profile = { ...remote.profile, ...pedido.patch }
      // Hub v2: patch só com o legado limpa a mexeriquinha.
      if (!('mexeriquinha' in pedido.patch) && 'avatarConfig' in pedido.patch) delete profile.mexeriquinha
      return remote = { ...remote, settingsVersion: `v${pedidos.length + 1}`, profile }
    },
  })
  return { model, pedidos, perfil: () => remote.profile }
}

test('Hub v2: trocar o avatar manda os dois campos num patch só, coerentes, com a versão esperada', async () => {
  const { model, pedidos, perfil } = montar(['name', 'description', 'avatarConfig', 'mexeriquinha'])
  await model.open('a')
  model.changeAvatar(v2)
  await model.flush()
  assert.equal(pedidos.length, 1)
  assert.equal(pedidos[0]!.expectedVersion, 'v1')
  assert.deepEqual(Object.keys(pedidos[0]!.patch).sort(), ['avatarConfig', 'mexeriquinha'])
  assert.deepEqual(pedidos[0]!.patch.mexeriquinha, v2)
  assert.deepEqual(pedidos[0]!.patch.avatarConfig, projetarAvatarLegado(v2))
  assert.equal(model.getSnapshot().dirty, false)
  // Mesma aparência com as chaves em outra ordem não suja o rascunho.
  model.changeAvatar({ conjuntoId: 'pomarca', modeloId: 'pomarca', estilo: '3d', v: 2 })
  assert.equal(model.getSnapshot().dirty, false)
  // Escolha legada depois: só avatarConfig no patch, e a v2 sai do perfil e do rascunho.
  const legado = { ...DEFAULT_CONFIG, color: '#123456' }
  model.changeAvatar(legado)
  // O save anterior ainda estava terminando: esta troca vai no save seguinte, encadeado.
  await new Promise((r) => setTimeout(r, 0))
  await model.flush()
  assert.deepEqual(Object.keys(pedidos[1]!.patch), ['avatarConfig'])
  assert.equal(perfil().mexeriquinha, undefined)
  assert.equal(model.getSnapshot().draft!.mexeriquinha, undefined)
  assert.equal(model.getSnapshot().dirty, false)
})

test('Hub antigo (sem o campo mexeriquinha): v2 não é enviada; o legado continua salvando', async () => {
  const { model, pedidos } = montar(['name', 'description', 'avatarConfig'])
  await model.open('a')
  model.changeAvatar(v2)
  await model.flush()
  assert.equal(pedidos.length, 0)
  assert.equal(model.getSnapshot().dirty, false)
  model.changeAvatar({ ...DEFAULT_CONFIG, color: '#123456' })
  await model.flush()
  assert.equal(pedidos.length, 1)
  assert.deepEqual(Object.keys(pedidos[0]!.patch), ['avatarConfig'])
})
