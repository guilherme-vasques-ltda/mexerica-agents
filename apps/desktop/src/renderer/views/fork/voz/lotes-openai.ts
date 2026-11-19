// voz-16a (D71): estado do protocolo OpenAI por response e por invocação.
type Resposta = {
  id: string | null
  estado: 'gerando' | 'concluida' | 'abandonada'
  continuar: boolean
  chamadas: Map<string, boolean>
}
const resposta = (id: string | null): Resposta => ({ id, estado: 'gerando', continuar: false, chamadas: new Map() })

export class LotesOpenAI {
  private atual: Resposta | null = null
  private readonly criadas = new Set<string>()

  /** Reserva a geração solicitada; seu ID só chega no response.created. */
  solicitar() { this.atual = resposta(null) }

  iniciar(id: unknown): boolean {
    if (typeof id !== 'string' || !id || this.criadas.has(id)) return false
    this.criadas.add(id)
    this.atual = resposta(id)
    return true
  }

  abandonar() {
    if (!this.atual) return
    this.atual.estado = 'abandonada'
    this.atual.continuar = false
  }

  terminar(id: unknown, status: unknown): boolean {
    const r = this.atual
    if (!r || r.id === null || id !== r.id || r.estado !== 'gerando') return false
    r.estado = status === 'completed' ? 'concluida' : 'abandonada'
    if (r.estado === 'abandonada') r.continuar = false
    return true
  }

  /** Dedup antes do await; a conclusão só pode alterar esta resposta/invocação. */
  invocar(id: unknown, callId: string): ((continuar?: boolean) => boolean) | null {
    const r = this.atual
    if (!r || r.id === null || id !== r.id || r.estado === 'abandonada' || r.chamadas.has(callId)) return null
    r.chamadas.set(callId, true)
    return (continuar = true) => {
      if (this.atual !== r || r.estado === 'abandonada' || r.chamadas.get(callId) !== true) return false
      r.chamadas.set(callId, false)
      if (continuar) r.continuar = true
      return true
    }
  }

  get ocupado(): boolean {
    const r = this.atual
    return !!r && r.estado !== 'abandonada' && (r.estado === 'gerando' || [...r.chamadas.values()].some(Boolean))
  }

  get continuar(): boolean { return this.atual?.continuar ?? false }
}
