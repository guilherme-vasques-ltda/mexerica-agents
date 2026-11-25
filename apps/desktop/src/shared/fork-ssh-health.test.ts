import assert from 'node:assert/strict'
import { createHash, createHmac, randomBytes, timingSafeEqual } from 'node:crypto'
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
const ROTULO = 'mexerica/health-proof/v1'
const SIGN_KEY = Buffer.alloc(32, 7).toString('base64url')
const COMPUTER = createHash('sha256').update(Buffer.alloc(32, 7)).digest().subarray(0, 16).toString('base64url')

const prova = (token: string, nonce: string) => createHmac('sha256', token).update(ROTULO).update(Buffer.from(nonce, 'base64url')).digest('hex')

/** Um Hub de mentira atrás do túnel: a identidade só sai com a prova do token que ele conhece, e
 * nada de credencial pode chegar até ele sem essa prova. */
function hubOf(token: string, impostor = false) {
  const seen: { url: string; authorization: string | undefined }[] = []
  const servers = new Set<Server>()
  return {
    seen,
    /** Sobe o Hub na porta que o túnel encaminha e devolve o servidor. */
    listen: (port: number) => new Promise<Server>((resolve) => {
      const server = createServer((request, response) => {
        seen.push({ url: request.url ?? '', authorization: request.headers.authorization })
        const query = new URL(request.url ?? '/', 'http://127.0.0.1').searchParams
        const nonce = query.get('nonce') ?? ''
        const proved = query.get('proof') === prova(token, nonce)
        const identidade = ['', COMPUTER, '', ''].join('\n')
        const receipt = createHmac('sha256', token).update('mexerica/health-receipt/v1').update(Buffer.from(nonce, 'base64url')).update(identidade).digest('hex')
        response.writeHead(query.has('proof') && !proved ? 401 : 200, { 'content-type': 'application/json' })
        // O impostor conhece os campos públicos e os devolve, mas não tem o token para a prova nem o recibo.
        if (impostor) {
          response.end(JSON.stringify({ ok: true, environment: 'main', busy: false, computerId: COMPUTER }))
          return
        }
        response.end(JSON.stringify(proved ? { ok: true, environment: 'main', busy: false, computerId: COMPUTER, receipt } : { ok: true, environment: 'main', busy: false }))
      })
      servers.add(server)
      server.listen(port, '127.0.0.1', () => resolve(server))
    }),
    close: () => Promise.all([...servers].map((server) => new Promise<void>((resolve) => server.close(() => resolve())))),
  }
}

function rig(options: { infoToken?: string; tunnelPid?: number; impostor?: boolean } = {}) {
  const hub = hubOf(TOKEN, options.impostor ?? false)
  const handlers = new Map<string, (...args: unknown[]) => unknown>()
  const profiles = [{ id: 'fixture', host: 'ssh.example.invalid', user: null, port: null, remotePort: 19422, identityFile: null, name: '', computerId: COMPUTER }]
  const info = { name: 'computador-ficticio', computerId: COMPUTER, signKey: SIGN_KEY, boxKey: null, token: options.infoToken ?? TOKEN, running: true }
  const tunnels: EventEmitter[] = []
  /** Quem o `lsof` deste teste diz que é dono de cada porta encaminhada. */
  const owning = new Map<number, number | undefined>()
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
      // O servidor deste teste roda neste processo; quando o cenário é "a porta não é do ssh", o
      // `lsof` aponta outro dono.
      owning.set(Number(args[1]), 'tunnelPid' in options ? 4123 : process.pid)
      tunnels.push(child)
      void hub.listen(Number(args[1]))
    } else if (args[0] === '-a') {
      // `lsof -a -n -P -p <pid> -iTCP@127.0.0.1:<porta> -sTCP:LISTEN -t`: só a porta que o próprio
      // túnel deste teste atende aparece com o pid dele, como no sistema de verdade.
      const port = Number(/@127\.0\.0\.1:(\d+)/.exec(args.join(' '))?.[1])
      const owner = owning.get(port)
      if (owner === undefined) {
        queueMicrotask(() => child.emit('close', 1))
      } else {
        answer(`${owner}\n`)
      }
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
    'node:crypto': { createHash, createHmac, randomBytes, timingSafeEqual },
    'node:net': { createServer: createTcpServer },
    'node:path': path,
    'node:fs': {
      // `lsof` presente: é o caminho que existe no macOS e no Windows, não só o `/proc` do Linux.
      existsSync: (file: string) => file === '/fixture/known_hosts' || file === '/usr/sbin/lsof',
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
  assert.deepEqual(r.seen.map((call) => call.authorization), [undefined, undefined], 'no credential travels')
  assert.ok(r.seen.every((call) => !call.url.includes(TOKEN)), 'nor appears in the query')
  assert.equal(r.seen[0]!.url, '/health', 'the readiness probe is anonymous')
  assert.ok(new URL(r.seen[1]!.url, 'http://127.0.0.1').searchParams.has('proof'), 'the identity probe carries the proof')
  r.controller.disconnectAll()
})

test('a port taken by something else never receives the host token', async () => {
  // O processo do túnel não é o dono do listener: o Hub de mentira responde, mas o ssh não prova nada.
  const r = rig({ tunnelPid: 999_999 })
  r.connect()
  await r.wait()
  assert.deepEqual(r.seen.filter((call) => call.authorization !== undefined), [], 'no credential is ever sent')
  assert.ok(r.seen.every((call) => !new URL(call.url, 'http://127.0.0.1').searchParams.has('proof')), 'and no proof either: the port is not the ssh tunnel')
  assert.notEqual(r.status()?.kind, 'connected')
  r.controller.disconnectAll()
})

test('a listener that answers the public identity without the token is not accepted', async () => {
  const r = rig({ impostor: true })
  r.connect()
  await r.wait()
  assert.notEqual(r.status()?.kind, 'connected', 'a forged identity does not attach')
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
