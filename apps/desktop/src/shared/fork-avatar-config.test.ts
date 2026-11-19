import assert from 'node:assert/strict'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import { AVATAR_HUB_REQUIRED, avatarHubNotice, botAccent, configForRole, configFromRole, confirmAvatar, DEFAULT_CONFIG, legacyAvatar, normalizedAvatar, randomAvatar, requireAvatarHub, roleSuggestion, ROLES, validAvatar } from './fork-avatar-config.ts'
import data from './fork-avatar-data.json' with { type: 'json' }
import type { Mood } from '../renderer/lib/donnabot-avatars/src/core/types.ts'

test('o desenho padrão e o sorteio inicial diferem da Donna, mantendo os 30 presets', () => {
  // Exercita o modelo real da biblioteca sem alterar seu src nem compilar o app.
  const bundle = buildSync({ entryPoints: [new URL('../renderer/lib/donnabot-avatars/src/core/model.ts', import.meta.url).pathname], bundle: true, platform: 'node', format: 'cjs', write: false })
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, { module })
  const { buildFace } = module.exports as typeof import('../renderer/lib/donnabot-avatars/src/core/model.ts')
  const donna = configFromRole(ROLES.find((r) => r.id === 'donna')!)
  for (const mood of ['idle', 'happy', 'think', 'sleep', 'listen'] as Mood[]) {
    assert.notEqual(JSON.stringify(buildFace(DEFAULT_CONFIG, mood)), JSON.stringify(buildFace(donna, mood)), mood)
  }
  assert.equal(ROLES.length, 30)
  for (let i = 0; i < 100; i++) assert.notDeepEqual(randomAvatar(), donna)
  for (const [color, swatch] of Object.entries(data.legacyColors)) {
    const cfg = legacyAvatar('hex', color)
    assert.equal(cfg.color, swatch.avatarHex)
    assert.equal(cfg.shape, 'hexa')
    assert.equal(validAvatar(cfg), true)
    assert.notDeepEqual(cfg, donna)
  }
  assert.deepEqual(legacyAvatar('constructor', 'toString'), DEFAULT_CONFIG)
})

test('Donna exige escolha explícita e persona só casa com o nome do bot', () => {
  for (const [name, description] of [
    ['Donna', ''],
    ['Assistente da Donna', ''],
    ['Zeca', 'Reporte à Donna um resumo diário.'],
    ['Assistente geral', 'Atue como assistente geral.'],
    ['Zeca', 'Atue como general assistant.'],
  ] as const) {
    assert.deepEqual(configForRole(name, description), DEFAULT_CONFIG)
    assert.deepEqual(configForRole(name, description, { avatarColor: 'green', avatarShape: 'cloud' }), legacyAvatar('cloud', 'green'))
  }
  for (const [id, description] of [
    ['psicologa', 'Mantenha a calma ao responder.'],
    ['suporte', 'Revise contratos e peça ajuda quando tiver dúvida.'],
    ['research', 'Sempre cite a fonte.'],
  ] as const) {
    const role = ROLES.find((r) => r.id === id)!
    assert.deepEqual(configForRole('Zeca', description), DEFAULT_CONFIG)
    assert.deepEqual(configForRole(role.name), configFromRole(role))
    assert.deepEqual(configForRole('Zeca', role.role), configFromRole(role))
  }
  const donna = configFromRole(ROLES.find((r) => r.id === 'donna')!)
  assert.deepEqual(normalizedAvatar(donna, 'Zeca'), donna, 'escolha explícita preservada')
})

test('função usa palavras inteiras em pt/en e ambiguidade preserva o legado', () => {
  for (const name of ['Pesquisadora', 'Researcher', 'Minha pesquisadora']) {
    assert.deepEqual(configForRole(name), configForRole('Fonte'))
  }
  assert.deepEqual(configForRole('Marketing', 'calendário'), configForRole('Hype'))
  assert.deepEqual(configForRole('Maria', 'Cuida do marketing: calendário de conteúdo e campanhas'), DEFAULT_CONFIG)
  assert.deepEqual(configForRole('XresearcherX', 'devagar'), DEFAULT_CONFIG)
  assert.deepEqual(configForRole('Jurídico', '', { avatarColor: 'green', avatarShape: 'cloud' }), legacyAvatar('cloud', 'green'))
  assert.deepEqual(configForRole('Maria', 'Responsável pelas atas'), configForRole('Ata'))
  assert.deepEqual(configForRole('Maria', 'Cuida de customer support'), configForRole('Ajuda'))
  const cfg = configForRole('Hype')
  assert.equal(botAccent({ avatarConfig: cfg }), cfg.color)
})

test('aguarda o hello antes de pedir atualização e exige suporte para salvar', () => {
  for (const hello of [null, undefined]) {
    assert.equal(avatarHubNotice(hello), 'Conectando ao Hub…')
    assert.throws(() => requireAvatarHub(hello), { message: 'Conectando ao Hub…' })
  }
  for (const hello of [{}, { avatarEditing: false }]) {
    assert.equal(avatarHubNotice(hello), AVATAR_HUB_REQUIRED)
    assert.throws(() => requireAvatarHub(hello), { message: AVATAR_HUB_REQUIRED })
  }
  assert.equal(avatarHubNotice({ avatarEditing: true }), null)
  assert.throws(() => requireAvatarHub({}), { message: AVATAR_HUB_REQUIRED })
  assert.doesNotThrow(() => requireAvatarHub({ avatarEditing: true }))
  assert.throws(() => confirmAvatar(undefined, DEFAULT_CONFIG), /Atualize o Hub/)
  assert.throws(() => confirmAvatar(configFromRole(ROLES.find((r) => r.id === 'donna')!), DEFAULT_CONFIG), /não confirmou/)
  assert.doesNotThrow(() => confirmAvatar({ ...DEFAULT_CONFIG }, DEFAULT_CONFIG))
})

test('função sugere nome somente quando vazio ou ainda igual à sugestão anterior', () => {
  assert.equal(roleSuggestion('', '', 'Fonte'), 'Fonte')
  assert.equal(roleSuggestion('Fonte', 'Fonte', 'Hype'), 'Hype')
  assert.equal(roleSuggestion('Meu bot', '', 'Hype'), 'Meu bot')
  assert.equal(roleSuggestion('Minha analista', 'Fonte', 'Hype'), 'Minha analista')
})
