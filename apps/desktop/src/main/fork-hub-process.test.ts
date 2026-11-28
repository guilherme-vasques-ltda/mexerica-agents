import assert from 'node:assert/strict'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import { once } from 'node:events'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { setTimeout as delay } from 'node:timers/promises'
import { fileURLToPath } from 'node:url'
import { spawnHubProcess } from './fork-hub-process.ts'
import { attestHubProcess } from './fork-hub-signature.ts'
import { loadMainModule } from '../shared/test-support/main-module.ts'

const darwin = process.platform === 'darwin'
const fixture = fileURLToPath(new URL('../../native/fork-hub/fixture.c', import.meta.url))

function cdhash(path: string) {
  const { stderr, status } = spawnSync('/usr/bin/codesign', ['--display', '--verbose=4', path], { encoding: 'utf8' })
  assert.equal(status, 0)
  const match = /^CDHash=([0-9a-f]{40})$/m.exec(stderr)
  assert.ok(match?.[1])
  return match[1]
}

test('macOS encerra Hub e motores em outros grupos, mesmo após a saída do líder', { skip: !darwin }, async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'hub-process-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const supervisor = join(directory, 'supervisor')
  const engine = join(directory, 'engine')
  const build = await import('../../tools/fork-build-hub-supervisor.mjs' as string)
  build.buildHubSupervisor(supervisor, [], ['HUB_EOF_MS=400', 'HUB_TERM_MS=250', 'HUB_KILL_MS=4000'])
  execFileSync('/usr/bin/clang', ['-std=c11', '-D_DARWIN_C_SOURCE', '-Wall', '-Wextra', '-Werror', fixture, '-o', engine], { stdio: 'inherit' })
  execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', engine], { stdio: 'inherit' })
  const expected = cdhash(supervisor)
  const attest = (pid: number) => attestHubProcess(pid, expected)
  const neighbour = spawn('/bin/sleep', ['25'], { detached: true, stdio: 'ignore', env: {} })
  t.after(async () => {
    if (neighbour.exitCode === null && neighbour.signalCode === null) {
      const closed = once(neighbour, 'close')
      neighbour.kill()
      await closed
    }
  })
  const state = async (marker: string) => {
    for (let i = 0; i < 200; i++) {
      const text = await readFile(marker, 'utf8').catch(() => '')
      if (text.endsWith('\n')) return text.trim().split(' ').map(Number)
      await delay(10)
    }
    assert.fail('o motor não informou seus grupos')
  }

  for (const mode of ['stay', 'exit']) {
    await t.test(mode, async (inner) => {
      const marker = join(directory, mode)
      const hub = spawnHubProcess(supervisor, engine, [marker, mode], { env: {} }, attest)
      inner.after(() => hub.stop())
      const pid = await hub.started
      const [leaf, pgid, sameGroup, session] = await state(marker)
      assert.ok(leaf && pgid && sameGroup && session && neighbour.pid)
      assert.notEqual(pgid, pid)
      assert.notEqual(pgid, leaf)
      assert.notEqual(pgid, session)
      assert.notEqual(pid, session)
      assert.doesNotThrow(() => process.kill(leaf, 0))
      assert.doesNotThrow(() => process.kill(sameGroup, 0))
      if (mode === 'exit') await hub.exited
      const stopping = hub.stop()
      assert.equal(hub.stop(), stopping)
      await stopping
      for (const target of [pid, leaf, sameGroup]) {
        const until = performance.now() + 2000
        for (;;) {
          try { process.kill(target, 0) } catch (error) {
            assert.equal((error as NodeJS.ErrnoException).code, 'ESRCH')
            break
          }
          assert.ok(performance.now() < until, 'o processo continuou visível após o supervisor confirmar a parada')
          await delay(10)
        }
      }
      assert.doesNotThrow(() => process.kill(neighbour.pid!, 0))
    })
  }

  await t.test('uma recusa no exec não deixa o supervisor esperando outro processo', async () => {
    const hub = spawnHubProcess(supervisor, '/not-a-real-hub', [], { env: {} }, attest)
    await assert.rejects(hub.started, /hub_supervisor_invalid/)
    await hub.exited
    await hub.stop()
  })

  await t.test('ausência ambígua recusa a parada em vez de sinalizar o grupo âncora', async () => {
    const marker = join(directory, 'anchor')
    const hub = spawnHubProcess(supervisor, engine, [marker, 'anchor'], { env: {} }, attest)
    await hub.started
    const [pid] = await state(marker)
    assert.ok(pid)
    await hub.exited
    for (let i = 0; i < 2; i++) await assert.rejects(hub.stop(), /hub_process_group_still_alive/)
    assert.doesNotThrow(() => process.kill(pid, 0))
  })

  await t.test('o binário recusado não recebe a ordem de iniciar o Hub', async () => {
    const marker = join(directory, 'denied')
    const hub = spawnHubProcess(supervisor, engine, [marker, 'stay'], { env: {} }, pid => attestHubProcess(pid, cdhash(engine)))
    await assert.rejects(hub.started, /hub_bundle_signature_invalid/)
    await hub.exited
    for (let i = 0; i < 2; i++) await assert.rejects(hub.stop(), /hub_process_group_still_alive/)
    await assert.rejects(readFile(marker), { code: 'ENOENT' })
  })
})

test('o bootstrap não lê credenciais sem atestação nem publica após perdê-la', async (t) => {
  for (const failAt of [1, 2, 3, 0]) {
    await t.test('atestação ' + failAt, async () => {
      const events: string[] = []
      let attestations = 0
      const dummy = '/fixture'
      const resources = join(dummy, 'Contents', 'Resources')
      const binary = join(resources, 'donnabot-host')
      const { launchPrivateHub } = loadMainModule<typeof import('./fork-private-hub.ts')>('fork-private-hub', {
        'node:fs': {
          constants: {},
          mkdtempSync: () => join(dummy, 'session'),
          lstatSync: () => ({ uid: 42, mode: 0o600, isSocket: () => true, isDirectory: () => false }),
          openSync: () => { events.push('token'); return 1 },
          fstatSync: () => ({ size: 64 }),
          readFileSync: () => 'a'.repeat(64),
          closeSync: () => {},
        },
        'node:path': { join },
        'node:timers/promises': { setTimeout: delay },
        '../shared/fork-private-files': { privateDirectory: (p: string) => p, validatePrivateFile: () => {} },
        './fork-hub-signature': {
          verifyBundledHub: () => ({ binary, cdhash: 'fixture', sha256: 'fixture' }),
          attestHubProcess: (pid: number) => {
            assert.equal(pid, 123)
            events.push('atestação')
            if (++attestations === failAt) throw new Error('assinatura recusada')
          },
        },
        './fork-hub-process': {
          spawnHubProcess: () => ({ started: Promise.resolve(123), exited: new Promise(() => {}), stop: async () => {} }),
        },
        './fork-proved-loopback': {
          provarSocket: async () => {
            events.push('prova')
            return { ok: true, environment: 'dev', computerId: 'fixture', binaryPath: binary, binaryHash: 'fixture' }
          },
        },
      }, { process: { resourcesPath: resources, getuid: () => 42 }, Buffer, AbortController })
      const hub = launchPrivateHub({
        executable: 'donnabot-host', environment: 'dev', dataDir: dummy, temp: dummy, cwd: dummy,
        command: () => ({ args: [], env: {} }),
      })
      try {
        if (failAt) await assert.rejects(hub.ready, /assinatura recusada/)
        else assert.equal((await hub.ready).computerId, 'fixture')
        assert.deepEqual(events, failAt === 1 ? ['atestação'] : failAt === 2 ? ['atestação', 'atestação'] :
          ['atestação', 'atestação', 'token', 'prova', 'atestação'])
      } finally {
        await hub.stop()
      }
    })
  }
})
