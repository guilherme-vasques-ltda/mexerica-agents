// DON-908 (C8), só na demo: `donnaCostReport` com números fictícios, nas formas do contrato.
// `?custo=<cena>`: normal (dinheiro do motor, uma conversa com ajudantes), parcial (parte dos turnos sem preço),
// sem-preco (só tokens), duas-moedas (USD e BRL lado a lado, sem conversão), vazio (nada registrado), antigo (Hub sem o
// RPC) e so-dono (403). Sem `?custo`, a cena normal. Meses anteriores ao corrente voltam vazios.

type Cena = 'normal' | 'parcial' | 'sem-preco' | 'duas-moedas' | 'vazio' | 'antigo' | 'so-dono'
const CENAS: readonly Cena[] = ['normal', 'parcial', 'sem-preco', 'duas-moedas', 'vazio', 'antigo', 'so-dono']
const pedida = new URLSearchParams(location.search).get('custo')
const cena: Cena = (CENAS as readonly string[]).includes(pedida ?? '') ? (pedida as Cena) : 'normal'

interface Totais { turns: number; unpricedTurns: number; inputTokens: number | null; cachedReadTokens: number | null; cachedWriteTokens: number | null; outputTokens: number | null; costs: { amount: number; currency: string; source: 'motor' }[] }

/** Um total fictício: `usd` em dólares do motor; a cena decide se o preço some ou fica parcial. */
function tot(turns: number, usd: number, tokensEscala: number, brl = 0): Totais {
  const semPreco = cena === 'sem-preco'
  const parcial = cena === 'parcial' ? Math.ceil(turns / 4) : 0
  const costs = semPreco ? [] : [{ amount: usd, currency: 'USD', source: 'motor' as const }, ...(cena === 'duas-moedas' && brl ? [{ amount: brl, currency: 'BRL', source: 'motor' as const }] : [])]
  return { turns, unpricedTurns: semPreco ? turns : parcial, inputTokens: 12_000 * tokensEscala, cachedReadTokens: 310_000 * tokensEscala, cachedWriteTokens: 42_000 * tokensEscala, outputTokens: 9_000 * tokensEscala, costs }
}
const vazio = (): Totais => ({ turns: 0, unpricedTurns: 0, inputTokens: null, cachedReadTokens: null, cachedWriteTokens: null, outputTokens: null, costs: [] })

// Os ajudantes de Crivo já somam na conversa principal dela (o Hub manda pronto; nada é somado aqui na UI).
const linhas = () => {
  const conv = (botId: string, sessionId: string, title: string, parentSessionId: string | null, totals: Totais) => ({ botId, chatId: botId, sessionId, title, parentSessionId, totals })
  const conversations = [
    conv('crivo', 'crivo', '', null, tot(18, 4.82, 6, 9.4)),
    conv('crivo', 'crivo-revisao', 'Revisar a guarda de datas', 'crivo', tot(5, 1.37, 2)),
    conv('crivo', 'crivo-testes', '', 'crivo', tot(3, 0.64, 1)),
    conv('byte', 'byte', '', null, tot(11, 2.15, 3)),
    conv('byte', 'byte-docs', 'Notas da versão', null, tot(4, 0.38, 1)),
    conv('sprint', 'sprint', '', null, tot(2, 0.09, 0.2, 0.6)),
  ]
  const bots = [
    { botId: 'crivo', name: 'Crivo', totals: tot(18, 4.82, 6, 9.4) },
    { botId: 'byte', name: 'Byte', totals: tot(15, 2.53, 4) },
    { botId: 'sprint', name: 'Berga', totals: tot(2, 0.09, 0.2, 0.6) },
  ]
  return { conversations, helpers: conversations.filter((c) => c.parentSessionId !== null), bots, total: tot(35, 7.44, 10, 10) }
}

export class FakeCusto {
  constructor(private readonly hubId: () => string) {}

  atende = (method: string) => method === 'donnaCostReport'

  async call(b: Record<string, unknown>, fail: (m: string) => unknown): Promise<unknown> {
    if (cena === 'antigo') throw fail('unknown method: donnaCostReport')
    if (cena === 'so-dono') throw Object.assign(new Error('owner only') as Error & { status?: number }, { status: 403 })
    const corrente = new Date().toISOString().slice(0, 7)
    const month = typeof b.month === 'string' ? b.month : corrente
    const base = { hubId: this.hubId(), measuredAt: Date.now(), month }
    const dados = cena === 'vazio' || month < corrente ? { conversations: [], helpers: [], bots: [], total: vazio() } : linhas()
    if (typeof b.botId !== 'string') return { ...base, ...dados, conversation: null }
    // O balão: todo o histórico da sessão pedida (principal = sem sessionId), independente do mês.
    const sessionId = typeof b.sessionId === 'string' ? b.sessionId : b.botId
    const achada = cena === 'vazio' ? null : linhas().conversations.find((c) => c.botId === b.botId && c.sessionId === sessionId)
    const conversation = achada ?? { botId: b.botId, chatId: b.botId, sessionId, title: '', parentSessionId: null, totals: vazio() }
    return { ...base, ...dados, conversation }
  }
}
