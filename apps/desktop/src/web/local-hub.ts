// Sessão local da web: o Hub serve esta página na mesma origem (127.0.0.1) e a sessão é um
// cookie HttpOnly dele. Aqui ficam só o token CSRF e os metadados que o Hub devolve — em
// memória, nunca em localStorage/sessionStorage e nunca na URL. Sem sessão, nada sai do
// navegador (fail-closed): `call` e `stream` recusam antes de fazer qualquer requisição.

import type { CallError, CodyncBridge, HostSnapshot } from '../shared/ipc.ts'

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
  private geracao = new AbortController()
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
  get signal(): AbortSignal { return this.geracao.signal }

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
    const anterior = this.sessao
    this.sessao = sessao
    if (!sessao || !anterior || anterior.csrfToken !== sessao.csrfToken || anterior.hostId !== sessao.hostId || anterior.computerId !== sessao.computerId) {
      const geracao = this.geracao
      this.geracao = new AbortController()
      geracao.abort()
    }
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
    const signal = this.signal
    let res: Response
    try {
      res = await this.meuFetch(`${this.origin}/web/auth/session`, {
        headers: { 'X-Mexerica-Web': '1' },
        credentials: 'same-origin',
        cache: 'no-store',
        signal,
      })
    } catch { return this.isAuthenticated }
    if (signal.aborted) return this.isAuthenticated
    if (res.status === 401) { this.handleUnauthorized(); return false }
    if (!res.ok) return this.isAuthenticated
    // Corpo inválido é tratado como sem sessão (fail-closed), nunca como "segue como estava".
    let sessao: SessaoWeb | null = null
    try {
      sessao = this.lerSessao(await res.json())
    } catch { /* sessão inválida */ }
    if (!signal.aborted) {
      if (sessao) this.guardar(sessao)
      else this.handleUnauthorized()
    }
    return this.isAuthenticated
  }

  /** Envia o token local do Hub; o cookie HttpOnly nasce aqui, na resposta do Hub. */
  async login(token: string): Promise<ErroLogin | null> {
    if (typeof token !== 'string' || !token.trim()) return 'invalid'
    const signal = this.signal
    let res: Response
    try {
      res = await this.meuFetch(`${this.origin}/web/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-Mexerica-Web': '1' },
        credentials: 'same-origin',
        cache: 'no-store',
        signal,
        body: JSON.stringify({ token }),
      })
    } catch { return 'offline' }
    if (res.status === 401) return 'invalid'
    if (res.status === 429) return 'rateLimited'
    if (!res.ok) return 'error'
    try {
      const sessao = this.lerSessao(await res.json())
      if (!sessao || signal.aborted) return 'error'
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
const INDISPONIVEL = erroLocal(0, 'local hub unreachable')
const EPOCH_DIVERGENTE = 'workspace_epoch_mismatch'
const CODIGOS_RPC = new Set([
  'invalid_request', 'access_denied', 'not_connected', 'reconnect_required', 'unavailable', 'cursor_invalid',
  'name_invalid', 'name_taken', 'group_missing', 'bot_missing', 'migrating',
  'skill_installation_unsupported', 'skill_installation_stale', 'skill_installation_changed',
  'skill_installation_unavailable', 'skill_mutation_outcome_unknown',
  'skill_selection_unsupported', 'skill_selection_conflict',
])

function pedidoDaSessao(geracao: AbortSignal) {
  const controller = new AbortController()
  const abortar = () => controller.abort()
  if (geracao.aborted) abortar()
  else geracao.addEventListener('abort', abortar, { once: true })
  return { signal: controller.signal, abortar, liberar: () => geracao.removeEventListener('abort', abortar) }
}

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
      if (!csrf) throw SEM_SESSAO
      const geracao = session.signal
      const { signal, abortar, liberar } = pedidoDaSessao(geracao)
      const timeout = setTimeout(abortar, timeoutMs)
      try {
        let res: Response
        try {
          res = await fetchImpl(`${session.origin}/api/${method}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', 'X-Mexerica-Csrf': csrf },
            credentials: 'same-origin',
            cache: 'no-store',
            body: JSON.stringify(body ?? {}),
            signal,
          })
        } catch { throw geracao.aborted ? SEM_SESSAO : INDISPONIVEL }
        if (geracao.aborted || res.status === 401) {
          if (!geracao.aborted) session.handleUnauthorized()
          if (res.body) void res.body.cancel().catch(() => {})
          throw SEM_SESSAO
        }
        const texto = await res.text().catch(() => { throw geracao.aborted ? SEM_SESSAO : INDISPONIVEL })
        if (geracao.aborted) throw SEM_SESSAO
        if (signal.aborted) throw INDISPONIVEL
        let valor: unknown
        try { valor = texto ? JSON.parse(texto) : {} }
        catch { if (res.ok) throw INDISPONIVEL }
        if (res.ok) return valor
        const envelope = valor as { code?: unknown; error?: unknown } | null
        const legado = envelope?.error
        const codigoLegado = typeof legado === 'string' && /^skill_/.test(legado) && CODIGOS_RPC.has(legado) ? legado : undefined
        const code = envelope?.code
        const codigo = typeof code === 'string' && CODIGOS_RPC.has(code) ? code : codigoLegado
        const erro = erroLocal(res.status, codigoLegado ?? `Host error ${res.status}`)
        if (res.status === 409 && code === EPOCH_DIVERGENTE) erro.code = EPOCH_DIVERGENTE
        else if (codigo) erro.code = codigo
        throw erro
      } finally {
        clearTimeout(timeout)
        liberar()
      }
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
        queueMicrotask(() => onEnd(SEM_SESSAO))
        return () => {}
      }
      const geracao = session.signal
      const { signal, abortar, liberar } = pedidoDaSessao(geracao)
      // Timeout ocioso, não total: o Hub pinge a cada 15 s, então silêncio desse tamanho é Hub caído.
      let ocioso: ReturnType<typeof setTimeout> | null = null
      const armar = () => {
        if (ocioso) clearTimeout(ocioso)
        ocioso = setTimeout(abortar, timeoutOcioso)
      }
      const encerrar = (erro: CallError | null) => {
        if (ocioso) { clearTimeout(ocioso); ocioso = null }
        onEnd(erro)
      }
      void (async () => {
        let res: Response | undefined
        let reader: ReadableStreamDefaultReader<Uint8Array> | undefined
        const cancelarLeitura = () => { if (reader) void reader.cancel().catch(() => {}) }
        let fim: CallError | null = null
        try {
          armar()
          res = await fetchImpl(destino, {
            headers: { Accept: 'text/event-stream', 'X-Mexerica-Csrf': csrf },
            credentials: 'same-origin',
            cache: 'no-store',
            signal,
          })
          if (signal.aborted) throw INDISPONIVEL
          if (res.status === 401) {
            session.handleUnauthorized()
            fim = SEM_SESSAO
            return
          }
          if (!res.ok || !res.body) {
            fim = erroLocal(res.status, `Host error ${res.status}`)
            if (res.status === 409 && res.body) {
              const envelope = await res.json().catch(() => null) as { code?: unknown } | null
              if (signal.aborted) throw INDISPONIVEL
              if (envelope?.code === EPOCH_DIVERGENTE) fim.code = EPOCH_DIVERGENTE
            }
            return
          }
          reader = res.body.getReader()
          signal.addEventListener('abort', cancelarLeitura, { once: true })
          const decoder = new TextDecoder()
          let buffer = ''
          for (;;) {
            const { value, done } = await reader.read()
            if (signal.aborted) throw INDISPONIVEL
            if (done) break
            armar()
            buffer += decoder.decode(value, { stream: true })
            let nl: number
            while ((nl = buffer.indexOf('\n')) >= 0) {
              const line = buffer.slice(0, nl).replace(/\r$/, '')
              buffer = buffer.slice(nl + 1)
              if (line.startsWith('data:')) {
                if (signal.aborted) throw INDISPONIVEL
                onData(line.slice(5).trimStart())
              }
            }
          }
        } catch {
          fim = geracao.aborted ? SEM_SESSAO : INDISPONIVEL
        } finally {
          liberar()
          signal.removeEventListener('abort', cancelarLeitura)
          if (fim && reader) cancelarLeitura()
          else if (fim && res?.body) void res.body.cancel().catch(() => {})
          reader?.releaseLock()
          encerrar(fim)
        }
      })()
      return () => {
        if (ocioso) { clearTimeout(ocioso); ocioso = null }
        abortar()
      }
    },
  }
}
