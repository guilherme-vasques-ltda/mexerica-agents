// Sessão local da web: o Hub serve esta página na mesma origem (127.0.0.1) e a sessão é um
// cookie HttpOnly dele. Aqui ficam só o token CSRF e os metadados que o Hub devolve — em
// memória, nunca em localStorage/sessionStorage e nunca na URL. Sem sessão, nada sai do
// navegador (fail-closed): `call` e `stream` recusam antes de fazer qualquer requisição.

import type { CallError, CodyncBridge, HostSnapshot } from '../shared/ipc.ts'
import { hostCallError } from '../shared/host-call-error.ts'

/** O que o Hub devolve em `/web/auth/session` e `/web/auth/login` quando há sessão. */
export interface SessaoWeb {
  csrfToken: string
  computerId: string
  hostId: string
  hostName: string
}

/** Categoria limpa de erro no login; a tela traduz, o texto do serviço nunca aparece. */
export type ErroLogin = 'invalid' | 'rateLimited' | 'offline' | 'error'

export interface OpcoesSessaoLocal {
  /** Injetável para teste; o padrão é o `fetch` do ambiente. */
  fetchImpl?: typeof fetch
}

export class LocalWebSession {
  private sessao: SessaoWeb | null = null
  private ouvintes = new Set<(autenticado: boolean) => void>()
  private expiracoes = new Set<() => void>()
  private readonly meuFetch: typeof fetch
  readonly origin: string

  constructor(origin: string, opts: OpcoesSessaoLocal = {}) {
    this.origin = origin
    this.meuFetch = opts.fetchImpl ?? ((...args: Parameters<typeof fetch>) => fetch(...args))
  }

  /** O fetch que esta sessão usa; o host local reutiliza o mesmo injetável. */
  get fetchImpl(): typeof fetch { return this.meuFetch }

  get isAuthenticated(): boolean { return this.sessao !== null }
  get csrfToken(): string | null { return this.sessao?.csrfToken ?? null }
  get computerId(): string | null { return this.sessao?.computerId ?? null }
  get hostName(): string | null { return this.sessao?.hostName ?? null }

  /** Mudanças de sessão (login, saída, expiração); devolve a função de cancelar. */
  subscribe(cb: (autenticado: boolean) => void): () => void {
    this.ouvintes.add(cb)
    return () => { this.ouvintes.delete(cb) }
  }

  /** A sessão morreu depois de autenticado (401 no meio do app); recarregar é o fail-closed. */
  onExpired(cb: () => void): () => void {
    this.expiracoes.add(cb)
    return () => { this.expiracoes.delete(cb) }
  }

  private notificar() {
    for (const cb of this.ouvintes) cb(this.isAuthenticated)
  }

  private guardar(sessao: SessaoWeb | null) {
    this.sessao = sessao
    this.notificar()
  }

  private lerSessao(corpo: unknown): SessaoWeb | null {
    if (typeof corpo !== 'object' || corpo === null) return null
    const c = corpo as Record<string, unknown>
    if (c.authenticated !== true) return null
    const { csrfToken, computerId, hostId, hostName } = c
    if ([csrfToken, computerId, hostId, hostName].some((v) => typeof v !== 'string' || !v)) return null
    return { csrfToken: csrfToken as string, computerId: computerId as string, hostId: hostId as string, hostName: hostName as string }
  }

  /** Sessão do cookie HttpOnly do Hub, se ainda vale. Rede caída não desautentica: segue o que havia. */
  async checkSession(): Promise<boolean> {
    let res: Response
    try {
      res = await this.meuFetch(`${this.origin}/web/auth/session`, {
        headers: { 'X-Mexerica-Web': '1' },
        credentials: 'same-origin',
        cache: 'no-store',
      })
    } catch { return this.isAuthenticated }
    if (res.status === 401) { this.guardar(null); return false }
    if (!res.ok) return this.isAuthenticated
    // Corpo inválido é tratado como sem sessão (fail-closed), nunca como "segue como estava".
    try {
      this.guardar(this.lerSessao(await res.json()))
    } catch {
      this.guardar(null)
    }
    return this.isAuthenticated
  }

  /** Envia o token local do Hub; o cookie HttpOnly nasce aqui, na resposta do Hub. */
  async login(token: string): Promise<ErroLogin | null> {
    if (typeof token !== 'string' || !token.trim()) return 'invalid'
    let res: Response
    try {
      res = await this.meuFetch(`${this.origin}/web/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-Mexerica-Web': '1' },
        credentials: 'same-origin',
        cache: 'no-store',
        body: JSON.stringify({ token }),
      })
    } catch { return 'offline' }
    if (res.status === 401) return 'invalid'
    if (res.status === 429) return 'rateLimited'
    if (!res.ok) return 'error'
    try {
      const sessao = this.lerSessao(await res.json())
      if (!sessao) return 'error'
      this.guardar(sessao)
      return null
    } catch { return 'error' }
  }

  /** Encerra no Hub e esquece a sessão local; falha de rede também esquece (fail-closed). */
  async logout(): Promise<void> {
    const csrf = this.sessao?.csrfToken
    if (csrf) {
      try {
        await this.meuFetch(`${this.origin}/web/auth/logout`, {
          method: 'POST',
          headers: { 'X-Mexerica-Csrf': csrf },
          credentials: 'same-origin',
          cache: 'no-store',
        })
      } catch { /* sem rede: a sessão local morre de qualquer forma */ }
    }
    this.guardar(null)
  }

  /** Chamado pela ponte em 401: a sessão caiu; quem ouve `onExpired` decide o que fazer. */
  handleUnauthorized(): void {
    if (!this.sessao) return
    this.guardar(null)
    for (const cb of this.expiracoes) cb()
  }
}

export interface OpcoesHostLocal {
  fetchImpl?: typeof fetch
  /** Tempo ocioso do SSE antes de encerrar (o Hub pinge a cada 15 s). Teste injeta um valor curto. */
  timeoutOciosoMs?: number
}

/** Erro técnico do transporte: código curto (a UI traduz pelo catálogo, o serviço nunca fala). */
function erroLocal(status: number, codigo: string): CallError {
  return { status, message: codigo }
}

const SEM_SESSAO = erroLocal(401, 'no web session')

/** O `CodyncBridge['host']` da web local: só a origem da página, só com sessão viva. */
export function createLocalWebHost(session: LocalWebSession, opts: OpcoesHostLocal = {}): CodyncBridge['host'] {
  const fetchImpl = opts.fetchImpl ?? session.fetchImpl
  const timeoutOcioso = opts.timeoutOciosoMs ?? 45_000

  const snapshot = (): HostSnapshot => {
    const computerId = session.computerId
    return session.isAuthenticated && computerId
      ? { state: { kind: 'running' }, local: { computerId, baseURL: session.origin, token: 'web-session' }, dev: false, logPath: '' }
      : { state: { kind: 'failed', message: '' }, local: null, dev: false, logPath: '' }
  }

  return {
    snapshot: async () => snapshot(),
    onChange: (cb) => session.subscribe(() => cb(snapshot())),
    install() {},
    restart() {},

    async call(baseURL, _token, method, body, timeoutMs) {
      if (baseURL !== session.origin) throw erroLocal(0, 'same-origin host only')
      if (!/^[A-Za-z0-9_-]+$/.test(method)) throw erroLocal(0, 'invalid method')
      const csrf = session.csrfToken
      if (!csrf) { session.handleUnauthorized(); throw SEM_SESSAO }
      let res: Response
      try {
        res = await fetchImpl(`${session.origin}/api/${method}`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', 'X-Mexerica-Csrf': csrf },
          credentials: 'same-origin',
          cache: 'no-store',
          body: JSON.stringify(body ?? {}),
          signal: AbortSignal.timeout(timeoutMs),
        })
      } catch { throw erroLocal(0, 'local hub unreachable') }
      // 401 invalida a sessão antes de ler o corpo: corpo ilegível não deixa a sessão viva (fail-closed).
      if (res.status === 401) session.handleUnauthorized()
      let texto: string
      try {
        texto = await res.text()
      } catch { throw erroLocal(0, 'local hub unreachable') }
      if (res.status === 401) throw hostCallError(401, texto)
      if (!res.ok) throw hostCallError(res.status, texto)
      return texto ? JSON.parse(texto) : {}
    },

    stream(url, _token, onData, onEnd) {
      let destino: URL
      try { destino = new URL(url, session.origin) } catch { destino = null as unknown as URL }
      if (!destino || destino.origin !== session.origin) {
        queueMicrotask(() => onEnd(erroLocal(0, 'same-origin stream only')))
        return () => {}
      }
      const csrf = session.csrfToken
      if (!csrf) {
        queueMicrotask(() => { session.handleUnauthorized(); onEnd(SEM_SESSAO) })
        return () => {}
      }
      const controller = new AbortController()
      // Timeout ocioso, não total: o Hub pinge a cada 15 s, então silêncio desse tamanho é Hub caído.
      let ocioso: ReturnType<typeof setTimeout> | null = null
      const armar = () => {
        if (ocioso) clearTimeout(ocioso)
        ocioso = setTimeout(() => controller.abort(), timeoutOcioso)
      }
      const encerrar = (erro: CallError | null) => {
        if (ocioso) { clearTimeout(ocioso); ocioso = null }
        onEnd(erro)
      }
      void (async () => {
        try {
          armar()
          const res = await fetchImpl(destino, {
            headers: { Accept: 'text/event-stream', 'X-Mexerica-Csrf': csrf },
            credentials: 'same-origin',
            cache: 'no-store',
            signal: controller.signal,
          })
          if (res.status === 401) {
            session.handleUnauthorized()
            encerrar(hostCallError(401, await res.text()))
            return
          }
          if (!res.ok || !res.body) throw hostCallError(res.status, res.ok ? '' : await res.text())
          const reader = res.body.getReader()
          const decoder = new TextDecoder()
          let buffer = ''
          for (;;) {
            const { value, done } = await reader.read()
            if (done) break
            armar()
            buffer += decoder.decode(value, { stream: true })
            let nl: number
            while ((nl = buffer.indexOf('\n')) >= 0) {
              const line = buffer.slice(0, nl).replace(/\r$/, '')
              buffer = buffer.slice(nl + 1)
              if (line.startsWith('data:')) onData(line.slice(5).trimStart())
            }
          }
          encerrar(null)
        } catch (error) {
          const e = error as Partial<CallError>
          const mensagem = typeof e.message === 'string' && e.message ? e.message : 'local hub unreachable'
          encerrar(controller.signal.aborted ? erroLocal(0, 'local hub unreachable') : erroLocal(e.status ?? 0, mensagem))
        } finally {
          if (ocioso) { clearTimeout(ocioso); ocioso = null }
        }
      })()
      return () => {
        if (ocioso) { clearTimeout(ocioso); ocioso = null }
        controller.abort()
      }
    },
  }
}
