import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { EventEmitter } from 'node:events'
import { readFileSync, promises as realFs } from 'node:fs'
import { createServer, type Server } from 'node:http'
import { createServer as createTcpServer } from 'node:net'
import * as path from 'node:path'
import { test } from 'node:test'
import type { SSHState } from './ssh.ts'
import { loadMainModule, settle } from './test-support/main-module.ts'

/**
 * O túnel SSH de um computador: o `/health` anônimo só prova que a porta responde; a identidade
 * exige o token do Hub remoto, e ele só sai depois de o ssh provar que é o dono do listener.
 */
const TOKEN = 'token-local-ficticio'
const SIGN_KEY = Buffer.alloc(32, 7).toString('base64url')
const COMPUTER = createHash('sha256').update(Buffer.alloc(32, 7)).digest().subarray(0, 16).toString('base64url')

/** Um Hub de mentira atrás do túnel: a identidade só sai com o bearer que ele conhece. */
function hubOf(token: string) {
  const seen: (string | undefined)[] = []
  const servers = new Set<Server>()
  return {
    seen,
    /** Sobe o Hub na porta que o túnel encaminha e devolve o servidor. */
    listen: (port: number) => new Promise<Server>((resolve) => {
      const server = createServer((request, response) => {
        seen.push(request.headers.authorization)
        const identified = request.headers.authorization === `Bearer ${token}`
        response.writeHead(identified ? 200 : request.headers.authorization ? 401 : 200, { 'content-type': 'application/json' })
        response.end(JSON.stringify(identified ? { ok: true, environment: 'main', busy: false, computerId: COMPUTER } : { ok: true, environment: 'main', busy: false }))
      })
      servers.add(server)
      server.listen(port, '127.0.0.1', () => resolve(server))
    }),
    close: () => Promise.all([...servers].map((server) => new Promise<void>((resolve) => server.close(() => resolve())))),
  }
}

function rig(options: { infoToken?: string; tunnelPid?: number } = {}) {
  const hub = hubOf(TOKEN)
  const handlers = new Map<string, (...args: unknown[]) => unknown>()
  const profiles = [{ id: 'fixture', host: 'ssh.example.invalid', user: null, port: null, remotePort: 19422, identityFile: null, name: '', computerId: COMPUTER }]
  const info = { name: 'computador-ficticio', computerId: COMPUTER, signKey: SIGN_KEY, boxKey: null, token: options.infoToken ?? TOKEN, running: true }
  const tunnels: EventEmitter[] = []
  let authorized = false

  const spawn = (_bin: string, args: string[]) => {
    const child = Object.assign(new EventEmitter(), {
      stdout: new EventEmitter(),
      stderr: new EventEmitter(),
      stdin: { end: () => {} },
      pid: undefined as number | undefined,
      exitCode: null as number | null,
      signalCode: null as string | null,
      kill() {
        if (this.exitCode !== null) return
        this.exitCode = 0
        void hub.close()
      },
    })
    const answer = (stdout: string) => queueMicrotask(() => {
      child.stdout.emit('data', Buffer.from(stdout))
      child.emit('close', 0)
    })
    if (args[0] === 'config') answer('hostname ssh.example.invalid\nport 22\n')
    else if (args[0] === '-F') answer('ssh.example.invalid ssh-ed25519 Zml4dHVyYQ==\n')
    else if (args[0] === 'remote') answer(JSON.stringify(info))
    else if (args[0] === 'tunnel') {
      child.pid = 'tunnelPid' in options ? options.tunnelPid : process.pid
      tunnels.push(child)
      void hub.listen(Number(args[1]))
    }
    return child
  }

  const { registerSSH } = loadMainModule<typeof import('../main/ssh.ts')>('ssh', {
    './environment': { identity: {} },
    './host-controller': { dataDir: '/fixture' },
    './fork-desktop-profile': { DesktopSSHProfiles: class { load() { return profiles } } },
    './ssh-command': {
      resolveArguments: () => ['config'],
      knownHostsFiles: () => ['/fixture/known_hosts'],
      infoArguments: () => ['remote'],
      tunnelArguments: (_profile: unknown, port: number) => ['tunnel', String(port)],
    },
    'node:child_process': { spawn },
    'node:crypto': { createHash },
    'node:net': { createServer: createTcpServer },
    'node:path': path,
    'node:fs': {
      existsSync: (file: string) => file === '/fixture/known_hosts',
      readFileSync: (file: string) => file === '/fixture/ssh-profiles.json' ? JSON.stringify(profiles) : readFileSync(file),
      promises: { readFile: realFs.readFile, readdir: realFs.readdir, readlink: realFs.readlink },
    },
    'node:os': { homedir: () => '/fixture', platform: () => 'linux' },
    electron: {
      app: { getPath: () => '/fixture', on() {} },
      ipcMain: { on: (name: string, fn: (...args: unknown[]) => unknown) => handlers.set(name, fn), handle: (name: string, fn: (...args: unknown[]) => unknown) => handlers.set(name, fn) },
      dialog: {},
    },
  }, { Buffer, process, setTimeout, clearTimeout, fetch })

  const controller = registerSSH(() => [], () => authorized)
  const status = () => (handlers.get('ssh:state')!() as SSHState).status.fixture
  return {
    seen: hub.seen,
    controller,
    tunnels,
    status,
    connect() {
      authorized = true
      handlers.get('ssh:connect')!('', 'fixture')
    },
    /** Deixa o fluxo correr até o estado parar de mudar (o túnel nasce num spawn de verdade). */
    async wait() {
      for (let i = 0; i < 200 && status()?.kind === 'connecting'; i++) await new Promise((resolve) => setTimeout(resolve, 20))
      await settle()
      await hub.close()
    },
  }
}

test('the tunnel identity is read with the host token, after ssh proves it owns the port', async () => {
  const r = rig()
  r.connect()
  await r.wait()
  const status = r.status()
  assert.equal(status?.kind, 'connected', JSON.stringify(status))
  assert.equal(r.seen[0], undefined, 'the readiness probe carries no token')
  assert.deepEqual(r.seen.slice(1), [`Bearer ${TOKEN}`], 'the identity probe carries the host token')
  r.controller.disconnectAll()
})

test('a port taken by something else never receives the host token', async () => {
  // O processo do túnel não é o dono do listener: o Hub de mentira responde, mas o ssh não prova nada.
  const r = rig({ tunnelPid: 999_999 })
  r.connect()
  await r.wait()
  assert.deepEqual(r.seen.filter((authorization) => authorization !== undefined), [], 'no bearer leaves before the ssh ownership check')
  assert.notEqual(r.status()?.kind, 'connected')
  r.controller.disconnectAll()
})

test('a host that refuses the token is not the computer the profile points at', async () => {
  const r = rig({ infoToken: 'outro-token-ficticio' })
  r.connect()
  await r.wait()
  const status = r.status()
  assert.equal(status?.kind, 'failed', JSON.stringify(status))
  assert.equal(status?.kind === 'failed' ? status.motivo?.chave : null, 'ajustes.ssh.erro.tunelOutro')
  r.controller.disconnectAll()
})
