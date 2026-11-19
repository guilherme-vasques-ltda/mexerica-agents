import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { seedProfiles, sshAliasExists, sshConfigHasHost } from './semente.ts'

const ALIAS = 'meu-hub'

test('Host exato: aceita formas válidas, recusa parecidos, curingas e comentários', () => {
  assert.equal(sshConfigHasHost('Host meu-hub\n  HostName 10.0.0.1\n', ALIAS), true)
  assert.equal(sshConfigHasHost('  host=outro "meu-hub"\n', ALIAS), true)
  assert.equal(sshConfigHasHost('HOST a b meu-hub\r\n', ALIAS), true)
  assert.equal(sshConfigHasHost('Host meu-hub-2\nHost meu-*\n# Host meu-hub\nHostName meu-hub\n', ALIAS), false)
  assert.equal(sshConfigHasHost('', ALIAS), false)
})

test('Include: caminho relativo a ~/.ssh, ~/ e curinga no nome', () => {
  const home = mkdtempSync(join(tmpdir(), 'semente-'))
  try {
    assert.equal(sshAliasExists(ALIAS, home), false, 'sem ~/.ssh/config')
    mkdirSync(join(home, '.ssh', 'config.d'), { recursive: true })
    writeFileSync(join(home, '.ssh', 'config'), 'Include config.d/*.conf\nHost outro\n')
    assert.equal(sshAliasExists(ALIAS, home), false)
    writeFileSync(join(home, '.ssh', 'config.d', 'hub.conf'), 'Host meu-hub\n')
    assert.equal(sshAliasExists(ALIAS, home), true)
    writeFileSync(join(home, '.ssh', 'config'), 'Include ~/.ssh/config.d/hub.conf\n')
    assert.equal(sshAliasExists(ALIAS, home), true)
    writeFileSync(join(home, '.ssh', 'config'), 'Include config\n') // laço: para no limite
    assert.equal(sshAliasExists(ALIAS, home), false)
  } finally { rmSync(home, { recursive: true, force: true }) }
})

test('(1) alias presente: um perfil do Hub, como antes', () => {
  const p = seedProfiles({ main: true, profilesFileExists: false, aliasExists: () => true, alias: ALIAS, port: 19422 })
  assert.equal(p?.length, 1)
  assert.equal(p![0].host, ALIAS)
  assert.equal(p![0].remotePort, 19422)
})

test('(2) alias ausente: nada criado (sem gravar nem conectar)', () => {
  assert.equal(seedProfiles({ main: true, profilesFileExists: false, aliasExists: () => false, alias: ALIAS, port: 19422 }), null)
})

test('(3) arquivo de perfis já existe ou build Dev: nada muda e o config nem é lido', () => {
  const never = () => { throw new Error('não deveria ler o config') }
  assert.equal(seedProfiles({ main: true, profilesFileExists: true, aliasExists: never, alias: ALIAS, port: 19422 }), null)
  assert.equal(seedProfiles({ main: false, profilesFileExists: false, aliasExists: never, alias: ALIAS, port: 19422 }), null)
})
