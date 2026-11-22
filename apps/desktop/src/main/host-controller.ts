// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { execFile, execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { EventEmitter } from 'node:events'
import { existsSync, mkdirSync, promises as fs, readFileSync, realpathSync, writeFileSync } from 'node:fs'
import { homedir, platform } from 'node:os'
import { basename, join, resolve } from 'node:path'
import { app } from 'electron'
import type { HostState, LocalHost } from '../shared/ipc'
import { unregisterScreenAgent } from './screen'
import { identity, hostDataDir } from './environment'
import { hostEnvironmentError, isCodyncDataDir, isCodyncPort } from '../shared/environment'
import { forkFeatures, officialDesktop } from '../shared/fork-features'
import { launchdServiceAbsent, localHostOff as localHostOffFor, macPlistValid, uninstallArgs, type UninstallLevel } from '../shared/fork-mac-local-hub'
import { envAlias } from '../shared/env-alias'
import type { HostSetupStage } from './analytics'

/** `MEXERICA_PORT` (antiga `DONNABOT_PORT`, ainda aceita) / `MEXERICA_HOME` point the app at a dev host started with `donnabot-host serve`. */
const devPortRaw = officialDesktop ? undefined : envAlias(process.env, 'MEXERICA_PORT', 'DONNABOT_PORT')
export const devPort = devPortRaw ? Number(devPortRaw) : null
export const port = devPort ?? identity.port
export const dataDir = hostDataDir
// Donnabot runs beside an installed Codync and Codync Dev: never their port or data folder.
if (isCodyncPort(port) || isCodyncDataDir(homedir(), resolve(dataDir))) {
  throw new Error(`${identity.name} requires its own host port and data directory, not Codync's.`)
}
export const baseURL = `http://127.0.0.1:${port}`
/**
 * Sem host neste computador (estado `disabled`; o serviço `app.donnabot.host` não é criado). Só a
 * chave de build `macLocalHub` com a escolha "Neste Mac" do usuário liga o Hub local, e só no Mac;
 * a escolha é lida a cada chamada. Um host de desenvolvimento (`DONNABOT_PORT`) continua valendo.
 */
const localHostOff = () =>
  localHostOffFor({ feature: forkFeatures.macLocalHub, isMac: platform() === 'darwin', devPort, read: (key) => prefs.get(key) }) &&
  !forkFeatures.localHost
/** Modo "Neste Mac" ativo: o serviço só vale com o plist do modo só-loopback (`--bind 127.0.0.1 --only-this-mac`). */
const macLocalMode = () => forkFeatures.macLocalHub && platform() === 'darwin' && prefs.get('hubLocation') === 'mac' && devPort === null

interface Health {
  environment?: string
  ok?: boolean
  computerId?: string
  binaryPath?: string
  binaryHash?: string
  busy?: boolean
}

export async function fetchHealth(url = baseURL, timeoutMs = 2000): Promise<Health | null> {
  try {
    const res = await fetch(`${url}/health`, { signal: AbortSignal.timeout(timeoutMs) })
    if (!res.ok) return null
    return (await res.json()) as Health
  } catch {
    return null
  }
}

export function run(bin: string, args: string[]): Promise<{ status: number; output: string }> {
  return new Promise((resolve) => {
    // A command that never finishes would leave the window on "Starting the host…" for good.
    execFile(bin, args, { maxBuffer: 8 * 1024 * 1024, windowsHide: true, timeout: 120_000 }, (error, stdout, stderr) => {
      const status = error ? (typeof error.code === 'number' ? error.code : 1) : 0
      resolve({ status, output: `${stdout}${stderr}`.trim() || (error?.killed ? `donnabot-host ${args[0]} didn't finish within 2 minutes.` : '') })
    })
  })
}

const isMac = platform() === 'darwin'
const isWindows = platform() === 'win32'
/** macOS and Windows ship the host inside the app; Linux uses the installed one. */
export const bundlesHost = isMac || isWindows

/** The service definition `donnabot-host install` wrote (`null`: not installed). */
function serviceDefinition(): string | null {
  try {
    if (isWindows) {
      // The per-user Run key that starts the host at sign-in.
      const key = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run'
      return execFileSync('reg', ['query', key, '/v', identity.windowsService], { encoding: 'utf8', windowsHide: true })
    }
    const file = isMac ? launchAgent() : join(homedir(), `.config/systemd/user/${identity.systemdUnit}`)
    return readFileSync(file, 'utf8')
  } catch {
    return null
  }
}

const launchAgent = () => join(homedir(), `Library/LaunchAgents/${identity.hostLabel}.plist`)
const serviceInstalled = () => serviceDefinition() !== null

/**
 * D76: tira o LaunchAgent do host deixado por um build anterior, pelo label e sem o binário (o app
 * local não traz host). Só no macOS: no Linux, o mesmo usuário pode estar rodando o próprio Hub.
 */
async function removeLaunchAgent() {
  if (!isMac) return
  const target = `gui/${process.getuid?.() ?? ''}/${identity.hostLabel}`
  // Versões anteriores podem ter apagado o plist deixando o serviço CARREGADO (com argumentos
  // antigos): o gate é o launchd, não o arquivo.
  const before = await run('/bin/launchctl', ['print', target])
  if (before.status !== 0 && !launchdServiceAbsent(before.status, before.output)) {
    throw new Error(`The host service couldn't be checked: ${before.output || 'launchctl failed'}`)
  }
  const loaded = before.status === 0
  if (!loaded && !serviceInstalled()) return
  const bootout = loaded ? await run('/bin/launchctl', ['bootout', target]) : { status: 0, output: '' }
  // "Já ausente" não é falha; o que vale é o serviço ter sumido (`print` falha por ausência).
  const print = await run('/bin/launchctl', ['print', target])
  if (!launchdServiceAbsent(print.status, print.output)) {
    throw new Error(`The host service is still loaded or couldn't be checked: ${bootout.output || print.output || 'launchctl failed'}`)
  }
  try {
    await fs.rm(launchAgent(), { force: true })
  } catch (error) {
    console.error(`host: couldn't remove ${launchAgent()}:`, error)
    throw new Error(`Couldn't remove the host service file: ${error instanceof Error ? error.message : String(error)}`)
  }
  if (serviceInstalled()) throw new Error("The host service file is still there after removing it.") /* textos-ok: diagnóstico técnico do serviço, não é texto de interface */
}

// textos-ok: diagnóstico técnico do serviço, não é texto de interface
const PLIST_NOT_LOOPBACK = "The host service wasn't installed in this-Mac-only mode."

/** A failed check of the host or its binary, and the `host_setup_failed` stage it counts as. */
class SetupError extends Error {
  constructor(readonly stage: HostSetupStage, message: string) {
    super(message)
  }
}

/**
 * Manages the local donnabot-host (binary, background service, health): bundled with the app,
 * else a Homebrew / cargo install. Emits `change`, and `setupFailed` (a `HostSetupStage`) when
 * it couldn't get a working host.
 */
export class HostController extends EventEmitter {
  state: HostState = { kind: 'starting' }
  local: LocalHost | null = null
  private loop: NodeJS.Timeout | null = null
  private generation = 0
  private installing = false
  /** Muda a cada remoção/escolha de "remoto": uma instalação em andamento que perdeu a vez se desfaz. */
  private installToken = 0
  private replacedStale = false
  private expectedHash: string | null = null
  private failures = 0
  private preparingForUpdate = false

  constructor(private readonly canUseHost = () => true) { super() }

  suspend() {
    this.installToken++
    this.stopLoop()
    this.setLocal(null)
  }

  /**
   * Only Donnabot's own host: the one inside the app, `MEXERICA_HOST_BIN` (or legacy `DONNABOT_HOST_BIN`), this checkout's builds
   * (dev) or ~/.local/bin. Never PATH, Homebrew or any `codync-host`: that is the installed Codync's.
   */
  get binary(): string | null {
    if (officialDesktop) {
      const bin = join(process.resourcesPath, isWindows ? 'donnabot-host.exe' : 'donnabot-host')
      return existsSync(bin) ? bin : null
    }
    const candidates = [
      app.isPackaged && isMac ? join(process.resourcesPath, 'donnabot-host') : null,
      app.isPackaged && isWindows ? join(process.resourcesPath, 'donnabot-host.exe') : null,
      envAlias(process.env, 'MEXERICA_HOST_BIN', 'DONNABOT_HOST_BIN') ?? null,
      ...(identity.environment === 'dev' ? [
        join(__dirname, '../../build/native/donnabot-host'),
        join(__dirname, '../../../../host/target/debug/donnabot-host'),
        join(homedir(), '.local/bin/donnabot-dev-host'),
      ] : [
        join(homedir(), '.local/bin/donnabot-host'),
      ]),
    ]
    return candidates.find((c): c is string => !!c && isDonnabotHost(c) && existsSync(c)) ?? null
  }

  get logPath() {
    return join(dataDir, 'host.log')
  }

  private setState(state: HostState) {
    this.state = state
    this.emit('change')
  }

  private setupFailed(stage: HostSetupStage, state: HostState) {
    this.emit('setupFailed', stage)
    this.setState(state)
  }

  private setLocal(local: LocalHost | null) {
    this.local = local
    this.emit('change')
  }

  start() {
    void this.refresh()
  }

  private uninstalled() {
    return prefs.get('hostUninstalled') === true
  }

  async refresh() {
    if (!this.canUseHost() || this.preparingForUpdate) return
    if (localHostOff()) {
      if (officialDesktop) return this.setState({ kind: 'disabled' })
      this.installToken++
      this.stopLoop()
      try {
        await removeLaunchAgent()
      } catch (error) {
        return this.setupFailed('install', { kind: 'failed', message: error instanceof Error ? error.message : String(error) })
      }
      return this.setState({ kind: 'disabled' })
    }
    if (!this.binary && devPort === null) return this.setupFailed('missing_binary', { kind: 'missingBinary' })
    if (devPort === null && prefs.get('hostRestartAfterAppUpdate') === true && serviceInstalled()) return this.install()
    if (devPort === null && !serviceInstalled()) {
      // The app is the way to run the host: set it up right away, unless the user took it out.
      if (this.uninstalled()) return this.setState({ kind: 'notInstalled' })
      return this.install()
    }
    // A service left by another copy of the app (moved, deleted or replaced) can't start this
    // app's host; the health check only catches that once something answers.
    if (devPort === null && this.binary && !serviceRuns(this.binary) && (macLocalMode() || !this.uninstalled())) return this.install()
    this.connect()
  }

  private async verifyBinary(bin: string) {
    const result = await run(bin, ['compat'])
    if (result.status !== 0) throw new SetupError('verify', result.output || 'Cannot verify the host environment.')
    const info = JSON.parse(result.output) as { environment?: string }
    const mismatch = hostEnvironmentError(identity.environment, info.environment)
    if (mismatch) throw new SetupError('environment', mismatch)
  }

  /** A service operation must never invoke the other environment's binary. */
  async install() {
    const bin = this.binary
    if (!this.canUseHost() || this.preparingForUpdate || this.installing || !bin || devPort !== null || localHostOff()) return
    this.installing = true
    const token = this.installToken
    // A escolha pode mudar durante as esperas: perdeu a vez se o token mudou ou o Hub local saiu.
    const cancelled = () => !this.canUseHost() || token !== this.installToken || localHostOff()
    let retry = false
    this.stopLoop()
    prefs.set('hostUninstalled', false)
    this.setState({ kind: 'starting' })
    try {
      await this.verifyBinary(bin)
      if (cancelled()) {
        retry = this.canUseHost() && !localHostOff()
        if (!retry) this.setState({ kind: 'disabled' })
        return
      }
      const result = await run(bin, ['install', '--port', String(port)])
      if (cancelled()) {
        // A escolha mudou durante o `install`: desfaz o que ele possa ter criado.
        if (this.canUseHost()) {
          if (localHostOff()) await this.undoInstall()
          else retry = true
        }
      } else if (result.status !== 0) {
        this.setupFailed('install', { kind: 'failed', message: result.output || 'Install failed' })
      } else if (macLocalMode() && !serviceRuns(bin)) {
        // Estado final conferido: o plist gravado tem de ser o do modo só-loopback.
        this.setupFailed('install', { kind: 'failed', message: PLIST_NOT_LOOPBACK })
      } else {
        await sleep(1000)
        if (cancelled()) {
          if (localHostOff()) await this.undoInstall()
          else retry = true
        } else this.connect()
      }
    } catch (error) {
      // Only the binary's checks throw here (unreadable `compat` output too).
      this.setupFailed(error instanceof SetupError ? error.stage : 'verify', { kind: 'failed', message: error instanceof Error ? error.message : String(error) })
    } finally {
      this.installing = false
      if (retry) void this.refresh()
    }
  }

  /** A escolha virou "remoto" com uma instalação em curso: tira o serviço e confirma o estado final. */
  private async undoInstall() {
    try {
      await removeLaunchAgent()
      this.setState({ kind: 'disabled' })
    } catch (error) {
      this.setupFailed('install', { kind: 'failed', message: error instanceof Error ? error.message : String(error) })
    }
  }

  restart() {
    if (devPort !== null) {
      this.setState({ kind: 'failed', message: 'Restart the manually started development host in its terminal.' })
      return
    }
    this.replacedStale = false
    void this.install()
  }

  /** One loopback API call from the main process; throws when the host isn't running. */
  async call<T>(method: string, params: object = {}, timeoutMs = 5000): Promise<T> {
    if (!this.canUseHost()) throw new Error('account_required')
    const local = this.local
    if (!local) throw new Error('The host isn’t running')
    const res = await fetch(`${local.baseURL}/api/${method}`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${local.token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(params),
      signal: AbortSignal.timeout(timeoutMs),
    })
    if (!res.ok) throw new Error(`${method}: HTTP ${res.status}`)
    return (await res.json()) as T
  }

  async uninstall(level: UninstallLevel = 1): Promise<boolean> {
    if (officialDesktop && localHostOff()) return true
    // Sem host neste computador, "Reset all data" não depende do binário (que o app local não traz).
    if (localHostOff()) {
      this.installToken++
      this.stopLoop()
      try {
        await removeLaunchAgent()
      } catch (error) {
        this.setState({ kind: 'failed', message: error instanceof Error ? error.message : String(error) })
        return false
      }
      return true
    }
    const bin = this.binary
    if (!bin || devPort !== null) return false
    try {
      await this.verifyBinary(bin)
      this.stopLoop()
      const result = await run(bin, uninstallArgs(level, isWindows))
      if (result.status !== 0) throw new Error(result.output || 'Uninstall failed')
      prefs.set('hostUninstalled', true)
      this.setLocal(null)
      this.setState({ kind: 'notInstalled' })
      return true
    } catch (error) {
      this.setState({ kind: 'failed', message: error instanceof Error ? error.message : String(error) })
      return false
    }
  }

  /**
   * Stops the service before the app replaces itself; `resumeAfterCancelledUpdate` undoes it.
   * macOS and Windows: there the host runs from inside the app (and Windows can't replace a
   * running program). Linux uses the installed host, which an app update leaves alone.
   */
  async prepareForUpdate() {
    if (devPort !== null || !bundlesHost) return
    if (this.installing) throw new Error('The host is being installed. Try again when it finishes.')
    this.preparingForUpdate = true
    if (serviceInstalled()) prefs.set('hostRestartAfterAppUpdate', true)
    this.stopLoop()
    unregisterScreenAgent()
    const bin = this.binary
    if (!bin) throw new Error('The bundled host is missing.')
    await this.verifyBinary(bin)
    const result = await run(bin, ['stop'])
    if (result.status !== 0) throw new Error(result.output || 'The old host could not be stopped.')
  }

  resumeAfterCancelledUpdate() {
    if (!this.preparingForUpdate) return
    this.preparingForUpdate = false
    if (serviceInstalled()) void this.install()
    else void this.refresh()
  }

  async isIdleForUpdate(): Promise<boolean> {
    if (this.installing || this.preparingForUpdate || devPort !== null) return false
    if (!serviceInstalled()) return true
    const health = await fetchHealth()
    return health?.busy === false
  }

  private stopLoop() {
    this.generation++
    if (this.loop) clearTimeout(this.loop)
    this.loop = null
  }

  /** Watches the host's health; the renderer's store handles the event stream itself. */
  private connect() {
    if (!this.canUseHost() || this.preparingForUpdate || localHostOff()) return
    this.stopLoop()
    const generation = this.generation
    this.failures = 0
    let attached = false
    let firstFailureAt = 0
    const tick = async () => {
      if (!this.canUseHost() || generation !== this.generation) return
      const problem = await this.attachLocal()
      // It may have started a reinstall (a stale build), which connects on its own.
      if (!this.canUseHost() || generation !== this.generation) return
      if (!problem) {
        attached = true
        this.failures = 0
        firstFailureAt = 0
        this.setState({ kind: 'running' })
        if (devPort === null && prefs.get('hostRestartAfterAppUpdate') === true) prefs.set('hostRestartAfterAppUpdate', false)
      } else {
        this.failures++
        if (this.failures === 1) firstFailureAt = Date.now()
        // The other environment's host doesn't become this one's by waiting: shown right away.
        // "Failed" keeps the old ~25 s of unanswered tries before it shows; the retry pace is faster.
        if (problem.stage === 'environment' || Date.now() - firstFailureAt > 25_000) this.setupFailed(problem.stage, { kind: 'failed', message: problem.message })
        else if (this.state.kind !== 'running') this.setState({ kind: 'starting' })
      }
      // Before the first attach a host that is only starting gets short, growing retries
      // (250 ms → 5 s); once attached, the watch keeps the 5 s it always had.
      if (generation === this.generation) this.loop = setTimeout(tick, attached ? 5000 : retryDelay(this.failures))
    }
    void (async () => {
      const bin = this.binary
      if (bin && devPort === null) this.expectedHash = await hashFile(bin)
      void tick()
    })()
  }

  /** Attaches the local host once it answers, and again when its token or identity changed; else says why not. */
  private async attachLocal(): Promise<SetupError | null> {
    // 4 s, not 2: a host that is merely slow still answers inside one try instead of never.
    const health = await fetchHealth(baseURL, 4000)
    if (!health?.computerId) return new SetupError('not_responding', "The host isn't responding. See the log for details.")
    const mismatch = hostEnvironmentError(identity.environment, health.environment)
    if (mismatch) return new SetupError('environment', mismatch)
    const token = await readToken()
    if (!token) return new SetupError('not_responding', `The host is running, but its token (${join(dataDir, 'token')}) can't be read.`)
    if (devPort === null) {
      // A rebuild can change the host without changing the release version.
      const bin = this.binary
      if (!this.expectedHash || !bin) return new SetupError('not_responding', "Couldn't read this app's host to check the one that's running.")
      const matches = health.binaryHash === this.expectedHash && samePath(health.binaryPath, resolved(bin))
      if (!matches) {
        if (!this.replacedStale) {
          this.replacedStale = true
          void this.install()
        }
        return new SetupError('not_responding', "Another build of the host is running, not this app's. Try again to replace it.")
      }
    }
    if (this.local?.computerId === health.computerId && this.local.token === token) return null
    this.setLocal({ computerId: health.computerId, baseURL, token })
    return null
  }
}

async function readToken(): Promise<string | null> {
  try {
    return (await fs.readFile(join(dataDir, 'token'), 'utf8')).trim() || null
  } catch {
    return null
  }
}

async function hashFile(path: string): Promise<string | null> {
  try {
    return createHash('sha256').update(await fs.readFile(path)).digest('hex')
  } catch {
    return null
  }
}

/** The OS's own resolution, like the host's `canonicalize` (JS `realpathSync` keeps 8.3 names and the input's case). */
function resolved(path: string) {
  try {
    return realpathSync.native(path)
  } catch {
    return path
  }
}

/** A Donnabot host binary by its file name (`donnabot-host`, `donnabot-dev-host`, `.exe`); never `codync-host`. */
export const isDonnabotHost = (path: string) => /^donnabot(-dev)?-host(\.exe)?$/i.test(basename(path))

/** Windows paths ignore case. */
const samePath = (a: string | undefined, b: string) => (isWindows ? a?.toLowerCase() === b.toLowerCase() : a === b)

/** Whether the installed service starts this binary. */
function serviceRuns(bin: string) {
  const service = serviceDefinition()
  if (service === null) return false
  // Windows' service starts the launcher next to the host (donnabot-hostw.exe).
  // Modo "Neste Mac": vale a lista de argumentos inteira, não só o caminho do binário.
  if (macLocalMode()) return macPlistValid(service, [bin, resolved(bin)], port)
  const runs = (path: string) => service.includes(isWindows ? path.replace(/donnabot-host\.exe$/, identity.supervisor) : path)
  return runs(bin) || runs(resolved(bin))
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

/** Retry spacing before the first attach: 250 ms, doubling to the 5 s watch pace. */
const retryDelay = (failures: number) => Math.min(5000, 250 * 2 ** Math.max(0, failures - 1))

/** Main-process preferences (a small JSON file in the app's data folder). */
export const prefs = (() => {
  const file = () => join(app.getPath('userData'), 'host-prefs.json')
  let cache: Record<string, unknown> | null = null
  const load = () => {
    if (cache) return cache
    try {
      cache = JSON.parse(readFileSync(file(), 'utf8')) as Record<string, unknown>
    } catch {
      cache = {}
    }
    return cache
  }
  return {
    get: (key: string) => load()[key],
    set: (key: string, value: unknown) => {
      const data = load()
      data[key] = value
      // Synchronous: a marker set right before the app quits (an update install) must be on disk.
      try {
        mkdirSync(app.getPath('userData'), { recursive: true })
        writeFileSync(file(), JSON.stringify(data))
      } catch (error) {
        console.error(`prefs: couldn't save ${key}:`, error)
      }
    },
  }
})()
