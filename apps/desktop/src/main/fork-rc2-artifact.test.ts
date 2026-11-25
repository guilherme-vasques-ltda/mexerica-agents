import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { chmodSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import type { Rc2Component } from '../shared/fork-rc2-profile.ts'
import { rc2PathOverlaps, verifyRc2Artifact, verifyRc2StorageRoots } from './fork-rc2-artifact.ts'

const macho = (cputype: number, payload = 'fixture') =>
  Buffer.concat([Buffer.from([0xcf, 0xfa, 0xed, 0xfe, cputype, 0x00, 0x00, 0x01]), Buffer.alloc(24), Buffer.from(payload)])
const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex')
const hubBytes = macho(0x0c)
const helperBytes = macho(0x0c, 'speech')

function inventory(root: string, hub: Rc2Component, helper?: Rc2Component): { root: string; components: Rc2Component[] } {
  mkdirSync(join(root, 'rc2-hub'), { recursive: true })
  writeFileSync(join(root, hub.path), hubBytes)
  const components = [hub]
  if (helper) {
    writeFileSync(join(root, helper.path), helperBytes)
    components.push(helper)
  }
  return { root, components }
}

function fixture(t: object, hubPayload: (path: string) => void = () => {}) {
  const root = mkdtempSync(join(tmpdir(), 'rc2-artifact-'))
  ;(t as { after(clean: () => void): void }).after(() => rmSync(root, { recursive: true, force: true }))
  const hub: Rc2Component = { role: 'hub', path: 'rc2-hub/donnabot-host', arch: 'arm64', sha256: sha256(hubBytes), cdhash: 'ab'.repeat(20) }
  const helper: Rc2Component = { role: 'helper', path: 'codync-speech', arch: 'arm64', sha256: sha256(helperBytes), cdhash: 'cd'.repeat(20) }
  const staged = inventory(root, hub, helper)
  hubPayload(join(root, hub.path))
  return { ...staged, hub, helper }
}

test('verifica Hub e helper declarados no manifesto antes de liberar o caminho', t => {
  const { root, components } = fixture(t)
  assert.deepEqual(verifyRc2Artifact(root, components), ['rc2-hub/donnabot-host', 'codync-speech'])
})

test('binário ausente, trocado, link, slice errado ou diretório no lugar do arquivo são recusados', t => {
  for (const tamper of [
    (path: string) => rmSync(path),
    (path: string) => writeFileSync(path, Buffer.concat([hubBytes, Buffer.from('trocado')])),
    (path: string) => writeFileSync(path, macho(0x07)),
    (path: string) => writeFileSync(path, Buffer.from([0xcf, 0xfa])),
    (path: string) => { rmSync(path); symlinkSync('alvo-invalido', path) },
    (path: string) => { rmSync(path); mkdirSync(path) },
  ]) {
    const { root, components } = fixture(t, tamper)
    assert.throws(() => verifyRc2Artifact(root, components), /invalid_rc2_artifact/)
  }
})

test('hash divergente de qualquer componente declarado recusa o artefato inteiro', t => {
  const { root, hub, helper } = fixture(t)
  const altered: Rc2Component = { ...helper, sha256: sha256(macho(0x0c, 'outra')) }
  assert.throws(() => verifyRc2Artifact(root, [hub, altered]), /invalid_rc2_artifact/)
  assert.throws(() => verifyRc2Artifact(root, [hub, { ...altered, role: 'hub' }]), /invalid_rc2_artifact/)
  assert.deepEqual(verifyRc2Artifact(root, [hub, helper]), [hub.path, helper.path])
})

test('pasta intermediária simbólica que sai da raiz é recusada mesmo com binário válido fora dela', t => {
  const outside = mkdtempSync(join(tmpdir(), 'rc2-fora-'))
  t.after(() => rmSync(outside, { recursive: true, force: true }))
  mkdirSync(join(outside, 'rc2-hub'))
  writeFileSync(join(outside, 'rc2-hub', 'donnabot-host'), hubBytes)
  const { root, hub } = fixture(t)
  rmSync(join(root, 'rc2-hub'), { recursive: true })
  symlinkSync(join(outside, 'rc2-hub'), join(root, 'rc2-hub'))
  assert.throws(() => verifyRc2Artifact(root, [hub]), /invalid_rc2_artifact/)
})

test('tabela de casos da guarda de raízes: um focal por linha', t => {
  const home = mkdtempSync(join(tmpdir(), 'rc2-raiz-home-'))
  const appData = mkdtempSync(join(tmpdir(), 'rc2-raiz-appdata-'))
  t.after(() => { rmSync(home, { recursive: true, force: true }); rmSync(appData, { recursive: true, force: true }) })
  // ausente: nada existe sob as âncoras, nada a verificar no disco
  verifyRc2StorageRoots(home, appData)
  // ausente parcial: raiz existente com subcaminho ainda não criado
  mkdirSync(join(appData, 'Mexerica RC 2'))
  verifyRc2StorageRoots(home, appData)
  // sobreposição nos dois sentidos por link: raiz rc.2 apontando para dentro da rc.1
  const dentro = join(appData, 'Mexerica RC', 'dentro')
  mkdirSync(join(appData, 'Mexerica RC'), { recursive: true })
  mkdirSync(dentro)
  rmSync(join(appData, 'Mexerica RC 2'), { recursive: true })
  symlinkSync(dentro, join(appData, 'Mexerica RC 2'))
  assert.throws(() => verifyRc2StorageRoots(home, appData), /invalid_rc2_artifact/)
  // sobreposição no sentido inverso: raiz rc.1 apontando para dentro da rc.2
  rmSync(join(appData, 'Mexerica RC 2'))
  rmSync(join(appData, 'Mexerica RC'), { recursive: true })
  mkdirSync(join(appData, 'Mexerica RC 2'))
  symlinkSync(join(appData, 'Mexerica RC 2'), join(appData, 'Mexerica RC'))
  assert.throws(() => verifyRc2StorageRoots(home, appData), /invalid_rc2_artifact/)
  // raiz rc.1 por link recusa mesmo apontando para lugar neutro
  rmSync(join(appData, 'Mexerica RC'))
  const neutro = mkdtempSync(join(tmpdir(), 'rc2-neutro-'))
  t.after(() => rmSync(neutro, { recursive: true, force: true }))
  symlinkSync(neutro, join(appData, 'Mexerica RC'))
  assert.throws(() => verifyRc2StorageRoots(home, appData), /invalid_rc2_artifact/)
  rmSync(join(appData, 'Mexerica RC'))
})

test('permissão negada em componente existente recusa em vez de tratar como ausente', t => {
  if (process.geteuid && process.geteuid() === 0) return
  const home = mkdtempSync(join(tmpdir(), 'rc2-raiz-home-'))
  const appData = mkdtempSync(join(tmpdir(), 'rc2-raiz-appdata-'))
  t.after(() => { chmodSync(appData, 0o700); rmSync(home, { recursive: true, force: true }); rmSync(appData, { recursive: true, force: true }) })
  mkdirSync(join(appData, 'Mexerica RC 2'))
  chmodSync(appData, 0o000)
  assert.throws(() => verifyRc2StorageRoots(home, appData), /invalid_rc2_artifact/)
})

test('erro de I/O que não é ausência recusa a guarda', () => {
  const home = join(tmpdir(), 'a'.repeat(300))
  assert.throws(() => verifyRc2StorageRoots(home, join(tmpdir(), 'rc2-raiz-appdata-')), /invalid_rc2_artifact/)
})

test('sobreposição compara com caixa dobrada para volumes insensíveis', () => {
  assert.equal(rc2PathOverlaps('/raiz/dados', '/RAIZ/Dados'), true)
  assert.equal(rc2PathOverlaps('/raiz/dados/sub', '/RAIZ/Dados'), true)
  assert.equal(rc2PathOverlaps('/raiz/dados 2', '/RAIZ/Dados'), false)
  assert.equal(rc2PathOverlaps('/raiz/outra coisa', '/RAIZ/Dados'), false)
})
