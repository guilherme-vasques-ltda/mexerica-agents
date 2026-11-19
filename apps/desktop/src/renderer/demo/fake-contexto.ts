// DON-906 (C2), só na demo: `donnaContext`/`donnaContextCompact` com números fictícios, nas formas do contrato.
// `?contexto=<cena>`: normal (42%), alto (94%), resumida (com última compressão), sem-botao (adaptador não
// comprime), ocupado (turno aberto), ausente (sem `context`), repetida (resumida há pouco: o Hub recusa Comprimir). Sem `?contexto`, todos os bots ficam na cena normal.
// `window.donnabotContextoDemo.comprimir(botId)` simula uma compressão do motor (pulso do anel).
// C3 (DON-906/DON-910): `?contexto=pausado` = 2 falhas seguidas, resumo automático pausado (Comprimir agora despausa).
// `donnaContextSettings`/`donnaContextSettingsSave` com o mesmo DTO do Hub; `?ajustes=manual` (120 mil, nunca),
// `?ajustes=sem-dono` (o Hub recusa: 403) ou `?ajustes=antigo` (Hub sem o RPC).

type Cena = 'normal' | 'alto' | 'resumida' | 'sem-botao' | 'ocupado' | 'ausente' | 'repetida' | 'pausado'
const CENAS: readonly Cena[] = ['normal', 'alto', 'resumida', 'sem-botao', 'ocupado', 'ausente', 'repetida', 'pausado']
const pedida = new URLSearchParams(location.search).get('contexto')
const cena: Cena = (CENAS as readonly string[]).includes(pedida ?? '') ? (pedida as Cena) : 'normal'
const SIZE = 200_000
const cenaAjustes = new URLSearchParams(location.search).get('ajustes')
const AUTO = Math.min(SIZE * 0.75, 200_000)
// Regra atual do crate: comprime sozinho em 75% da janela, no máximo 200 mil (200 mil → 150 mil).
const hoje = (h: number, m: number) => { const d = new Date(); d.setHours(h, m, 0, 0); return d.getTime() }

interface Estado { epoch: number; used: number; lastCompaction: { at: number; byHost: boolean } | null; busy: boolean; canCompact: boolean; failures: number }
interface Ajustes { triggerTokens: number | null; idle: 'beforeExpiry' | 'never' }

export class FakeContexto {
  private estados = new Map<string, Estado>()
  private ajustes: Ajustes = cenaAjustes === 'manual' ? { triggerTokens: 120_000, idle: 'never' } : { triggerTokens: null, idle: 'beforeExpiry' }

  constructor(private readonly hubId: () => string) {
    ;(window as unknown as Record<string, unknown>).donnabotContextoDemo = {
      comprimir: (botId: string, at = Date.now()) => this.comprimido(botId, at),
    }
  }

  atende = (method: string) => method === 'donnaContext' || method === 'donnaContextCompact' || method === 'donnaContextSettings' || method === 'donnaContextSettingsSave'

  private estado(botId: string): Estado {
    let e = this.estados.get(botId)
    if (!e) {
      e = {
        epoch: cena === 'resumida' || cena === 'repetida' ? 1 : 0,
        used: cena === 'alto' ? 188_000 : cena === 'resumida' || cena === 'repetida' ? 31_000 : 84_000,
        lastCompaction: cena === 'resumida' || cena === 'repetida' ? { at: hoje(14, 32), byHost: false } : null,
        busy: cena === 'ocupado',
        canCompact: cena !== 'sem-botao',
        failures: cena === 'pausado' ? 2 : 0,
      }
      this.estados.set(botId, e)
    }
    return e
  }

  private comprimido(botId: string, at: number) {
    const e = this.estado(botId)
    e.epoch++
    e.used = 31_000
    e.lastCompaction = { at, byHost: true }
    e.busy = false
    e.failures = 0
  }

  async call(method: string, b: Record<string, unknown>, fail: (m: string) => unknown): Promise<unknown> {
    if (method === 'donnaContextSettings' || method === 'donnaContextSettingsSave') return this.callAjustes(method, b, fail)
    const botId = typeof b.botId === 'string' ? b.botId : ''
    const sessionId = typeof b.sessionId === 'string' ? b.sessionId : botId
    if (method === 'donnaContext') {
      // scopeId estável por ACP na demo: um por conversa/sessão.
      const topo = { hubId: this.hubId(), botId, sessionId, scopeId: `demo-scope-${botId}-${sessionId}` }
      // Só a principal tem medida na demo; uma extra volta sem `context`.
      if (cena === 'ausente' || sessionId !== botId) return { ...topo, epoch: 0 }
      const e = this.estado(botId)
      return { ...topo, epoch: e.epoch, context: { used: e.used, size: SIZE, updatedAt: Date.now(), trigger: this.ajustes.triggerTokens ?? AUTO, lastCompaction: e.lastCompaction ?? undefined, canCompact: e.canCompact, busy: e.busy, contextId: `demo-${botId}`, automaticPaused: e.failures >= 2, automaticFailures: e.failures } }
    }
    const e = this.estado(botId)
    if (!e.canCompact) throw fail('compaction_unsupported: este motor não comprime sob pedido')
    if (e.busy) throw fail('context_busy: espere o turno terminar')
    if (cena === 'repetida') throw fail('context_repeat_guard')
    if (b.contextId !== `demo-${botId}`) throw fail('context_changed: o motor mudou')
    e.busy = true
    setTimeout(() => this.comprimido(botId, Date.now()), 1500)
    return { accepted: true }
  }

  /** O ajuste do Hub inteiro, com a mesma conferência do Hub: null ou inteiro seguro ≥ 50 mil, múltiplo de 1000; idle fechado. */
  private async callAjustes(method: string, b: Record<string, unknown>, fail: (m: string) => unknown): Promise<unknown> {
    if (cenaAjustes === 'antigo') throw fail('unknown method: ' + method)
    if (cenaAjustes === 'sem-dono') throw { status: 403, message: 'forbidden: owner only' }
    if (method === 'donnaContextSettings') return { ...this.ajustes }
    const t = b.triggerTokens
    if (b.idle !== 'beforeExpiry' && b.idle !== 'never') throw fail('invalid_idle')
    if (t !== null && !(typeof t === 'number' && Number.isSafeInteger(t) && t >= 50_000 && t % 1000 === 0)) throw fail('invalid_trigger')
    this.ajustes = { triggerTokens: t, idle: b.idle }
    return { ...this.ajustes }
  }
}
