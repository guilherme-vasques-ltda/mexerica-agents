import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'
import * as fsModule from 'node:fs'
import * as pathModule from 'node:path'
import * as cryptoModule from 'node:crypto'
import * as rcProfileModule from '../shared/fork-rc-profile.ts'
import { assinarArm64 } from '../shared/test-support/rc2-assinatura.ts'
import { rcPaths } from '../shared/fork-rc-profile.ts'
import * as profileModule from '../shared/fork-rc2-profile.ts'
import { compileRc2Profile, rc2ManifestPath, rc2Paths, rc2PathsIsolated, rc2Components, validateRc2Bootstrap } from '../shared/fork-rc2-profile.ts'
import { Rc2HubLifecycle, type Rc2ChildHandle, type Rc2Connection, type Rc2Schedule } from './fork-rc2-local-hub.ts'

const mode = 'official-rc2'
const version = '0.0.1-rc.2'
const target = { platform: 'darwin', arch: 'arm64' }
const profile = {
  profile: 'rc2-local', version, appId: 'app.mexerica.mac.rc2',
  name: 'Mexerica RC 2', scheme: 'mexerica-rc2', packageName: 'mexerica-rc2',
}
const hubAssinado = assinarArm64('fixture do hub rc.2 da entrada empacotada')
const hubBytes = hubAssinado.bytes
const hubSha256 = hubAssinado.sha256
const components = [{ role: 'hub', path: 'donnabot-host', arch: 'arm64', sha256: hubSha256, cdhash: hubAssinado.cdhash }]

test('só o modo explícito compila o perfil local para a candidata exata em ARM64', () => {
  for (const other of ['official-rc', 'official', 'open', 'development']) {
    assert.equal(compileRc2Profile(other, version, target, components), null)
  }
  assert.deepEqual(compileRc2Profile(mode, version, target, components)?.distribution, profile)
  for (const other of [
    { ...target, arch: 'x64' }, { ...target, platform: 'linux' }, { ...target, platform: 'win32' },
  ]) assert.throws(() => compileRc2Profile(mode, version, other, components), /invalid_rc2_profile/)
  for (const other of ['0.0.1', '0.0.1-rc.1', '0.0.1-rc.3', '0.1.0-rc.2', `${version}\n`]) {
    assert.throws(() => compileRc2Profile(mode, other, target, components), /invalid_rc2_profile/)
  }
})

test('inventário fechado: exige Hub único, caminhos relativos e hashes hexadecimais exatos', () => {
  const bad = [
    [], [{}], { role: 'hub' }, 'hub', null,
    [{ ...components[0], role: 'helper' }],
    [{ ...components[0], sha256: hubSha256.toUpperCase() }],
    [{ ...components[0], cdhash: 'cd'.repeat(19) }],
    [{ ...components[0], arch: 'x64' }],
    [{ ...components[0], path: 'donnabot-host ' }],
    [{ ...components[0], path: '/donnabot-host' }],
    [{ ...components[0], path: '../donnabot-host' }],
    [{ ...components[0], path: 'donnabot-host/../../host' }],
    [{ ...components[0], sha256: '0'.repeat(63) }],
    [...components, { ...components[0], role: 'helper' }],
    [...components, { ...components[0], role: 'helper', path: components[0].path.toUpperCase() }],
    [...components, { ...components[0], path: 'outro-hub', role: 'executor' }],
    [...components, { role: 'helper', path: 'codync-speech', arch: 'arm64', sha256: hubSha256, cdhash: 'cd'.repeat(20) },
      { role: 'helper', path: 'outro-helper', arch: 'arm64', sha256: hubSha256, cdhash: 'cd'.repeat(20) }],
  ]
  for (const inventory of bad) assert.throws(() => rc2Components(inventory), /invalid_rc2_profile/)
  const helper = [{ role: 'helper', path: 'codync-speech', arch: 'arm64', sha256: hubSha256, cdhash: 'cd'.repeat(20) }]
  assert.deepEqual(rc2Components([...components, ...helper]).map(component => component.role), ['hub', 'helper'])
})

test('bootstrap exige pin compilado, metadados coerentes, inventário válido e bytes exatos antes de admitir o perfil', () => {
  const compiled = compileRc2Profile(mode, version, target, components)!
  const metadata = { mexericaProfile: profile.profile, mexericaAppId: profile.appId, name: profile.packageName,
    productName: profile.name, version }
  let reads = 0
  const bootstrap = {
    ...target, packaged: true, name: profile.name, version, compiledProfile: profile.profile, compiledPin: compiled.pin,
    readMetadata: (): unknown => metadata, readManifest: () => { reads++; return compiled.bytes },
  }
  for (const invalid of [
    { packaged: false }, { arch: 'x64' }, { platform: 'linux' }, { compiledPin: undefined },
    { compiledProfile: 'rc' }, { name: 'Mexerica RC' }, { version: '0.0.1-rc.1' },
    { readMetadata: () => ({ ...metadata, mexericaProfile: 'rc' }) },
    { readMetadata: () => ({ ...metadata, mexericaAppId: 'app.donnabot.mac.rc' }) },
  ]) assert.throws(() => validateRc2Bootstrap({ ...bootstrap, ...invalid }), /invalid_rc2_profile/)
  assert.equal(reads, 0)
  assert.throws(() => validateRc2Bootstrap({ ...bootstrap, readManifest: () => Buffer.concat([compiled.bytes, Buffer.from('\n')]) }), /invalid_rc2_profile/)
  assert.throws(() => validateRc2Bootstrap({ ...bootstrap, compiledPin: '0'.repeat(64) }), /invalid_rc2_profile/)
  const legacy = Buffer.from(`${JSON.stringify(profile)}\n`)
  assert.throws(() => validateRc2Bootstrap({ ...bootstrap, compiledPin: createHash('sha256').update(legacy).digest('hex'),
    readManifest: () => legacy }), /invalid_rc2_profile/)
  assert.deepEqual(validateRc2Bootstrap(bootstrap), { ...profile, platform: 'darwin', arch: 'arm64', components })
})

test('caminhos da rc.2 ficam separados dos da rc.1 em qualquer papel de usuário', () => {
  const home = '/fixture/home'
  const appData = '/fixture/appdata'
  const userData = join(appData, profile.name)
  assert.deepEqual(rc2Paths(home, appData), {
    userData,
    sessionData: join(userData, 'session'), cache: join(userData, 'cache'), logs: join(userData, 'logs'),
    crashDumps: join(userData, 'crashDumps'), temp: join(userData, 'tmp'),
    hubRoot: join(home, '.mexerica-rc2'),
  })
  assert.equal(rc2PathsIsolated(home, appData), true)
  assert.equal(rc2PathsIsolated(home, join(home, '.mexerica-rc')), false)
  assert.equal(rc2PathsIsolated(join(appData, 'Mexerica RC'), appData), false)
  for (const path of Object.values(rcPaths(home, appData))) {
    assert.ok(!path.includes('rc2'))
  }
  for (const path of Object.values(rc2Paths(home, appData))) {
    assert.ok(!path.includes('.mexerica-rc') || path.includes('.mexerica-rc2'))
  }
})

function source(url: string, imports: Record<string, unknown>, globals: Record<string, unknown> = {}) {
  const output = ts.transpileModule(readFileSync(new URL(url, import.meta.url), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023 },
  }).outputText
  const exports: Record<string, unknown> = {}
  runInNewContext(output, {
    exports, ...globals, require: (name: string) => Object.hasOwn(imports, name) ? imports[name] : assert.fail(`unexpected import: ${name}`),
  })
  return exports
}

test('configuração de compilação segregada produz só entrada inerte e o manifesto fixado com o inventário', () => {
  type Artifact = { type: string; fileName: string; source: Buffer }
  type Section = { define: Record<string, string>; build: { outDir: string; rollupOptions: { input: Record<string, string> } };
    plugins: { generateBundle(this: { emitFile(artifact: Artifact): void }): void }[] }
  const attempt = (platform: string, arch: string, mode: string, candidate: string, inventory: unknown = components) =>
    source('../../electron.vite.rc2.config.ts', {
      'electron-vite': { defineConfig: (config: (input: { mode: string }) => unknown) => config({ mode }) },
      'node:fs': { readFileSync: (path: string) => {
        if (path === 'package.json') return JSON.stringify({ version: candidate })
        if (path === 'build/official-rc2/components.json') return JSON.stringify(inventory)
        return assert.fail(`unexpected read: ${path}`)
      } },
      'node:path': { resolve: (path: string) => path },
      './src/shared/fork-rc2-profile.ts': profileModule,
    }, { process: { platform, arch } }).default as Record<string, Section>
  assert.throws(() => attempt('darwin', 'arm64', 'official-rc', version), /invalid_rc2_profile/)
  assert.throws(() => attempt('darwin', 'x64', mode, version), /invalid_rc2_profile/)
  assert.throws(() => attempt('darwin', 'arm64', mode, '0.0.1-rc.1'), /invalid_rc2_profile/)
  for (const platform of ['linux', 'win32']) assert.throws(() => attempt(platform, 'arm64', mode, version), /invalid_rc2_profile/)
  assert.throws(() => attempt('darwin', 'arm64', mode, version, []), /invalid_rc2_profile/)
  assert.throws(() => attempt('darwin', 'arm64', mode, version, [{ ...components[0], sha256: '0'.repeat(63) }]), /invalid_rc2_profile/)
  const config = attempt('darwin', 'arm64', mode, version)
  assert.deepEqual(Object.keys(config), ['main'])
  assert.deepEqual(Object.entries(config.main.build.rollupOptions.input), [['rc2', 'src/main/fork-rc2-entry.ts']])
  assert.equal(config.main.build.outDir, 'build/official-rc2/out/main')
  const emitted: Artifact[] = []
  config.main.plugins[0].generateBundle.call({ emitFile: artifact => emitted.push(artifact) })
  const compiled = compileRc2Profile(mode, version, target, components)!
  assert.equal(config.main.define.__MEXERICA_RC2_PROFILE__, JSON.stringify(profile.profile))
  assert.equal(config.main.define.__MEXERICA_RC2_MANIFEST_HASH__, JSON.stringify(compiled.pin))
  assert.equal(emitted.length, 1)
  assert.equal(emitted[0].type, 'asset')
  assert.equal(emitted[0].fileName, rc2ManifestPath)
  assert.ok(emitted[0].source.equals(compiled.bytes))
  assert.deepEqual(JSON.parse(emitted[0].source.toString('utf8')), { ...profile, platform: 'darwin', arch: 'arm64', components })
})

test('entrada empacotada verifica manifesto e binários do artefato antes de qualquer outra leitura ou escrita', async t => {
  const compiled = compileRc2Profile(mode, version, target, components)!
  const metadata = { mexericaProfile: profile.profile, mexericaAppId: profile.appId, name: profile.packageName,
    productName: profile.name, version }
  const project = mkdtempSync(join(tmpdir(), 'rc2-entry-'))
  t.after(() => rmSync(project, { recursive: true, force: true }))
  mkdirSync(join(project, 'resources'))
  writeFileSync(join(project, 'package.json'), JSON.stringify(metadata))
  writeFileSync(join(project, 'resources', rc2ManifestPath), compiled.bytes)
  const hubPath = join(project, 'resources', 'donnabot-host')
  const run = (arch: string, pin: string | undefined, transporte: unknown = null) => {
    const accesses: string[] = []
    const reads: string[] = []
    const exits: number[] = []
    const quitHandlers: Array<() => void> = []
    let paradas = 0
    let ciclo: { sessao: () => unknown; transporte: unknown } | null = null
    let transportLookups = 0
    const recordingFs = new Proxy(fsModule, { get: (target, key: string) => {
      accesses.push(key)
      if (key === 'readFileSync' || key === 'lstatSync' || key === 'openSync' || key === 'fstatSync') {
        return (path: string | number, ...rest: unknown[]) => {
          reads.push(`${key}:${path}`)
          const read = (target as unknown as Record<string, (path: string | number, ...rest: unknown[]) => unknown>)[key]
          return read(path, ...rest)
        }
      }
      return (target as Record<string, unknown>)[key]
    } })
    const artifact = source('./fork-rc2-artifact.ts', {
      'node:crypto': cryptoModule, 'node:fs': recordingFs, 'node:path': pathModule,
      '../shared/fork-rc2-profile.ts': profileModule, '../shared/fork-rc-profile.ts': rcProfileModule,
      './fork-rc2-cdhash.ts': source('./fork-rc2-cdhash.ts', { 'node:crypto': cryptoModule }, { Buffer }),
    })
    source('./fork-rc2-entry.ts', {
      'node:fs': recordingFs, 'node:path': pathModule,
      'electron': { app: { isPackaged: true, getName: () => profile.name, getVersion: () => version,
        getAppPath: () => project, getPath: (name: string) => name === 'home' ? '/fixture/home' : '/fixture/appdata',
        on: (name: string, handler: () => void) => { if (name === 'will-quit') quitHandlers.push(handler) },
        exit: (code: number) => exits.push(code) } },
      '../shared/fork-rc2-profile.ts': profileModule,
      './fork-rc2-artifact.ts': artifact,
      './fork-rc2-local-hub.ts': { Rc2HubLifecycle: class {
        sessao: () => unknown
        transporte: unknown
        constructor(sessao: () => unknown, transporte: unknown) {
          this.sessao = sessao
          this.transporte = transporte
          ciclo = this
        }
        stop() { paradas++; return Promise.resolve(true) }
      } },
      './fork-rc2-transport.ts': { rc2NativeTransport: () => { transportLookups++; return transporte } },
    }, { process: { platform: 'darwin', arch, resourcesPath: join(project, 'resources') },
      __MEXERICA_RC2_PROFILE__: profile.profile, __MEXERICA_RC2_MANIFEST_HASH__: pin })
    return { accesses, reads, exits, transportLookups, quitHandlers, paradas: () => paradas, ciclo: () => ciclo }
  }
  for (const tamper of [
    { label: 'binário ausente', apply: () => {} },
    { label: 'binário trocado', apply: () => writeFileSync(hubPath, Buffer.concat([hubBytes, Buffer.from('trocado')])) },
    { label: 'slice x64', apply: () => writeFileSync(hubPath, Buffer.concat([Buffer.from([0xcf, 0xfa, 0xed, 0xfe, 0x07, 0x00, 0x00, 0x01]), Buffer.alloc(24)])) },
    { label: 'link simbólico', apply: () => { rmSync(hubPath); symlinkSync('alvo-invalido', hubPath) } },
  ]) {
    if (tamper.label === 'binário ausente') rmSync(hubPath, { force: true })
    else { writeFileSync(hubPath, hubBytes); tamper.apply() }
    const { accesses, reads, exits, transportLookups } = run('arm64', compiled.pin)
    assert.deepEqual(exits, [1], tamper.label)
    assert.equal(transportLookups, 0, tamper.label)
    // a recusa acontece na etapa mais cedo possível: inspeção do caminho ou leitura pelo descritor fixado
    const porCaminho = tamper.label === 'binário ausente' || tamper.label === 'link simbólico'
    assert.ok(porCaminho ? reads[reads.length - 1] === `lstatSync:${hubPath}` : /^readFileSync:\d+$/.test(reads[reads.length - 1]), tamper.label)
    assert.ok(!accesses.includes('writeFileSync') && !accesses.includes('mkdirSync') && !accesses.includes('appendFileSync'), tamper.label)
    rmSync(hubPath, { force: true })
  }
  writeFileSync(hubPath, hubBytes)
  const good = run('arm64', compiled.pin)
  assert.deepEqual(good.exits, [1])
  assert.equal(good.transportLookups, 1)
  const hubLstat = good.reads.indexOf(`lstatSync:${hubPath}`)
  assert.ok(hubLstat > 2)
  const hubOpen = good.reads.indexOf(`openSync:${hubPath}`)
  assert.ok(hubOpen === hubLstat + 1, 'o descritor é aberto logo após a inspeção do caminho')
  const fd = good.reads[hubOpen + 1].split(':')[1]
  assert.equal(good.reads[hubOpen + 1], `fstatSync:${fd}`)
  assert.deepEqual(good.reads, [
    `readFileSync:${join(project, 'package.json')}`,
    `readFileSync:${join(project, 'resources', rc2ManifestPath)}`,
    ...good.reads.slice(2, hubLstat).filter(read => read.startsWith('lstatSync:')),
    `lstatSync:${hubPath}`,
    `openSync:${hubPath}`,
    `fstatSync:${fd}`,
    `readFileSync:${fd}`,
  ])
  assert.ok(!good.accesses.includes('writeFileSync') && !good.accesses.includes('mkdirSync'))
  // Com transporte nativo disponível, a entrada registra o encerramento no will-quit e nada
  // roda enquanto o app vive: o runtime segue desligado até a prova nativa da ativação.
  const transporteFixo = { marcador: 'transporte-do-teste' }
  const ligado = run('arm64', compiled.pin, transporteFixo)
  assert.deepEqual(ligado.exits, [1])
  assert.equal(ligado.transportLookups, 1)
  assert.equal(ligado.quitHandlers.length, 1)
  assert.ok(ligado.ciclo() !== null)
  assert.equal(ligado.paradas(), 0)
  ligado.quitHandlers[0]()
  assert.equal(ligado.paradas(), 1, 'encerrar o Electron encerra o Hub pelo will-quit')
  assert.equal(ligado.ciclo()!.transporte, transporteFixo)
  const overlappingExits: number[] = []
  source('./fork-rc2-entry.ts', {
    'node:fs': fsModule, 'node:path': pathModule,
    'electron': { app: { isPackaged: true, getName: () => profile.name, getVersion: () => version,
      getAppPath: () => project, getPath: (name: string) => name === 'home' ? '/fixture/raiz' : '/fixture/raiz/.mexerica-rc',
      exit: (code: number) => overlappingExits.push(code) } },
    '../shared/fork-rc2-profile.ts': profileModule,
    './fork-rc2-artifact.ts': source('./fork-rc2-artifact.ts', {
      'node:crypto': cryptoModule, 'node:fs': fsModule, 'node:path': pathModule,
      '../shared/fork-rc2-profile.ts': profileModule, '../shared/fork-rc-profile.ts': rcProfileModule,
      './fork-rc2-cdhash.ts': source('./fork-rc2-cdhash.ts', { 'node:crypto': cryptoModule }, { Buffer }),
    }),
    './fork-rc2-local-hub.ts': { Rc2HubLifecycle },
    './fork-rc2-transport.ts': { rc2NativeTransport: () => null },
  }, { process: { platform: 'darwin', arch: 'arm64', resourcesPath: join(project, 'resources') },
    __MEXERICA_RC2_PROFILE__: profile.profile, __MEXERICA_RC2_MANIFEST_HASH__: compiled.pin })
  assert.deepEqual(overlappingExits, [1])
  const refused = run('x64', compiled.pin)
  assert.deepEqual(refused.exits, [1])
  assert.deepEqual(refused.reads, [])
})

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(done => { resolve = done })
  return { promise, resolve }
}

async function collect(events: AsyncIterable<string>) {
  const lines: string[] = []
  for await (const line of events) lines.push(line)
  return lines
}

function fixture() {
  let session: object | null = null
  let current: Rc2Connection | undefined
  const opened: AbortSignal[] = []
  const closed: Rc2Connection[] = []
  const connection: Rc2Connection = {
    hello: async () => ({ computerId: 'fixture-computer', hostId: 'fixture-hub' }),
    async *events() { yield '{"type":"hello"}' },
    close: async () => { closed.push(connection) },
  }
  const lifecycle = new Rc2HubLifecycle(() => session, async signal => {
    opened.push(signal)
    if (!current) throw new Error('fixture_unavailable')
    return current
  })
  return {
    lifecycle, opened, closed, connection,
    session: (value: object | null) => { session = value },
    transport: (value: Rc2Connection = connection) => { current = value },
  }
}

test('só sessão corrente e transporte injetado liberam hello e eventos', async () => {
  const f = fixture()
  f.transport()
  assert.equal(await f.lifecycle.start(), false)
  assert.equal(f.opened.length, 0)
  assert.equal(await f.lifecycle.hello(), null)
  assert.deepEqual(await collect(f.lifecycle.events()), [])
  f.session({})
  assert.equal(await f.lifecycle.start(), true)
  assert.equal(await f.lifecycle.start(), true)
  assert.equal(f.opened.length, 1)
  assert.deepEqual(await f.lifecycle.hello(), { computerId: 'fixture-computer', hostId: 'fixture-hub' })
  assert.deepEqual(await collect(f.lifecycle.events()), ['{"type":"hello"}'])
  await f.lifecycle.stop()
  await f.lifecycle.stop()
  assert.equal(f.opened[0].aborted, true)
  assert.equal(f.closed.length, 1)
  assert.equal(await f.lifecycle.hello(), null)
})

test('suspensão corta o stream e descarta respostas que chegam após a troca de sessão', async () => {
  const f = fixture()
  const reply = deferred<{ computerId: string; hostId: string }>()
  const event = deferred<string>()
  const enteredHello = deferred<void>()
  const enteredEvents = deferred<void>()
  f.transport({
    ...f.connection,
    hello: () => { enteredHello.resolve(); return reply.promise },
    async *events() { enteredEvents.resolve(); yield await event.promise },
  })
  f.session({})
  assert.equal(await f.lifecycle.start(), true)
  const pendingHello = f.lifecycle.hello()
  const stream = f.lifecycle.events()
  const pendingEvent = stream.next()
  await enteredHello.promise
  await enteredEvents.promise
  f.session({})
  assert.deepEqual(await collect(f.lifecycle.events()), [])
  assert.equal(await f.lifecycle.hello(), null)
  const stopped = f.lifecycle.stop()
  assert.equal(f.opened[0].aborted, true)
  reply.resolve({ computerId: 'fixture-old-computer', hostId: 'fixture-old-hub' })
  event.resolve('{"type":"hello"}')
  assert.equal(await pendingHello, null)
  assert.equal((await pendingEvent).done, true)
  await stopped
})

test('retomar o stream depois da troca de sessão não pede outro evento à fonte', async () => {
  const f = fixture()
  let pulls = 0
  f.transport({
    ...f.connection,
    async *events() { pulls++; yield '{"type":"first"}'; pulls++; yield '{"type":"second"}' },
  })
  f.session({})
  assert.equal(await f.lifecycle.start(), true)
  const stream = f.lifecycle.events()
  assert.deepEqual(await stream.next(), { value: '{"type":"first"}', done: false })
  assert.equal(pulls, 1)
  f.session({})
  assert.equal((await stream.next()).done, true)
  assert.equal(pulls, 1)
  assert.equal(await f.lifecycle.stop(), true)
})

test('aguarda fechar conexão de início cancelado antes de admitir nova geração', async () => {
  const attempt = deferred<Rc2Connection>()
  const draining = deferred<void>()
  const closed: string[] = []
  let opens = 0
  let session = {}
  const { connection } = fixture()
  const lifecycle = new Rc2HubLifecycle(() => session, () => {
    opens++
    return opens === 1 ? attempt.promise : Promise.resolve(connection)
  })
  const first = lifecycle.start()
  await Promise.resolve()
  session = {}
  const second = lifecycle.start()
  assert.equal(opens, 1)
  attempt.resolve({ ...connection, close: async () => { closed.push('old'); await draining.promise } })
  await Promise.resolve()
  assert.equal(opens, 1)
  draining.resolve()
  assert.equal(await first, false)
  assert.equal(await second, true)
  assert.deepEqual(closed, ['old'])
  assert.equal(opens, 2)
  await lifecycle.stop()
})

test('parada explícita invalida um início que já esperava a drenagem', async () => {
  const f = fixture()
  const draining = deferred<void>()
  f.transport({ ...f.connection, close: () => draining.promise })
  f.session({})
  assert.equal(await f.lifecycle.start(), true)
  f.session({})
  const next = f.lifecycle.start()
  const stopped = f.lifecycle.stop()
  draining.resolve()
  assert.equal(await stopped, true)
  assert.equal(await next, false)
  assert.equal(f.opened.length, 1)
})

test('se a sessão mudar durante o início, fecha a geração obsoleta mesmo sem observador', async () => {
  const attempt = deferred<Rc2Connection>()
  const draining = deferred<void>()
  const closeStarted = deferred<void>()
  let current = {}
  let active = 0
  let finished = false
  let signal: AbortSignal | undefined
  const { connection } = fixture()
  const lifecycle = new Rc2HubLifecycle(() => current, startSignal => { signal = startSignal; return attempt.promise })
  const first = lifecycle.start().then(result => { finished = true; return result })
  await Promise.resolve()
  current = {}
  active++
  attempt.resolve({ ...connection, close: async () => { closeStarted.resolve(); await draining.promise; active-- } })
  await Promise.race([closeStarted.promise, first])
  assert.equal(signal?.aborted, true)
  assert.equal(finished, false)
  assert.equal(await lifecycle.hello(), null)
  draining.resolve()
  assert.equal(await first, false)
  assert.equal(active, 0)
  assert.equal(await lifecycle.stop(), true)
})

test('troca antes da primeira microtarefa não abre transporte para sessão obsoleta', async () => {
  const f = fixture()
  f.transport()
  f.session({})
  const first = f.lifecycle.start()
  f.session({})
  assert.equal(await first, false)
  assert.equal(f.opened.length, 0)
  assert.equal(await f.lifecycle.start(), true)
  await f.lifecycle.stop()
})

test('falha transportada não vaza erro nem tenta fallback; fechamento incerto não reabre', async () => {
  const f = fixture()
  f.session({})
  assert.equal(await f.lifecycle.start(), false)
  assert.equal(f.opened.length, 1)
  assert.equal(await f.lifecycle.hello(), null)
  f.transport({ ...f.connection, close: async () => { throw new Error('fixture_close_failure') } })
  assert.equal(await f.lifecycle.start(), true)
  assert.equal(await f.lifecycle.stop(), false)
  f.transport()
  assert.equal(await f.lifecycle.start(), false)
  assert.equal(f.opened.length, 2)
})

async function flush(ticks = 12) {
  for (let i = 0; i < ticks; i++) await Promise.resolve()
}

function manualClock() {
  const pending: Array<() => void> = []
  const schedule: Rc2Schedule = (_ms, fire) => { pending.push(fire); return () => {} }
  return { schedule, fire: () => { for (const fire of pending.splice(0)) fire() } }
}

const shortDeadlines = { open: 1, hello: 1, close: 1, eof: 1, sigterm: 1, sigkill: 1, probe: 1 }

function fakeChild(diesOn: { end?: boolean; terminate?: boolean; kill?: boolean } = {}) {
  const calls: string[] = []
  let settle!: () => void
  let fail!: () => void
  const exited = new Promise<void>((resolve, reject) => { settle = resolve; fail = reject })
  const die = () => settle()
  const child: Rc2ChildHandle = {
    end: () => { calls.push('end'); if (diesOn.end) die() },
    terminate: () => { calls.push('terminate'); if (diesOn.terminate) die() },
    kill: () => { calls.push('kill'); if (diesOn.kill !== false) die() },
    exited,
    survivors: () => 0,
  }
  return { child, calls, die, fail }
}

const helloFixture = { computerId: 'fixture-computer', hostId: 'fixture-hub' }

function connectionFixture(child: Rc2ChildHandle | undefined, close: () => Promise<void>): Rc2Connection {
  return {
    hello: async () => ({ ...helloFixture }),
    async *events() { yield '{"type":"hello"}' },
    close,
    child,
  }
}

test('parada do filho que ignora EOF e SIGTERM termina no SIGKILL e só conclui após o close', async () => {
  const clock = manualClock()
  const { child, calls } = fakeChild()
  const closed: string[] = []
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(child, async () => { closed.push('connection') }),
    { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  let settled = false
  const stopped = lifecycle.stop().then(result => { settled = true; return result })
  await flush()
  assert.equal(settled, false)
  assert.deepEqual(calls, ['end'])
  clock.fire()
  await flush()
  assert.equal(settled, false)
  assert.deepEqual(calls, ['end', 'terminate'])
  clock.fire()
  await flush()
  assert.deepEqual(calls, ['end', 'terminate', 'kill'])
  assert.equal(await stopped, true)
  assert.deepEqual(closed, ['connection'])
})

test('filho que fecha no EOF não recebe SIGTERM nem SIGKILL', async () => {
  const { child, calls } = fakeChild({ end: true })
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(child, async () => {}),
    { deadlines: shortDeadlines, schedule: manualClock().schedule })
  assert.equal(await lifecycle.start(), true)
  assert.equal(await lifecycle.stop(), true)
  assert.deepEqual(calls, ['end'])
})

test('filho que fecha no SIGTERM não recebe SIGKILL', async () => {
  const clock = manualClock()
  const { child, calls } = fakeChild({ terminate: true })
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(child, async () => {}),
    { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  await flush()
  clock.fire()
  assert.equal(await stopped, true)
  assert.deepEqual(calls, ['end', 'terminate'])
})

test('SIGKILL sem close no prazo deixa encerramento incerto e recusa nova geração', async () => {
  const clock = manualClock()
  const { child } = fakeChild({ kill: false })
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(child, async () => {}),
    { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  for (const _ of [1, 2, 3]) { await flush(); clock.fire() }
  assert.equal(await stopped, false)
  assert.equal(await lifecycle.start(), false)
})

test('troca de sessão encerra o filho antigo antes de abrir o novo e não sinaliza o novo', async () => {
  const clock = manualClock()
  const { child: oldChild, calls: oldCalls } = fakeChild({ terminate: true })
  const { child: newChild, calls: newCalls } = fakeChild({ end: true })
  let session: object | null = { name: 'a' }
  let opens = 0
  const transport = async () => connectionFixture(++opens === 1 ? oldChild : newChild, async () => {})
  const lifecycle = new Rc2HubLifecycle(() => session, transport, { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  session = { name: 'b' }
  const switched = lifecycle.start()
  await flush()
  assert.equal(opens, 1)
  assert.deepEqual(oldCalls, ['end'])
  clock.fire()
  assert.equal(await switched, true)
  assert.equal(opens, 2)
  assert.deepEqual(oldCalls, ['end', 'terminate'])
  assert.deepEqual(newCalls, [])
  assert.deepEqual(await lifecycle.hello(), { ...helloFixture })
  await lifecycle.stop()
})

test('timeout de abertura marca encerramento incerto, aborta o transporte e drena a conexão tardia', async () => {
  const clock = manualClock()
  const attempt = deferred<Rc2Connection>()
  const { child: lateChild, calls: lateCalls } = fakeChild({ end: true })
  const closed: string[] = []
  let signals = 0
  let session: object | null = {}
  const transport = async (signal: AbortSignal) => {
    void signal.addEventListener('abort', () => { signals++ })
    return attempt.promise
  }
  const lifecycle = new Rc2HubLifecycle(() => session, transport, { deadlines: shortDeadlines, schedule: clock.schedule })
  const first = lifecycle.start()
  await flush()
  clock.fire()
  assert.equal(await first, false)
  assert.equal(signals, 1)
  assert.equal(await lifecycle.hello(), null)
  assert.equal(await lifecycle.start(), false)
  const stopped = lifecycle.stop()
  assert.equal(await stopped, false)
  attempt.resolve(connectionFixture(lateChild, async () => { closed.push('late') }))
  await flush()
  assert.deepEqual(closed, ['late'])
  assert.deepEqual(lateCalls, ['end'])
  assert.equal(await lifecycle.start(), false)
})

test('morte espontânea do filho invalida a geração sem deixar encerramento incerto', async () => {
  const clock = manualClock()
  const { child, die } = fakeChild()
  const { child: nextChild } = fakeChild({ end: true })
  let opens = 0
  let session: object | null = {}
  const transport = async () => connectionFixture(++opens === 1 ? child : nextChild, async () => {})
  const lifecycle = new Rc2HubLifecycle(() => session, transport, { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  die()
  await flush()
  assert.equal(await lifecycle.hello(), null)
  assert.deepEqual(await collect(lifecycle.events()), [])
  assert.equal(await lifecycle.start(), true)
  assert.equal(opens, 2)
  assert.equal(await lifecycle.stop(), true)
})

test('sem reanexação: o transporte recebe apenas o sinal de aborto, nunca processo, porta ou caminho', async () => {
  let observed: unknown
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async signal => {
    observed = signal
    return connectionFixture(undefined, async () => {})
  }, { deadlines: shortDeadlines, schedule: manualClock().schedule })
  assert.equal(await lifecycle.start(), true)
  assert.ok(observed instanceof AbortSignal)
  await lifecycle.stop()
})

test('close da conexão sem retorno no prazo não bloqueia o encerramento do filho e deixa o estado inseguro', async () => {
  const clock = manualClock()
  const { child, calls } = fakeChild({ end: true })
  const closePending = deferred<void>()
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(child, () => closePending.promise),
    { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  await flush()
  clock.fire()
  await flush()
  assert.deepEqual(calls, ['end'])
  assert.equal(await stopped, false)
  assert.equal(await lifecycle.start(), false)
  closePending.resolve()
})

test('exceção em uma etapa do escalonamento não impede as seguintes', async () => {
  const clock = manualClock()
  const throws = { end: new Error('fixture_end_failure'), terminate: new Error('fixture_terminate_failure'), kill: new Error('fixture_kill_failure') }
  const sessionValue: object = {}
  const session = () => sessionValue
  const close = async () => {}
  {
    const { child, calls } = fakeChild({ terminate: true })
    child.end = () => { calls.push('end-throw'); throw throws.end }
    const lifecycle = new Rc2HubLifecycle(session, async () => connectionFixture(child, close),
      { deadlines: shortDeadlines, schedule: clock.schedule })
    assert.equal(await lifecycle.start(), true)
    const stopped = lifecycle.stop()
    await flush()
    clock.fire()
    assert.equal(await stopped, true)
    assert.deepEqual(calls, ['end-throw', 'terminate'])
  }
  {
    const { child, calls } = fakeChild()
    child.end = () => { calls.push('end-throw'); throw throws.end }
    child.terminate = () => { calls.push('terminate-throw'); throw throws.terminate }
    const lifecycle = new Rc2HubLifecycle(session, async () => connectionFixture(child, close),
      { deadlines: shortDeadlines, schedule: clock.schedule })
    assert.equal(await lifecycle.start(), true)
    const stopped = lifecycle.stop()
    await flush()
    clock.fire()
    await flush()
    clock.fire()
    assert.equal(await stopped, true)
    assert.deepEqual(calls, ['end-throw', 'terminate-throw', 'kill'])
  }
  {
    const { child } = fakeChild({ kill: false })
    child.end = () => { throw throws.end }
    child.terminate = () => { throw throws.terminate }
    child.kill = () => { throw throws.kill }
    const lifecycle = new Rc2HubLifecycle(session, async () => connectionFixture(child, close),
      { deadlines: shortDeadlines, schedule: clock.schedule })
    assert.equal(await lifecycle.start(), true)
    const stopped = lifecycle.stop()
    for (const _ of [1, 2, 3]) { await flush(); clock.fire() }
    assert.equal(await stopped, false)
    assert.equal(await lifecycle.start(), false)
  }
})

test('rejeição de exited do filho em execução invalida a geração e recusa reabertura', async () => {
  const clock = manualClock()
  const { child, fail } = fakeChild()
  const closed: string[] = []
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(child, async () => { closed.push('connection') }),
    { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  fail()
  await flush()
  assert.equal(await lifecycle.hello(), null)
  assert.deepEqual(await collect(lifecycle.events()), [])
  assert.equal(await lifecycle.start(), false)
  assert.deepEqual(closed, ['connection'])
  await lifecycle.stop()
})

test('prazos ausentes, nulos, negativos ou de tipo errado são recusados na construção', () => {
  const sessionValue: object = {}
  const session = () => sessionValue
  const transport = async () => connectionFixture(undefined, async () => {})
  for (const deadlines of [
    {}, { open: 0, hello: 1, close: 1, eof: 1, sigterm: 1, sigkill: 1, probe: 1 },
    { open: -1, hello: 1, close: 1, eof: 1, sigterm: 1, sigkill: 1, probe: 1 },
    { open: 1, hello: 1, close: 1, eof: 1, sigterm: 1, probe: 1 },
    { open: 1, hello: 0, close: 1, eof: 1, sigterm: 1, sigkill: 1, probe: 1 },
    { open: 1, hello: 1, close: 1, eof: 1, sigterm: 1, sigkill: 1 },
    { open: 1, hello: 1, close: 1, eof: 1, sigterm: 1, sigkill: 2, probe: 3 },
    { open: 1, hello: 1, close: 1, eof: 2, sigterm: 1, sigkill: 1, probe: 2 },
    { open: 1, hello: 1, close: 1, eof: 1, sigterm: 1, sigkill: 1, extra: 1 },
    { open: 1, hello: Number.NaN, close: 1, eof: 1, sigterm: 1, sigkill: 1, probe: 1 },
    { open: '1', hello: 1, close: 1, eof: 1, sigterm: 1, sigkill: 1, probe: 1 },
    { open: 2 ** 31, hello: 1, close: 1, eof: 1, sigterm: 1, sigkill: 1, probe: 1 },
  ]) assert.throws(() => new Rc2HubLifecycle(session, transport, { deadlines: deadlines as never, schedule: manualClock().schedule }), /invalid_rc2_deadlines/)
  assert.ok(new Rc2HubLifecycle(session, transport, { deadlines: shortDeadlines, schedule: manualClock().schedule }))
})

test('descarte de conexão tardia limita o close pendurado e ainda encerra o filho tardio', async () => {
  const clock = manualClock()
  const attempt = deferred<Rc2Connection>()
  const { child: lateChild, calls: lateCalls } = fakeChild({ terminate: true })
  const closePending = deferred<void>()
  let session: object | null = {}
  const transport = async () => attempt.promise
  const lifecycle = new Rc2HubLifecycle(() => session, transport, { deadlines: shortDeadlines, schedule: clock.schedule })
  const first = lifecycle.start()
  await flush()
  clock.fire()
  assert.equal(await first, false)
  attempt.resolve(connectionFixture(lateChild, () => closePending.promise))
  await flush()
  clock.fire()
  await flush()
  clock.fire()
  await lateChild.exited
  assert.deepEqual(lateCalls, ['end', 'terminate'])
  closePending.resolve()
})

test('abertura pendurada não prende hello nem eventos além do prazo', async () => {
  const clock = manualClock()
  const attempt = deferred<Rc2Connection>()
  const { child: lateChild } = fakeChild({ end: true })
  const closed: string[] = []
  let session: object | null = {}
  const transport = async () => attempt.promise
  const lifecycle = new Rc2HubLifecycle(() => session, transport, { deadlines: shortDeadlines, schedule: clock.schedule })
  const first = lifecycle.start()
  const pendingHello = lifecycle.hello()
  const pendingEvent = lifecycle.events().next()
  await flush()
  clock.fire()
  assert.equal(await first, false)
  assert.equal(await pendingHello, null)
  assert.equal((await pendingEvent).done, true)
  assert.equal(await lifecycle.start(), false)
  attempt.resolve(connectionFixture(lateChild, async () => { closed.push('late') }))
  await flush()
  assert.deepEqual(closed, ['late'])
})

test('start disparado por listener de abort é recusado enquanto o close não confirma', async () => {
  const clock = manualClock()
  const { child: firstChild } = fakeChild({ end: true })
  const { child: secondChild } = fakeChild({ end: true })
  const closePending = deferred<void>()
  let session: object | null = { name: 'a' }
  let opens = 0
  let sneaky: Promise<boolean> | null = null
  let lifecycle: Rc2HubLifecycle
  const transport = async (signal: AbortSignal) => {
    const visit = ++opens
    if (visit === 1) void signal.addEventListener('abort', () => { sneaky = lifecycle.start() })
    return connectionFixture(visit === 1 ? firstChild : secondChild, () => closePending.promise)
  }
  lifecycle = new Rc2HubLifecycle(() => session, transport, { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  assert.ok(sneaky)
  assert.equal(await sneaky, false)
  assert.equal(opens, 1)
  closePending.resolve()
  assert.equal(await stopped, true)
  assert.equal(opens, 1, 'nenhuma abertura entre a parada e a confirmação do close')
  await lifecycle.stop()
})

test('start que chega durante drenagem alheia é recusado sem fila', async () => {
  const clock = manualClock()
  const { child } = fakeChild({ end: true })
  const closePending = deferred<void>()
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(child, () => closePending.promise),
    { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  const late = lifecycle.start()
  assert.equal(await late, false)
  closePending.resolve()
  assert.equal(await stopped, true)
  assert.equal(await lifecycle.start(), true, 'depois do close confirmado a partida é decisão nova do chamador')
  await lifecycle.stop()
})

test('hello sem resposta dentro do prazo encerra filho e conexão e invalida a geração', async () => {
  const clock = manualClock()
  const { child, calls } = fakeChild({ end: true })
  const closed: string[] = []
  let helloCalls = 0
  let session: object | null = {}
  const reply = deferred<{ computerId: string, hostId: string }>()
  const lifecycle = new Rc2HubLifecycle(() => session, async () => ({
    hello: () => { helloCalls++; return reply.promise },
    async *events() { yield '{"type":"hello"}' },
    close: async () => { closed.push('connection') },
    child,
  }), { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const pendingHello = lifecycle.hello()
  await flush()
  assert.equal(helloCalls, 1)
  clock.fire()
  assert.equal(await pendingHello, null)
  assert.deepEqual(calls, ['end'])
  assert.deepEqual(closed, ['connection'])
  assert.equal(await lifecycle.hello(), null)
  assert.equal(await lifecycle.start(), true, 'reabertura permitida porque o encerramento confirmou no prazo')
  reply.resolve({ computerId: 'tardia', hostId: 'tardia' })
  await lifecycle.stop()
})

test('hello pendente é cancelado pela parada sem esperar resposta nem prazo', async () => {
  const clock = manualClock()
  const { child } = fakeChild({ end: true })
  const reply = deferred<{ computerId: string, hostId: string }>()
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => ({
    hello: () => reply.promise,
    async *events() {},
    close: async () => {},
    child,
  }), { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const pendingHello = lifecycle.hello()
  await flush()
  const stopped = lifecycle.stop()
  assert.equal(await pendingHello, null)
  reply.resolve({ computerId: 'tardia', hostId: 'tardia' })
  assert.equal(await stopped, true)
  assert.equal(await lifecycle.hello(), null)
})

test('hello rejeitado pelo transporte devolve null sem invalidar a geração', async () => {
  let attempts = 0
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => ({
    hello: () => { if (++attempts === 1) return Promise.reject(new Error('fixture_hello_failure')); return Promise.resolve({ ...helloFixture }) },
    async *events() {},
    close: async () => {},
  }), { deadlines: shortDeadlines, schedule: manualClock().schedule })
  assert.equal(await lifecycle.start(), true)
  assert.equal(await lifecycle.hello(), null)
  assert.deepEqual(await lifecycle.hello(), { ...helloFixture }, 'a geração segue viva')
  await lifecycle.stop()
})

test('filho morto com descendente vivo só confirma depois que o sinal de grupo alcança o sobrevivente', async () => {
  const clock = manualClock()
  const { child, calls, die } = fakeChild()
  const grupoMorto = deferred<void>()
  const comDescendente: Rc2ChildHandle = {
    ...child,
    kill: () => { calls.push('kill'); grupoMorto.resolve(); die() },
    survivors: async () => { await grupoMorto.promise; return 0 },
  }
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(comDescendente, async () => {}),
    { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  await flush()
  assert.deepEqual(calls, ['end'], 'filho morto, sonda do grupo pendurada: não confirma')
  clock.fire()
  await flush()
  assert.deepEqual(calls, ['end', 'terminate'])
  clock.fire()
  await flush()
  assert.deepEqual(calls, ['end', 'terminate', 'kill'], 'SIGKILL de grupo alcança o sobrevivente')
  assert.equal(await stopped, true)
})

test('sobrevivente que persiste após o SIGKILL nunca vira sucesso e bloqueia reabertura', async () => {
  const clock = manualClock()
  const { child } = fakeChild({ end: true })
  const imortal: Rc2ChildHandle = { ...child, survivors: () => 1 }
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(imortal, async () => {}),
    { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  for (const _ of [1, 2, 3]) { await flush(); clock.fire() }
  assert.equal(await stopped, false)
  assert.equal(await lifecycle.start(), false)
})

test('sonda de sobreviventes que falha conta como sobrevivente', async () => {
  const clock = manualClock()
  const { child } = fakeChild({ end: true })
  const sondaFalha: Rc2ChildHandle = { ...child, survivors: () => { throw new Error('fixture_probe_failure') } }
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(sondaFalha, async () => {}),
    { deadlines: shortDeadlines, schedule: clock.schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  for (const _ of [1, 2, 3]) { await flush(); clock.fire() }
  assert.equal(await stopped, false)
  assert.equal(await lifecycle.start(), false)
})

test('sonda que zera dentro do prazo do SIGKILL confirma antes do vencimento do prazo', async () => {
  let agora = 0
  const pendentes: Array<{ vencimento: number, fire: () => void }> = []
  const schedule: Rc2Schedule = (ms, fire) => { pendentes.push({ vencimento: agora + ms, fire }); return () => { } }
  const avancar = (ms: number) => {
    const alvo = agora + ms
    agora = alvo
    const vencidos = pendentes.filter(timer => timer.vencimento <= alvo)
    for (const timer of vencidos) pendentes.splice(pendentes.indexOf(timer), 1)
    vencidos.sort((primeiro, segundo) => primeiro.vencimento - segundo.vencimento)
    for (const timer of vencidos) timer.fire()
  }
  const { child, calls, die } = fakeChild()
  let sondas = 0
  const comSonda: Rc2ChildHandle = {
    ...child,
    kill: () => { calls.push('kill'); die() },
    survivors: () => (sondas++ >= 1 ? 0 : 1),
  }
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(comSonda, async () => { }),
    { deadlines: { open: 1, hello: 1, close: 1, eof: 2, sigterm: 3, sigkill: 10, probe: 1 }, schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  await flush()
  avancar(2)
  await flush()
  avancar(3)
  await flush()
  assert.equal(sondas, 1, 'a primeira sonda pós-SIGKILL ainda via sobrevivente')
  avancar(1)
  assert.equal(await stopped, true, 'a re-consulta confirmou antes do vencimento do prazo do SIGKILL')
  assert.deepEqual(calls, ['end', 'terminate', 'kill'])
  assert.ok(pendentes.every(timer => timer.vencimento > agora), 'o prazo do SIGKILL segue pendente: não venceu')
})

test('hello que lança de forma síncrona devolve null sem invalidar a geração', async () => {
  let attempts = 0
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => ({
    hello: () => { if (attempts++ === 0) throw new Error('fixture_hello_sync_throw'); return Promise.resolve({ ...helloFixture }) },
    async *events() { },
    close: async () => { },
    child: undefined,
  }), { deadlines: shortDeadlines, schedule: manualClock().schedule })
  assert.equal(await lifecycle.start(), true)
  assert.equal(await lifecycle.hello(), null)
  assert.deepEqual(await lifecycle.hello(), { ...helloFixture }, 'a geração segue viva')
  await lifecycle.stop()
})

test('sonda que falha de forma transitória é re-consultada e confirma dentro do prazo', async () => {
  let agora = 0
  const pendentes: Array<{ vencimento: number, fire: () => void }> = []
  const schedule: Rc2Schedule = (ms, fire) => { pendentes.push({ vencimento: agora + ms, fire }); return () => { } }
  const avancar = (ms: number) => {
    const alvo = agora + ms
    agora = alvo
    const vencidos = pendentes.filter(timer => timer.vencimento <= alvo)
    for (const timer of vencidos) pendentes.splice(pendentes.indexOf(timer), 1)
    vencidos.sort((primeiro, segundo) => primeiro.vencimento - segundo.vencimento)
    for (const timer of vencidos) timer.fire()
  }
  const { child, calls } = fakeChild({ end: true })
  let consultas = 0
  const intermitente: Rc2ChildHandle = {
    ...child,
    survivors: () => { if (++consultas === 1) throw new Error('fixture_probe_transient'); return 0 },
  }
  let session: object | null = {}
  const lifecycle = new Rc2HubLifecycle(() => session, async () => connectionFixture(intermitente, async () => { }),
    { deadlines: { open: 1, hello: 1, close: 1, eof: 10, sigterm: 10, sigkill: 10, probe: 1 }, schedule })
  assert.equal(await lifecycle.start(), true)
  const stopped = lifecycle.stop()
  await flush()
  // primeira consulta lança: conta como sobrevivente e agenda re-consulta, não aborta a etapa
  assert.equal(consultas, 1)
  avancar(1)
  assert.equal(await stopped, true, 'a re-consulta confirmou zero dentro do prazo do EOF')
  assert.deepEqual(calls, ['end'])
})
