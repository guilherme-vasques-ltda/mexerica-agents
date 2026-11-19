import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { randomBytes } from 'node:crypto'
import test from 'node:test'
import { recoveryDiagnostic, RECOVERY_REASONS, RECOVERY_STAGES } from '../main/fork-account/recovery-diagnostic.ts'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { loadMainModule } from './test-support/main-module.ts'

test('diagnóstico aceita somente valores fechados e falha do console não interfere', () => {
  const original = console.warn; const rows: unknown[][] = []
  try {
    console.warn = (...args: unknown[]) => { rows.push(args) }
    recoveryDiagnostic('check_branch', 'password_unreadable', 403)
    recoveryDiagnostic('secret-path', 'password_unreadable', 403)
    recoveryDiagnostic('check_branch', 'secret-body', 403)
    recoveryDiagnostic('check_branch', 'password_unreadable', 'secret-status')
    recoveryDiagnostic('check_branch', 'password_unreadable', 600)
    assert.equal(rows.length, 3)
    for (const row of rows) {
      assert.equal(row.length, 2); assert.equal(row[0], 'account:recovery-diagnostic')
      const value = JSON.parse(String(row[1])) as { stage: string; reason: string; status: number | null }
      assert.deepEqual(Object.keys(value), ['stage', 'reason', 'status'])
      assert.ok((RECOVERY_STAGES as readonly string[]).includes(value.stage))
      assert.ok((RECOVERY_REASONS as readonly string[]).includes(value.reason))
    }
    assert.equal((JSON.parse(String(rows[0]?.[1])) as { status: unknown }).status, 403)
    assert.equal((JSON.parse(String(rows[1]?.[1])) as { status: unknown }).status, null)
    assert.equal((JSON.parse(String(rows[2]?.[1])) as { status: unknown }).status, null)
    assert.equal(JSON.stringify(rows).includes('secret-'), false)
    console.warn = () => { throw new Error('secret-error') }
    assert.doesNotThrow(() => recoveryDiagnostic('check_branch', 'password_unreadable'))
  } finally { console.warn = original }
})

test('check com journal ilegível reporta ramo sem rede e continua recusando quando logger falha', async () => {
  const events: unknown[][] = []; let broken = false; let requests = 0
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
    electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true } },
    './auth': {}, './environment': { identity: { appId: 'fixture', releaseUpdates: false }, buildConfig: {} },
    '../shared/fork-features': { forkFeatures: { cloudAccess: false } }, './fork-account/config': { accountConfig, accountBrowserURL },
    './fork-account/storage': { AccountStorage: class {} },
  }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, fetch: async () => { requests++; throw new Error('secret-path') }, console: { warn: (...args: unknown[]) => { if (broken) throw new Error('secret-error'); events.push(args) } } })
  const account = new mod.AccountService()
  const inner = account as unknown as { passwordAttemptUnreadable: boolean; state: { ready: boolean } }
  inner.passwordAttemptUnreadable = true; inner.state.ready = false
  assert.equal((await account.reconcile('check')).kind, 'unavailable')
  assert.equal(JSON.stringify(events).includes('secret-'), false); assert.equal(requests, 0)
  broken = true
  assert.equal((await account.reconcile('check')).kind, 'unavailable')
  assert.equal(requests, 0)
})

test('classificação fechada distingue wrappers, seleção e usuário sem registrar dados', async () => {
  const { classifyClient } = await import('../main/fork-account/recovery-classification.ts')
  const original = console.warn
  const rows: string[] = []
  try {
    console.warn = (_prefix: unknown, json: unknown) => { rows.push(String(json)) }
    const cases: [unknown, unknown, string[]][] = [
      [{ response: null, client: { sessions: [] } }, { response: null, client: { sessions: [] } }, ['response_null', 'client_sidecar', 'sessions_absent']],
      [{ response: { sessions: [] }, meta: { client: {} } }, { sessions: [] }, ['response_wrapper', 'meta_client_sidecar', 'multiple_wrappers', 'sessions_empty']],
      [{}, { sessions: [{ id: 'secret-id', status: 'pending' }], last_active_session_id: 'secret-id' }, ['response_absent', 'selected_pending', 'selected_user_missing']],
    ]
    for (const [payload, selected, expected] of cases) {
      rows.length = 0; classifyClient(payload, selected)
      const reasons = rows.map(row => (JSON.parse(row) as { reason: string }).reason)
      for (const reason of expected) assert.ok(reasons.includes(reason), reason)
      assert.equal(rows.join('').includes('secret-id'), false)
    }
    console.warn = () => { throw new Error('fixture') }
    assert.doesNotThrow(() => classifyClient({}, {}))
  } finally { console.warn = original }
})
