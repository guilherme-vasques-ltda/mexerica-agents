// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { writePrivateFile } from '../shared/fork-private-files'
import { identity, isRc } from './environment'
import { DesktopSSHProfiles } from './fork-desktop-profile'
import { spawn, type ChildProcess } from 'node:child_process'
import { createHash } from 'node:crypto'
import type { CapturedSSHLocal, LocalCloudIntent } from '../shared/fork-cloud-local'
import { existsSync, promises as fs, readFileSync, statSync } from 'node:fs'
import { createServer } from 'node:net'
import { homedir, platform } from 'node:os'
import { join } from 'node:path'
import { app, dialog, ipcMain, type BrowserWindow } from 'electron'
import { SSH_STORAGE_STATUS, type SSHAttachment, type SSHMotivo, type SSHProfile, type SSHState, type SSHStatus } from '../shared/ssh'
import { dataDir } from './host-controller'
import { localHealth, provarIdentidade } from './fork-local-health'
import { postProvado } from './fork-proved-loopback'
import { hubSSHAlias } from './fork-ssh-semente/hub-alias'
import { seedProfiles, sshAliasExists } from './fork-ssh-semente/semente'
import { infoArguments, knownHostsFiles, orphanTunnelPattern, resolveArguments, tunnelArguments } from './ssh-command'
import { COMANDO_INSTALAR_HOST, comandoSSH, motivoSSH } from '../shared/ssh-startup.ts'
import type { ChaveTexto } from '../renderer/lib/fork-i18n/index.ts'

// SSH computers: profiles, host key checks and one
// OpenSSH tunnel per connected profile. Arguments are always an argv array, never a shell string.

const SSH = '/usr/bin/ssh'
const KEYGEN = '/usr/bin/ssh-keygen'
const KEYSCAN = '/usr/bin/ssh-keyscan'
const LSOF = ['/usr/sbin/lsof', '/usr/bin/lsof'].find((p) => existsSync(p)) ?? null
const home = homedir()

// MARK: pure helpers

// D454 (fatia 4): passos e falhas vão com o motivo (chave do catálogo e parâmetros) e o mesmo texto em inglês;
// a tela mostra o motivo no idioma escolhido (shared/ssh-startup.ts).
type Params = Record<string, string | number>
const falha = (chave: ChaveTexto, params?: Params) => { const m = motivoSSH(chave, params); return { message: m.texto, motivo: m.motivo } }
const etapa = (chave: ChaveTexto) => { const m = motivoSSH(chave); return { step: m.texto, motivo: m.motivo } }

/** A reason the profile can't be used, or null. */
export function validate(p: SSHProfile): SSHMotivo | null {
  if (p.host.startsWith('-') || !/^[A-Za-z0-9._-]+$/.test(p.host)) {
    return motivoSSH('ajustes.ssh.erro.host').motivo
  }
  if (p.user !== null && !/^[A-Za-z_][A-Za-z0-9._-]*$/.test(p.user)) return motivoSSH('ajustes.ssh.erro.usuario').motivo
  const inRange = (n: number) => Number.isInteger(n) && n >= 1 && n <= 65535
  if (p.port !== null && !inRange(p.port)) return motivoSSH('ajustes.ssh.erro.porta').motivo
  if (!inRange(p.remotePort)) return motivoSSH('marca.ssh.portaFaixa').motivo
  if (isRc && p.remotePort !== identity.port) return motivoSSH('marca.ssh.portaRc', { porta: identity.port }).motivo
  if (p.identityFile !== null) {
    let ok = false
    try {
      ok = p.identityFile.startsWith('/') && statSync(p.identityFile).isFile()
    } catch {}
    if (!ok) return motivoSSH('ajustes.ssh.erro.chave').motivo
  }
  return null
}

/** The far side rejected our key (BatchMode never prompts, so a password or passphrase can't help). */
export const authRefused = (stderr: string) => stderr.includes('Permission denied') || stderr.includes('Too many authentication failures')

/** What `ssh -G` resolved the destination to. */
export interface Resolved {
  hostname: string
  port: number
  hostKeyAlias: string | null
  proxyJump: string | null
  proxyCommand: string | null
  inet6: boolean
}

const usesProxy = (r: Resolved) => r.proxyJump !== null || r.proxyCommand !== null
/** The name ssh looks up in known_hosts. */
export const knownHostsName = (r: Resolved) => {
  const name = r.hostKeyAlias ?? r.hostname
  return r.port === 22 ? name : `[${name}]:${r.port}`
}

export function parseConfig(text: string): Resolved | null {
  const values = new Map<string, string>()
  for (const line of text.split(/\r?\n/)) {
    const at = line.indexOf(' ')
    if (at < 0) continue
    const key = line.slice(0, at).toLowerCase()
    if (!values.has(key)) values.set(key, line.slice(at + 1).trim())
  }
  const set = (k: string) => {
    const v = values.get(k)
    return v && v !== 'none' ? v : null
  }
  const hostname = set('hostname')
  const port = Number(values.get('port'))
  if (!hostname || !Number.isInteger(port) || !values.get('port')) return null
  return { hostname, port, hostKeyAlias: set('hostkeyalias'), proxyJump: set('proxyjump'), proxyCommand: set('proxycommand'), inet6: values.get('addressfamily') === 'inet6' }
}

const keyscanArguments = (r: Resolved) => ['-T', '5', '-p', String(r.port), '--', r.hostname]

/** `type key` of each host key line (known_hosts or ssh-keyscan output); comments and revoked keys skipped. */
export function hostKeys(text: string, certAuthorities = true): Set<string> {
  const keys = new Set<string>()
  for (const line of text.split(/\r?\n/)) {
    const fields = line.split(/\s+/).filter(Boolean)
    const first = fields[0]
    if (!first || first.startsWith('#')) continue
    if (first.startsWith('@')) {
      if (!certAuthorities || first !== '@cert-authority') continue
      fields.shift()
    }
    if (fields.length >= 3) keys.add(`${fields[1]} ${fields[2]}`)
  }
  return keys
}

/** ssh-keyscan's lines, filed under the name ssh will look up. */
export const knownHostsLines = (scan: string, name: string) => [...hostKeys(scan)].sort().map((k) => `${name} ${k}`)

/** A computer id is the first 16 bytes of SHA-256 of its signing key, base64url. */
function consistent(id: string, signKey: string) {
  const key = Buffer.from(signKey, 'base64url')
  return key.length === 32 && createHash('sha256').update(key).digest().subarray(0, 16).toString('base64url') === id
}

// MARK: processes

interface Result {
  status: number
  stdout: string
  stderr: string
}

/** The last thing the tool said, for showing next to an error. */
const lastLine = (text: string) => text.split(/\r?\n/).filter(Boolean).pop() ?? ''

/** Runs a tool to completion; both pipes are drained while it runs. */
function run(bin: string, args: string[], input?: string): Promise<Result> {
  return new Promise((resolve) => {
    let stdout = ''
    let stderr = ''
    let child: ChildProcess
    try {
      child = spawn(bin, args, { stdio: [input === undefined ? 'ignore' : 'pipe', 'pipe', 'pipe'] })
    } catch {
      return resolve({ status: -1, stdout, stderr })
    }
    child.stdout?.on('data', (d: Buffer) => (stdout += d.toString()))
    child.stderr?.on('data', (d: Buffer) => (stderr += d.toString()))
    child.on('error', () => resolve({ status: -1, stdout, stderr }))
    child.on('close', (code) => resolve({ status: code ?? -1, stdout, stderr }))
    if (input !== undefined) child.stdin?.end(input)
  })
}

/** Tunnels a crashed or force-quit Codync left running: only Codync's tunnels carry this known_hosts file. */
const killOrphanTunnels = () => run('/usr/bin/pkill', ['-f', '--', orphanTunnelPattern(dataDir)])

/** Binds port 0 on loopback to learn a free port. */
function freeLocalPort(): Promise<number | null> {
  return new Promise((resolve) => {
    const server = createServer()
    server.once('error', () => resolve(null))
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      server.close(() => resolve(typeof address === 'object' && address ? address.port : null))
    })
  })
}

/** Whether process `pid` holds the listening socket on 127.0.0.1:`port`. */
async function sshListens(pid: number, port: number): Promise<boolean> {
  if (LSOF) {
    const found = await run(LSOF, ['-a', '-n', '-P', '-p', String(pid), `-iTCP@127.0.0.1:${port}`, '-sTCP:LISTEN', '-t'])
    return found.status === 0 && found.stdout.split(/\r?\n/).includes(String(pid))
  }
  if (platform() !== 'linux') return false
  // No lsof: the listening socket's inode from /proc/net/tcp, then whether `pid` holds it.
  try {
    const local = `0100007F:${port.toString(16).toUpperCase().padStart(4, '0')}`
    const inodes = (await fs.readFile('/proc/net/tcp', 'utf8'))
      .split('\n')
      .map((l) => l.trim().split(/\s+/))
      .filter((f) => f[1] === local && f[3] === '0A')
      .map((f) => `socket:[${f[9]}]`)
    if (inodes.length === 0) return false
    for (const fd of await fs.readdir(`/proc/${pid}/fd`)) {
      const link = await fs.readlink(`/proc/${pid}/fd/${fd}`).catch(() => '')
      if (inodes.includes(link)) return true
    }
  } catch {}
  return false
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

/** What `donnabot-host info --json` prints. */
interface RemoteInfo {
  name: string
  computerId: string
  signKey: string
  boxKey?: string | null
  token: string
  running: boolean
}

function parseInfo(text: string): RemoteInfo | null {
  try {
    const v = JSON.parse(text) as Partial<RemoteInfo>
    return typeof v.name === 'string' && typeof v.computerId === 'string' && typeof v.signKey === 'string' && typeof v.token === 'string' && typeof v.running === 'boolean'
      ? (v as RemoteInfo)
      : null
  } catch {
    return null
  }
}

// MARK: the manager

type Outcome = { kind: 'attached' } | { kind: 'stop'; status: SSHStatus } | { kind: 'retry'; message: string; motivo?: SSHMotivo }
interface Token {
  cancelled: boolean
}

const signInRefused = (p: SSHProfile) => falha('ajustes.ssh.erro.recusou', { host: p.host, comando: comandoSSH(p.host) })

/** Somente dados próprios, sem propriedades extras, protótipos ou getters. */
function exactRecord(value: unknown, keys: string[]): Record<string, unknown> | null {
  try {
    if (!value || typeof value !== 'object' || Array.isArray(value) || Object.getPrototypeOf(value) !== Object.prototype) return null
    const descriptors = Object.getOwnPropertyDescriptors(value)
    if (Reflect.ownKeys(value).length !== keys.length || keys.some((key) => !Object.prototype.hasOwnProperty.call(descriptors, key) || !('value' in descriptors[key]))) return null
    return Object.fromEntries(keys.map((key) => [key, descriptors[key].value]))
  } catch { return null }
}

function canonicalBytes(value: unknown, length: number): value is string {
  if (typeof value !== 'string' || !/^[A-Za-z0-9_-]+$/.test(value)) return false
  const bytes = Buffer.from(value, 'base64url')
  return bytes.length === length && bytes.toString('base64url') === value
}

/** Copia e valida antes da primeira espera; nenhum objeto de entrada vira body. */
function localCloudRoute(input: unknown): { method: string; body: object } | null {
  try {
    if (!input || typeof input !== 'object' || Array.isArray(input) || Object.getPrototypeOf(input) !== Object.prototype) return null
    const kind = Object.getOwnPropertyDescriptor(input, 'kind')
    if (!kind || !('value' in kind)) return null
    if (kind.value === 'hello' || kind.value === 'cloudStatus' || kind.value === 'accessRequests') {
      return exactRecord(input, ['kind']) ? { method: kind.value, body: {} } : null
    }
    if (kind.value === 'claimSign') {
      const v = exactRecord(input, ['kind', 'claimId', 'nonce', 'userId'])
      if (!v || typeof v.claimId !== 'string' || !/^clm_[A-Za-z0-9_-]{1,60}$/.test(v.claimId)
        || !canonicalBytes(v.nonce, 16) || typeof v.userId !== 'string' || v.userId.length < 1 || v.userId.length > 256
        || /[\x00-\x1f\x7f]/.test(v.userId)) return null
      return { method: 'claimSign', body: { claimId: v.claimId, nonce: v.nonce, userId: v.userId } }
    }
    if (kind.value === 'decideAccessRequest') {
      const v = exactRecord(input, ['kind', 'requestId', 'expect'])
      const expected = v && exactRecord(v.expect, ['deviceKey', 'code', 'transcript'])
      if (!v || typeof v.requestId !== 'string' || !/^req_[A-Za-z0-9_-]{1,60}$/.test(v.requestId) || !expected
        || !canonicalBytes(expected.deviceKey, 32) || typeof expected.code !== 'string' || !/^[0-9]{6}$/.test(expected.code)
        || !canonicalBytes(expected.transcript, 32)) return null
      return { method: 'decideAccessRequest', body: { requestId: v.requestId, approve: true,
        expect: { deviceKey: expected.deviceKey, code: expected.code, transcript: expected.transcript } } }
    }
  } catch { return null }
  return null
}

/** Um envio sem resposta é incerto; o consumidor não deve repeti-lo automaticamente. */
export class LocalCloudCallError extends Error {
  readonly kind: 'notSent' | 'unknown'
  constructor(kind: 'notSent' | 'unknown') { super(`Local cloud call: ${kind}`); this.kind = kind }
}

type LocalCaptureDeps = {
  profileId: string; computerId: string; signKey: string; baseURL: string; token: string
  current: () => boolean; listener: () => Promise<boolean>
}

/** Fábrica interna do Main; não é um ingresso IPC. */
export function createLocalCloudCapture(deps: LocalCaptureDeps): CapturedSSHLocal {
  const current = () => { try { return deps.current() === true } catch { return false } }
  return Object.freeze({
    profileId: deps.profileId, computerId: deps.computerId, signKey: deps.signKey, current,
    wasNotSent: (error: unknown) => error instanceof LocalCloudCallError && error.kind === 'notSent',
    async execute(intent: LocalCloudIntent): Promise<unknown> {
      const route = localCloudRoute(intent)
      if (!route || !current()) throw new LocalCloudCallError('notSent')
      let ownsListener = false
      try { ownsListener = await deps.listener() } catch { /* falha fechada */ }
      if (!current() || !ownsListener) throw new LocalCloudCallError('notSent')
      const payload = JSON.stringify(route.body)
      if (!current()) throw new LocalCloudCallError('notSent')
      let res: Awaited<ReturnType<typeof postProvado>> | undefined
      try {
        res = await postProvado(deps.baseURL, `/api/${route.method}`, deps.token, payload, 20_000, { atual: current })
        if (!current() || res.statusCode !== 200) throw new LocalCloudCallError('unknown')
        const stillListening = await deps.listener()
        if (!current() || !stillListening) throw new LocalCloudCallError('unknown')
        res.setEncoding('utf8')
        let text = ''
        for await (const chunk of res) text += chunk as string
        const value: unknown = JSON.parse(text)
        if (!current()) throw new LocalCloudCallError('unknown')
        const listenerAtDelivery = await deps.listener()
        if (!current() || !listenerAtDelivery) throw new LocalCloudCallError('unknown')
        return value
      } catch { throw new LocalCloudCallError('unknown') } finally {
        res?.destroy()
      }
    },
  })
}

class SSHComputers {
  profiles: SSHProfile[] = []
  private status = new Map<string, SSHStatus>()
  private attachments = new Map<string, SSHAttachment>()
  private tunnels = new Map<string, ChildProcess>()
  private tasks = new Map<string, Token>()
  private backoff = new Map<string, number>()
  private connectedAt = new Map<string, number>()
  /** Host key lines waiting for the user's "Trust and connect". */
  private pendingKeys = new Map<string, string[]>()
  private readonly profileStore: DesktopSSHProfiles | undefined
  private storageBlocked: 'ajustes.ssh.erro.perfisLeitura' | 'ajustes.ssh.erro.perfisDurabilidadeBloqueada' | undefined

  constructor(private file: string, private emit: (s: SSHState) => void, private canConnect: () => boolean) {
    this.profileStore = process.platform === 'win32' ? undefined : new DesktopSSHProfiles(file)
    try {
      if (!this.profileStore) {
        this.profiles = JSON.parse(readFileSync(file, 'utf8')) as SSHProfile[]
        return
      }
      const profiles = this.profileStore.load(!isRc)
      if (profiles !== null) {
        this.profiles = profiles
        return
      }
    } catch {
      if (this.profileStore) {
        this.storageBlocked = 'ajustes.ssh.erro.perfisLeitura'
        this.status.set(SSH_STORAGE_STATUS, { kind: 'failed', ...falha(this.storageBlocked) })
        return
      }
    }
    if (isRc) return
    // D76/D293: na primeira abertura, o Hub como computador SSH (editável ou removível em Computadores),
    // só se o alias existir no ~/.ssh/config desta máquina. Só no Donnabot: um build Dev nunca aponta para o Hub.
    const alias = hubSSHAlias(process.env)
    const seed = seedProfiles({ main: identity.environment === 'main', profilesFileExists: this.profileStore ? false : existsSync(file),
      aliasExists: () => sshAliasExists(alias, home), alias, port: identity.port })
    if (seed && !this.persist(seed, seed[0]?.id)) {
      this.profiles = seed
      if (!this.profileStore) this.persistWindows()
    }
  }

  get state(): SSHState {
    return { profiles: this.profiles, status: Object.fromEntries(this.status), attachments: [...this.attachments.values()] }
  }

  /**
   * As guardas de um túnel vivo para `profileId`: identidade do anexo, túnel e perfil atuais,
   * processo de pé e a porta ainda do ssh. `null` = não há captura válida agora.
   */
  private captureRaw(profileId: string, consumerCurrent: () => boolean) {
    const attachment = this.attachments.get(profileId)
    const child = this.tunnels.get(profileId)
    const status = this.status.get(profileId)
    const profile = this.profiles.find((entry) => entry.id === profileId)
    if (!attachment || !child || !profile || status?.kind !== 'connected' || child.pid === undefined
      || !canonicalBytes(attachment.computer.id, 16) || !canonicalBytes(attachment.computer.signKey, 32)
      || !consistent(attachment.computer.id, attachment.computer.signKey)
      || attachment.baseURL !== `http://127.0.0.1:${status.localPort}` || !Number.isInteger(status.localPort)
      || status.localPort < 1 || status.localPort > 65535) return null
    // Fail-closed como o wrapper de createLocalCloudCapture: callback que lança = captura morta.
    const current = () => {
      try {
        return consumerCurrent() === true && this.attachments.get(profileId) === attachment
          && this.tunnels.get(profileId) === child && this.status.get(profileId) === status
          && this.profiles.find((entry) => entry.id === profileId) === profile
          && child.exitCode === null && child.signalCode === null
      } catch {
        return false
      }
    }
    const listener = () => sshListens(child.pid!, status.localPort)
    return current() ? { attachment, current, listener } : null
  }

  captureLocal(profileId: string, consumerCurrent: () => boolean): CapturedSSHLocal | null {
    if (typeof profileId !== 'string' || typeof consumerCurrent !== 'function') return null
    const raw = this.captureRaw(profileId, consumerCurrent)
    if (!raw) return null
    const { attachment, current, listener } = raw
    const captured = createLocalCloudCapture({ profileId, computerId: attachment.computer.id, signKey: attachment.computer.signKey,
      baseURL: attachment.baseURL, token: attachment.token, current,
      listener })
    return captured.current() ? captured : null
  }

  /**
   * Captura crua do túnel do `computerId` para fluxos que não são intents (o cofre seguro, MX-209):
   * mesmas guardas de identidade, validade e posse do listener de `captureLocal`, mais `current()`
   * já incluindo a consumidora — desligar o túnel ou trocar a captura a invalida na hora.
   */
  captureRequest(computerId: string, consumerCurrent: () => boolean) {
    if (typeof computerId !== 'string' || typeof consumerCurrent !== 'function') return null
    for (const [profileId, attachment] of this.attachments) {
      if (attachment.computer.id !== computerId) continue
      const raw = this.captureRaw(profileId, consumerCurrent)
      if (!raw) return null
      return { baseURL: raw.attachment.baseURL, token: raw.attachment.token, current: raw.current, listener: raw.listener }
    }
    return null
  }

  private changed() {
    this.emit(this.state)
  }

  private setStatus(id: string, s: SSHStatus) {
    this.status.set(id, s)
    this.changed()
  }

  async connectAll() {
    if (this.storageBlocked || !this.canConnect()) return
    // Before this copy has tunnels of its own, so only leftovers from an earlier copy match.
    if (this.tunnels.size === 0 && this.tasks.size === 0) await killOrphanTunnels()
    for (const p of this.profiles) if (!this.tasks.has(p.id) && !this.tunnels.has(p.id)) this.connect(p.id)
  }

  save(profile: SSHProfile): SSHMotivo | null {
    const old = this.profiles.find((p) => p.id === profile.id)
    // A different address may be a different computer: learn its identity again.
    const p = old && (old.host !== profile.host || old.port !== profile.port) ? { ...profile, computerId: null } : profile
    const problem = validate(p)
    if (problem) return problem
    const next = old ? this.profiles.map((x) => (x.id === p.id ? p : x)) : [...this.profiles, p]
    const storage = this.persist(next, p.id)
    if (storage) return motivoSSH(storage).motivo
    if (old) this.disconnect(p.id)
    this.profiles = next
    if (!this.profileStore) this.persistWindows()
    this.connect(p.id)
    return null
  }

  remove(id: string) {
    if (this.profileStore && id === SSH_STORAGE_STATUS) return
    const next = this.profiles.filter((p) => p.id !== id)
    if (this.persist(next, id)) return
    this.disconnect(id)
    this.profiles = next
    this.status.delete(id)
    this.backoff.delete(id)
    if (!this.profileStore) this.persistWindows()
    this.changed()
  }

  connect(id: string) {
    if (this.storageBlocked || (this.profileStore && id === SSH_STORAGE_STATUS) || !this.canConnect()) return
    const old = this.tasks.get(id)
    if (old) old.cancelled = true
    const token: Token = { cancelled: false }
    this.tasks.set(id, token)
    void this.run(id, token)
  }

  /** Ends the tunnel for this session (bots on it go away until the next connect). */
  disconnect(id: string, notify = true) {
    if (this.profileStore && id === SSH_STORAGE_STATUS) return
    const task = this.tasks.get(id)
    if (task) task.cancelled = true
    this.tasks.delete(id)
    this.stopTunnel(id, notify)
    this.status.set(id, { kind: 'idle' })
    if (notify) this.changed()
  }

  disconnectAll() {
    for (const id of new Set([...this.tunnels.keys(), ...this.tasks.keys()])) this.disconnect(id)
  }

  /** Dados de perfil capturados pelo Main; não são prova de origem PL1. */
  personalProfile(id: string) {
    const profile=this.profiles.find(p=>p.id===id)
    return profile ? Object.freeze({...profile}) : null
  }

  /** The user compared the fingerprints: remember the key in Codync's known_hosts and go on. */
  async trustHostKey(id: string) {
    if (this.storageBlocked) return
    const lines = this.pendingKeys.get(id)
    if (this.status.get(id)?.kind !== 'confirmHostKey' || !lines) return
    try {
      writePrivateFile(join(dataDir, 'ssh_known_hosts'), lines.join('\n') + '\n', true)
      const profile=this.profiles.find(p=>p.id===id)
      // Falha parcial é unknownLegacy no PL1; não rompe a política SSH existente.
      if (!isRc && profile) { try { await (await import('./fork-cloud/personal-ssh')).recordPersonalTofu(dataDir,profile,lines) } catch {} }
    } catch (e) {
      return this.setStatus(id, { kind: 'failed', ...falha('ajustes.ssh.erro.salvarChave', { detalhe: e instanceof Error ? e.message : String(e) }) })
    }
    this.pendingKeys.delete(id)
    this.connect(id)
  }

  // MARK: connecting

  private async run(id: string, token: Token) {
    while (!token.cancelled) {
      const profile = this.profiles.find((p) => p.id === id)
      if (!profile) return
      const outcome = await this.attach(profile, token)
      // Disconnected or replaced meanwhile: the newer task owns the state now.
      if (token.cancelled) return
      switch (outcome.kind) {
        case 'attached':
          this.backoff.delete(id)
          this.tasks.delete(id)
          return
        case 'stop':
          this.tasks.delete(id)
          return this.setStatus(id, outcome.status)
        case 'retry': {
          const delay = this.backoff.get(id) ?? 1
          this.backoff.set(id, Math.min(delay * 2, 30))
          this.setStatus(id, { kind: 'retrying', message: outcome.message, motivo: outcome.motivo })
          // Full jitter, like the device relay backoff.
          await sleep(1000 * (0.5 + Math.random() * Math.max(0, delay - 0.5)))
        }
      }
    }
  }

  private async attach(profile: SSHProfile, token: Token): Promise<Outcome> {
    const id = profile.id
    const stop = (status: SSHStatus): Outcome => ({ kind: 'stop', status })
    const idle = stop({ kind: 'idle' })
    const problem = validate(profile)
    if (problem) return stop({ kind: 'failed', message: motivoSSH(problem.chave as ChaveTexto, problem.params).texto, motivo: problem })
    this.setStatus(id, { kind: 'connecting', ...etapa('ajustes.ssh.passo.lendo') })
    const config = await run(SSH, resolveArguments(profile))
    if (token.cancelled) return idle
    const resolved = config.status === 0 ? parseConfig(config.stdout) : null
    if (!resolved) return stop({ kind: 'failed', ...falha('ajustes.ssh.erro.config', { host: profile.host, detalhe: lastLine(config.stderr) }) })
    const name = knownHostsName(resolved)

    // Host key (never by reading ssh's error text).
    const recorded = await this.recordedKeys(name)
    if (token.cancelled || !this.canConnect()) return idle
    if (recorded.size === 0) {
      if (usesProxy(resolved)) {
        return stop({ kind: 'failed', ...falha('ajustes.ssh.erro.jump', { host: profile.host }) })
      }
      this.setStatus(id, { kind: 'connecting', ...etapa('ajustes.ssh.passo.chave') })
      const scan = await run(KEYSCAN, keyscanArguments(resolved))
      if (token.cancelled || !this.canConnect()) return idle
      const lines = knownHostsLines(scan.stdout, name)
      if (lines.length === 0) return { kind: 'retry', ...falha('ajustes.ssh.erro.alcance', { host: resolved.hostname, porta: resolved.port }) }
      const prints = await run(KEYGEN, ['-l', '-f', '-'], lines.join('\n') + '\n')
      if (token.cancelled || !this.canConnect()) return idle
      const fingerprints = prints.stdout.split(/\r?\n/).filter(Boolean)
      if (prints.status !== 0 || fingerprints.length === 0) return stop({ kind: 'failed', ...falha('ajustes.ssh.erro.lerChave', { host: profile.host }) })
      this.pendingKeys.set(id, lines)
      return stop({ kind: 'confirmHostKey', name, fingerprints })
    }

    this.setStatus(id, { kind: 'connecting', ...etapa('ajustes.ssh.passo.entrando') })
    const result = await run(SSH, infoArguments(profile, home, identity.dataFolder, dataDir))
    if (token.cancelled) return idle
    if (result.status === 255) {
      if (authRefused(result.stderr)) return stop({ kind: 'failed', ...signInRefused(profile) })
      if (!usesProxy(resolved)) {
        const current = hostKeys((await run(KEYSCAN, keyscanArguments(resolved))).stdout)
        // Only plain host keys can be compared: a `@cert-authority` key is never what keyscan returns.
        const pinned = await this.recordedKeys(name, false)
        if (current.size > 0 && pinned.size > 0 && ![...current].some((k) => pinned.has(k))) {
          return stop({ kind: 'failed', ...falha('marca.ssh.chaveMudou', { host: profile.host }) })
        }
      }
      return { kind: 'retry', ...falha('ajustes.ssh.erro.conectar', { host: profile.host, detalhe: lastLine(result.stderr) }) }
    }
    const info = parseInfo(result.stdout)
    if (isRc && (result.status !== 0 || !info?.running)) return stop({ kind: 'failed', ...falha('marca.ssh.hubRcIndisponivel') })
    if (result.status === 127) return stop({ kind: 'notInstalled' })
    if (!info) {
      if (result.status === 0) return stop({ kind: 'notInstalled' })
      return stop({ kind: 'failed', ...falha('ajustes.ssh.erro.identidade', { host: profile.host, comando: COMANDO_INSTALAR_HOST, detalhe: lastLine(result.stderr) }) })
    }
    if (!consistent(info.computerId, info.signKey)) return stop({ kind: 'failed', ...falha('ajustes.ssh.erro.identidadeInvalida', { host: profile.host }) })
    if (profile.computerId && profile.computerId !== info.computerId) {
      return stop({ kind: 'failed', ...falha('ajustes.ssh.erro.outroComputador', { host: profile.host }) })
    }
    if (!profile.computerId) {
      const storage = this.remember(id, info.computerId, info.name)
      if (storage) return stop({ kind: 'failed', ...falha(storage) })
    }
    if (!info.running) return { kind: 'retry', ...falha('ajustes.ssh.erro.parado', { nome: info.name, comando: COMANDO_INSTALAR_HOST }) }

    // Tunnel: a free loopback port each attempt; a port taken in between just makes ssh exit (ExitOnForwardFailure).
    let lastProblem = ''
    for (let attempt = 0; attempt < 3; attempt++) {
      const port = await freeLocalPort()
      if (port === null) return { kind: 'retry', ...falha('ajustes.ssh.erro.semPorta') }
      this.setStatus(id, { kind: 'connecting', ...etapa('ajustes.ssh.passo.tunel') })
      const tunnel = this.startTunnel(profile, port, resolved.inet6)
      if (!tunnel) return stop({ kind: 'failed', ...falha('ajustes.ssh.erro.iniciar') })
      const { child, tail } = tunnel
      const alive = () => child.exitCode === null && child.signalCode === null
      const baseURL = `http://127.0.0.1:${port}`
      // Anônimo: só a prova de que a porta responde, sem identidade e sem token.
      let answers = false
      for (let i = 0; i < 60 && alive() && !token.cancelled; i++) {
        answers = (await localHealth(baseURL, 1000)) !== null
        if (answers) break
        await sleep(250)
      }
      if (token.cancelled) {
        child.kill()
        return idle
      }
      if (!answers) {
        if (alive()) child.kill()
        else lastProblem = await tail.lastLine()
        continue
      }
      // O /health anônimo não prova que a resposta veio pelo ssh: outro usuário local pode ter
      // tomado a porta. A identidade vem da prova do token do Hub remoto, que um impostor não tem;
      // o dono do listener continua conferido antes, como sinal de que o túnel está de pé.
      const listens = child.pid !== undefined && (await sshListens(child.pid, port))
      // Disconnected, removed or edited meanwhile: this attempt must not register its tunnel.
      if (token.cancelled) {
        child.kill()
        return idle
      }
      if (!listens || !alive()) {
        console.error(`ssh: tunnel port ${port} answered by something other than ssh`)
        child.kill()
        // O detalhe entra no motivo `abrirTunel`; como a saída do ssh, vai no inglês do catálogo.
        lastProblem = motivoSSH('ajustes.ssh.erro.portaOcupada').texto
        continue
      }
      const identity = await provarIdentidade(baseURL, 1000, info.token)
      if (token.cancelled) {
        child.kill()
        return idle
      }
      // O túnel pode cair durante a espera; sem o ssh vivo, nada é registrado e a tentativa se repete.
      if (!alive()) {
        lastProblem = await tail.lastLine()
        continue
      }
      // Sem resposta é falha de passagem, que a próxima tentativa resolve com um ssh novo: este
      // processo é encerrado aqui, nunca fica rodando junto do substituto. Prova recusada ou outro
      // `computerId` é o túnel indo para um Hub que não é este computador.
      if (identity.kind === 'unreachable') {
        child.kill()
        lastProblem = motivoSSH('ajustes.ssh.erro.portaOcupada').texto
        continue
      }
      if (identity.kind === 'refused' || identity.health.computerId !== info.computerId) {
        child.kill()
        return stop({ kind: 'failed', ...falha('ajustes.ssh.erro.tunelOutro', { host: profile.host }) })
      }
      this.watch(child, id, tail)
      this.tunnels.set(id, child)
      this.connectedAt.set(id, Date.now())
      this.attachments.set(id, {
        profileId: id,
        computer: { id: info.computerId, name: info.name, signKey: info.signKey, boxKey: info.boxKey ?? null },
        baseURL,
        token: info.token,
      })
      this.setStatus(id, { kind: 'connected', localPort: port })
      return { kind: 'attached' }
    }
    if (authRefused(lastProblem)) return stop({ kind: 'failed', ...signInRefused(profile) })
    return { kind: 'retry', ...falha('ajustes.ssh.erro.abrirTunel', { host: profile.host, detalhe: lastProblem }) }
  }

  private async recordedKeys(name: string, certAuthorities = true) {
    const keys = new Set<string>()
    for (const file of knownHostsFiles(home, dataDir, isRc)) {
      if (!existsSync(file)) continue
      const found = await run(KEYGEN, ['-F', name, '-f', file])
      if (found.status === 0) for (const k of hostKeys(found.stdout, certAuthorities)) keys.add(k)
    }
    return keys
  }

  /** ssh's stderr is read as it arrives: left unread, the pipe fills and ssh blocks mid-write, freezing the tunnel. */
  private startTunnel(profile: SSHProfile, localPort: number, inet6: boolean) {
    let text = ''
    let done = false
    let child: ChildProcess
    try {
      child = spawn(SSH, tunnelArguments(profile, localPort, home, dataDir, inet6, isRc), { stdio: ['ignore', 'ignore', 'pipe'] })
    } catch {
      return null
    }
    child.on('error', () => {})
    child.stderr?.on('data', (d: Buffer) => (text = (text + d.toString()).slice(-4096)))
    child.stderr?.on('close', () => (done = true))
    const tail = {
      /** ssh's last line, once it exited (waits briefly for the pipe to reach its end). */
      async lastLine() {
        for (let i = 0; i < 40 && !done; i++) await sleep(25)
        return lastLine(text)
      },
    }
    return { child, tail }
  }

  /** A tunnel that drops is reopened with backoff (reset once it stayed up a minute). */
  private watch(child: ChildProcess, id: string, tail: { lastLine(): Promise<string> }) {
    child.once('exit', () => {
      void tail.lastLine().then((detail) => {
        // Only the current tunnel: a replaced or disconnected one ends without effect.
        if (this.tunnels.get(id) !== child) return
        console.log(`ssh tunnel ended: ${detail}`)
        const since = this.connectedAt.get(id)
        if (since !== undefined && Date.now() - since >= 60_000) this.backoff.delete(id)
        this.stopTunnel(id)
        this.setStatus(id, { kind: 'retrying', ...falha('ajustes.ssh.erro.caiu', { detalhe: detail }) })
        this.connect(id)
      })
    })
  }

  private stopTunnel(id: string, notify = true) {
    const child = this.tunnels.get(id)
    this.tunnels.delete(id)
    child?.kill()
    this.connectedAt.delete(id)
    if (this.attachments.delete(id) && notify) this.changed()
  }

  private remember(id: string, computerId: string, name: string) {
    const next = this.profiles.map((p) => (p.id === id ? { ...p, computerId, name: p.name || name } : p))
    const storage = this.persist(next, id)
    if (storage) return storage
    this.profiles = next
    if (!this.profileStore) this.persistWindows()
    this.changed()
    return null
  }

  private persistWindows() {
    void fs.mkdir(app.getPath('userData'), { recursive: true }).then(() => fs.writeFile(this.file, JSON.stringify(this.profiles)))
  }

  private persist(next: SSHProfile[], affectedId: string | undefined) {
    if (!this.profileStore) return null
    if (this.storageBlocked) return this.storageBlocked
    const outcome = this.profileStore.save(next)
    if (outcome.kind === 'saved') {
      this.status.delete(SSH_STORAGE_STATUS)
      return null
    }
    if (outcome.kind === 'uncertain') {
      if (outcome.profiles) {
        const visible = new Map(outcome.profiles.map((profile) => [profile.id, profile]))
        for (const prior of this.profiles) {
          const current = visible.get(prior.id)
          if (!current || current.computerId !== prior.computerId || current.host !== prior.host ||
              current.port !== prior.port || current.remotePort !== prior.remotePort || current.user !== prior.user ||
              current.identityFile !== prior.identityFile) {
            this.disconnect(prior.id, false)
            this.pendingKeys.delete(prior.id)
          }
        }
        const prior = new Map(this.profiles.map((profile) => [profile.id, profile]))
        this.profiles = outcome.profiles.map((profile) => {
          const old = prior.get(profile.id)
          return old && JSON.stringify(old) === JSON.stringify(profile) ? old : profile
        })
      } else {
        this.storageBlocked = 'ajustes.ssh.erro.perfisDurabilidadeBloqueada'
        if (affectedId !== undefined) {
          this.disconnect(affectedId, false)
          this.pendingKeys.delete(affectedId)
        }
      }
    }
    const problem = outcome.kind === 'refused' ? 'ajustes.ssh.erro.perfisGravacao' : outcome.profiles ?
      'ajustes.ssh.erro.perfisDurabilidade' : 'ajustes.ssh.erro.perfisDurabilidadeBloqueada'
    this.setStatus(SSH_STORAGE_STATUS, { kind: 'failed', ...falha(problem) })
    return problem
  }
}

/** SSH computers for every window: `ssh:*` handlers, `ssh:change` events, tunnels stopped on quit. */
export function registerSSH(getWindows: () => BrowserWindow[], canConnect: () => boolean) {
  const ssh = new SSHComputers(join(app.getPath('userData'), 'ssh-profiles.json'), (state) => {
    for (const w of getWindows()) if (!w.isDestroyed()) w.webContents.send('ssh:change', state)
  }, canConnect)
  ipcMain.handle('ssh:state', () => ssh.state)
  ipcMain.handle('ssh:save', (_e, profile: SSHProfile) => ssh.save(profile))
  ipcMain.on('ssh:remove', (_e, id: string) => ssh.remove(id))
  ipcMain.on('ssh:connect', (_e, id: string) => ssh.connect(id))
  ipcMain.on('ssh:disconnect', (_e, id: string) => ssh.disconnect(id))
  ipcMain.on('ssh:trustHostKey', (_e, id: string) => void ssh.trustHostKey(id))
  ipcMain.handle('ssh:chooseKey', async () => {
    const result = await dialog.showOpenDialog({ defaultPath: join(home, '.ssh'), properties: ['openFile', 'showHiddenFiles'] })
    return result.canceled ? null : (result.filePaths[0] ?? null)
  })
  // ssh children outlive the app unless stopped.
  app.on('will-quit', () => ssh.disconnectAll())
  void ssh.connectAll()
  return { connectAll: () => ssh.connectAll(), disconnectAll: () => ssh.disconnectAll(),
    captureLocal: (profileId: string, current: () => boolean) => ssh.captureLocal(profileId, current),
    /** MX-209: captura com guardas para o cofre seguro, por computerId. */
    captureRequest: (computerId: string, current: () => boolean) => ssh.captureRequest(computerId, current),
    attachments: () => ssh.state.attachments }
}
