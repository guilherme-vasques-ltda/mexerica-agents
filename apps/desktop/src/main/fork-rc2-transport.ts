import { spawn, type ChildProcess } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import type { ClientRequest, IncomingMessage } from 'node:http'
import { request } from 'node:http'
import type { Rc2VerifiedArtifact } from './fork-rc2-artifact.ts'
import type { Rc2ChildHandle, Rc2Connection, Rc2Hello, Rc2Transport } from './fork-rc2-local-hub.ts'

const invalidBootstrap = () => new Error('invalid_rc2_bootstrap')

const lineLimit = 4096
const payloadLimit = 256
const streamLimit = 1_000_000

// O caminho do descritor fixado só existe onde a execução por descritor tem valor comprovado;
// sem base, não há transporte e o perfil segue inerte. O executável tem de ser binário: o kernel
// abre o objeto pelo descritor no exec; interpretador de script perderia o descritor no segundo exec.
export function rc2FdExecBase(platform: string): string | null {
  if (platform === 'linux') return '/proc/self/fd'
  return null
}

// Linha única de bootstrap do filho: JSON estrito com porta no intervalo não privilegiado.
// A porta declarada nunca vale autoridade: fora do intervalo o pai não conecta e o bearer
// não vaza para serviço privilegiado local.
export function rc2BootstrapPort(line: string): number {
  let parsed: unknown
  try { parsed = JSON.parse(line) } catch { throw invalidBootstrap() }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw invalidBootstrap()
  const candidate = parsed as Record<string, unknown>
  const keys = Object.keys(candidate)
  if (keys.length !== 2 || !keys.includes('v') || !keys.includes('port') || candidate.v !== 1) throw invalidBootstrap()
  const port = candidate.port
  if (typeof port !== 'number' || !Number.isInteger(port) || port < 1024 || port > 65535) throw invalidBootstrap()
  return port
}

export function rc2HelloPayload(body: string): Rc2Hello {
  let parsed: unknown
  try { parsed = JSON.parse(body) } catch { throw invalidBootstrap() }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw invalidBootstrap()
  const candidate = parsed as Record<string, unknown>
  const keys = Object.keys(candidate)
  if (keys.length !== 2 || !keys.includes('computerId') || !keys.includes('hostId')) throw invalidBootstrap()
  const computerId = candidate.computerId
  const hostId = candidate.hostId
  const valid = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && value.length <= payloadLimit
  if (!valid(computerId) || !valid(hostId)) throw invalidBootstrap()
  return { computerId, hostId }
}

export function rc2NativeTransport(verified: Rc2VerifiedArtifact): Rc2Transport | null {
  const base = rc2FdExecBase(process.platform)
  const hub = base ? verified.components.find(component => component.role === 'hub') : null
  if (!base || !hub) return null
  return signal => new Promise<Rc2Connection>((resolve, reject) => {
    let child: ChildProcess
    try {
      // Grupo próprio, sem env do login e stdout exclusivo do protocolo: nada do filho entra em log.
      child = spawn(`${base}/${hub.fd}`, [], { detached: true, stdio: ['pipe', 'pipe', 'ignore'], env: {} })
    } catch { reject(invalidBootstrap()); return }
    if (child.pid === null || child.pid === undefined) {
      // O exec nunca aconteceu: recusa limpa, nada vivo, recomeço permitido.
      child.on('error', () => reject(invalidBootstrap()))
      return
    }
    const pid = child.pid
    const bearer = randomBytes(32).toString('hex')
    const nonce = randomBytes(32).toString('hex')
    const inflight = new Set<ClientRequest>()
    let settled = false
    let bootstrapPort: number | null = null
    let buffer = Buffer.alloc(0)

    const exited = new Promise<void>((resolveExit, rejectExit) => {
      child.once('exit', () => resolveExit())
      child.once('error', () => rejectExit())
    })
    const survivors = (): number => {
      try { process.kill(-pid, 0); return 1 } catch (error) { return (error as NodeJS.ErrnoException).code === 'ESRCH' ? 0 : 1 }
    }
    const handle: Rc2ChildHandle = {
      // Pedido gracioso por EOF no stdin; os sinais vêm para o grupo inteiro, descendentes incluídos.
      end: () => { try { child.stdin?.end() } catch { /* o escalonamento decide */ } },
      terminate: () => { process.kill(-pid, 'SIGTERM') },
      kill: () => { process.kill(-pid, 'SIGKILL') },
      exited,
      survivors,
    }
    const settleRefusal = () => {
      if (settled) return
      settled = true
      signal.removeEventListener('abort', onAbort)
      for (const req of inflight) req.destroy()
      // Depois do pid, nenhuma falha rejeita: a conexão de recusa carrega o handle real e o
      // escalonamento do supervisor confirma a morte do grupo. hello nunca resolve — o prazo
      // do supervisor é que invalida a geração; nada além disso depende do filho.
      resolve({ hello: () => new Promise<Rc2Hello>(() => { }),
        async *events() { throw invalidBootstrap() },
        close: async () => { try { child.stdin?.end() } catch { /* o escalonamento decide */ } },
        child: handle })
    }
    const settleHealthy = (connection: Rc2Connection) => {
      if (settled) return
      settled = true
      signal.removeEventListener('abort', onAbort)
      resolve(connection)
    }
    const onAbort = () => settleRefusal()
    if (signal.aborted) settleRefusal()
    else signal.addEventListener('abort', onAbort, { once: true })

    let resumeLine: (() => void) | null = null
    const lineReady = new Promise<void>(resume => { resumeLine = resume })
    child.stdin?.on('error', () => { /* filho morto: o escalonamento confirma */ })
    child.stdout?.on('error', () => settleRefusal())
    child.stdout?.on('data', (chunk: Buffer) => {
      if (settled) return
      buffer = Buffer.concat([buffer, chunk])
      const newline = buffer.indexOf(0x0a)
      if (bootstrapPort === null) {
        if (newline < 0) {
          if (buffer.length > lineLimit) settleRefusal()
          return
        }
        const line = buffer.subarray(0, newline).toString('utf8')
        try { bootstrapPort = rc2BootstrapPort(line) } catch { return settleRefusal() }
      }
      // Guarda única de excesso: qualquer byte além da linha válida — no mesmo chunk ou em
      // qualquer um depois, pois a quebra antiga continua no buffer e o tamanho cresce.
      if (buffer.length > newline + 1) return settleRefusal()
      resumeLine?.()
    })
    child.stdin?.write(`${JSON.stringify({ nonce, bearer })}\n`)

    const helloOnce = (): Promise<Rc2Hello> => new Promise((resolveHello, rejectHello) => {
      if (bootstrapPort === null) { rejectHello(invalidBootstrap()); return }
      const port = bootstrapPort
      const req = request({ host: '127.0.0.1', port, path: '/hello', method: 'GET', agent: false,
        headers: { Host: `127.0.0.1:${port}`, Authorization: `Bearer ${bearer}` } }, (response: IncomingMessage) => {
        if (response.statusCode !== 200) { response.resume(); rejectHello(invalidBootstrap()); return }
        const chunks: Buffer[] = []
        response.on('data', (part: Buffer) => chunks.push(part))
        response.on('end', () => {
          try { resolveHello(rc2HelloPayload(Buffer.concat(chunks).toString('utf8'))) }
          catch (error) { rejectHello(error as Error) }
        })
      })
      inflight.add(req)
      req.on('error', () => rejectHello(invalidBootstrap()))
      req.on('close', () => inflight.delete(req))
      req.end()
    })

    void (async () => {
      try {
        // A abertura é corrida: filho morto antes da linha ou sem linha válida é recusa
        // carregada no handle; abort da geração já resolveu a recusa fora daqui.
        await Promise.race([exited.then(() => { throw invalidBootstrap() }), lineReady])
        if (settled || bootstrapPort === null) return
        // Abertura autenticada: só resolve conexão saudável depois de um hello com o bearer da geração.
        await Promise.race([exited.then(() => { throw invalidBootstrap() }), helloOnce()])
        if (settled) return
        const port = bootstrapPort
        const connection: Rc2Connection = {
          hello: helloOnce,
          child: handle,
          close: async () => { for (const req of inflight) req.destroy() },
          async *events() {
            const queue: string[] = []
            let failure: Error | null = null
            let done = false
            let wake: (() => void) | null = null
            const req = request({ host: '127.0.0.1', port, path: '/events', method: 'GET', agent: false,
              headers: { Host: `127.0.0.1:${port}`, Authorization: `Bearer ${bearer}` } }, (response: IncomingMessage) => {
              const contentType = String(response.headers['content-type'] ?? '')
              if (response.statusCode !== 200 || !contentType.startsWith('text/event-stream')) {
                response.resume(); failure = invalidBootstrap(); done = true; wake?.(); return
              }
              let pending = ''
              response.setEncoding('utf8')
              response.on('data', (part: string) => {
                pending += part
                if (pending.length > streamLimit) { failure = invalidBootstrap(); response.destroy(); done = true; wake?.(); return }
                for (;;) {
                  const boundary = pending.indexOf('\n\n')
                  if (boundary < 0) return
                  const block = pending.slice(0, boundary)
                  pending = pending.slice(boundary + 2)
                  const data = block.split('\n')
                    .filter(line => line.startsWith('data:'))
                    .map(line => line.slice(5).startsWith(' ') ? line.slice(6) : line.slice(5))
                    .join('\n')
                  if (data) queue.push(data)
                  wake?.()
                }
              })
              response.on('end', () => { done = true; wake?.() })
              response.on('error', () => { failure = invalidBootstrap(); done = true; wake?.() })
            })
            inflight.add(req)
            req.on('error', () => { failure = invalidBootstrap(); done = true; wake?.() })
            req.on('close', () => inflight.delete(req))
            req.end()
            try {
              for (;;) {
                while (queue.length > 0) yield queue.shift()!
                if (failure) throw failure
                if (done) return
                await new Promise<void>(resume => { wake = resume })
                wake = null
              }
            } finally { req.destroy() }
          },
        }
        settleHealthy(connection)
      } catch { settleRefusal() }
    })()
  })
}
