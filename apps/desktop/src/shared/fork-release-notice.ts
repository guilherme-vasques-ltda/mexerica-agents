// Nível 1 de atualizações: o aviso discreto de nova versão. O app nunca baixa nem instala nada por
// conta própria — só lê a release marcada como latest no GitHub e abre a página quando a pessoa
// decide. Toda validação aqui é fail-closed: dado ausente, ilegível ou fora do previsto é recusado,
// nunca interpretado como "há atualização".

export const GITHUB_RELEASES_API_URL = 'https://api.github.com/repos/guilherme-vasques-ltda/mexerica-agents/releases/latest'
export const RELEASE_TAG_URL_PREFIX = 'https://github.com/guilherme-vasques-ltda/mexerica-agents/releases/tag/'
export const ALLOWED_REDIRECT_HOSTS = new Set(['api.github.com', 'github.com'])
export const MAX_RELEASE_RESPONSE_BYTES = 262_144
export const RELEASE_FETCH_TIMEOUT_MS = 8_000
export const AUTO_CHECK_DELAY_MS = 5_000
export const AUTO_CHECK_COOLDOWN_MS = 6 * 60 * 60 * 1000
export const MANUAL_CHECK_COOLDOWN_MS = 60 * 1000

export type ReleaseNoticeStatus = 'idle' | 'checking' | 'available' | 'up-to-date' | 'error'

export interface ReleaseNoticeState {
  supported: boolean
  currentVersion: string
  status: ReleaseNoticeStatus
  availableVersion: string | null
  availableTag: string | null
  releaseUrl: string | null
  dismissedVersion: string | null
  lastCheckAt: number | null
}

export interface StableSemver {
  major: number
  minor: number
  patch: number
  canonical: string
}

/** SemVer estável, sem pré-lançamento nem metadata: o app só compara releases publicadas completas. */
const STABLE_SEMVER = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/

export function parseStableSemver(raw: unknown): StableSemver | null {
  if (typeof raw !== 'string') return null
  const match = STABLE_SEMVER.exec(raw)
  if (!match) return null
  const major = Number(match[1])
  const minor = Number(match[2])
  const patch = Number(match[3])
  if (![major, minor, patch].every(Number.isSafeInteger)) return null
  return { major, minor, patch, canonical: `${major}.${minor}.${patch}` }
}

export function compareSemver(a: StableSemver, b: StableSemver): -1 | 0 | 1 {
  if (a.major !== b.major) return a.major > b.major ? 1 : -1
  if (a.minor !== b.minor) return a.minor > b.minor ? 1 : -1
  if (a.patch !== b.patch) return a.patch > b.patch ? 1 : -1
  return 0
}

/** Só `candidate > current` estrito: versão igual, downgrade ou qualquer lado ilegível é `false`. */
export function isNewerRelease(currentRaw: unknown, candidateRaw: unknown): boolean {
  const current = parseStableSemver(currentRaw)
  const candidate = parseStableSemver(candidateRaw)
  if (!current || !candidate) return false
  return compareSemver(candidate, current) === 1
}

/** O trecho depois do prefixo da tag: um SemVer estável sozinho, sem barra, ponto-ponto ou escape. */
const URL_TAG = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/

/**
 * A única URL que o aviso aceita abrir: a página da tag da release deste repositório, exata, em
 * HTTPS, sem porta, credencial, query ou fragmento. `expectedTag`, quando informado, precisa ser o
 * trecho final da URL (a `tag_name` da resposta).
 */
export function validateReleaseUrl(raw: unknown, expectedTag?: string): string | null {
  if (typeof raw !== 'string' || raw.length === 0) return null
  if (/[\s\u0000-\u001f\u007f]/.test(raw)) return null
  if (!raw.startsWith(RELEASE_TAG_URL_PREFIX)) return null
  const tag = raw.slice(RELEASE_TAG_URL_PREFIX.length)
  if (!URL_TAG.test(tag)) return null
  if (expectedTag !== undefined && tag !== expectedTag) return null
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    return null
  }
  if (url.protocol !== 'https:' || url.hostname !== 'github.com' || url.port !== '') return null
  if (url.username !== '' || url.password !== '' || url.search !== '' || url.hash !== '') return null
  if (url.href !== raw) return null
  return raw
}

/** Porta explícita na autoridade de uma URL absoluta, inclusive protocol-relative (a WHATWG normaliza a porta padrão, então o parse não vê). */
const PORTA_EXPLICITA = /^(?:[a-zA-Z][a-zA-Z0-9+.-]*:)?\/\/[^/?#]*:\d+(?:[/?#]|$)/

/** Um redirect aceito: só HTTPS, sem porta nem credencial, e apenas para os hosts permitidos. */
export function validateRedirectUrl(location: unknown, baseUrl: string): string | null {
  if (typeof location !== 'string' || location.length === 0) return null
  if (/[\s\u0000-\u001f\u007f]/.test(location)) return null
  if (PORTA_EXPLICITA.test(location)) return null
  let url: URL
  try {
    url = new URL(location, baseUrl)
  } catch {
    return null
  }
  if (url.protocol !== 'https:' || url.port !== '' || url.username !== '' || url.password !== '') return null
  if (!ALLOWED_REDIRECT_HOSTS.has(url.hostname)) return null
  return url.href
}

export type LatestReleaseResult =
  | { kind: 'available'; version: string; tag: string; url: string }
  | { kind: 'up-to-date' }
  | { kind: 'invalid' }

/**
 * A resposta de `/releases/latest`: rascunho e pré-lançamento não são release publicada (nada a
 * avisar); tag ou URL fora do previsto invalida a resposta inteira (fail-closed, nunca um aviso com
 * dado que não passou na validação).
 */
export function parseLatestReleasePayload(payload: unknown, currentVersion: string): LatestReleaseResult {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return { kind: 'invalid' }
  const record = payload as Record<string, unknown>
  if (record.draft === true || record.prerelease === true) return { kind: 'up-to-date' }
  if (typeof record.tag_name !== 'string' || typeof record.html_url !== 'string') return { kind: 'invalid' }
  const parsed = parseStableSemver(record.tag_name)
  if (!parsed) return { kind: 'invalid' }
  const url = validateReleaseUrl(record.html_url, record.tag_name)
  if (!url) return { kind: 'invalid' }
  if (isNewerRelease(currentVersion, record.tag_name)) {
    return { kind: 'available', version: parsed.canonical, tag: `v${parsed.canonical}`, url }
  }
  return { kind: 'up-to-date' }
}

export function shouldShowReleaseBanner(state: ReleaseNoticeState | null | undefined): boolean {
  return Boolean(state && state.supported && state.availableVersion && state.availableTag && state.releaseUrl && state.dismissedVersion !== state.availableVersion)
}
