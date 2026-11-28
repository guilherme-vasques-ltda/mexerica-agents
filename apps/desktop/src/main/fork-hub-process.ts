import { spawn, type SpawnOptions } from 'node:child_process'
import { setTimeout as delay } from 'node:timers/promises'

const stillAlive = () => new Error('hub_process_group_still_alive')
const invalid = () => new Error('hub_supervisor_invalid')

export function spawnHubProcess(
  supervisor: string,
  executable: string,
  args: readonly string[],
  options: Pick<SpawnOptions, 'cwd' | 'env'>,
  attestSupervisor: (pid: number) => void,
  stopTimeout = 26000,
) {
  const child = spawn(supervisor, [executable, ...args], { ...options, detached: true, stdio: ['pipe', 'pipe', 'ignore'] })
  const started = Promise.withResolvers<number>()
  const exited = Promise.withResolvers<void>()
  let authorised = false
  let stopping: Promise<void> | undefined
  let closed = false
  const finished = new Promise<boolean>(resolve => child.once('close', (code) => {
    closed = true
    started.reject(invalid())
    exited.resolve()
    resolve(child.pid === undefined || (authorised && code === 0))
  }))
  const end = () => { child.stdin.end() }
  const protocolFailed = () => {
    started.reject(invalid())
    exited.resolve()
    end()
  }
  child.on('error', protocolFailed)
  child.stdin.on('error', protocolFailed)
  child.stdout.on('error', protocolFailed)
  child.once('spawn', () => {
    if (child.stdin.writableEnded) return
    try {
      if (child.pid === undefined) throw invalid()
      attestSupervisor(child.pid)
      authorised = true
      child.stdin.write('s')
    } catch (error) {
      started.reject(error)
      end()
    }
  })
  let buffer = ''
  let state: 'starting' | 'running' | 'exited' = 'starting'
  child.stdout.setEncoding('ascii').on('data', (chunk: string) => {
    buffer += chunk
    while (buffer.includes('\n')) {
      const newline = buffer.indexOf('\n')
      const line = buffer.slice(0, newline)
      buffer = buffer.slice(newline + 1)
      if (authorised && state === 'starting' && /^[1-9][0-9]{0,9}$/.test(line) && Number(line) !== child.pid) {
        state = 'running'
        started.resolve(Number(line))
        continue
      }
      if (state === 'running' && line === 'x') {
        state = 'exited'
        exited.resolve()
        continue
      }
      protocolFailed()
      return
    }
    if (buffer.length > 10) protocolFailed()
  })
  return {
    started: started.promise,
    exited: exited.promise,
    stop() {
      if (stopping) return stopping
      end()
      stopping = (async () => {
        const timer = new AbortController()
        try {
          const result = closed ? await finished : await Promise.race([
            finished,
            delay(stopTimeout, false, { signal: timer.signal }),
          ])
          if (!result) throw stillAlive()
        } finally {
          timer.abort()
        }
      })().catch((error: unknown) => {
        stopping = undefined
        throw error
      })
      return stopping
    },
  }
}
