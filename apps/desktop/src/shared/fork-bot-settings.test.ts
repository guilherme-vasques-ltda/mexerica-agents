import assert from 'node:assert/strict'
import test from 'node:test'
import type { BotEditor, BotEditorSave } from './fork-bot-editor.ts'
import { normalizeBot, draftOf, type Bot } from './models.ts'
import type { BotStore } from '../renderer/store/bot-store.ts'
import { captureSkillChoice, engineSettingsIntent, saveBotSettingsIntent } from '../renderer/store/fork-bot-settings-intents.ts'
import { botEditorUpdate } from './bot-autonomy.ts'
import { BotSettingsStore, botSettingsSignature } from '../renderer/views/fork/bot-settings/bot-settings-store.ts'
import { tradutor } from '../renderer/lib/fork-i18n/index.ts'
import { textoDaMensagem } from '../renderer/views/fork/bot-settings/mensagem.ts'

// Fatia 3 (D454): as mensagens do editor e das skills seguem o idioma do <html>; o texto conferido aqui é o pt-BR.
(globalThis as { document?: unknown }).document ??= { documentElement: { lang: 'pt-BR' } }


const editor = (botId: string, version = 'v1'): BotEditor => ({
  botId, settingsVersion: version, profile: { name: botId, description: '', avatarConfig: null },
  capabilities: { editableFields: ['name', 'description', 'avatarConfig'], unavailableSections: ['engine'] },
  permissions: { canEdit: true, canDelete: true }, costInfo: null, speedStatus: null, engineAccounts: [], engineRuntime: null,
  application: 'next_turn', workspaceIssue: null, message: 'Aplica no próximo turno.',
})
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: Error) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

test('Hub que apara texto não remove espaço/Enter do rascunho nem reenvia só whitespace', async () => {
  let remote = editor('a')
  const requests: BotEditorSave[] = []
  const model = new BotSettingsStore({
    read: async () => remote,
    save: async (request) => {
      requests.push(request)
      const profile = { ...remote.profile, ...request.patch }
      profile.name = profile.name.trim()
      profile.description = profile.description.trim()
      return remote = { ...remote, settingsVersion: `v${requests.length + 1}`, profile }
    },
  })
  try {
    await model.open('a')
    model.change('description', 'Regra 1.\n')
    assert.equal(await model.flush(), true)
    assert.equal(model.getSnapshot().draft?.description, 'Regra 1.\n')
    assert.equal(model.getSnapshot().dirty, false)
    await model.refresh() // eco no mirror também não deve aparar o textarea
    assert.equal(model.getSnapshot().draft?.description, 'Regra 1.\n')
    model.change('description', model.getSnapshot().draft!.description + 'Regra 2.')
    await model.flush()
    assert.equal(remote.profile.description, 'Regra 1.\nRegra 2.')
    model.change('name', 'Ana ')
    await model.flush()
    await model.refresh()
    assert.equal(model.getSnapshot().draft?.name, 'Ana ')
    model.change('name', model.getSnapshot().draft!.name + 'Clara')
    await model.flush()
    assert.equal(remote.profile.name, 'Ana Clara')
    model.change('name', 'Ana Clara  ')
    model.change('description', remote.profile.description + '\n')
    assert.equal(model.getSnapshot().dirty, false)
    await model.flush()
    assert.equal(requests.length, 4, 'nenhum patch só por whitespace')
  } finally { model.pause() }
})

test('comando próprio só envia intenção preenchida e aguarda o evento bot para mudar o mirror', async () => {
  assert.equal(engineSettingsIntent('custom'), null, 'selecionar só abre o campo local')
  assert.equal(engineSettingsIntent('custom', '  \n'), null, 'comando vazio não chega ao Hub')
  const patch = engineSettingsIntent('custom', 'meu-agente --acp', 'codex')!
  assert.deepEqual(patch, { backend: 'custom', command: 'meu-agente --acp', model: null, modelOptions: {} })
  assert.deepEqual(engineSettingsIntent('custom', 'outro --acp', 'custom'), { backend: 'custom', command: 'outro --acp' }, 'editar comando preserva modelo/opções')
  assert.deepEqual(engineSettingsIntent('codex', null, 'claude'), { backend: 'codex', command: null, model: null, modelOptions: {} })
  const mirror = { backend: 'codex' }
  const bots = new Map([['a', mirror]])
  const calls: unknown[] = []
  const store = { bots, ready: async () => ({ call: async (method: string, body: unknown) => {
    calls.push({ method, body })
    return { bot: { backend: 'custom' } }
  } }) } as unknown as BotStore
  await saveBotSettingsIntent(store, 'a', patch)
  assert.deepEqual(calls, [{ method: 'updateBot', body: { id: 'a', ...patch } }])
  assert.equal(bots.get('a'), mirror, 'resposta do updateBot não escreve no mirror')
})

test('voltar de comando próprio ao motor salvo não envia patch de configuração', () => {
  const savedBackend = 'codex'
  assert.equal(engineSettingsIntent('custom', null, savedBackend), null, 'abrir comando próprio é local')
  assert.equal(engineSettingsIntent(savedBackend, null, savedBackend), null, 'voltar preserva modelo, opções e comando salvos')
})

test('reescolher o motor atual não envia patch de configuração', () => {
  for (const savedBackend of ['codex', 'claude']) {
    assert.equal(engineSettingsIntent(savedBackend, null, savedBackend), null, 'não reinicia a sessão nem limpa a configuração atual')
  }
})

test('só configuração e principal invalidam a leitura; status/atividade não relêem o editor', async () => {
  let remote = editor('a')
  let reads = 0
  const model = new BotSettingsStore({
    read: async () => { reads++; return remote },
    save: async (request) => {
      assert.equal(request.expectedVersion, remote.settingsVersion)
      return remote = { ...remote, profile: { ...remote.profile, ...request.patch } }
    },
  })
  try {
    await model.open('a')
    let bot: Partial<Bot> = { pinned: false, position: 0, hidden: false, connectors: [], logins: [], fastMode: false }
    let signature = botSettingsSignature(bot, null)
    bot = { ...bot, status: 'working', activity: 'Executando ferramenta', unread: 1, lastMessage: 'Progresso', lastAt: 42,
      rev: 2, startedAt: 40, workingChat: 'grupo', resumeAt: 43, memoryRev: 3, fastModeSupport: 'offered', fastModeIssue: 'Aviso' }
    const progress = botSettingsSignature(bot, null)
    if (progress !== signature) await model.refresh()
    assert.equal(progress, signature)
    assert.equal(reads, 1, 'evento só de progresso não dispara releitura')
    for (const patch of [{ pinned: true }, { position: 1 }, { hidden: true }, { browserPreference: 'computer' as const }]) {
      bot = { ...bot, ...patch }
      const next = botSettingsSignature(bot, null)
      assert.notEqual(next, signature)
      signature = next
      remote = { ...remote, settingsVersion: next, profile: { ...remote.profile, browserPreference: bot.browserPreference } }
      await model.refresh()
      assert.equal(model.getSnapshot().conflict, false)
      assert.equal(model.getSnapshot().editor?.settingsVersion, next)
      assert.equal(model.getSnapshot().draft?.browserPreference, bot.browserPreference)
    }
    const next = botSettingsSignature(bot, 'outra-principal')
    assert.notEqual(next, signature)
    remote = { ...remote, settingsVersion: next }
    await model.refresh()
    assert.equal(reads, 6, 'fixar, reordenar, ocultar, preferência e principal disparam releitura')
    model.change('description', 'Depois de fixar, reordenar, ocultar e trocar a principal')
    assert.equal(await model.flush(), true)
    assert.equal(model.getSnapshot().conflict, false)
  } finally { model.pause() }
})

test('corrigir invalid_field reativa o debounce sem retry manual', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  let remote = editor('a')
  let requests = 0
  const model = new BotSettingsStore({
    read: async () => remote,
    save: async (request) => {
      requests++
      if (!request.patch.name?.trim()) throw new Error('invalid_field: nome vazio')
      return remote = { ...remote, profile: { ...remote.profile, ...request.patch } }
    },
  })
  try {
    await model.open('a')
    model.change('name', '')
    assert.equal(await model.flush(), false)
    model.change('name', 'Nome válido')
    assert.equal(model.getSnapshot().error, null)
    t.mock.timers.tick(600)
    await new Promise<void>((resolve) => setImmediate(resolve))
    assert.equal(requests, 2)
    assert.equal(remote.profile.name, 'Nome válido')
    assert.equal(model.getSnapshot().dirty, false)
  } finally { model.pause() }
})

test('queda durante save confirmado não deixa a próxima edição pausada', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  let remote = editor('a')
  const response = deferred<BotEditor>()
  let requests = 0
  const model = new BotSettingsStore({
    read: async () => remote,
    save: async (request) => ++requests === 1 ? response.promise : remote = { ...remote, profile: { ...remote.profile, ...request.patch } },
  })
  try {
    await model.open('a')
    model.change('name', 'Confirmado')
    const saving = model.flush()
    model.setOnline(false)
    remote = { ...remote, settingsVersion: 'v2', profile: { ...remote.profile, name: 'Confirmado' } }
    response.resolve(remote)
    assert.equal(await saving, true)
    assert.equal(model.getSnapshot().paused, false)
    model.setOnline(true)
    await model.refresh()
    model.change('description', 'Edição seguinte')
    t.mock.timers.tick(600)
    await new Promise<void>((resolve) => setImmediate(resolve))
    assert.equal(requests, 2)
    assert.equal(remote.profile.description, 'Edição seguinte')
  } finally { model.pause() }
})

test('resposta perdida aceita confirmação do próprio save e preserva digitação posterior', async () => {
  let remote = editor('a')
  const requests: BotEditorSave[] = []
  const model = new BotSettingsStore({
    read: async () => remote,
    save: async (request) => {
      requests.push(request)
      remote = { ...remote, settingsVersion: `v${requests.length + 1}`, profile: { ...remote.profile, ...request.patch, description: request.patch.description?.trim() ?? remote.profile.description } }
      if (requests.length === 1) throw new Error('Request timed out')
      return remote
    },
  })
  try {
    await model.open('a')
    model.change('description', 'Texto enviado\n')
    assert.equal(await model.flush(), false)
    model.change('name', 'Digitado depois')
    await model.refresh()
    assert.equal(model.getSnapshot().conflict, false)
    assert.equal(model.getSnapshot().draft?.name, 'Digitado depois')
    assert.equal(model.getSnapshot().draft?.description, 'Texto enviado\n')
    assert.equal(model.getSnapshot().editor?.settingsVersion, 'v2')
    assert.equal(await model.retry(), true)
    assert.deepEqual(requests[1], { botId: 'a', expectedVersion: 'v2', patch: { name: 'Digitado depois' } })
  } finally { model.pause() }
})

test('falha de leitura informa carregamento e retry recupera o editor', async () => {
  let fail = true
  const model = new BotSettingsStore({ read: async () => { if (fail) throw new Error('timeout'); return editor('a') }, save: async () => editor('a') })
  await model.open('a')
  assert.equal(model.getSnapshot().error, 'bots.ajustes.erro.carregar')
  assert.equal(textoDaMensagem(tradutor('pt-BR'), { chave: model.getSnapshot().error! }), 'Não foi possível carregar as configurações. Tentar novamente.')
  fail = false
  assert.equal(await model.retry(), true)
  assert.equal(model.getSnapshot().error, null)
  assert.ok(model.getSnapshot().draft)
})

test('identidade serializa patches, preserva digitação, isola bots e exige resolução de conflito', async () => {
  const requests: BotEditorSave[] = []
  const responses: ReturnType<typeof deferred<BotEditor>>[] = []
  let remote = editor('a')
  const model = new BotSettingsStore({
    read: async (id) => id === remote.botId ? remote : editor(id),
    save: (request) => {
      requests.push(request)
      const response = deferred<BotEditor>()
      responses.push(response)
      return response.promise
    },
  })
  try {
    await model.open('a')
    model.change('name', 'Primeiro')
    const first = model.flush()
    model.change('description', 'Texto digitado durante a resposta')
    model.change('name', 'Último nome')
    assert.equal(requests.length, 1, 'só um save em voo')
    assert.deepEqual(requests[0], { botId: 'a', expectedVersion: 'v1', patch: { name: 'Primeiro' } })
    const saved = { ...editor('a', 'v2'), profile: { ...editor('a').profile, name: 'Primeiro' } }
    responses[0].resolve(saved)
    await new Promise<void>((resolve) => setImmediate(resolve))
    assert.equal(model.getSnapshot().draft?.name, 'Último nome')
    assert.equal(model.getSnapshot().draft?.description, 'Texto digitado durante a resposta')
    assert.deepEqual(requests[1], { botId: 'a', expectedVersion: 'v2', patch: { name: 'Último nome', description: 'Texto digitado durante a resposta' } })

    await model.open('b')
    responses[1].resolve({ ...saved, settingsVersion: 'v3', profile: { ...saved.profile, name: 'Último nome' } })
    assert.equal(await first, false, 'resposta do bot anterior não confirma o atual')
    assert.equal(model.getSnapshot().botId, 'b')
    assert.equal(model.getSnapshot().draft?.name, 'b')
    assert.equal(model.getSnapshot().dirty, false)

    model.change('description', 'Preservar este rascunho')
    const conflict = model.flush()
    responses[2].reject(new Error('settings_conflict: alterado por outra janela'))
    assert.equal(await conflict, false)
    assert.equal(model.getSnapshot().conflict, true)
    assert.equal(model.getSnapshot().error, 'bots.ajustes.erro.conflito')
    assert.equal(model.getSnapshot().draft?.description, 'Preservar este rascunho')
    await model.retry()
    assert.equal(requests.length, 3, 'não sobrescreve remoto por retry de conflito')
    remote = editor('b', 'externa')
    await model.discard()
    assert.equal(model.getSnapshot().editor?.settingsVersion, 'externa')
    assert.equal(model.getSnapshot().conflict, false)

    model.change('name', 'Pendente offline')
    model.setOnline(false)
    model.change('name', 'Não deve editar offline')
    assert.equal(model.getSnapshot().draft?.name, 'Pendente offline')
    model.setOnline(true)
    await model.refresh()
    assert.equal(await model.flush(), false, 'reconexão não reenvia intenção sozinha')
    assert.equal(requests.length, 3)
    const retry = model.retry()
    await new Promise<void>((resolve) => setImmediate(resolve))
    responses[3].resolve({ ...remote, settingsVersion: 'v4', profile: { ...remote.profile, name: 'Pendente offline' }, application: 'apply_pending' })
    assert.equal(await retry, true)
    assert.equal(model.getSnapshot().dirty, false)
    assert.equal(model.getSnapshot().editor?.application, 'apply_pending')
  } finally { model.pause() }
})

test('conta igual não salva; troca envia somente forkEditor e opções com ordem diferente são iguais', async () => {
  let remote = editor('a')
  remote.profile = { ...remote.profile, backend: 'codex', model: 'gpt-6.1-sol', modelOptions: { effort: 'high', speed: 'normal' }, fastMode: false, forkEditor: { engineAccountId: 'ac_ficticia1' } }
  remote.capabilities.editableFields = ['name', 'description', 'avatarConfig', 'backend', 'model', 'modelOptions', 'fastMode', 'forkEditor']
  const requests: BotEditorSave[] = []
  const model = new BotSettingsStore({
    read: async () => remote,
    save: async (request) => {
      requests.push(request)
      return remote = { ...remote, settingsVersion: 'v2', profile: { ...remote.profile, ...request.patch } }
    },
  })
  try {
    await model.open('a')
    model.change('forkEditor', { engineAccountId: 'ac_ficticia1' })
    model.change('modelOptions', { speed: 'normal', effort: 'high' })
    assert.equal(model.getSnapshot().dirty, false)
    await model.flush()
    assert.equal(requests.length, 0)
    model.change('forkEditor', { engineAccountId: 'ac_ficticia2' })
    assert.equal(await model.flush(), true)
    assert.deepEqual(requests[0].patch, { forkEditor: { engineAccountId: 'ac_ficticia2' } })
    assert.notEqual(botSettingsSignature({ ...({} as Bot), forkEditor: { engineAccountId: 'ac_ficticia1' } }, null), botSettingsSignature({ ...({} as Bot), forkEditor: { engineAccountId: 'ac_ficticia2' } }, null))
  } finally { model.pause() }
})

test('mirror e rascunho preservam engineAccountId selecionado sem outro estado de conta', () => {
  const bot = normalizeBot({ id: 'fixture', forkEditor: { engineAccountId: 'ac_fixture' } })
  assert.deepEqual(bot.forkEditor, { engineAccountId: 'ac_fixture' })
  assert.deepEqual(draftOf(bot).forkEditor, bot.forkEditor)
  assert.deepEqual(normalizeBot({ id: 'legacy' }).forkEditor, { engineAccountId: null })
})

test('c1: escolha de skill presa ao gesto, sem lista inteira no perfil nem envio após troca de origem', async () => {
  const version = 'a'.repeat(32), installationId = '12345678-1234-1234-1234-123456789abc:7'
  const bot = normalizeBot({ id: 'a', skills: ['base'], skillSelection: { protocol: 1, version } })
  assert.equal(Object.hasOwn(botEditorUpdate(draftOf(bot)), 'skills'), false)
  const calls: unknown[] = []
  const client = { call: async (method: string, body?: unknown) => { calls.push({ method, body }); return { bot: { id: 'a', skills: ['base', 's'], skillSelection: { protocol: 1, version: 'b'.repeat(32) } } } } }
  const store = { hello: { hostId: 'hub' }, connectedClient: client, principalEpoch: 1, spaceId: undefined, ready: async () => client }
  const choice = captureSkillChoice(store as unknown as BotStore, bot, { id: 's', installationId, contentStatus: 'verified' }, true)
  assert.ok('send' in choice)
  // O mirror muda durante o flush/await: o pedido conserva a condição do gesto.
  bot.skillSelection = { protocol: 1, version: 'c'.repeat(32) }
  await choice.send()
  assert.deepEqual(calls, [{ method: 'updateBot', body: { hubId: 'hub', id: 'a', skillSelection: { protocol: 1, expectedVersion: version, skillId: 's', enabled: true, expectedInstallationId: installationId } } }])
  const other = captureSkillChoice(store as unknown as BotStore, bot, { id: 's', installationId, contentStatus: 'verified' }, false)
  assert.ok('send' in other)
  store.principalEpoch++
  await assert.rejects(other.send(), /A conexão com o Hub mudou/)
  assert.equal(calls.length, 1)
  const old = captureSkillChoice(store as unknown as BotStore, { id: 'a', skillSelection: null }, { id: 's', installationId, contentStatus: 'verified' }, true)
  assert.ok('refused' in old && 'chave' in old.refused && old.refused.chave === 'bots.ajustes.skill.semProtecao')
  assert.match(textoDaMensagem(tradutor('pt-BR'), old.refused), /Atualize o Hub/)
  assert.match(textoDaMensagem(tradutor('en'), old.refused), /Update the Hub/)
})

// D70, R1 do #572 (itens 1 e 2): o conflito vindo de uma releitura guarda a CHAVE (nunca o texto cru da chave na
// tela) e o texto sai no idioma da renderização: com o erro à vista, trocar o idioma troca o texto.
test('conflito na releitura guarda a chave e o texto segue o idioma da renderização', async () => {
  let remote = editor('a', 'v1')
  const model = new BotSettingsStore({ read: async () => remote, save: async () => remote })
  await model.open('a')
  model.change('description', 'rascunho local')
  remote = editor('a', 'v2')
  await model.refresh()
  const erro = model.getSnapshot().error
  assert.equal(model.getSnapshot().conflict, true)
  assert.equal(erro, 'bots.ajustes.erro.conflito')
  assert.equal(textoDaMensagem(tradutor('pt-BR'), { chave: erro! }), 'Este bot mudou em outra janela.')
  assert.equal(textoDaMensagem(tradutor('en'), { chave: erro! }), 'This bot changed in another window.')
  assert.equal(textoDaMensagem(tradutor('ar-AE'), { chave: erro! }), 'تغيّر هذا البوت في نافذة أخرى.')
})
