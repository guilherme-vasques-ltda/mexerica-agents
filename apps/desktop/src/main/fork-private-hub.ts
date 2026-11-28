import { closeSync, constants, fstatSync, lstatSync, mkdtempSync, openSync, readFileSync, rmdirSync, unlinkSync } from 'node:fs'
import { join } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import { privateDirectory, validatePrivateFile } from '../shared/fork-private-files'
import { spawnHubProcess } from './fork-hub-process'
import { verifyBundledHub } from './fork-hub-signature'
import { provarSocket } from './fork-proved-loopback'

function readToken(dataDir: string) {
  const path = join(privateDirectory(dataDir, false), 'token')
  const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK)
  try {
    const stat = fstatSync(fd)
    validatePrivateFile(stat)
    if (stat.size !== 64) throw new Error('hub_token_invalid')
    const token = readFileSync(fd, 'utf8')
    if (!/^[0-9a-f]{64}$/.test(token)) throw new Error('hub_token_invalid')
    return token
  } finally {
    closeSync(fd)
  }
}

export function launchPrivateHub(options: {
  executable: Parameters<typeof verifyBundledHub>[1]
  environment: string
  dataDir: string
  temp: string
  cwd: string
  command: (socket: string) => { args: string[]; env: NodeJS.ProcessEnv }
}) {
  const sessionDir = mkdtempSync(join(options.temp, 'h-'))
  const socketPath = join(sessionDir, 's')
  const originalDir = lstatSync(sessionDir)
  const cleanup = () => {
    try {
      const current = lstatSync(sessionDir)
      if (!current.isDirectory() || current.dev !== originalDir.dev || current.ino !== originalDir.ino) return
      try {
        const socket = lstatSync(socketPath)
        if (socket.isSocket() && socket.uid === originalDir.uid) unlinkSync(socketPath)
      } catch {}
      rmdirSync(sessionDir)
    } catch {}
  }
  const controller = new AbortController()
  let child: ReturnType<typeof spawnHubProcess>
  let identity: ReturnType<typeof verifyBundledHub>
  try {
    privateDirectory(sessionDir)
    if (Buffer.byteLength(socketPath) > 103) throw new Error('hub_socket_path_too_long')
    const { args, env } = options.command(socketPath)
    identity = verifyBundledHub(process.resourcesPath, options.executable)
    child = spawnHubProcess(identity.binary, args, { cwd: options.cwd, env })
  } catch (error) {
    cleanup()
    throw error
  }
  let stopping: Promise<void> | undefined
  const stop = () => {
    controller.abort()
    return stopping ??= child.stop().then(cleanup).catch((error: unknown) => {
      stopping = undefined
      throw error
    })
  }
  void child.exited.then(() => controller.abort())
  const ready = (async () => {
    const startup = AbortSignal.any([controller.signal, AbortSignal.timeout(30_000)])
    while (!startup.aborted) {
      try {
        const stat = lstatSync(socketPath)
        if (!stat.isSocket() || stat.uid !== process.getuid?.() || (stat.mode & 0o7777) !== 0o600) throw new Error('hub_socket_invalid')
        const token = readToken(options.dataDir)
        const health = await provarSocket(socketPath, token, AbortSignal.any([startup, AbortSignal.timeout(2_000)]))
        startup.throwIfAborted()
        if (health.ok !== true || health.environment !== options.environment || typeof health.computerId !== 'string' || !health.computerId ||
            health.binaryPath !== identity.binary || health.binaryHash !== identity.sha256) {
          throw new Error('hub_identity_invalid')
        }
        return { socketPath, token, signal: controller.signal, computerId: health.computerId }
      } catch {
        await delay(150, undefined, { signal: startup })
      }
    }
    throw new Error('hub_startup_failed')
  })()
  return { ready, exited: child.exited, stop }
}
