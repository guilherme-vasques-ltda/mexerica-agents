import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs'
import { chmodSync, fstatSync, mkdirSync, mkdtempSync, realpathSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import * as path from 'node:path'
import { test } from 'node:test'
import { loadMainModule } from '../shared/test-support/main-module.ts'
import { assinarArm64 } from '../shared/test-support/rc2-assinatura.ts'
import type { Rc2Component } from '../shared/fork-rc2-profile.ts'
import { rc2PathOverlaps, verifyRc2Artifact, verifyRc2StorageRoots } from './fork-rc2-artifact.ts'

const hubAssinado = assinarArm64('fixture do hub rc.2 para o gate de artefato')
const helperAssinado = assinarArm64('fixture do helper de voz rc.2', { ident: 'fixture.speech.invalid.example' })

function inventory(root: string, hub: Rc2Component, helper?: Rc2Component): { root: string, components: Rc2Component[] } {
  mkdirSync(join(root, 'rc2-hub'), { recursive: true })
  writeFileSync(join(root, hub.path), hubAssinado.bytes)
  const components = [hub]
  if (helper) {
    writeFileSync(join(root, helper.path), helperAssinado.bytes)
    components.push(helper)
  }
  return { root, components }
}

function fixture(t: object, hubPayload: (path: string) => void = () => {}) {
  const root = mkdtempSync(join(realpathSync(tmpdir()), 'rc2-artifact-'))
  ;(t as { after(clean: () => void): void }).after(() => rmSync(root, { recursive: true, force: true }))
  const hub: Rc2Component = { role: 'hub', path: 'rc2-hub/donnabot-host', arch: 'arm64', sha256: hubAssinado.sha256, cdhash: hubAssinado.cdhash }
  const helper: Rc2Component = { role: 'helper', path: 'codync-speech', arch: 'arm64', sha256: helperAssinado.sha256, cdhash: helperAssinado.cdhash }
  const staged = inventory(root, hub, helper)
  hubPayload(join(root, hub.path))
  return { ...staged, hub, helper }
}

test('verifica Hub e helper declarados no manifesto e devolve descritores fixados de cada papel', t => {
  const { root, components } = fixture(t)
  const verified = verifyRc2Artifact(root, components)
  assert.deepEqual(verified.components.map(({ role, path: pathOf }) => [role, pathOf]),
    [['hub', 'rc2-hub/donnabot-host'], ['helper', 'codync-speech']])
  for (const component of verified.components) {
    assert.equal(typeof component.fd, 'number')
    assert.ok(fstatSync(component.fd).isFile())
  }
  verified.close()
  for (const component of verified.components) assert.throws(() => fstatSync(component.fd))
})

test('binário ausente, trocado, link, slice errado ou diretório no lugar do arquivo são recusados', t => {
  for (const tamper of [
    (path: string) => rmSync(path),
    (path: string) => writeFileSync(path, Buffer.concat([hubAssinado.bytes, Buffer.from('trocado')])),
    (path: string) => writeFileSync(path, Buffer.concat([Buffer.from([0xcf, 0xfa, 0xed, 0xfe, 0x07, 0x00, 0x00, 0x01]), hubAssinado.bytes.subarray(8)])),
    (path: string) => writeFileSync(path, Buffer.from([0xcf, 0xfa])),
    (path: string) => { rmSync(path); symlinkSync('alvo-invalido', path) },
    (path: string) => { rmSync(path); mkdirSync(path) },
    (path: string) => writeFileSync(path, assinarArm64('corpo divergente do pin do manifesto').bytes),
  ]) {
    const { root, components } = fixture(t, tamper)
    assert.throws(() => verifyRc2Artifact(root, components), /invalid_rc2_artifact/)
  }
})

test('recusa fecha o descritor do componente que falhou e recusas repetidas não acumulam', t => {
  const { root, hub, helper } = fixture(t)
  const alterado: Rc2Component = { ...helper, sha256: assinarArm64('outro corpo').sha256 }
  const closed: number[] = []
  const opened: number[] = []
  const verify = loadMainModule<{ verifyRc2Artifact: typeof verifyRc2Artifact }>('fork-rc2-artifact', {
    'node:fs': { ...fs, openSync: (path: string, flags?: string | number | null) => { const fd = fs.openSync(path, flags ?? 'r'); opened.push(fd); return fd },
      closeSync: (fd: number) => { closed.push(fd); fs.closeSync(fd) } },
    'node:path': path,
    'node:crypto': crypto,
  }, { Buffer }).verifyRc2Artifact
  for (const _ of [1, 2, 3]) {
    assert.throws(() => verify(root, [hub, alterado]), /invalid_rc2_artifact/)
  }
  assert.equal(opened.length, 6, 'cada recusa abriu exatamente os dois componentes')
  assert.equal(closed.length, 6, 'toda abertura foi fechada: o descritor do componente que falhou também')
  const verified = verify(root, [hub, helper])
  assert.equal(closed.length, 6, 'o caminho feliz não fecha nada antes do close explícito')
  verified.close()
  assert.equal(closed.length, 8, 'o close explícito fecha os dois descritores verificados')
})

test('descritor fixado continua lendo os bytes verificados depois de trocar o arquivo no caminho', t => {
  const { root, components } = fixture(t)
  const verified = verifyRc2Artifact(root, components)
  const hub = verified.components.find(component => component.role === 'hub')!
  const substituto = join(root, 'rc2-hub', 'substituto')
  writeFileSync(substituto, assinarArm64('bytes substitutos no mesmo caminho').bytes)
  renameSync(substituto, join(root, 'rc2-hub', 'donnabot-host'))
  const lido = Buffer.alloc(hubAssinado.bytes.length)
  fs.readSync(hub.fd, lido, 0, lido.length, 0)
  assert.ok(lido.equals(hubAssinado.bytes))
  assert.ok(!fs.readFileSync(join(root, 'rc2-hub', 'donnabot-host')).equals(hubAssinado.bytes))
  verified.close()
})

test('verificação abre cada componente exatamente uma vez e não relê caminho depois de fixar', t => {
  const { root, components } = fixture(t)
  const opens: string[] = []
  const reads: string[] = []
  const countingFs = new Proxy(fs, {
    get(target, key: string) {
      if (key === 'openSync') return (path: string, ...rest: unknown[]) => { opens.push(String(path)); return (target as never as Record<string, (path: string, ...rest: unknown[]) => number>)[key](path, ...rest) }
      if (key === 'readFileSync') return (path: string | number, ...rest: unknown[]) => {
        reads.push(String(path))
        return (target as never as Record<string, (path: string | number, ...rest: unknown[]) => Buffer>)[key](path, ...rest)
      }
      return (target as Record<string, unknown>)[key]
    },
  })
  const verify = loadMainModule<{ verifyRc2Artifact: typeof verifyRc2Artifact }>('fork-rc2-artifact', {
    'node:fs': countingFs, 'node:path': path, 'node:crypto': { createHash },
  }, { Buffer }).verifyRc2Artifact
  const verified = verify(root, components)
  const hubPath = join(root, components[0].path)
  const helperPath = join(root, components[1].path)
  assert.deepEqual(opens, [hubPath, helperPath])
  assert.ok(reads.filter(read => read === hubPath || read === helperPath).length === 0, 'nenhuma leitura por caminho após a abertura')
  verified.close()
})

test('troca na janela entre inspeção e abertura é detectada pela divergência do descritor', t => {
  const { root, components } = fixture(t)
  const hubPath = join(root, components[0].path)
  const real = fs.lstatSync(hubPath)
  const comPrototipo = (stat: object, mudanca: object) =>
    Object.assign(Object.create(Object.getPrototypeOf(real)), stat, mudanca)
  const divergente = comPrototipo(real, { ino: real.ino + 1 })
  const fstat = { ...fs, fstatSync: (fd: number) => comPrototipo(fs.fstatSync(fd), {}) }
  const verifyDivergente = loadMainModule<{ verifyRc2Artifact: typeof verifyRc2Artifact }>('fork-rc2-artifact', {
    'node:fs': { ...fs, fstatSync: () => divergente }, 'node:path': path, 'node:crypto': crypto,
  }, { Buffer }).verifyRc2Artifact
  assert.throws(() => verifyDivergente(root, components), /invalid_rc2_artifact/)
  // controle pelo mesmo caminho de simulação: sem divergência, o fstat preservado é aceito
  const verifyIgual = loadMainModule<{ verifyRc2Artifact: typeof verifyRc2Artifact }>('fork-rc2-artifact', {
    'node:fs': fstat, 'node:path': path, 'node:crypto': crypto,
  }, { Buffer }).verifyRc2Artifact
  verifyIgual(root, components).close()
})

test('divergência isolada de dev no fstat é recusada com ino e size iguais', t => {
  const { root, components } = fixture(t)
  const hubPath = join(root, components[0].path)
  const real = fs.lstatSync(hubPath)
  const fstat = (fd: number) => Object.assign(Object.create(Object.getPrototypeOf(real)), fs.fstatSync(fd))
  const verify = loadMainModule<{ verifyRc2Artifact: typeof verifyRc2Artifact }>('fork-rc2-artifact', {
    'node:fs': { ...fs, fstatSync: (fd: number) => Object.assign(fstat(fd), { dev: real.dev + 1 }) },
    'node:path': path, 'node:crypto': crypto,
  }, { Buffer }).verifyRc2Artifact
  assert.throws(() => verify(root, components), /invalid_rc2_artifact/)
  const verifyIgual = loadMainModule<{ verifyRc2Artifact: typeof verifyRc2Artifact }>('fork-rc2-artifact', {
    'node:fs': { ...fs, fstatSync: fstat }, 'node:path': path, 'node:crypto': crypto,
  }, { Buffer }).verifyRc2Artifact
  verifyIgual(root, components).close()
})

test('tamanho divergente entre inspeção e descritor é recusado mesmo com inode igual', t => {
  const { root, components } = fixture(t)
  const hubPath = join(root, components[0].path)
  const real = fs.lstatSync(hubPath)
  const fstatCom = (mudar: (base: object) => object) => (fd: number) => {
    const stat = fs.fstatSync(fd)
    const base = Object.assign(Object.create(Object.getPrototypeOf(real)), stat)
    return stat.ino === real.ino ? Object.assign(base, mudar(base)) : base
  }
  const carregar = (fstat: typeof fs.fstatSync) =>
    loadMainModule<{ verifyRc2Artifact: typeof verifyRc2Artifact }>('fork-rc2-artifact', {
      'node:fs': { ...fs, fstatSync: fstat }, 'node:path': path, 'node:crypto': crypto,
    }, { Buffer }).verifyRc2Artifact
  assert.throws(() => carregar(fstatCom(() => ({ size: real.size + 1 })))(root, components), /invalid_rc2_artifact/)
  carregar(fstatCom(() => ({})))(root, components).close()
})

test('cdhash errado com sha256 correto recusa depois de ler o descritor, não antes', t => {
  const { root, hub } = fixture(t)
  const outro = assinarArm64('corpo diferente do pin de cdhash')
  writeFileSync(join(root, hub.path), outro.bytes)
  const reads: string[] = []
  const countingFs = new Proxy(fs, { get(target, key: string) {
    if (key === 'readFileSync') return (alvo: string | number, ...rest: unknown[]) => {
      reads.push(String(alvo))
      return (target as never as Record<string, (alvo: string | number, ...rest: unknown[]) => Buffer>)[key](alvo, ...rest)
    }
    return (target as Record<string, unknown>)[key]
  } })
  const verify = loadMainModule<{ verifyRc2Artifact: typeof verifyRc2Artifact }>('fork-rc2-artifact', {
    'node:fs': countingFs, 'node:path': path, 'node:crypto': crypto,
  }, { Buffer }).verifyRc2Artifact
  const pinErrado: Rc2Component = { ...hub, sha256: outro.sha256, cdhash: hubAssinado.cdhash }
  assert.throws(() => verify(root, [pinErrado]), /invalid_rc2_artifact/)
  assert.ok(/^\d+$/.test(reads[reads.length - 1]), 'o sha256 coerente foi lido pelo descritor antes da recusa de cdhash')
})

test('hash divergente de qualquer componente declarado recusa o artefato inteiro', t => {
  const { root, hub, helper } = fixture(t)
  const altered: Rc2Component = { ...helper, sha256: assinarArm64('outra').sha256 }
  assert.throws(() => verifyRc2Artifact(root, [hub, altered]), /invalid_rc2_artifact/)
  assert.throws(() => verifyRc2Artifact(root, [hub, { ...altered, role: 'hub' }]), /invalid_rc2_artifact/)
  verifyRc2Artifact(root, [hub, helper]).close()
})

test('hash correto não autoriza x64, binário universal nem cabeçalho incompleto', t => {
  const { root, hub } = fixture(t)
  const alterado = assinarArm64('corpo universal')
  const universal = Buffer.alloc(32)
  universal.writeUInt32BE(0xcafebabe, 0)
  universal.writeUInt32BE(1, 4)
  universal.writeUInt32BE(0x0100000c, 8)
  universal.writeUInt32BE(universal.length, 16)
  universal.writeUInt32BE(alterado.bytes.length, 20)
  for (const [label, bytes] of [
    ['x64', Buffer.concat([Buffer.from([0xcf, 0xfa, 0xed, 0xfe, 0x07, 0x00, 0x00, 0x01]), alterado.bytes.subarray(8)])],
    ['universal', Buffer.concat([universal, alterado.bytes])],
    ...[8, 16, 31].map(length => [`cabeçalho de ${length} bytes`, alterado.bytes.subarray(0, length)] as [string, Buffer]),
  ] as Array<[string, Buffer]>) {
    writeFileSync(join(root, hub.path), bytes)
    assert.throws(() => verifyRc2Artifact(root, [{ ...hub, sha256: createHash('sha256').update(bytes).digest('hex') }]), /invalid_rc2_artifact/, label)
  }
})

test('pasta intermediária simbólica que sai da raiz é recusada mesmo com binário válido fora dela', t => {
  const outside = mkdtempSync(join(tmpdir(), 'rc2-fora-'))
  t.after(() => rmSync(outside, { recursive: true, force: true }))
  mkdirSync(join(outside, 'rc2-hub'))
  writeFileSync(join(outside, 'rc2-hub', 'donnabot-host'), hubAssinado.bytes)
  const { root, hub } = fixture(t)
  rmSync(join(root, 'rc2-hub'), { recursive: true })
  symlinkSync(join(outside, 'rc2-hub'), join(root, 'rc2-hub'))
  assert.throws(() => verifyRc2Artifact(root, [hub]), /invalid_rc2_artifact/)
})

test('tabela de casos da guarda de raízes: um focal por linha', t => {
  const home = mkdtempSync(join(realpathSync(tmpdir()), 'rc2-raiz-home-'))
  const appData = mkdtempSync(join(realpathSync(tmpdir()), 'rc2-raiz-appdata-'))
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
  const home = mkdtempSync(join(realpathSync(tmpdir()), 'rc2-raiz-home-'))
  const appData = mkdtempSync(join(realpathSync(tmpdir()), 'rc2-raiz-appdata-'))
  t.after(() => { chmodSync(appData, 0o700); rmSync(home, { recursive: true, force: true }); rmSync(appData, { recursive: true, force: true }) })
  mkdirSync(join(appData, 'Mexerica RC 2'))
  chmodSync(appData, 0o000)
  assert.throws(() => verifyRc2StorageRoots(home, appData), /invalid_rc2_artifact/)
})

function loadStorageGuard(overrides: Partial<typeof fs>) {
  return loadMainModule<{ verifyRc2StorageRoots: typeof verifyRc2StorageRoots }>('fork-rc2-artifact', {
    'node:fs': { ...fs, ...overrides }, 'node:path': path, 'node:crypto': crypto,
  }).verifyRc2StorageRoots
}

test('erro de I/O que não é ausência recusa a guarda', t => {
  const { root } = fixture(t)
  const io = Object.assign(new Error('fixture'), { code: 'EIO' })
  const verify = loadStorageGuard({ lstatSync: () => { throw io } })
  assert.throws(() => verify(root, root), /invalid_rc2_artifact/)
})

test('recusa âncora simbólica mesmo quando a futura raiz da rc.2 ainda não existe', t => {
  const { root } = fixture(t)
  const home = join(root, 'conta')
  const old = join(home, '.mexerica-rc')
  mkdirSync(old, { recursive: true })
  const alias = join(root, 'alias')
  symlinkSync(old, alias, 'dir')
  assert.throws(() => verifyRc2StorageRoots(home, alias), /invalid_rc2_artifact/)
})

test('recusa aninhamento nos dois sentidos sem links nos caminhos', t => {
  const { root } = fixture(t)
  const old = join(root, '.mexerica-rc')
  const candidate = join(root, 'Mexerica RC 2')
  mkdirSync(old)
  mkdirSync(candidate)
  assert.throws(() => verifyRc2StorageRoots(root, old), /invalid_rc2_artifact/, 'rc.2 dentro da rc.1')
  assert.throws(() => verifyRc2StorageRoots(candidate, root), /invalid_rc2_artifact/, 'rc.1 dentro da rc.2')
})

test('compara os caminhos resolvidos dos dois lados, mesmo com raízes ainda ausentes', t => {
  const { root } = fixture(t)
  const canonical = realpathSync(root)
  const first = join(canonical, 'primeira')
  const second = join(canonical, 'segunda')
  mkdirSync(first)
  mkdirSync(second)
  for (const [anchor, resolved] of [
    [first, join(second, 'Mexerica RC 2')],
    [second, join(first, '.mexerica-rc')],
  ]) {
    const verify = loadStorageGuard({ realpathSync: ((entry: string) => entry === anchor ? resolved : realpathSync(entry)) as typeof realpathSync })
    assert.throws(() => verify(first, second), /invalid_rc2_artifact/)
  }
})

test('sobreposição compara com caixa dobrada para volumes insensíveis', () => {
  const base = join('RAIZ', 'Dados')
  assert.equal(rc2PathOverlaps(join('raiz', 'dados'), base), true)
  assert.equal(rc2PathOverlaps(join('raiz', 'dados', 'sub'), base), true)
  assert.equal(rc2PathOverlaps(base, join('raiz', 'dados', 'sub')), true)
  assert.equal(rc2PathOverlaps(join('raiz', 'dados 2'), base), false)
  assert.equal(rc2PathOverlaps(join('raiz', 'outra coisa'), base), false)
})

test('caminho resolvido fora da raiz é recusado mesmo com caminhada limpa', t => {
  const { root, components } = fixture(t)
  const hubPath = join(root, components[0].path)
  const fora = join(root, 'fora-da-raiz')
  const carregar = (realpath: (entry: string) => string) =>
    loadMainModule<{ verifyRc2Artifact: typeof verifyRc2Artifact }>('fork-rc2-artifact', {
      'node:fs': { ...fs, realpathSync: realpath }, 'node:path': path, 'node:crypto': crypto,
    }, { Buffer }).verifyRc2Artifact
  assert.throws(() => carregar(entry => entry === hubPath ? fora : fs.realpathSync(entry))(root, components), /invalid_rc2_artifact/)
  // controle: o mesmo canal de simulação com realpath intacto aceita
  carregar(entry => fs.realpathSync(entry))(root, components).close()
})
