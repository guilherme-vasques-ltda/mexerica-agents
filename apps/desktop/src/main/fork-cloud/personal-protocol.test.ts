import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { inspectPersonalManifest, personalCanonical } from './personal-protocol.ts'

const vectors = JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json', import.meta.url), 'utf8')).personalBootstrap
const raw = (v: unknown) => Buffer.from(JSON.stringify(v))

test('PL1 C1: manifesto e contexto têm os mesmos bytes e hashes públicos Rust/Worker', () => {
  const parsed = inspectPersonalManifest(Buffer.from(vectors.manifestCanonical))
  assert.deepEqual(parsed.context, vectors.context)
  assert.equal(JSON.stringify(parsed.context), vectors.contextCanonical)
  assert.equal(createHash('sha256').update(vectors.contextCanonical).digest('hex'), vectors.contextHash)
  assert.equal(parsed.context[6], vectors.manifestHash)
})

test('PL1 C1: perfil, raiz, contexto e bytes alternativos não passam pela inspeção', () => {
  for (const [key, value] of [['schema', 2], ['schemaVersion', 6], ['env', 'local'], ['extra', true], ['origin', 'https://cloud.example.invalid/']] as const) {
    assert.throws(() => inspectPersonalManifest(raw({ ...vectors.manifest, [key]: value })), key)
  }
  const operator = vectors.manifest.bootstrap
  for (const patch of [{ profile: 'ACS4' }, { wire: 2 }, { extra: true }, { issuerPublicKey: operator.issuerPublicKey }, { issuerPublicKey: vectors.manifest.personalBootstrap.issuerPublicKey + '=' }]) {
    assert.throws(() => inspectPersonalManifest(raw({ ...vectors.manifest, personalBootstrap: { ...vectors.manifest.personalBootstrap, ...patch } })))
  }
  for (const text of ['{"a":1,"a":2}', '[1.0]', '[1e0]', '[-0]', '[9007199254740992]', ' [1]', '\ufeff[1]', '["\\ud800"]']) {
    assert.throws(() => personalCanonical(Buffer.from(text)))
  }
  assert.throws(() => personalCanonical(Buffer.alloc(16385)))
})
