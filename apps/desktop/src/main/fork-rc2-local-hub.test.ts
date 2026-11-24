import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'
import * as profileModule from '../shared/fork-rc2-profile.ts'
import { compileRc2Profile, rc2Paths, validateRc2Bootstrap } from '../shared/fork-rc2-profile.ts'
import { Rc2HubLifecycle, nativeRc2Transport, type Rc2Connection } from './fork-rc2-local-hub.ts'

const mode = 'official-rc2'
const version = '0.0.1-rc.2'
const target = { platform: 'darwin', arch: 'arm64' }
const profile = {
  profile: 'rc2-local', version, appId: 'app.mexerica.mac.rc2',
  name: 'Mexerica RC 2', scheme: 'mexerica-rc2', packageName: 'mexerica-rc2',
}

test('só o modo explícito compila o perfil local para a candidata exata em ARM64', () => {
  for (const other of ['official-rc', 'official', 'open', 'development']) {
    assert.equal(compileRc2Profile(other, version, target), null)
  }
  assert.deepEqual(compileRc2Profile(mode, version, target)?.distribution, profile)
  for (const other of [
    { ...target, arch: 'x64' }, { ...target, platform: 'linux' }, { ...target, platform: 'win32' },
  ]) assert.throws(() => compileRc2Profile(mode, version, other), /invalid_rc2_profile/)
  for (const other of ['0.0.1', '0.0.1-rc.1', '0.0.1-rc.3', '0.1.0-rc.2', `${version}\n`]) {
    assert.throws(() => compileRc2Profile(mode, other, target), /invalid_rc2_profile/)
  }
})

test('bootstrap exige pin compilado, metadados coerentes e bytes exatos antes de admitir o perfil', () => {
  const compiled = compileRc2Profile(mode, version, target)!
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
  assert.deepEqual(validateRc2Bootstrap(bootstrap), profile)
  assert.deepEqual(rc2Paths('/fixture/profile', '/appdata'), {
    userData: '/appdata/Mexerica RC 2', sessionData: '/appdata/Mexerica RC 2/session',
    cache: '/appdata/Mexerica RC 2/cache', logs: '/appdata/Mexerica RC 2/logs',
    crashDumps: '/appdata/Mexerica RC 2/crashDumps', temp: '/appdata/Mexerica RC 2/tmp',
    hubRoot: '/fixture/profile/.mexerica-rc2',
  })
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

test('configuração de compilação segregada produz só entrada inerte e o manifesto fixado', () => {
  type Artifact = { type: string; fileName: string; source: Buffer }
  type Section = { define: Record<string, string>; build: { outDir: string; rollupOptions: { input: Record<string, string> } };
    plugins: { generateBundle(this: { emitFile(artifact: Artifact): void }): void }[] }
  const attempt = (platform: string, arch: string, mode: string, candidate: string) => source('../../electron.vite.rc2.config.ts', {
    'electron-vite': { defineConfig: (config: (input: { mode: string }) => unknown) => config({ mode }) },
    'node:fs': { readFileSync: (path: string) => { assert.equal(path, 'package.json'); return JSON.stringify({ version: candidate }) } },
    'node:path': { resolve: (path: string) => path },
    './src/shared/fork-rc2-profile.ts': profileModule,
  }, { process: { platform, arch } }).default as Record<string, Section>
  assert.throws(() => attempt('darwin', 'arm64', 'official-rc', version), /invalid_rc2_profile/)
  assert.throws(() => attempt('darwin', 'x64', mode, version), /invalid_rc2_profile/)
  assert.throws(() => attempt('darwin', 'arm64', mode, '0.0.1-rc.1'), /invalid_rc2_profile/)
  for (const platform of ['linux', 'win32']) assert.throws(() => attempt(platform, 'arm64', mode, version), /invalid_rc2_profile/)
  const config = attempt('darwin', 'arm64', mode, version)
  assert.deepEqual(Object.keys(config), ['main'])
  assert.deepEqual(Object.entries(config.main.build.rollupOptions.input), [['rc2', 'src/main/fork-rc2-entry.ts']])
  assert.equal(config.main.build.outDir, 'build/official-rc2/out/main')
  const emitted: Artifact[] = []
  config.main.plugins[0].generateBundle.call({ emitFile: artifact => emitted.push(artifact) })
  const compiled = compileRc2Profile(mode, version, target)!
  assert.equal(config.main.define.__MEXERICA_RC2_PROFILE__, JSON.stringify(profile.profile))
  assert.equal(config.main.define.__MEXERICA_RC2_MANIFEST_HASH__, JSON.stringify(compiled.pin))
  assert.equal(emitted.length, 1)
  assert.equal(emitted[0].type, 'asset')
  assert.equal(emitted[0].fileName, 'rc2-manifest.json')
  assert.ok(emitted[0].source.equals(compiled.bytes))
})

test('entrada empacotada sem adaptador fecha antes de abrir sessão, armazenamento ou IPC', () => {
  const compiled = compileRc2Profile(mode, version, target)!
  const metadata = { mexericaProfile: profile.profile, mexericaAppId: profile.appId, name: profile.packageName,
    productName: profile.name, version }
  for (const [arch, pin, reads] of [
    ['arm64', compiled.pin, ['/fixture/app.asar/package.json', '/fixture/rc2-manifest.json']], ['x64', compiled.pin, []], ['arm64', undefined, []],
  ] as const) {
    const observed: string[] = []
    const exits: number[] = []
    source('./fork-rc2-entry.ts', {
      'node:fs': { readFileSync: (path: string) => {
        observed.push(path)
        if (path === '/fixture/app.asar/package.json') return JSON.stringify(metadata)
        if (path === '/fixture/rc2-manifest.json') return compiled.bytes
        return assert.fail(`unexpected read: ${path}`)
      } },
      'node:path': { join: (...parts: string[]) => parts.join('/') },
      'electron': { app: { isPackaged: true, getName: () => profile.name, getVersion: () => version,
        getAppPath: () => '/fixture/app.asar', exit: (code: number) => exits.push(code) } },
      '../shared/fork-rc2-profile.ts': profileModule,
      './fork-rc2-local-hub.ts': { nativeRc2Transport },
    }, { process: { ...target, arch, resourcesPath: '/fixture' },
      __MEXERICA_RC2_PROFILE__: profile.profile, __MEXERICA_RC2_MANIFEST_HASH__: pin })
    assert.deepEqual(observed, reads)
    assert.deepEqual(exits, [1])
  }
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

test('nenhum adaptador nativo é oferecido; só sessão corrente e transporte injetado liberam hello e eventos', async () => {
  assert.equal(nativeRc2Transport(), null)
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
  f.transport({ ...f.connection, hello: () => reply.promise, async *events() { yield await event.promise } })
  f.session({})
  assert.equal(await f.lifecycle.start(), true)
  const pendingHello = f.lifecycle.hello()
  const stream = f.lifecycle.events()
  const pendingEvent = stream.next()
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
