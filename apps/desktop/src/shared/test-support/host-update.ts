// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { EventEmitter } from 'node:events'
import * as crypto from 'node:crypto'
import * as path from 'node:path'
import { environmentProfile, hostEnvironmentError, isCodyncDataDir, isCodyncPort } from '../environment.ts'
import { forkFeatures } from '../fork-features.ts'
import { rcIdentity } from '../fork-rc-profile.ts'
import { THROTTLE_SECONDS, expectedProgramArguments, launchdServiceAbsent, localHostOff, macPlistValid, uninstallArgs } from '../fork-mac-local-hub.ts'
import { loadMainModule, settle } from './main-module.ts'

export function hostUpdateFixture(options: { environment?: 'main' | 'dev'; rc?: boolean; platform?: string; devPort?: string; devPortVar?: string; env?: Record<string, string>; existing?: string[]; installed?: boolean; missingBinary?: boolean; restartMarker?: boolean; localHost?: boolean; macLocalHub?: boolean; hubLocation?: string; canUseHost?: boolean } = {}) {
  const identity = environmentProfile(options.environment ?? 'main')
  const platform = options.platform ?? 'darwin'
  const binary = platform === 'win32' ? '/bundle/donnabot-host.exe' : platform === 'linux' ? '/home/usuario/.local/bin/donnabot-host' : '/bundle/donnabot-host'
  const binaryBytes = Buffer.from('new signed host fixture')
  const persisted: Record<string, unknown> = { hostRestartAfterAppUpdate: options.restartMarker ?? false, ...(options.hubLocation ? { hubLocation: options.hubLocation } : {}) }
  const calls: string[][] = []
  const reads: string[] = []
  const probes: string[][] = []
  const warnings: string[] = []
  const removed: string[] = []
  const order: string[] = []
  const timers = new Map<number, { callback: () => void; ms: number }>()
  let timerId = 0
  const behavior = {
    stopStatus: 0,
    /** O token que o app lê do Hub (`null`: arquivo ilegível). O `/health` só mostra identidade com ele. */
    token: 'fixture-token' as string | null,
    installStatus: 0,
    installed: options.installed ?? true,
    busy: false,
    reachable: true,
    wrongBinary: false,
    binaryEnvironment: identity.environment as string,
    runningEnvironment: identity.environment as string,
    bootoutStatus: 0,
    loaded: options.installed ?? true,
    rmFails: false,
    /** `ProgramArguments` do plist do Mac; `null` usa a lista esperada do modo "só neste Mac". */
    plistArgs: null as string[] | null,
    onInstall: null as (() => void) | null,
  }
  const plist = () => {
    const args = behavior.plistArgs ?? expectedProgramArguments(binary, 19422)
    return `<plist><dict><key>Label</key><string>${identity.hostLabel}</string>\n  <key>ProgramArguments</key>\n  <array>${args.map((a) => `<string>${a}</string>`).join('')}</array><key>ThrottleInterval</key><integer>${THROTTLE_SECONDS}</integer></dict></plist>`
  }
  const service = () => {
    if (!behavior.installed) throw new Error('No service')
    if (platform === 'darwin' && options.macLocalHub) return plist()
    return binary.replace(/donnabot-host\.exe$/, identity.supervisor)
  }
  const fs = {
    existsSync: (name: string) => { reads.push(name); return (options.existing ?? []).includes(name) || (!options.missingBinary && name === binary) },
    realpathSync: (name: string) => name,
    readFileSync: (name: string) => { reads.push(name); return name.endsWith('host-prefs.json') ? JSON.stringify(persisted) : service() },
    mkdirSync: () => {},
    writeFileSync: (_name: string, value: string) => { Object.assign(persisted, JSON.parse(value)) },
    promises: {
      readFile: async (name: string) => {
        if (!name.endsWith('/token')) return binaryBytes
        if (behavior.token === null) throw new Error('token ausente')
        return behavior.token
      },
      rm: async (name: string) => { if (behavior.rmFails) throw new Error('permission denied'); removed.push(name); behavior.installed = false },
    },
  }
  const module = loadMainModule<typeof import('../../main/host-controller.ts')>('host-controller', {
    'node:events': { EventEmitter }, 'node:crypto': crypto, 'node:path': path, 'node:fs': fs,
    'node:os': { homedir: () => '/home/usuario', platform: () => platform },
    'node:child_process': {
      execFileSync: () => { reads.push('registry'); return service() },
      execFile: (_bin: string, args: string[], _options: unknown, callback: (error: { code: number } | null, stdout: string, stderr: string) => void) => {
        if (args[0] === 'compat') {
          callback(null, JSON.stringify({ environment: behavior.binaryEnvironment }), '')
          return
        }
        if (args[0] === 'print') {
          probes.push([...args])
          callback(behavior.loaded ? null : { code: 113 }, behavior.loaded ? 'service' : '', behavior.loaded ? '' : 'Could not find service' /* textos-ok: saída simulada do launchctl */)
          return
        }
        calls.push([...args]); order.push(args[0]!)
        if (args[0] === 'bootout') {
          if (behavior.bootoutStatus === 0) behavior.loaded = false
          callback(behavior.bootoutStatus ? { code: behavior.bootoutStatus } : null, '', behavior.bootoutStatus ? 'Boot-out failed: 5' /* textos-ok: saída simulada do launchctl */ : '')
          return
        }
        const status = args[0] === 'stop' ? behavior.stopStatus : behavior.installStatus
        if (args[0] === 'install') {
          behavior.onInstall?.()
          if (!status) { behavior.installed = true; behavior.loaded = true }
        }
        callback(status ? { code: status } : null, '', status ? 'service command failed' : '')
      },
    },
    electron: { app: { isPackaged: true, getPath: () => '/prefs' } },
    './environment': { identity: options.rc ? rcIdentity : identity, isRc: options.rc ?? false, localIdentity: identity,
      hostDataDir: path.join('/home/usuario', options.rc ? rcIdentity.dataFolder : identity.dataFolder) },
    '../shared/environment': { hostEnvironmentError, isCodyncDataDir, isCodyncPort },
    // O fluxo herdado do host local, por padrão; `localHost: false` é o corte do Donnabot (D76).
    '../shared/fork-features': { officialDesktop: options.rc ?? false, forkFeatures: { ...forkFeatures, localHost: options.localHost ?? true, macLocalHub: options.macLocalHub ?? false } },
    '../shared/fork-mac-local-hub': { THROTTLE_SECONDS, launchdServiceAbsent, localHostOff, macPlistValid, uninstallArgs },
    './screen': { unregisterScreenAgent: () => { order.push('unregisterScreen') } },
  }, {
    __dirname: '/app/out/main',
    console: { warn: (message: string) => warnings.push(message), log: () => {}, error: () => {} },
    process: { env: { ...(options.devPort ? { [options.devPortVar ?? 'DONNABOT_PORT']: options.devPort } : {}), ...options.env }, resourcesPath: '/bundle', getuid: () => 501 },
    setTimeout: (callback: () => void, ms: number) => { const id = ++timerId; timers.set(id, { callback, ms }); return id },
    clearTimeout: (id: number) => timers.delete(id),
    fetch: async (url: string) => {
      reads.push('health')
      if (!behavior.reachable) throw new Error('Host offline')
      // Como o Hub: a identidade só vem com a prova do token local; sem ela, só o anônimo.
      const query = new URL(url, 'http://127.0.0.1').searchParams
      const nonce = query.get('nonce') ?? ''
      const proved = behavior.token !== null &&
        query.get('proof') === crypto.createHmac('sha256', behavior.token).update('mexerica/health-proof/v1').update(Buffer.from(nonce, 'base64url')).digest('hex')
      const anonymous = { ok: true, environment: behavior.runningEnvironment, busy: behavior.busy }
      return { ok: true, json: async () => proved ? { ...anonymous, computerId: 'fixture-computer', binaryPath: binary, binaryHash: behavior.wrongBinary ? 'stale' : crypto.createHash('sha256').update(binaryBytes).digest('hex') } : anonymous }
    },
  })
  const host = new module.HostController(() => options.canUseHost ?? true)
  const tick = async (ms: number) => {
    for (const [id, timer] of [...timers]) {
      if (timer.ms !== ms) continue
      timers.delete(id)
      timer.callback()
    }
    await settle()
  }
  return { host, probes, warnings, devPort: module.devPort, prefs: module.prefs, persisted, calls, order, behavior, tick, removed, reads }
}
