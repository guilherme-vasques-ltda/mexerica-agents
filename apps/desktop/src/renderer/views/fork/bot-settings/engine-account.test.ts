import assert from 'node:assert/strict'
import test from 'node:test'
import type { BotEditor, BotEditorSave } from '../../../../shared/fork-bot-editor.ts'
import type { ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import { BotSettingsStore } from './bot-settings-store.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import { accountOptions, accountPatch, applyEnginePatch, cobrancaDaConta, modelOptionPatch, modelPatch, motorPatch, selectedAccount } from './engine-account.ts'

// Fatia 3 (D454): as mensagens do editor e das skills seguem o idioma do <html>; o texto conferido aqui é o pt-BR.
(globalThis as { document?: unknown }).document ??= { documentElement: { lang: 'pt-BR' } }


const id = (c: string) => `ac_${c.repeat(64)}`
const account = (patch: Partial<ManagedAccount>): ManagedAccount => ({
  id: id('a'), label: 'Pessoal', brand: 'OpenAI', backend: 'codex', billing: 'subscription', state: 'connected',
  models: [], revision: 1, origin: 'managed', botIds: [], ...patch,
})
const editor = (patch: Partial<BotEditor['profile']> = {}): BotEditor => ({
  botId: 'byte', settingsVersion: 'v1',
  profile: { name: 'Byte', description: '', avatarConfig: null, backend: 'codex', model: null, modelOptions: {}, fastMode: false, forkEditor: { engineAccountId: null }, ...patch },
  capabilities: { editableFields: ['name', 'description', 'avatarConfig', 'backend', 'model', 'modelOptions', 'fastMode', 'forkEditor'], unavailableSections: [] },
  permissions: { canEdit: true, canDelete: true }, costInfo: null, speedStatus: null,
  engineAccounts: [
    account({ id: id('l'), origin: 'legacy', label: 'Codex — perfil do Hub', state: 'unverified' }),
    account({ id: id('p'), label: 'Pessoal' }),
    account({ id: id('t'), label: 'Trabalho', state: 'disconnected' }),
    account({ id: id('c'), label: 'Claude', backend: 'claude', brand: 'Anthropic' }),
  ],
  engineRuntime: null, application: 'next_turn', workspaceIssue: null, message: '',
})

/** Editor real (BotSettingsStore) com transporte falso: prova o patch que chega ao Hub. */
async function open(remote: BotEditor) {
  const requests: BotEditorSave[] = []
  const model = new BotSettingsStore({
    read: async () => remote,
    save: async (request) => {
      requests.push(request)
      remote = { ...remote, settingsVersion: `v${requests.length + 1}`, profile: { ...remote.profile, ...request.patch } }
      return remote
    },
  })
  await model.open('byte')
  return { model, requests }
}

test('opções: padrão do Hub primeiro, só contas gerenciadas do mesmo motor, e a escolha salva nunca some', () => {
  const e = editor()
  assert.deepEqual(accountOptions(tradutor('pt-BR'), e, 'codex', null), [
    { id: '', label: 'Padrão do Hub · Não confirmada' },
    { id: id('p'), label: 'Pessoal · Conectada' },
    { id: id('t'), label: 'Trabalho · Desconectada' },
  ])
  assert.deepEqual(accountOptions(tradutor('pt-BR'), e, 'claude', null).map((o) => o.id), ['', id('c')])
  assert.deepEqual(accountOptions(tradutor('pt-BR'), e, 'gemini', null), [])
  assert.deepEqual(accountOptions(tradutor('pt-BR'), e, 'codex', id('x')).at(-1), { id: id('x'), label: 'Conta indisponível neste Hub' })
  // D70, R1 do #572 (item 6): em inglês, o estado da conta também sai no idioma (nada de "Hub default · Conectada").
  assert.deepEqual(accountOptions(tradutor('en'), e, 'codex', null).map((o) => o.label), ['Hub default · Not confirmed', 'Pessoal · Connected', 'Trabalho · Disconnected'])
  assert.equal(selectedAccount(e.profile), null)
  assert.equal(selectedAccount({ forkEditor: { engineAccountId: id('p') } }), id('p'))
})

test('trocar a conta salva só forkEditor com a versão lida; a mesma conta não salva', async () => {
  const { model, requests } = await open(editor())
  try {
    assert.equal(accountPatch(null, null), null)
    assert.ok(applyEnginePatch(model, model.getSnapshot().editor!, accountPatch(null, id('p'))!))
    await model.flush()
    assert.deepEqual(requests, [{ botId: 'byte', expectedVersion: 'v1', patch: { forkEditor: { engineAccountId: id('p') } } }])
    assert.equal(accountPatch(id('p'), id('p')), null)
    // `null` só pela escolha explícita do padrão do Hub.
    applyEnginePatch(model, model.getSnapshot().editor!, accountPatch(id('p'), null)!)
    await model.flush()
    assert.deepEqual(requests[1], { botId: 'byte', expectedVersion: 'v2', patch: { forkEditor: { engineAccountId: null } } })
  } finally { model.pause() }
})

test('trocar o motor de um bot com conta vai num save só; modelo manda só o que mudou', async () => {
  const { model, requests } = await open(editor({ forkEditor: { engineAccountId: id('p') }, model: 'gpt-6.1-sol', modelOptions: { effort: 'high' } }))
  try {
    applyEnginePatch(model, model.getSnapshot().editor!, motorPatch('claude'))
    await model.flush()
    assert.equal(requests.length, 1)
    assert.deepEqual(requests[0].patch, { backend: 'claude', model: null, modelOptions: {}, forkEditor: { engineAccountId: null } })
    applyEnginePatch(model, model.getSnapshot().editor!, modelPatch('claude-opus-5-5'))
    await model.flush()
    assert.deepEqual(requests[1].patch, { model: 'claude-opus-5-5' })
    applyEnginePatch(model, model.getSnapshot().editor!, modelOptionPatch({}, 'effort', 'max'))
    await model.flush()
    assert.deepEqual(requests[2].patch, { modelOptions: { effort: 'max' } })
    assert.deepEqual(modelOptionPatch({ effort: 'max', speed: 'fast' }, 'effort', ''), { modelOptions: { speed: 'fast' } })
  } finally { model.pause() }
})

test('Hub que não declara um campo editável recusa o patch inteiro, sem salvar parte dele', async () => {
  const remote = editor()
  remote.capabilities.editableFields = ['name', 'description', 'avatarConfig', 'forkEditor']
  const { model, requests } = await open(remote)
  try {
    assert.equal(applyEnginePatch(model, model.getSnapshot().editor!, motorPatch('claude')), false)
    await model.flush()
    assert.equal(requests.length, 0)
    assert.equal(model.getSnapshot().dirty, false)
  } finally { model.pause() }
})

test('motor só muda após save; erro preserva motor, conta e cadastro sem rascunho parcial', async () => {
  let remote = editor({ backend: 'claude', forkEditor: { engineAccountId: id('c') }, model: 'modelo-anterior' })
  let reply!: (saved: BotEditor) => void
  let fail!: (error: Error) => void
  const requests: BotEditorSave[] = []
  const model = new BotSettingsStore({ read: async () => remote, save: (request) => {
    requests.push(request)
    return new Promise((resolve, reject) => { reply = resolve; fail = reject })
  } })
  const settle = async () => { for (let i = 0; i < 6; i++) await new Promise((r) => setImmediate(r)) }
  await model.open('byte')
  try {
    const before = structuredClone(model.getSnapshot().draft)
    const changing = model.saveMotor('cursor')
    await settle()
    assert.deepEqual(model.getSnapshot().draft, before, 'pedido em voo não muda a seleção nem oculta a conta')
    fail(new Error('invalid_field: motor não reconhecido por este Hub.'))
    assert.equal(await changing, false)
    assert.deepEqual(model.getSnapshot().draft, before)
    assert.equal(model.getSnapshot().dirty, false)
    assert.equal(model.getSnapshot().editor?.engineAccounts, remote.engineAccounts)
    // R1 do #572: o estado guarda a chave; o texto sai na renderização.
    assert.equal(model.getSnapshot().error, 'bots.ajustes.erro.selecao')
    assert(!model.getSnapshot().error!.includes('nome'))
    const retry = model.saveMotor('cursor')
    await settle()
    assert.deepEqual(requests.at(-1), { botId: 'byte', expectedVersion: 'v1', patch: { backend: 'cursor', model: null, forkEditor: { engineAccountId: null } } })
    remote = { ...remote, settingsVersion: 'v2', profile: { ...remote.profile, ...requests.at(-1)!.patch } }
    reply(remote)
    assert.equal(await retry, true)
    assert.equal(model.getSnapshot().draft?.backend, 'cursor')
    assert.equal(selectedAccount(model.getSnapshot().draft), null)
    assert.equal(model.getSnapshot().draft?.model, null)
    assert.equal(model.getSnapshot().dirty, false)
    assert.equal(model.getSnapshot().error, null)
  } finally { model.pause() }
})


test('cobrança: API paga sem conexão continua Por chave; assinatura exige conexão', () => {
  assert.equal(cobrancaDaConta({ billing: 'paid_api', state: 'connected' }), 'paid_api')
  for (const state of ['disconnected', 'unavailable', 'unverified'] as const) assert.equal(cobrancaDaConta({ billing: 'paid_api', state }), 'porChave')
  assert.equal(cobrancaDaConta({ billing: 'subscription', state: 'disconnected' }), null)
  assert.equal(cobrancaDaConta({ billing: 'subscription', state: 'connected' }), 'subscription')
  assert.equal(cobrancaDaConta({ billing: 'unverified', state: 'connected' }), 'unverified')
})
