import assert from 'node:assert/strict'
import * as crypto from 'node:crypto'
import { EventEmitter } from 'node:events'
import * as path from 'node:path'
import { test } from 'node:test'
import type { SSHState } from './ssh.ts'
import { deferred, loadMainModule, settle } from './test-support/main-module.ts'

test('SSH does not start a remote command after authorization is revoked while reading a known key', async () => {
  const readingKey = deferred<EventEmitter & { stdout: EventEmitter }>()
  const handlers = new Map<string, (...args: string[]) => unknown>()
  const commands: string[] = []
  let authorized = false
  const { registerSSH } = loadMainModule<typeof import('../main/ssh.ts')>('ssh', {
    './environment': { identity: {} }, './host-controller': { dataDir: '/fixture' },
    './ssh-command': { resolveArguments: () => ['config'], knownHostsFiles: () => ['/fixture/known_hosts'], infoArguments: () => ['remote'] },
    'node:child_process': { spawn: (_bin: string, args: string[]) => {
      commands.push(args[0]!)
      const child = Object.assign(new EventEmitter(), { stdout: new EventEmitter() })
      if (args[0] === 'config') queueMicrotask(() => { child.stdout.emit('data', 'hostname ssh.example.invalid\nport 22\n'); child.emit('close', 0) })
      if (args[0] === '-F') readingKey.resolve(child)
      return child
    } },
    'node:crypto': crypto, 'node:net': {}, 'node:path': path,
    'node:fs': { existsSync: (file: string) => file === '/fixture/known_hosts', promises: {}, readFileSync: () => JSON.stringify([{ id: 'fixture', host: 'ssh.example.invalid', user: null, port: null, remotePort: 19422, identityFile: null }]) },
    'node:os': { homedir: () => '/fixture', platform: () => 'linux' },
    electron: { app: { getPath: () => '/fixture', on() {} }, ipcMain: { on: (name: string, fn: () => unknown) => handlers.set(name, fn), handle: (name: string, fn: () => unknown) => handlers.set(name, fn) } },
  }, { Buffer, process })
  const controller = registerSSH(() => [], () => authorized)
  assert.deepEqual(commands, [])
  authorized = true
  handlers.get('ssh:connect')!('', 'fixture')
  const key = await readingKey.promise
  authorized = false
  controller.disconnectAll()
  key.stdout.emit('data', 'ssh.example.invalid ssh-ed25519 Zml4dHVyZQ==\n')
  key.emit('close', 0)
  await settle()
  assert.deepEqual(commands, ['config', '-F'])
  assert.equal((handlers.get('ssh:state')!() as SSHState).status.fixture?.kind, 'idle')
})
