// MX-214 (rodadas 4 e 5): a Ilha é outra janela. O envio dela só vale se a janela da conversa CONFIRMAR o aceite; o
// resultado volta por este registro. Sem confirmação (prazo, destino ausente, resposta inválida) NÃO é aceite: a Ilha
// mantém o texto e diz que não confirmou. Resposta atrasada, repetida ou de outro envio (id) nunca resolve nada.
import type { ResultadoEnvioIlha } from '../shared/ipc'

export const PRAZO_RESPOSTA_ILHA_MS = 4000

/** Só estes dois valores podem vir da janela da conversa; qualquer outro é ignorado (e o prazo decide). */
export const RESULTADOS_DA_CONVERSA = ['aceito', 'bloqueado'] as const

export class RespostasDaIlha {
  private pendentes = new Map<string, (r: ResultadoEnvioIlha) => void>()

  /** Abre um envio: `id` vai junto do comando à janela da conversa; `resultado` resolve quando ela responde (ou no prazo). */
  abrir(prazo = PRAZO_RESPOSTA_ILHA_MS, id: string = globalThis.crypto.randomUUID()): { id: string; resultado: Promise<ResultadoEnvioIlha> } {
    const resultado = new Promise<ResultadoEnvioIlha>((resolve) => {
      const timer = setTimeout(() => { this.pendentes.delete(id); resolve('sem-confirmacao') }, prazo)
      this.pendentes.set(id, (r) => { clearTimeout(timer); resolve(r) })
    })
    return { id, resultado }
  }

  /** A janela da conversa respondeu. Id desconhecido, já resolvido ou resultado fora do formato: ignorado. */
  responder(id: unknown, resultado: unknown): boolean {
    if (typeof id !== 'string' || !RESULTADOS_DA_CONVERSA.includes(resultado as never)) return false
    const fechar = this.pendentes.get(id)
    if (!fechar) return false
    this.pendentes.delete(id)
    fechar(resultado as ResultadoEnvioIlha)
    return true
  }
}

/** O pedido de envio da Ilha, com formato validado antes de qualquer coisa chegar à janela da conversa. */
export interface PedidoDaIlha {
  ref: string
  text: string
  contentOrigin?: unknown
  nonce: string
}

const NONCE = /^[A-Za-z0-9-]{8,64}$/

/** `null` se o formato não bate (o envio nem é despachado e a Ilha mantém o texto). */
export function pedidoDaIlha(ref: unknown, text: unknown, contentOrigin: unknown, nonce: unknown): PedidoDaIlha | null {
  if (typeof ref !== 'string' || !ref || ref.length > 512) return null
  if (typeof text !== 'string' || !text.trim() || text.length > 1_000_000) return null
  if (typeof nonce !== 'string' || !NONCE.test(nonce)) return null
  if (contentOrigin !== undefined && contentOrigin !== null && (typeof contentOrigin !== 'object' || Array.isArray(contentOrigin))) return null
  return { ref, text, contentOrigin: contentOrigin ?? undefined, nonce }
}
