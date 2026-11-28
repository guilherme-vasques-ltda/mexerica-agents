import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { setTimeout as delay } from 'node:timers/promises'
import { spawnHubProcess } from './fork-hub-process.ts'

const supported = process.platform === 'darwin' || process.platform === 'linux'

test('o supervisor só conclui depois da saída do Hub e do descendente do grupo', { skip: !supported }, async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'hub-process-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const marker = join(directory, 'ready')
  const hub = spawnHubProcess(process.execPath, ['-e', `
    const { spawn } = require('node:child_process')
    const { writeFileSync } = require('node:fs')
    const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' })
    process.on('SIGTERM', () => {})
    child.on('exit', () => process.exit(0))
    child.on('spawn', () => writeFileSync(process.argv[1], String(child.pid)))
  `, marker], { env: {} }, { eof: 50, terminate: 2000, kill: 2000, probe: 10 })
  t.after(() => hub.stop())
  let child: number | undefined
  const end = performance.now() + 5000
  while (performance.now() < end) {
    const value = Number(await readFile(marker, 'utf8').catch(() => ''))
    if (value > 0) { child = value; break }
    await delay(20)
  }
  assert.ok(child)
  assert.doesNotThrow(() => process.kill(child!, 0))
  const first = hub.stop()
  assert.equal(hub.stop(), first)
  await first
  assert.throws(() => process.kill(child!, 0), { code: 'ESRCH' })
})

test('uma recusa no exec não deixa o supervisor esperando outro processo', { skip: !supported }, async () => {
  const hub = spawnHubProcess('/not-a-real-hub', [], { env: {} })
  await hub.exited
  await hub.stop()
})
