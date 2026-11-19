import { HostError, type HostTransport } from './host-client'
import type { ForkCloudEventsBody } from '../../shared/fork-cloud'

/** A chamada pode ter sido executada: BotSync não deve reenviá-la. */
export class RelayOutcomeUnknown extends HostError {
  readonly outcome = 'unknown' as const
  constructor() { super(0, 'Resultado não confirmado. Confira o estado antes de tentar novamente.') }
  get isTransient() { return false }
}

function eventsBody(path: string): ForkCloudEventsBody | null {
  const term = /^\/term\/([a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12})$/.exec(path)
  if (term) return { since: 0, client: 'login', kind: 'term', term: term[1] }
  if (!path.startsWith('/events?') || path.includes('#')) return null
  try {
    const url = new URL(path, 'http://localhost')
    if (url.origin !== 'http://localhost' || url.pathname !== '/events') return null
    const keys = [...url.searchParams.keys()]
    if (keys.some(k => !['since', 'client', 'spaceId'].includes(k)) || keys.filter(k => k === 'since').length !== 1 || keys.filter(k => k === 'client').length !== 1 || keys.filter(k => k === 'spaceId').length > 1) return null
    const since = url.searchParams.get('since'), client = url.searchParams.get('client'), spaceId = url.searchParams.get('spaceId')
    const valid = (v: string | null): v is string => !!v && v.length <= 128 && !/[\x00-\x1f\x7f]/.test(v)
    if (since === null || !/^(0|[1-9][0-9]*)$/.test(since) || !Number.isSafeInteger(Number(since)) || !valid(client) || (spaceId !== null && !valid(spaceId))) return null
    return { since: Number(since), client, ...(spaceId === null ? {} : { spaceId }) }
  } catch { return null }
}

/** Vinculado ao ID de um único canal qualificado; não escolhe chave nem fallback. */
export class ForkRelayTransport implements HostTransport {
  constructor(private readonly connectionId: string) {}
  async call(method: string, body: unknown, timeoutMs: number): Promise<unknown> {
    let result
    try { result = await window.codync.forkCloud.call(this.connectionId, method, body, timeoutMs) }
    catch { throw new RelayOutcomeUnknown() }
    switch (result.kind) {
      case 'ok': return result.value
      case 'err': throw new HostError(result.status, result.message, result.code)
      case 'unknown': throw new RelayOutcomeUnknown()
      case 'notSent': throw new HostError(result.reason === 'tooLarge' ? 413 : result.reason === 'busy' ? 429 : 0, 'Chamada não enviada ao Hub.')
    }
  }
  stream(path: string, onData: (line: string) => void, onEnd: (error: HostError | null) => void): () => void {
    const body = eventsBody(path)
    if (!body) { let active = true; queueMicrotask(() => { if (active) onEnd(new HostError(400, 'Assinatura inválida.')) }); return () => { active = false } }
    let active = true
    const cancel = window.codync.forkCloud.subscribeEvents(this.connectionId, body, event => {
      if (!active) return
      try { const line = JSON.stringify(event); if (line !== undefined) onData(line) } catch { /* evento inválido não vira comando */ }
    }, error => { if (!active) return; active = false; onEnd(error ? new HostError(error.status, error.message) : null) })
    return () => { if (!active) return; active = false; cancel() }
  }
}
