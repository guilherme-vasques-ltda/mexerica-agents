import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  VinculoAmbienteError,
  decodeVinculoAmbiente,
  isOpaqueBindingId,
  type VinculoAmbienteContext,
} from './fork-vinculo-ambiente.ts'

const CTX: VinculoAmbienteContext = {
  hubId: 'hub_01abc',
  spaceId: 'space_01def',
  botId: 'bot_01ghi',
}

function validConfigured(overrides: Record<string, unknown> = {}) {
  return {
    schema: 1,
    hubId: CTX.hubId,
    spaceId: CTX.spaceId,
    botId: CTX.botId,
    revision: 3,
    state: 'configured',
    execution: 'unavailable',
    project: { id: 'proj_01', label: 'Mexerica Core', gitLinked: true },
    machine: { kind: 'dock', id: 'mach_01', label: 'VPS Principal' },
    model: { id: 'gpt-6.2-sol', label: 'GPT-6.2 Sol' },
    access: { id: 'acc_01', label: 'Acesso leitura e código' },
    ...overrides,
  }
}

function validUnconfigured(overrides: Record<string, unknown> = {}) {
  return {
    schema: 1,
    hubId: CTX.hubId,
    spaceId: CTX.spaceId,
    botId: CTX.botId,
    revision: 0,
    state: 'unconfigured',
    execution: 'unavailable',
    ...overrides,
  }
}

test('decodifica projeção unconfigured válida com execution unavailable', () => {
  const res = decodeVinculoAmbiente(validUnconfigured(), CTX)
  assert.deepEqual(res, {
    schema: 1,
    hubId: CTX.hubId,
    spaceId: CTX.spaceId,
    botId: CTX.botId,
    revision: 0,
    state: 'unconfigured',
    execution: 'unavailable',
  })
})

test('decodifica projeção configured válida (local e dock) sem campos extras', () => {
  const dock = decodeVinculoAmbiente(validConfigured(), CTX)
  assert.equal(dock.state, 'configured')
  assert.equal(dock.execution, 'unavailable')
  assert.deepEqual(dock.machine, { kind: 'dock', id: 'mach_01', label: 'VPS Principal' })
  assert.deepEqual(dock.project, { id: 'proj_01', label: 'Mexerica Core', gitLinked: true })
  assert.deepEqual(dock.model, { id: 'gpt-6.2-sol', label: 'GPT-6.2 Sol' })
  assert.deepEqual(dock.access, { id: 'acc_01', label: 'Acesso leitura e código' })

  const local = decodeVinculoAmbiente(
    validConfigured({ machine: { kind: 'local', id: 'mach_local', label: 'Este computador' } }),
    CTX,
  )
  assert.equal(local.state, 'configured')
  assert.equal(local.machine.kind, 'local')
})

test('unconfigured rejeita qualquer um dos quatro objetos (project, machine, model, access)', () => {
  for (const key of ['project', 'machine', 'model', 'access']) {
    assert.throws(
      () => decodeVinculoAmbiente(validUnconfigured({ [key]: null }), CTX),
      VinculoAmbienteError,
      `unconfigured não pode aceitar ${key}`,
    )
  }
})

test('configured exige exatamente os quatro objetos obrigatórios', () => {
  for (const key of ['project', 'machine', 'model', 'access']) {
    const payload = validConfigured() as Record<string, unknown>
    delete payload[key]
    assert.throws(() => decodeVinculoAmbiente(payload, CTX), VinculoAmbienteError, `configured exige ${key}`)
  }
})

test('rejeita execution diferente de unavailable em ambos os estados', () => {
  for (const bad of ['available', 'ready', 'local', 'dock', '', null, true, 1]) {
    assert.throws(
      () => decodeVinculoAmbiente(validUnconfigured({ execution: bad }), CTX),
      VinculoAmbienteError,
    )
    assert.throws(
      () => decodeVinculoAmbiente(validConfigured({ execution: bad }), CTX),
      VinculoAmbienteError,
    )
  }
})

test('rejeita schema inválido ou revision fora de inteiro não negativo seguro', () => {
  for (const badSchema of [0, 2, '1', null, undefined]) {
    assert.throws(
      () => decodeVinculoAmbiente(validUnconfigured({ schema: badSchema }), CTX),
      VinculoAmbienteError,
    )
  }
  for (const badRev of [-1, 1.5, Number.NaN, Number.POSITIVE_INFINITY, Number.MAX_SAFE_INTEGER + 1, '1']) {
    assert.throws(
      () => decodeVinculoAmbiente(validUnconfigured({ revision: badRev }), CTX),
      VinculoAmbienteError,
    )
  }
})

test('rejeita dados de outro hub, space ou bot e contexto esperado inválido', () => {
  assert.throws(
    () => decodeVinculoAmbiente(validUnconfigured({ hubId: 'hub_outro' }), CTX),
    /cross_hub_response/,
  )
  assert.throws(
    () => decodeVinculoAmbiente(validUnconfigured({ spaceId: 'space_outro' }), CTX),
    /cross_space_response/,
  )
  assert.throws(
    () => decodeVinculoAmbiente(validUnconfigured({ botId: 'bot_outro' }), CTX),
    /cross_bot_response/,
  )
  assert.throws(
    () => decodeVinculoAmbiente(validUnconfigured(), { ...CTX, hubId: '/etc/passwd' }),
    VinculoAmbienteError,
  )
  assert.throws(
    () => decodeVinculoAmbiente(validUnconfigured(), null as unknown as VinculoAmbienteContext),
    VinculoAmbienteError,
  )
  assert.throws(
    () => decodeVinculoAmbiente(validUnconfigured(), undefined as unknown as VinculoAmbienteContext),
    VinculoAmbienteError,
  )
})

test('rejeita chaves inesperadas na raiz e nos objetos internos sem vazar campos', () => {
  assert.throws(
    () => decodeVinculoAmbiente(validUnconfigured({ grantToken: 'abc', extra: true }), CTX),
    VinculoAmbienteError,
  )
  assert.throws(
    () =>
      decodeVinculoAmbiente(
        validConfigured({
          project: { id: 'proj_01', label: 'Mexerica', gitLinked: false, localPath: '/srv/repo' },
        }),
        CTX,
      ),
    VinculoAmbienteError,
  )
  assert.throws(
    () =>
      decodeVinculoAmbiente(
        validConfigured({
          machine: { kind: 'dock', id: 'mach_01', label: 'VPS', host: '192.0.2.1' },
        }),
        CTX,
      ),
    VinculoAmbienteError,
  )
  assert.throws(
    () =>
      decodeVinculoAmbiente(
        validConfigured({
          access: { id: 'acc_01', label: 'Leitura', secret: 'nope' },
        }),
        CTX,
      ),
    VinculoAmbienteError,
  )
})

test('rejeita IDs que pareçam path, URL, chave ou segredo', () => {
  const badIds = [
    '',
    ' ',
    '/Users/you/repo',
    '../segredo',
    'C:' + '\\Users\\<usuario>\\repo',
    '~/projeto',
    'https://example.invalid/org/repo',
    'ssh://example.invalid/repo',
    'id?token=1',
    'id#frag',
    'id com espaco',
    'id\nquebra',
    'sk-' + 'proj-1234567890abcdef',
    'sk-' + 'ant-api03-123456',
    'ghp_' + '1234567890abcdef',
    'github_pat_' + '123456',
    'glpat-' + '1234567890abcdef',
    'xapp-' + '1-A0123-456',
    'hf_' + '1234567890abcdef',
    'dop_v1_' + '1234567890abcdef',
    'npm_' + '1234567890abcdef',
    'shpat_' + '1234567890abcdef',
    'eyJ' + 'hbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig',
    'Bearer abcdef',
    'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5',
    'a'.repeat(129),
  ]
  for (const bad of badIds) {
    assert.equal(isOpaqueBindingId(bad), false, `deveria rejeitar ID: ${bad}`)
    assert.throws(
      () =>
        decodeVinculoAmbiente(
          validConfigured({ project: { id: bad, label: 'Projeto', gitLinked: true } }),
          CTX,
        ),
      VinculoAmbienteError,
    )
  }
})

test('rótulos aceitam texto livre de exibição e rejeitam caracteres de controle ou excesso', () => {
  const ok = decodeVinculoAmbiente(
    validConfigured({
      access: { id: 'acc_01', label: 'São Paulo — Produção (leitura)' },
    }),
    CTX,
  )
  assert.equal(ok.state, 'configured')
  assert.equal(ok.access.label, 'São Paulo — Produção (leitura)')

  for (const badLabel of [
    '',
    '   ',
    'linha1\nlinha2',
    'tab\taqui',
    'nulo\x00fim',
    'c1\u0080fim',
    'c1\u0085fim',
    'c1\u009ffim',
    'sep\u2028fim',
    'bidi\u202efim',
    'lri\u2066fim',
    'shy\u00adfim',
    'zwsp\u200bfim',
    'x'.repeat(121),
  ]) {
    assert.throws(
      () =>
        decodeVinculoAmbiente(
          validConfigured({ model: { id: 'mod_01', label: badLabel } }),
          CTX,
        ),
      VinculoAmbienteError,
    )
  }
})

test('rejeita machine.kind inválido e gitLinked não booleano', () => {
  assert.throws(
    () =>
      decodeVinculoAmbiente(
        validConfigured({ machine: { kind: 'cloud', id: 'mach_01', label: 'Cloud' } }),
        CTX,
      ),
    VinculoAmbienteError,
  )
  assert.throws(
    () =>
      decodeVinculoAmbiente(
        validConfigured({ project: { id: 'proj_01', label: 'Proj', gitLinked: 'true' } }),
        CTX,
      ),
    VinculoAmbienteError,
  )
})

test('captura revision, gitLinked e kind em leitura única mesmo com getter mutável', () => {
  let revCalls = 0
  let gitCalls = 0
  let kindCalls = 0
  const payload = validConfigured()
  Object.defineProperty(payload, 'revision', {
    enumerable: true,
    get() {
      revCalls += 1
      return revCalls === 1 ? 4 : -99
    },
  })
  Object.defineProperty(payload.project, 'gitLinked', {
    enumerable: true,
    get() {
      gitCalls += 1
      return gitCalls === 1 ? true : ('mutado' as unknown as boolean)
    },
  })
  Object.defineProperty(payload.machine, 'kind', {
    enumerable: true,
    get() {
      kindCalls += 1
      return kindCalls === 1 ? 'dock' : ('cloud' as unknown as 'dock')
    },
  })

  const decoded = decodeVinculoAmbiente(payload, CTX)
  assert.equal(decoded.revision, 4)
  assert.equal(decoded.state, 'configured')
  assert.equal(decoded.project.gitLinked, true)
  assert.equal(decoded.machine.kind, 'dock')
})
