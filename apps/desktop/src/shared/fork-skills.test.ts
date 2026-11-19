import test from 'node:test'
import assert from 'node:assert/strict'
import { passiveSkills, SkillRequestScope, type SkillClient } from './fork-skills.ts'

test('envia identidade e digest pelo RPC existente, sem origem remota livre', async () => {
  const calls: unknown[] = []
  const client: SkillClient = { async call<T>(method: string, body?: unknown) {
    calls.push([method, body]); return { items: [] } as T
  } }
  await passiveSkills.catalog(client)
  await passiveSkills.install(client, { catalogId: 'find-skills', expectedDigest: 'digest' })
  await passiveSkills.remove(client, { id: 'find-skills-donna', expectedDigest: 'digest' })
  assert.deepEqual(calls, [
    ['marketSkills', undefined],
    ['installSkill', { catalogId: 'find-skills', expectedDigest: 'digest' }],
    ['removeSkill', { id: 'find-skills-donna', expectedDigest: 'digest' }],
  ])
})

test('resposta e erro atrasados de outro Hub ou antes da remoção são descartados', async () => {
  const scope = new SkillRequestScope()
  let resolve!: (value: number) => void
  const pending = scope.run(() => new Promise<number>((r) => { resolve = r }))
  scope.invalidate()
  resolve(1)
  assert.deepEqual(await pending, { current: false })
  let reject!: (e: Error) => void
  const failed = scope.run(() => new Promise<number>((_, r) => { reject = r }))
  scope.invalidate()
  reject(new Error('Hub anterior'))
  assert.deepEqual(await failed, { current: false })
  assert.deepEqual(await scope.run(async () => 2), { current: true, value: 2 })
  await assert.rejects(scope.run(async () => { throw new Error('atual') }), /atual/)
})

test('R6 captura encarnação e ausência explícita, recusa Hub antigo sem fallback', async () => {
  const { passiveSkillsV2 } = await import('./fork-skills.ts')
  const skill = { id: 'manual', name: 'Manual', description: 'synthetic', source: 'custom', path: '/synthetic/SKILL.md',
    receipt: null, installationId: '11111111-1111-4111-8111-111111111111:7', contentStatus: 'verified' as const }
  const captured = passiveSkillsV2.captureRemoval(skill)
  assert.deepEqual(captured, { id: skill.id, expectedInstallationId: skill.installationId, expectedReceipt: { kind: 'none' } })
  const calls: unknown[] = []
  const client: SkillClient = { async call<T>(method: string, body?: unknown): Promise<T> {
    calls.push([method, body]); throw new Error('Intenção de remoção inválida')
  } }
  await assert.rejects(passiveSkillsV2.remove(client, captured), /skill_installation_unsupported/)
  assert.deepEqual(calls, [['removeSkill', { ...captured, skillProtocol: 2 }]])
  for (const bad of [{ id: skill.id }, { ...captured, expectedReceipt: null },
    { ...captured, expectedReceipt: { kind: 'none', digest: 'invented' } },
    { ...captured, expectedInstallationId: 'invented' }, { ...captured, expectedDigest: 'old' }]) {
    await assert.rejects(passiveSkillsV2.remove(client, bad as typeof captured), /skill_installation_unsupported/)
  }
  assert.equal(calls.length, 1)
  const old: SkillClient = { async call<T>() { return { items: [{ ...skill, installationId: undefined }] } as T } }
  await assert.rejects(passiveSkillsV2.installed(old), /skill_installation_unsupported/)
  const oldDetail: SkillClient = { async call<T>() { return { markdown: 'old', receipt: null, catalog: null } as T } }
  await assert.rejects(passiveSkillsV2.details(oldDetail, { id: skill.id }), /skill_installation_unsupported/)
})

test('R6 mantém o recibo capturado e confere o envelope coerente de detalhes', async () => {
  const { passiveSkillsV2 } = await import('./fork-skills.ts')
  const receipt = { schema: 1 as const, catalogId: 'synthetic', digest: 'a'.repeat(64), origin: {
    repo: 'synthetic', revision: 'fixture', path: 'SKILL.md', sourceDigest: 'b'.repeat(64) },
  adaptation: null, license: 'synthetic-only', files: [], result: 'imported' as const,
  coverage: 'pinnedPassiveTextOnly' as const, limitations: [] }
  const skill = { id: 'synthetic', name: 'Synthetic', description: 'fixture', source: 'catalog:synthetic', path: '/synthetic/SKILL.md',
    receipt, installationId: '11111111-1111-4111-8111-111111111111:8', contentStatus: 'verified' as const }
  const captured = passiveSkillsV2.captureRemoval(skill)
  receipt.digest = 'c'.repeat(64)
  assert.equal(captured.expectedReceipt.kind === 'catalog' && captured.expectedReceipt.digest, 'a'.repeat(64))
  const client: SkillClient = { async call<T>() { return { skillProtocol: 2, markdown: 'LITERAL', catalog: null, receipt, installation: skill } as T } }
  assert.deepEqual((await passiveSkillsV2.details(client, { id: skill.id })).installation, skill)
  const wrong: SkillClient = { async call<T>() { return { skillProtocol: 2, markdown: 'LITERAL', catalog: null, receipt: null, installation: skill } as T } }
  await assert.rejects(passiveSkillsV2.details(wrong, { id: skill.id }), /skill_installation_unsupported/)
  // D306: alterada/ilegível também se remove, presa à mesma encarnação e ao mesmo recibo.
  for (const contentStatus of ['changed', 'unavailable'] as const) {
    assert.deepEqual(passiveSkillsV2.captureRemoval({ ...skill, receipt: { ...receipt, digest: 'a'.repeat(64) }, contentStatus }), captured)
  }
  const sent: unknown[] = []
  const install: SkillClient = { async call<T>(_m: string, body?: unknown) { sent.push(body); return { skillProtocol: 2, skill: { ...skill, receipt: null }, alreadyInstalled: false } as T } }
  await passiveSkillsV2.install(install, { catalogId: 'synthetic', expectedDigest: 'a'.repeat(64), expectedInstallationId: skill.installationId })
  assert.deepEqual(sent, [{ skillProtocol: 2, catalogId: 'synthetic', expectedDigest: 'a'.repeat(64), expectedInstallationId: skill.installationId }])
  await assert.rejects(passiveSkillsV2.install(install, { catalogId: 'synthetic', expectedDigest: 'a'.repeat(64), expectedInstallationId: 'synthetic' }), /skill_installation_unsupported/)
  assert.equal(sent.length, 1, 'encarnação inválida não sai do app')
})

test('R7 prova versão inclusive vazios e força discriminador em todas as consultas', async () => {
  const { passiveSkillsV2 } = await import('./fork-skills.ts')
  const calls: unknown[] = []
  const current: SkillClient = { async call<T>(method: string, body?: unknown) { calls.push([method, body]); return { skillProtocol: 2, items: [] } as T } }
  assert.deepEqual(await passiveSkillsV2.installed(current), [])
  assert.deepEqual(await passiveSkillsV2.catalog(current), [])
  assert.deepEqual(calls, [['skills', { skillProtocol: 2 }], ['marketSkills', { skillProtocol: 2 }]])
  for (const reply of [{ items: [] }, { skillProtocol: 1, items: [] }, { skillProtocol: '2', items: [] }, null]) {
    const old: SkillClient = { async call<T>() { return reply as T } }
    await assert.rejects(passiveSkillsV2.installed(old), /skill_installation_unsupported/)
    await assert.rejects(passiveSkillsV2.catalog(old), /skill_installation_unsupported/)
  }
})

test('R7 mutações enviam versão fechada: parser antigo recusa antes efeito; nunca há fallback', async () => {
  const { passiveSkillsV2 } = await import('./fork-skills.ts')
  const calls: {method: string; body: unknown}[] = []
  let effects = 0
  const old: SkillClient = { async call<T>(method: string, body?: unknown) {
    calls.push({method, body})
    // O probe Rust externo usa os parsers antigos literais; este teste fixa o contrato do cliente.
    if ((body as {skillProtocol?: unknown}).skillProtocol !== undefined) throw new Error('Texto manual inválido')
    effects++; return {} as T
  } }
  await assert.rejects(passiveSkillsV2.install(old, {catalogId: 'synthetic', expectedDigest: 'a'.repeat(64)}), /skill_installation_unsupported/)
  await assert.rejects(passiveSkillsV2.add(old, 'Manual', 'fixture', 'BODY'), /skill_installation_unsupported/)
  await assert.rejects(passiveSkillsV2.remove(old, {id: 'manual', expectedInstallationId: '11111111-1111-4111-8111-111111111111:1', expectedReceipt: {kind: 'none'}}), /skill_installation_unsupported/)
  assert.equal(effects, 0); assert.equal(calls.length, 3)
  assert.ok(calls.every(({body}) => (body as {skillProtocol: unknown}).skillProtocol === 2))
})

test('R7 reply perdida/malformada após envio é incerta, exige conciliação e não retenta', async () => {
  const { passiveSkillsV2, SkillMutationOutcomeUnknown } = await import('./fork-skills.ts')
  for (const reply of [undefined, {}, {skillProtocol: 1}, {skillProtocol: 2, alreadyInstalled: false}, new Error('timeout after commit')]) {
    let calls = 0
    const c: SkillClient = { async call<T>() { calls++; if (reply instanceof Error) throw reply; return reply as T } }
    await assert.rejects(passiveSkillsV2.add(c, 'Manual', 'fixture', 'BODY'), (error: unknown) => {
      assert.ok(error instanceof SkillMutationOutcomeUnknown)
      assert.equal(error.code, 'skill_mutation_outcome_unknown'); assert.equal(error.needsReconciliation, true)
      assert.equal(error.method, 'installSkill'); return true
    })
    assert.equal(calls, 1)
  }
  for (const reply of [undefined, {}, {skillProtocol: 1}, {skillProtocol: 2, unexpected: true}]) {
    let calls = 0
    const c: SkillClient = { async call<T>() { calls++; return reply as T } }
    await assert.rejects(passiveSkillsV2.remove(c, {id: 'manual', expectedInstallationId: '11111111-1111-4111-8111-111111111111:1', expectedReceipt: {kind: 'none'}}), SkillMutationOutcomeUnknown)
    assert.equal(calls, 1)
  }
  const stale = new Error('skill_installation_stale')
  const refused: SkillClient = { async call<T>(): Promise<T> { throw stale } }
  await assert.rejects(passiveSkillsV2.add(refused, 'Manual', 'fixture', 'BODY'), (error: unknown) => error === stale)
  const removed: SkillClient = { async call<T>() { return {skillProtocol: 2} as T } }
  assert.deepEqual(await passiveSkillsV2.remove(removed, {id: 'manual', expectedInstallationId: '11111111-1111-4111-8111-111111111111:1', expectedReceipt: {kind: 'none'}}), {skillProtocol: 2})
})
