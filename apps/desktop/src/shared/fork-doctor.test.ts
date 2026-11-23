import assert from 'node:assert/strict'
import test from 'node:test'
import { classifyDoctorError, parseHubDoctorReport } from './fork-doctor.ts'

const validReport = {
  version: 1,
  hubId: 'hub-1',
  spaceId: 'personal',
  collectedAt: 1_760_000_000_000,
  overall: 'waiting_user',
  checks: [
    { id: 'hub', area: 'hub', status: 'ok', code: 'hub_ok', count: null, actionKind: 'none' },
    { id: 'ai_engine', area: 'ai_engine', status: 'ok', code: 'ai_ok', count: 1, actionKind: 'none' },
    { id: 'approvals', area: 'approvals', status: 'warning', code: 'approvals_waiting', count: 2, actionKind: 'review_waiting' },
  ],
}

test('parseHubDoctorReport aceita relatório válido e valida hubId/spaceId esperados', () => {
  const parsed = parseHubDoctorReport(validReport, { hubId: 'hub-1', spaceId: 'personal' })
  assert.ok(parsed)
  assert.equal(parsed.overall, 'waiting_user')
  assert.equal(parsed.checks.length, 3)
  assert.equal(parsed.checks[2]?.count, 2)

  assert.equal(parseHubDoctorReport(validReport, { hubId: 'outro-hub', spaceId: 'personal' }), null)
  assert.equal(parseHubDoctorReport(validReport, { hubId: 'hub-1', spaceId: 'trabalho' }), null)
})

test('parseHubDoctorReport falha fechado em versões, áreas, status, contagens ou duplicatas inválidas', () => {
  assert.equal(parseHubDoctorReport(null), null)
  assert.equal(parseHubDoctorReport({ ...validReport, version: 2 }), null)
  assert.equal(parseHubDoctorReport({ ...validReport, overall: 'hub_offline' }), null)
  assert.equal(parseHubDoctorReport({ ...validReport, checks: [] }), null)
  assert.equal(
    parseHubDoctorReport({
      ...validReport,
      checks: [{ id: 'x', area: 'desconhecida', status: 'ok', code: 'ok', count: null, actionKind: 'none' }],
    }),
    null,
  )
  assert.equal(
    parseHubDoctorReport({
      ...validReport,
      checks: [{ id: 'x', area: 'hub', status: 'ok', code: 'ok', count: -1, actionKind: 'none' }],
    }),
    null,
  )
  assert.equal(
    parseHubDoctorReport({
      ...validReport,
      checks: [validReport.checks[0], validReport.checks[0]],
    }),
    null,
  )
})

test('classifyDoctorError distingue Hub antigo (404), recusa (403) e falha de conexão (hub_offline)', () => {
  assert.equal(classifyDoctorError(Object.assign(new Error('unknown method'), { status: 404 })), 'unsupported')
  assert.equal(classifyDoctorError(new Error('unknown method')), 'unsupported')
  assert.equal(classifyDoctorError(Object.assign(new Error('forbidden'), { status: 403 })), 'denied')
  assert.equal(classifyDoctorError(Object.assign(new Error('bad request'), { status: 400 })), 'unavailable')
  assert.equal(classifyDoctorError(new Error('ECONNREFUSED 127.0.0.1:19422')), 'hub_offline')
  assert.equal(classifyDoctorError(new Error('network timeout')), 'hub_offline')
})
