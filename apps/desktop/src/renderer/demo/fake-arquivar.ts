// DON-907 C5, só na demo: encerrar, continuar e "Arquivar sozinho" no formato do Hub (host/src/fork/sessoes/encerrar.rs).
// `?arquivar=1` anuncia `close`/`continue` e responde os RPCs; `?arquivar=hub-antigo` simula o Hub sem eles (nenhuma
// ação e a seção pede atualização). Sem o parâmetro, a demo fica como antes. Nada sai daqui.
const cena = () => (typeof location !== 'undefined' ? new URLSearchParams(location.search).get('arquivar') : null)
export const arquivarDemo = () => cena() === '1'
const antigo = () => cena() === 'hub-antigo'

/** A linha do cartão de continuação, como o Hub grava (`handoff::Reason::Continuation`). */
const LINHA = 'Your first message in this session starts from this summary of the closed one.'

type Row = { id: string; chatId: string; principal: boolean; state: string; closedReason: string | null; closedAt: number | null; revision: number; title: string }
type Fechada = { reason: string; at: number; revision: number }

export class FakeArquivar {
  private config = { days: 3 as number | null, revision: 0 }
  /** As extras que esta demo encerrou ou que a cena mostra encerradas por inatividade. */
  private fechadas = new Map<string, Fechada>()

  /** O que `sessions` anuncia a mais. */
  actions(): string[] { return arquivarDemo() ? ['close', 'continue'] : [] }

  /** A linha como o Hub a guarda depois das mudanças da demo; as encerradas da história = pela varredura (`idle`). */
  ajustar<R extends Row>(row: R): R {
    const f = this.fechadas.get(row.id)
    if (f) return { ...row, state: 'closed', closedReason: f.reason, closedAt: f.at, revision: f.revision }
    if (arquivarDemo() && row.state === 'closed') return { ...row, closedReason: 'idle' }
    return row
  }

  atende(method: string) { return method === 'sessionClose' || method === 'sessionContinue' || method.startsWith('sessionAutoClose') }

  /** `rows`: o catálogo atual; `criar`: uma extra nova de continuação com o cartão; `rev`: a próxima revisão. */
  call(method: string, b: Record<string, unknown>, deps: {
    rows: (chatId: string) => Row[]
    criar: (chatId: string, source: Row) => { session: Row; handoff: unknown }
    emit: (session: Row) => void
    rev: () => number
    fail: (m: string) => unknown
  }) {
    if (antigo() || !arquivarDemo()) throw deps.fail(`unknown method ${method}`)
    if (method === 'sessionAutoCloseConfig') return { config: { ...this.config, defaultDays: 3, maxDays: 90 } }
    if (method === 'sessionAutoCloseSave') {
      if (b.expectedRevision !== this.config.revision) throw deps.fail('autoclose_revision_conflict')
      const days = b.days as number | null
      if (days !== null && !(Number.isInteger(days) && days >= 1 && days <= 90)) throw deps.fail('invalid_field: days deve ser de 1 a 90, ou null.')
      this.config = { days, revision: deps.rev() }
      return { config: { ...this.config, defaultDays: 3, maxDays: 90 } }
    }
    const chatId = String(b.botId ?? '')
    const row = deps.rows(chatId).find((r) => r.id === b.sessionId)
    if (!row) throw deps.fail('unknown_session')
    if (method === 'sessionClose') {
      if (row.principal) throw deps.fail('principal_not_closable')
      if (row.state !== 'open') throw deps.fail('session_closed')
      if (row.revision !== b.expectedRevision) throw deps.fail('session_revision_conflict')
      this.fechadas.set(row.id, { reason: 'owner', at: Date.now(), revision: deps.rev() })
      const session = deps.rows(chatId).find((r) => r.id === row.id)!
      deps.emit(session)
      return { session, cancelled: { entryIds: [] }, stopRequested: false }
    }
    if (row.principal) throw deps.fail('principal_not_continuable')
    if (row.state !== 'closed') throw deps.fail('session_open')
    if (row.revision !== b.expectedRevision) throw deps.fail('session_revision_conflict')
    const { session, handoff } = deps.criar(chatId, row)
    deps.emit(session)
    return { session, source: row, handoff }
  }
}

/** O cartão Draft da nova sessão, com o resumo da origem (texto fictício). */
export const cartaoContinuacao = (title: string) => ({
  text: LINHA, style: 'info', author: 'byte',
  handoff: { status: 'draft', reason: 'continuation', text: `## Sessão encerrada\n- ${title}\n\n## Últimos pedidos\n- Rename the pace helpers to match the weeklyTotals naming.\n\n## Plano aberto\n- [pending] Update the call sites in charts.ts` },
})
