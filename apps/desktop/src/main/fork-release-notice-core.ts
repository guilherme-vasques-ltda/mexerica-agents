// Nível 1 de atualizações, serviço do processo principal (sem importar `electron`): busca a release
// mais recente com validação fail-closed (redirects, tamanho, payload) e controla o estado do aviso
// com persistência em disco. Nada aqui baixa ou instala: o máximo que o app faz é abrir a página da
// release quando a pessoa pede.

import { closeSync, constants, fchmodSync, fstatSync, lstatSync, mkdirSync, openSync, readFileSync, writeSync } from 'node:fs'
import { dirname } from 'node:path'
import {
  AUTO_CHECK_COOLDOWN_MS,
  AUTO_CHECK_DELAY_MS,
  GITHUB_RELEASES_API_URL,
  MANUAL_CHECK_COOLDOWN_MS,
  MAX_RELEASE_RESPONSE_BYTES,
  RELEASE_FETCH_TIMEOUT_MS,
  RELEASE_TAG_URL_PREFIX,
  isNewerRelease,
  parseLatestReleasePayload,
  parseStableSemver,
  validateRedirectUrl,
  validateReleaseUrl,
  type LatestReleaseResult,
  type ReleaseNoticeState,
  type ReleaseNoticeStatus,
} from '../shared/fork-release-notice.ts'

const REDIRECT_STATUSES = new Set([301, 302, 303, 307, 308])
const MAX_REDIRECTS = 3
const posix = process.platform !== 'win32'

export type FetchLatestReleaseOutcome = { ok: true; result: LatestReleaseResult } | { ok: false }

export interface FetchLatestReleaseOptions {
  currentVersion: string
  fetcher?: typeof fetch
  timeoutMs?: number
  maxBytes?: number
}

/**
 * Busca `GITHUB_RELEASES_API_URL` com redirects manuais: cada `Location` precisa passar em
 * `validateRedirectUrl` (só api.github.com/github.com em HTTPS) e o limite é de 3 saltos. O sinal de
 * timeout vale para a busca inteira (saltos e leitura do corpo somam no prazo). 404 é "sem release a
 * avisar" (ok, up-to-date); qualquer outra falha — rede, status, corpo grande demais, payload
 * ilegível — é `{ ok: false }`, nunca um aviso. Corpo não lido é sempre cancelado.
 */
export async function fetchLatestRelease(options: FetchLatestReleaseOptions): Promise<FetchLatestReleaseOutcome> {
  const fetcher = options.fetcher ?? fetch
  const timeoutMs = options.timeoutMs ?? RELEASE_FETCH_TIMEOUT_MS
  const maxBytes = options.maxBytes ?? MAX_RELEASE_RESPONSE_BYTES
  const signal = AbortSignal.timeout(timeoutMs)
  let url = GITHUB_RELEASES_API_URL
  let redirects = 0
  let response: Response | undefined
  while (response === undefined) {
    let current: Response
    try {
      current = await fetcher(url, {
        headers: {
          accept: 'application/vnd.github+json',
          'x-github-api-version': '2022-11-28',
          'user-agent': `Mexerica-Desktop/${options.currentVersion}`,
        },
        redirect: 'manual',
        cache: 'no-store',
        credentials: 'omit',
        signal,
      })
    } catch {
      return { ok: false }
    }
    if (current.status === 404) {
      try { await current.body?.cancel() } catch {}
      return { ok: true, result: { kind: 'up-to-date' } }
    }
    if (REDIRECT_STATUSES.has(current.status)) {
      try { await current.body?.cancel() } catch {}
      if (redirects >= MAX_REDIRECTS) return { ok: false }
      const next = validateRedirectUrl(current.headers.get('location'), url)
      if (!next) return { ok: false }
      redirects += 1
      url = next
      continue
    }
    response = current
  }
  if (response.status !== 200) {
    try { await response.body?.cancel() } catch {}
    return { ok: false }
  }
  const declared = Number(response.headers.get('content-length'))
  if (Number.isFinite(declared) && declared > maxBytes) {
    try { await response.body?.cancel() } catch {}
    return { ok: false }
  }
  let bytes: Uint8Array
  try {
    const reader = response.body?.getReader()
    if (reader) {
      const chunks: Uint8Array[] = []
      let total = 0
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        if (!value) continue
        total += value.byteLength
        if (total > maxBytes) {
          try { await reader.cancel() } catch {}
          return { ok: false }
        }
        chunks.push(value)
      }
      bytes = new Uint8Array(total)
      let offset = 0
      for (const chunk of chunks) {
        bytes.set(chunk, offset)
        offset += chunk.byteLength
      }
    } else {
      const buffer = await response.arrayBuffer()
      if (buffer.byteLength > maxBytes) return { ok: false }
      bytes = new Uint8Array(buffer)
    }
  } catch {
    return { ok: false }
  }
  let payload: unknown
  try {
    payload = JSON.parse(new TextDecoder().decode(bytes))
  } catch {
    return { ok: false }
  }
  const result = parseLatestReleasePayload(payload, options.currentVersion)
  return result.kind === 'invalid' ? { ok: false } : { ok: true, result }
}

export interface CachedRelease {
  version: string
  tag: string
  url: string
}

export interface ReleaseNoticeStoreData {
  dismissedVersion: string | null
  lastCheckAt: number | null
  cachedRelease: CachedRelease | null
}

export interface ReleaseNoticeStore {
  load(): ReleaseNoticeStoreData
  save(data: ReleaseNoticeStoreData): void
}

const EMPTY_STORE: ReleaseNoticeStoreData = { dismissedVersion: null, lastCheckAt: null, cachedRelease: null }

function readCachedRelease(raw: unknown): CachedRelease | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const record = raw as Record<string, unknown>
  if (typeof record.version !== 'string' || typeof record.tag !== 'string' || typeof record.url !== 'string') return null
  const parsed = parseStableSemver(record.version)
  if (!parsed || record.version !== parsed.canonical || record.tag !== `v${parsed.canonical}`) return null
  const url = validateReleaseUrl(record.url)
  if (!url) return null
  const tagDaUrl = parseStableSemver(url.slice(RELEASE_TAG_URL_PREFIX.length))
  if (!tagDaUrl || tagDaUrl.canonical !== parsed.canonical) return null
  return { version: record.version, tag: record.tag, url: record.url }
}

/**
 * O estado do aviso em `userData`: um arquivo JSON regular, gravado com 0600 (sem symlink, sem
 * escrita de grupo ou de outros, requisitos do perfil Open). A leitura abre pelo fd com
 * `O_NOFOLLOW` (sem janela entre o stat e o read) e valida campo a campo, devolvendo null para o
 * que estiver corrompido; erro de disco na escrita sobe para quem chamou tratar.
 */
export function createFileReleaseNoticeStore(filePath: string): ReleaseNoticeStore {
  return {
    load(): ReleaseNoticeStoreData {
      try {
        const fd = openSync(filePath, constants.O_RDONLY | (posix ? constants.O_NOFOLLOW : 0))
        try {
          if (!fstatSync(fd).isFile()) return { ...EMPTY_STORE }
          const parsed: unknown = JSON.parse(readFileSync(fd, 'utf8'))
          if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return { ...EMPTY_STORE }
          const record = parsed as Record<string, unknown>
          const dismissed = typeof record.dismissedVersion === 'string' ? parseStableSemver(record.dismissedVersion) : null
          const lastCheckAt = typeof record.lastCheckAt === 'number' && Number.isFinite(record.lastCheckAt) && record.lastCheckAt >= 0 ? record.lastCheckAt : null
          return { dismissedVersion: dismissed ? dismissed.canonical : null, lastCheckAt, cachedRelease: readCachedRelease(record.cachedRelease) }
        } finally {
          closeSync(fd)
        }
      } catch {
        return { ...EMPTY_STORE }
      }
    },
    save(data: ReleaseNoticeStoreData) {
      mkdirSync(dirname(filePath), { recursive: true, mode: 0o700 })
      try {
        if (!lstatSync(filePath).isFile()) throw new Error('release-notice: refusing non-regular state file')
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
      }
      const flags = constants.O_WRONLY | constants.O_CREAT | constants.O_TRUNC | (posix ? constants.O_NOFOLLOW : 0)
      const fd = openSync(filePath, flags, 0o600)
      try {
        if (posix) fchmodSync(fd, 0o600)
        writeSync(fd, `${JSON.stringify(data)}\n`)
      } finally {
        closeSync(fd)
      }
    },
  }
}

export interface ReleaseNoticeControllerDeps {
  currentVersion: string
  store: ReleaseNoticeStore
  fetcher?: typeof fetch
  openExternal: (url: string) => Promise<boolean>
  now?: () => number
  setTimer?: (callback: () => void, delayMs: number) => unknown
  clearTimer?: (handle: unknown) => void
  autoDelayMs?: number
  autoCooldownMs?: number
  manualCooldownMs?: number
  timeoutMs?: number
  maxBytes?: number
  onChange?: (state: ReleaseNoticeState) => void
}

export interface ReleaseNoticeController {
  getState(): ReleaseNoticeState
  start(): void
  stop(): void
  check(options?: { manual?: boolean }): Promise<ReleaseNoticeState>
  dismiss(): ReleaseNoticeState
  openDownload(): Promise<boolean>
}

/**
 * O estado do aviso na memória do Main. A checagem automática falha em silêncio (o app segue como
 * está); só a checagem que a pessoa pediu mostra erro. Cooldowns evitam batidas repetidas na rede,
 * e o cache da última release voltado do disco só conta se ainda for mais nova que a versão atual.
 */
export function createReleaseNoticeController(deps: ReleaseNoticeControllerDeps): ReleaseNoticeController {
  const now = () => deps.now?.() ?? Date.now()
  const setTimer = deps.setTimer ?? ((callback: () => void, delayMs: number) => setTimeout(callback, delayMs))
  const clearTimer = deps.clearTimer ?? ((handle: unknown) => clearTimeout(handle as ReturnType<typeof setTimeout>))
  const autoDelayMs = deps.autoDelayMs ?? AUTO_CHECK_DELAY_MS
  const autoCooldownMs = deps.autoCooldownMs ?? AUTO_CHECK_COOLDOWN_MS
  const manualCooldownMs = deps.manualCooldownMs ?? MANUAL_CHECK_COOLDOWN_MS

  let persisted: ReleaseNoticeStoreData
  try {
    persisted = deps.store.load()
  } catch {
    persisted = { ...EMPTY_STORE }
  }
  const cached = persisted.cachedRelease
  const restored = cached !== null && isNewerRelease(deps.currentVersion, cached.version) && validateReleaseUrl(cached.url) !== null
  let state: ReleaseNoticeState = {
    supported: true,
    currentVersion: deps.currentVersion,
    status: restored ? 'available' : 'idle',
    availableVersion: restored ? cached!.version : null,
    availableTag: restored ? cached!.tag : null,
    releaseUrl: restored ? cached!.url : null,
    dismissedVersion: persisted.dismissedVersion,
    lastCheckAt: persisted.lastCheckAt,
  }
  let pendingTimer: unknown = null
  let inFlight: Promise<ReleaseNoticeState> | null = null
  let inFlightManual = false

  const withinCooldown = (cooldownMs: number) => {
    if (state.lastCheckAt === null) return false
    const elapsed = now() - state.lastCheckAt
    return elapsed >= 0 && elapsed < cooldownMs
  }
  const setState = (change: Partial<ReleaseNoticeState>) => {
    state = { ...state, ...change }
    deps.onChange?.(state)
  }
  const persist = (cachedRelease: CachedRelease | null) => {
    try {
      deps.store.save({ dismissedVersion: state.dismissedVersion, lastCheckAt: state.lastCheckAt, cachedRelease })
    } catch {}
  }
  const cachedFromState = (): CachedRelease | null =>
    state.availableVersion !== null && state.availableTag !== null && state.releaseUrl !== null
      ? { version: state.availableVersion, tag: state.availableTag, url: state.releaseUrl }
      : null

  return {
    getState: () => ({ ...state }),
    start() {
      if (pendingTimer !== null || withinCooldown(autoCooldownMs)) return
      pendingTimer = setTimer(() => {
        pendingTimer = null
        void this.check({ manual: false })
      }, autoDelayMs)
    },
    stop() {
      if (pendingTimer === null) return
      clearTimer(pendingTimer)
      pendingTimer = null
    },
    async check(options?: { manual?: boolean }) {
      const manual = options?.manual === true
      if (inFlight) {
        if (manual) {
          inFlightManual = true
          if (state.status !== 'checking') setState({ status: 'checking' })
        }
        return inFlight
      }
      if (!manual && withinCooldown(autoCooldownMs)) return state
      if (manual && withinCooldown(manualCooldownMs) && state.status !== 'error') {
        const status: ReleaseNoticeStatus = cachedFromState() !== null ? 'available' : 'up-to-date'
        if (state.status !== status) setState({ status })
        return state
      }
      if (manual) setState({ status: 'checking' })
      inFlightManual = manual
      const attempt = (async () => {
        const outcome = await fetchLatestRelease({
          currentVersion: deps.currentVersion,
          fetcher: deps.fetcher,
          timeoutMs: deps.timeoutMs,
          maxBytes: deps.maxBytes,
        })
        if (!outcome.ok) {
          if (inFlightManual) setState({ status: 'error' })
          return state
        }
        if (outcome.result.kind === 'available') {
          setState({
            status: 'available',
            availableVersion: outcome.result.version,
            availableTag: outcome.result.tag,
            releaseUrl: outcome.result.url,
            lastCheckAt: now(),
          })
          persist(cachedFromState())
        } else {
          setState({
            status: 'up-to-date',
            availableVersion: null,
            availableTag: null,
            releaseUrl: null,
            lastCheckAt: now(),
          })
          persist(null)
        }
        return state
      })()
      inFlight = attempt
      try {
        return await attempt
      } finally {
        inFlight = null
        inFlightManual = false
      }
    },
    dismiss() {
      if (state.availableVersion === null) return { ...state }
      setState({ dismissedVersion: state.availableVersion })
      persist(cachedFromState())
      return { ...state }
    },
    async openDownload() {
      const url = validateReleaseUrl(state.releaseUrl)
      if (url === null) return false
      try {
        return await deps.openExternal(url)
      } catch {
        return false
      }
    },
  }
}
