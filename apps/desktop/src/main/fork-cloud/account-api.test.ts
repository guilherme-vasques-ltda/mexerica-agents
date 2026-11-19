import assert from 'node:assert/strict'
import test from 'node:test'
import * as wire from './crypto.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

test('ACS4 AccountApi publica pelo reconcile real do Worker com POST e assinatura de corpo vazio', { skip: 'depende de cloud/ (perfil opcional de auto-hospedagem, fora do primeiro recorte)' }, async () => {
  const device = wire.signerFromSeed(new Uint8Array(32).fill(31))
  const bootstrapId = 'bootstrap_fixture'
  const deployment = { deploymentId: 'fixture', env: 'main' as const, issuer: 'https://issuer.example', origin: 'https://cloud.example', manifestHash: 'a'.repeat(64) }
  const principal = { namespace: 'fixture', deployment, seal: { userId: 'user_fixture', sessionId: 'sess_fixture', epoch: 1 }, current: () => true, token: async () => 'fixture' }
  const row = { bootstrap_id: bootstrapId, hub_generation: 'generation_fixture', grant_id: 'grt_fixture', device_key: wire.b64url(device.publicKey), owner_user_id: principal.seal.userId, hash_receipt: 'b'.repeat(64) }
  const intent = { state: 'hubPending', expires_at: Date.now() + 60_000, revision: 1, hub_computer_id: 'hub_fixture', owner_user_id: principal.seal.userId }
  let batches = 0
  let notifications = 0
  class ApiError extends Error {
    readonly code: string
    constructor(code: string) { super(code); this.code = code }
  }
  // The body guard, CAS control flow and response are the actual Worker source.
  // Authentication, D1 and relay notification are isolated boundaries, not real cloud authority.
  const { reconcile } = loadMainModule<{ reconcile(ctx: unknown, ids: string[]): Promise<Response> }>(
    '../../../../cloud/src/fork/bootstrap/publication', {
      '../../routes/common': { ApiError, notify: async (_env: unknown, computerId: string) => {
        assert.equal(computerId, intent.hub_computer_id)
        assert.equal(intent.state, 'published')
        notifications++
      } },
      './codec': { identifier: (id: string) => id === bootstrapId },
      './intents': { available: () => {}, principal: async () => ({ owner: principal.seal.userId, deviceId: 'dev_fixture', deviceKey: row.device_key }) },
      './store': { readOwnedIntent: async () => intent },
      './revocations': { revocationStatus: async () => null, reconcileRevocation: () => { throw new Error('Unexpected revocation') } },
    }, { Response },
  )
  const db = { prepare: () => ({ bind: () => ({ first: async () => row }) }), batch: async () => {
    batches++; intent.state = 'published'
    return Array.from({ length: 3 }, () => ({ success: true, meta: { changes: 1 } }))
  } }
  const observed: { bytes: number; contentType: string | null; signatureValid: boolean; status: number }[] = []
  const fetchBoundary = async (input: URL, init?: RequestInit) => {
    const request = new Request(input, init)
    if (request.url === `${deployment.origin}/v1/health`) return Response.json({ ok: true, deployment })
    assert.equal(request.method, 'POST')
    assert.equal(request.url, `${deployment.origin}/v1/bootstrap/intents/${bootstrapId}/reconcile`)
    const raw = new Uint8Array(await request.arrayBuffer())
    const signature = request.headers.get('Codync-Sig') ?? ''
    const fields = Object.fromEntries(signature.split(',').map(field => field.split('=')))
    const canonical = wire.sigCanonical(request.method, input.host, input.pathname, Number(fields.ts), fields.nonce!, raw)
    const valid = fields.kid === row.device_key && wire.verifyEd25519(device.publicKey, wire.unb64url(fields.sig, 64)!, Buffer.from(canonical))
    let response: Response
    try {
      response = await reconcile({ url: input, raw, now: Date.now(), env: { DB: db }, deployment: { manifest: deployment, manifestHash: deployment.manifestHash } }, [bootstrapId])
    } catch (error) {
      if (!(error instanceof ApiError)) throw error
      response = Response.json({ error: { code: error.code } }, { status: 400 })
    }
    observed.push({ bytes: raw.length, contentType: request.headers.get('Content-Type'), signatureValid: valid, status: response.status })
    return response
  }
  const { AccountApi } = loadMainModule<typeof import('./account-api.ts')>('fork-cloud/account-api', {
    './crypto': wire, './device-keys': { deviceSigner: async () => device },
    './pins': { matchesDeployment: (_expected: unknown, value: { ok: boolean }) => value.ok === true },
  }, { Buffer, URL, fetch: fetchBoundary })
  const api = new AccountApi(principal)
  assert.equal((await api.execute({ kind: 'health' })).kind, 'ok')
  const result = await api.execute({ kind: 'bootstrapReconcile', bootstrapId })
  assert.deepEqual(observed, [{ bytes: 0, contentType: null, signatureValid: true, status: 200 }])
  assert.equal(result.kind, 'ok')
  if (result.kind === 'ok') assert.equal(result.value.state, 'published')
  assert.equal(batches, 1)
  assert.equal(notifications, 1)
})
