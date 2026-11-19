import assert from 'node:assert/strict'
import { test } from 'node:test'
import { chownSync, lstatSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, type Stats } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { privateDirectory, validatePrivateFile, writePrivateFile } from './fork-private-files.ts'

test('D520 B1 recusa link, dono impróprio e conserva arquivos privados', () => {
  const root = mkdtempSync(join(realpathSync(tmpdir()), 'mexerica-private-'))
  try {
    privateDirectory(root)
    const path = join(root, 'ssh_known_hosts')
    writePrivateFile(path, 'PRIMEIRO\n')
    writePrivateFile(path, 'SEGUNDO\n', true)
    assert.equal(readFileSync(path, 'utf8'), 'PRIMEIRO\nSEGUNDO\n')
    assert.equal(lstatSync(root).mode & 0o7777, 0o700)
    const meta = lstatSync(path)
    assert.equal(meta.mode & 0o7777, 0o600)
    assert.throws(() => validatePrivateFile(meta, meta.uid + 1))
    assert.throws(() => validatePrivateFile({ ...meta, isFile: () => true, nlink: 2 } as Stats))
    symlinkSync(path, join(root, 'link'))
    assert.throws(() => writePrivateFile(join(root, 'link'), 'RECUSADO'))
    symlinkSync(root, join(root, 'parent-link'))
    assert.throws(() => writePrivateFile(join(root, 'parent-link', 'novo'), 'RECUSADO'))
    assert.equal(readFileSync(path, 'utf8'), 'PRIMEIRO\nSEGUNDO\n')
    // Em executor root descartável, prova de dono real além do validador sintético acima.
    if (process.getuid?.() === 0) {
      const foreign = join(root, 'outro-dono')
      writePrivateFile(foreign, 'CONSERVAR')
      chownSync(foreign, 65534, lstatSync(foreign).gid)
      assert.throws(() => writePrivateFile(foreign, 'RECUSADO'))
      assert.equal(readFileSync(foreign, 'utf8'), 'CONSERVAR')
    }
  } finally { rmSync(root, { recursive: true, force: true }) }
})

test('D520 B1 modos explícitos independem de umask permissiva no subprocesso', async () => {
  const { spawnSync } = await import('node:child_process')
  const { pathToFileURL } = await import('node:url')
  const root = mkdtempSync(join(realpathSync(tmpdir()), 'mexerica-umask-'))
  try {
    const file = join(root, 'nested', 'token')
    const module = pathToFileURL(join(import.meta.dirname, 'fork-private-files.ts')).href
    const source = `process.umask(0); const {writePrivateFile}=await import(${JSON.stringify(module)}); writePrivateFile(process.argv[1], 'SINTETICO');`
    const result = spawnSync(process.execPath, ['--experimental-strip-types', '--input-type=module', '-e', source, file], { encoding: 'utf8' })
    assert.equal(result.status, 0, result.stderr)
    assert.equal(lstatSync(join(root, 'nested')).mode & 0o7777, 0o700)
    assert.equal(lstatSync(file).mode & 0o7777, 0o600)
  } finally { rmSync(root, { recursive: true, force: true }) }
})
