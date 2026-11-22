import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { onboardingDismissal } from './fork-onboarding-dismissal.ts'

test('adiamento sem Hub sobrevive à reabertura e fica no perfil e instalação de origem', () => {
  const dir = mkdtempSync(join(tmpdir(), 'onboarding-'))
  const file = join(dir, 'prefs.json')
  const openPrefs = () => {
    const data: Record<string, unknown> = JSON.parse(readFileSync(file, 'utf8'))
    return {
      get: (key: string) => data[key],
      set(key: string, value: unknown) {
        data[key] = value
        writeFileSync(file, JSON.stringify(data))
        return true
      },
    }
  }
  const local = ['main', 'profile-a', 'local-hub-a', 19422, 'deployment-a']
  try {
    writeFileSync(file, '{}')
    assert.equal(onboardingDismissal(openPrefs(), local, 'account-a', 'account-a', 'defer'), true)
    const prefs = openPrefs()
    assert.equal(onboardingDismissal(prefs, local, 'account-a', 'account-a', 'read'), true)
    assert.equal(onboardingDismissal(prefs, local, undefined, 'account-a', 'read'), null)
    assert.equal(onboardingDismissal(prefs, local, 'account-b', 'account-a', 'clear'), null)
    assert.equal(onboardingDismissal(prefs, local, 'account-b', 'account-b', 'read'), false)
    assert.equal(onboardingDismissal(prefs, ['main', 'profile-b', 'local-hub-a', 19422, 'deployment-a'], 'account-a', 'account-a', 'read'), false)
    assert.equal(onboardingDismissal(prefs, ['main', 'profile-a', 'local-hub-b', 19422, 'deployment-a'], 'account-a', 'account-a', 'read'), false)
    assert.equal(onboardingDismissal(prefs, local, 'account-a', 'account-a', 'install'), null)
    const failingPrefs = { get: prefs.get, set: () => false }
    assert.equal(onboardingDismissal(failingPrefs, local, 'account-a', 'account-a', 'clear'), null)
    assert.equal(onboardingDismissal(failingPrefs, local, 'account-a', 'account-a', 'read'), true)
    const saved = JSON.parse(readFileSync(file, 'utf8'))
    assert.deepEqual(Object.values(saved), [true])
    assert.match(Object.keys(saved)[0]!, /^onboardingDismissed\.v1\.[a-f0-9]{64}$/)
    assert.equal(onboardingDismissal(prefs, local, 'account-a', 'account-a', 'clear'), false)
    assert.equal(onboardingDismissal(openPrefs(), local, 'account-a', 'account-a', 'read'), false)
  } finally {
    rmSync(dir, { recursive: true })
  }
})
