import { spawn, type SpawnOptions } from 'node:child_process'
import { setTimeout as delay } from 'node:timers/promises'

interface StopDeadlines {
  eof: number
  terminate: number
  kill: number
  probe: number
}

const defaults: StopDeadlines = { eof: 15000, terminate: 3000, kill: 5000, probe: 50 }

export function spawnHubProcess(
  executable: string,
  args: readonly string[],
  options: Pick<SpawnOptions, 'cwd' | 'env'>,
  deadlines = defaults,
) {
  const child = spawn(executable, args, { ...options, detached: true, stdio: ['pipe', 'ignore', 'ignore'] })
  const pid = child.pid
  let closed = false
  const exited = new Promise<void>((resolve) => child.once('close', () => { closed = true; resolve() }))
  child.on('error', () => {})
  child.stdin?.on('error', () => {})

  function signalGroup(signal: NodeJS.Signals | 0) {
    if (pid === undefined) return false
    try {
      process.kill(-pid, signal)
      return true
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ESRCH') return false
      throw error
    }
  }

  async function goneWithin(ms: number) {
    const end = performance.now() + ms
    for (;;) {
      if (closed && !signalGroup(0)) return true
      const left = end - performance.now()
      if (left <= 0) return false
      await delay(Math.min(left, deadlines.probe))
    }
  }

  let stopping: Promise<void> | undefined
  let attempted = false
  return {
    exited,
    stop() {
      if (stopping) return stopping
      stopping = (async () => {
        if (attempted) {
          if (await goneWithin(0)) return
          throw new Error('hub_process_group_still_alive')
        }
        attempted = true
        child.stdin?.end()
        if (await goneWithin(deadlines.eof)) return
        signalGroup('SIGTERM')
        if (await goneWithin(deadlines.terminate)) return
        signalGroup('SIGKILL')
        if (await goneWithin(deadlines.kill)) return
        throw new Error('hub_process_group_still_alive')
      })().catch((error: unknown) => {
        stopping = undefined
        throw error
      })
      return stopping
    },
  }
}
