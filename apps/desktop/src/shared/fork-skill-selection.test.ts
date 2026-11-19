import test from 'node:test'
import assert from 'node:assert/strict'
import { draftOf, normalizeBot } from './models.ts'
import type { InstalledSkillV2 } from './fork-skills.ts'
import { captureSkillSelection, captureSkillSelectionOrigin, readSkillSelectionStamp, sendSkillSelection,
  SkillSelectionOutcomeUnknown, SkillSelectionRefusal, type SkillSelectionClient, type SkillSelectionSource } from './fork-skill-selection.ts'

const version = 'a'.repeat(32)
const installationId = '12345678-1234-1234-1234-123456789abc:42'
const skill = (): InstalledSkillV2 => ({ id: 's', name: 'Nome', description: 'privado', source: 'local', path: '/privado', receipt: null, installationId, contentStatus: 'verified' })
const bot = () => ({ id: 'b', skillSelection: { protocol: 1 as const, version } })
class Fixture implements SkillSelectionSource {
  hello: { hostId: string } | null = { hostId: 'hub-a' }
  principalEpoch = 1
  spaceId: string | undefined = 'personal'
  calls: { method: string; body: unknown }[] = []
  answer: () => Promise<unknown> = async () => ({ bot: { id: 'b', skillSelection: { protocol: 1, version: 'b'.repeat(32) }, skills: ['s'] } })
  connectedClient: SkillSelectionClient | null = { call: async <T>(method: string, body?: unknown) => { this.calls.push({ method, body }); return await this.answer() as T } }
  readyHook: () => Promise<SkillSelectionClient> = async () => this.connectedClient!
  ready() { return this.readyHook() }
}
const code = (expected: string) => (e: unknown) => e instanceof SkillSelectionRefusal && e.code === expected

test('stamp exato, normalização e draft não inventam nem reenviam condição', () => {
  for (const raw of [undefined, null, {}, { protocol: 1, version: version.toUpperCase() }, { protocol: 1, version, extra: true }, { protocol: 2, version }]) {
    assert.equal(readSkillSelectionStamp(raw), null)
    assert.equal(normalizeBot({ id: 'b', skillSelection: raw as never }).skillSelection, null)
  }
  const normalized = normalizeBot(bot())
  assert.deepEqual(normalized.skillSelection, bot().skillSelection)
  assert.equal(Object.hasOwn(draftOf(normalized), 'skillSelection'), false)
})

test('captura congela escalares, valida instalação e envia apenas intenção tipada', async () => {
  const s = skill(), b = bot(), f = new Fixture()
  const origin = captureSkillSelectionOrigin(f), request = captureSkillSelection(origin, b, s, true)
  b.skillSelection.version = 'b'.repeat(32); s.id = 'outro'; s.installationId = 'outro'; s.contentStatus = 'changed'
  const result = await sendSkillSelection(origin, request)
  assert.deepEqual(f.calls, [{ method: 'updateBot', body: { hubId: 'hub-a', id: 'b', skillSelection: { protocol: 1, expectedVersion: version, skillId: 's', enabled: true, expectedInstallationId: installationId } } }])
  assert.equal(result.origin, origin)
  assert.deepEqual(result.bot.skills, ['s'])
  assert.throws(() => captureSkillSelection(origin, { id: 'b' }, skill(), true), code('skill_selection_unsupported'))
  assert.throws(() => captureSkillSelection(origin, bot(), { ...skill(), contentStatus: 'changed' }, true), code('skill_installation_changed'))
  assert.equal(captureSkillSelection(origin, bot(), { ...skill(), contentStatus: 'changed' }, false).skillSelection.enabled, false)
  assert.throws(() => captureSkillSelection(origin, bot(), { ...skill(), contentStatus: 'unavailable' }, true), code('skill_installation_unavailable'))
  for (const id of ['12345678-1234-1234-1234-123456789abc:0', '12345678-1234-1234-1234-123456789abc:18446744073709551616', installationId.toUpperCase()]) {
    assert.throws(() => captureSkillSelection(origin, bot(), { ...skill(), installationId: id }, true), code('skill_installation_stale'))
  }
})

test('origem trocada antes ou durante ready, reconexão e ABA não enviam', async () => {
  for (const change of ['hub', 'client', 'aba', 'ready-client', 'space']) {
    const f = new Fixture(), origin = captureSkillSelectionOrigin(f), request = captureSkillSelection(origin, bot(), skill(), true)
    f.readyHook = async () => {
      if (change === 'hub') f.hello = { hostId: 'hub-b' }
      if (change === 'client') f.connectedClient = { call: async <T>() => undefined as T }
      if (change === 'aba') { f.hello = { hostId: 'hub-b' }; f.principalEpoch++; f.hello = { hostId: 'hub-a' } }
      if (change === 'space') f.spaceId = 'outro'
      return change === 'ready-client' ? { call: async <T>() => undefined as T } : origin.client
    }
    await assert.rejects(sendSkillSelection(origin, request), code('skill_selection_origin_changed'))
    assert.equal(f.calls.length, 0)
  }
  const unavailable = new Fixture()
  unavailable.readyHook = async () => { throw new Error('offline') }
  const unavailableOrigin = captureSkillSelectionOrigin(unavailable)
  await assert.rejects(sendSkillSelection(unavailableOrigin, captureSkillSelection(unavailableOrigin, bot(), skill(), true)), code('skill_selection_not_ready'))
  assert.equal(unavailable.calls.length, 0)
  const f = new Fixture(); f.spaceId = 'outro'
  assert.throws(() => captureSkillSelectionOrigin(f), code('skills_personal_only'))
})

test('resposta inválida ou perdida é incerta; recusa explícita não repete', async () => {
  for (const answer of [async () => ({ bot: { id: 'b', skills: ['s'] } }), async () => ({ bot: { id: 'b', skillSelection: bot().skillSelection, skills: [] } }), async () => { throw new Error('queda') }]) {
    const f = new Fixture(); f.answer = answer
    const origin = captureSkillSelectionOrigin(f)
    await assert.rejects(sendSkillSelection(origin, captureSkillSelection(origin, bot(), skill(), true)), (e: unknown) => e instanceof SkillSelectionOutcomeUnknown && e.needsReconciliation)
    assert.equal(f.calls.length, 1)
  }
  for (const refusal of ['skill_selection_unsupported', 'skill_selection_conflict', 'skill_installation_stale', 'skill_installation_changed', 'skill_installation_unavailable']) {
    const f = new Fixture(); f.answer = async () => { throw new Error(refusal) }
    const origin = captureSkillSelectionOrigin(f)
    await assert.rejects(sendSkillSelection(origin, captureSkillSelection(origin, bot(), skill(), true)), code(refusal))
    assert.equal(f.calls.length, 1)
  }
  const f = new Fixture(); f.answer = async () => { f.principalEpoch++; return { bot: { ...bot(), skills: ['s'] } } }
  const origin = captureSkillSelectionOrigin(f)
  await assert.rejects(sendSkillSelection(origin, captureSkillSelection(origin, bot(), skill(), true)), SkillSelectionOutcomeUnknown)
  assert.equal(f.calls.length, 1)
})


test('captura pertence à origem do gesto mesmo com nonce e instalação idênticos', async () => {
  const a = new Fixture(), b = new Fixture()
  b.hello = { hostId: 'hub-b' }
  const originA = captureSkillSelectionOrigin(a), originB = captureSkillSelectionOrigin(b)
  const request = captureSkillSelection(originA, bot(), skill(), true)
  let readyCalls = 0
  b.readyHook = async () => { readyCalls++; return originB.client }
  await assert.rejects(sendSkillSelection(originB, request), code('skill_selection_origin_changed'))
  assert.equal(readyCalls, 0)
  assert.equal(b.calls.length, 0)
  a.principalEpoch++
  await assert.rejects(sendSkillSelection(originA, request), code('skill_selection_origin_changed'))
  assert.throws(() => captureSkillSelection(originA, bot(), skill(), true), code('skill_selection_origin_changed'))
  assert.equal(a.calls.length, 0)
})
