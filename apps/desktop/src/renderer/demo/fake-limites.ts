// DON-907 C4, só na demo: os limites de uso e a espera do limite em Crivo, no formato do contrato do Hub
// (docs/reference/design/don-907-c4/README.md). `?limites=espera|soneca|desligada|pronta|cancelada|hub-antigo`. Nada sai daqui.
import type { EntryData } from '@shared/models'

const cena = () => (typeof location !== 'undefined' ? new URLSearchParams(location.search).get('limites') : null)
const volta = '18:30 (BRT)'
// Os textos literais do Hub (host/src/fork/usage_limits/ator.rs `usage_notice`/`publish_usage_cancelled`, e waiting.rs).
const textos = {
  espera: `Limite da assinatura atingido; volto às ${volta}.`,
  soneca: `Em soneca até o limite voltar, às ${volta}. Sem tentativas nem avisos até lá; depois retomo automaticamente.`,
  desligada: 'Limite da assinatura atingido. A retomada automática está desligada; o pedido está guardado. Use Retomar agora para tentar continuar.',
  pronta: 'O prazo informado para o limite chegou. Use Retomar agora para tentar continuar.',
  cancelada: 'Não vou retomar este pedido. O pedido e a sessão continuam guardados.',
}

export class FakeLimites {
  private ajustes = { autoResume: cena() !== 'desligada' && cena() !== 'pronta', snooze: cena() === 'soneca' }
  private seq = 1

  /** O aviso que a cena pede, para o histórico de Crivo; `null` sem `?limites`. */
  aviso(at: number): EntryData | null {
    const c = cena()
    if (!c) return null
    if (c === 'hub-antigo') return { text: textos.espera, style: 'info', author: 'crivo', subscriptionWait: { resumeAt: at + 3_600_000, retry: false } }
    const estado = c in textos ? (c as keyof typeof textos) : 'espera'
    return this.dados(estado, at + 3_600_000)
  }

  private dados(estado: keyof typeof textos, resumeAt: number): EntryData {
    return {
      text: textos[estado], style: 'info', author: 'crivo',
      subscriptionWait: { resumeAt, retry: estado === 'desligada' || estado === 'pronta' || estado === 'cancelada', waitId: `espera-${this.seq}`, cancelled: estado === 'cancelada', snoozed: estado === 'soneca', alreadyStarted: false, autoResume: estado !== 'cancelada' && this.ajustes.autoResume, ready: estado === 'pronta' },
      // `retry` = o Hub aceita retomar à mão agora (contrato C4): recém-bloqueada sem retomada automática, de volta, ou cancelada antes do envio.
    }
  }

  atende(method: string) { return method.startsWith('donnaUsage') }

  /** `muda`: grava os dados novos no aviso (e emite); devolve o aviso atual. */
  call(method: string, b: Record<string, unknown>, aviso: { id: string; data: EntryData } | undefined, muda: (data: EntryData) => void, fail: (m: string) => unknown) {
    if (cena() === 'hub-antigo') throw fail('unknown method')
    switch (method) {
      case 'donnaUsageLimits': return { ...this.ajustes }
      case 'donnaUsageLimitsSave':
        if (typeof b.autoResume !== 'boolean' || typeof b.snooze !== 'boolean') throw fail('invalid_field')
        this.ajustes = { autoResume: b.autoResume, snooze: b.snooze }
        return { ...this.ajustes }
    }
    const w = aviso?.data.subscriptionWait
    if (!aviso || !w || b.entryId !== aviso.id || b.waitId !== w.waitId) throw fail('wait_mismatch: esta espera já não existe')
    const resumeAt = w.resumeAt ?? Date.now()
    if (method === 'donnaUsageWaitCancel') { muda(this.dados('cancelada', resumeAt)); return { cancelled: true, alreadyStarted: false } }
    if (method === 'donnaUsageWaitResume') { this.seq++; muda(this.dados('espera', resumeAt)); return { retryRequested: true } }
    throw fail(`unknown method ${method}`)
  }
}
