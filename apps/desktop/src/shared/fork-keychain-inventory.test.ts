import assert from 'node:assert/strict'
import test from 'node:test'
import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join, relative } from 'node:path'

// Explicit families reviewed by SEG in #861. New use requires review, not auto-discovery.
const allowed = [
  'fork-account/device-key.ts', 'fork-account/password-attempt.ts', 'fork-account/service.ts',
  'fork-cloud/bootstrap-journal.ts', 'fork-cloud/bootstrap-pin.ts', 'fork-cloud/enroll.ts',
  'fork-cloud/personal-journal.ts', 'fork-cloud/personal-link-vault.ts', 'fork-cloud/personal-pin-main.ts',
  'fork-cloud/pins.ts', 'fork-voz/cofre.ts', 'fork-voz/registrar.ts',
  'index.ts', // Only passes the codec to the macOS startup canary gate.
  'environment.ts', // Historical comment: no crypto call before ready.
]
test('inventário safeStorage: grep recursivo recusa consumidores fora da lista SEG', () => {
  const root = fileURLToPath(new URL('../main/', import.meta.url))
  const matches = readdirSync(root, { recursive: true, withFileTypes: true })
    .filter(e => e.isFile() && /\.[cm]?[jt]sx?$/.test(e.name) && !/\.(test|spec)\./.test(e.name))
    .map(e => join(e.parentPath, e.name))
    .filter(file => /\bsafeStorage\b/.test(readFileSync(file, 'utf8')))
    .map(file => relative(root, file).replaceAll('\\', '/')).sort()
  assert.deepEqual(matches, allowed.toSorted(), 'Novo consumidor: atualizar inventário e obter revisão SEG.')
})

// Counts include declarations and forwarding wrappers, not just safeStorage calls.
// Injection sites are explicit: a new family or a use in an existing file must be reviewed.
const codecUses: Record<string, [number, number]> = {
  'fork-account/device-key.ts': [3, 2],
  'fork-account/password-attempt.ts': [1, 1],
  'fork-account/service.ts': [2, 1],
  'fork-cloud/bootstrap-journal.ts': [2, 1],
  'fork-cloud/bootstrap-pin.ts': [1, 1],
  'fork-cloud/enroll.ts': [1, 0],
  'fork-cloud/personal-journal.ts': [1, 1],
  'fork-cloud/personal-link-vault.ts': [1, 1],
  'fork-cloud/personal-pin-main.ts': [2, 2],
  'fork-cloud/personal-pin-storage.ts': [3, 3],
  'fork-cloud/personal-pin-store.ts': [4, 3],
  'fork-cloud/pins.ts': [0, 2],
  'fork-keychain/generation.ts': [2, 3],
  'fork-voz/cofre.ts': [2, 2],
}
const injectionSites = [
  ['index.ts', "prepareKeychain(app.getPath('userData'), process.platform, safeStorage)"],
  ['fork-voz/registrar.ts', "new CofreVoz({ dados: app.getPath('userData'), sistema: safeStorage, plataforma: process.platform })"],
  ['fork-cloud/personal-pin-main.ts', 'new PersonalPinStore(p,binding,authority,{current:secure,encryptString:s=>safeStorage.encryptString(s),decryptString:b=>safeStorage.decryptString(b)},directory)'],
] as const

test('inventário conta operações de codec e pontos de injeção por arquivo', () => {
  const root = fileURLToPath(new URL('../main/', import.meta.url))
  const actual: Record<string, [number, number]> = {}
  for (const entry of readdirSync(root, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile() || !/\.[cm]?[jt]sx?$/.test(entry.name) || /\.(test|spec)\./.test(entry.name)) continue
    const file = join(entry.parentPath, entry.name), source = readFileSync(file, 'utf8')
    const counts: [number, number] = [(source.match(/\bencryptString\b/g) ?? []).length, (source.match(/\bdecryptString\b/g) ?? []).length]
    if (counts.some(Boolean)) actual[relative(root, file).replaceAll('\\', '/')] = counts
  }
  assert.deepEqual(actual, codecUses, 'Uso novo do codec exige revisão SEG.')
  for (const [file, site] of injectionSites) assert.equal(readFileSync(join(root, file), 'utf8').split(site).length - 1, 1, file)
  // Imports, comments and property accesses are excluded; bare safeStorage is a forwarding site.
  const forwards: Record<string, number> = {}
  for (const file of allowed) {
    const source = readFileSync(join(root, file), 'utf8').replace(/\/\*[\s\S]*?\*\/|\/\/[^\n]*/g, '').replace(/import[\s\S]*?from\s*['"][^'"]+['"]/g, '')
    const n = (source.match(/\bsafeStorage\b(?!\s*\.)/g) ?? []).length
    if (n) forwards[file] = n
  }
  assert.deepEqual(forwards, { 'fork-voz/registrar.ts': 1, 'index.ts': 1 })
})
