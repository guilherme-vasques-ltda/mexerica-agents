import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { chmodSync, existsSync, readFileSync, renameSync, statSync, writeFileSync } from 'node:fs'
import { test } from 'node:test'
import { verifyRc2SignedCopy } from './fork-rc2-developer-id.ts'
import type { Rc2Component } from '../shared/fork-rc2-profile.ts'

const bytes = Buffer.from('bytes fictícios')
const sha256 = createHash('sha256').update(bytes).digest('hex')
const component: Rc2Component = { path: 'hub', role: 'hub', arch: 'arm64', sha256, cdhash: 'a'.repeat(40) }

test('vincula a resposta nativa ao CDHash e aos bytes da cópia privada', () => {
  let copy = ''
  verifyRc2SignedCopy(bytes, component, (path, role) => {
    copy = path
    assert.equal(role, 'hub')
    assert.deepEqual(readFileSync(path), bytes)
    assert.equal(statSync(path).mode & 0o777, 0o400)
    return component.cdhash
  })
  assert.ok(!existsSync(copy))
})

test('recusa sem decisão positiva da plataforma, com outro CDHash ou com SHA incorreto', () => {
  for (const reject of [null, 'b'.repeat(40)]) {
    assert.throws(() => verifyRc2SignedCopy(bytes, component, () => reject), /invalid_rc2_artifact/)
  }
  assert.throws(() => verifyRc2SignedCopy(bytes, { ...component, sha256: '0'.repeat(64) },
    () => assert.fail('cópia com SHA divergente não chega à plataforma')), /invalid_rc2_artifact/)
})

test('recusa substituição e escrita na cópia durante a verificação', () => {
  for (const change of [
    (path: string) => { renameSync(path, path + '.anterior'); writeFileSync(path, 'substituto') },
    (path: string) => { renameSync(path, path + '.anterior'); writeFileSync(path, bytes) },
    (path: string) => { chmodSync(path, 0o600); writeFileSync(path, 'alterado') },
  ]) {
    let copy = ''
    assert.throws(() => verifyRc2SignedCopy(bytes, component, path => {
      copy = path
      change(path)
      return component.cdhash
    }), /invalid_rc2_artifact/)
    assert.ok(!existsSync(copy))
  }
})
