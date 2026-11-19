// voz-encerrar (D71): texto do bot nunca passa pelo detector; autorização vem dos eventos de entrada.

export const TETO_DESPEDIDA_MS = 3000
export const ENCERRAMENTO_RECUSADO = 'Só posso encerrar por um pedido na fala da pessoa neste turno.'
export const ENCERRAMENTO_PELO_BOTAO = 'Use o botão vermelho para encerrar.'
export type PermissaoEncerramento = boolean | 'confirmar' | 'botao'

/** Frase inteira normalizada: sem acentos, caixa e pontuação; aspas ficam (citação não é pedido). */
function normalizar(texto: string, idioma: string) {
  const frase = texto.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase()
    .replace(/(?<=\p{L})\s*[—–-]\s*(?=\p{L})/gu, ',')
    .replace(/[.!?,;:…¿¡]/g, ' ').replace(/\s+/g, ' ').trim()
  return { frase, lingua: idioma.toLowerCase().replace('_', '-').split('-')[0] }
}

/** Lista deliberadamente curta: não procura verbos soltos no meio de uma conversa. D416: despedidas simples encerram. */
export function pediuEncerramento(texto: string, idioma: string): boolean {
  const { frase, lingua } = normalizar(texto, idioma)
  if (lingua === 'pt') return /^(?:(?:por favor )?(?:desliga|pode desligar|encerra|encerre a chamada|desligar a chamada|tchau pode fechar)(?: por favor)?|(?:(?:ok|entao|valeu|obrigad[oa]) )?(?:tchau(?: tchau)?|ate (?:depois|mais|logo)))$/.test(frase)
  if (lingua === 'en') return /^(?:(?:please )?(?:hang up|end the call|you can hang up|bye you can close the call)(?: please)?|(?:(?:ok|okay|thanks|thank you) )?(?:bye(?: bye)?|goodbye|see you later|talk to you later))$/.test(frase)
  return false
}

/**
 * D416: "para", "espera", "fica quieto" mantêm a chamada aberta. Nos motores locais a frase não vai ao bot
 * e a escuta continua; não muta o microfone nem para o trabalho do bot (controle que este recorte não oferece).
 */
export function pediuSilencio(texto: string, idioma: string): boolean {
  const { frase, lingua } = normalizar(texto, idioma)
  if (lingua === 'pt') return /^(?:por favor )?(?:para|pare|para de falar|espera|espere|espera ai|pera|pera ai|fica quiet[oa]|fique quiet[oa]|silencio)(?: por favor)?$/.test(frase)
  if (lingua === 'en') return /^(?:please )?(?:stop|stop talking|wait|hold on|be quiet|quiet)(?: please)?$/.test(frase)
  return false
}

// Compatibilidade: os motores e os efeitos usam a única guarda em operador-voz.
export { OrigemEncerramento, OrigemGemini } from './operador-voz.ts'

/** Bloqueia trabalho novo antes de aguardar somente o áudio da despedida já em curso. */
export class Despedida {
  fechando = false
  private timer: ReturnType<typeof setTimeout> | null = null
  private concluida = false
  private readonly pararPedidos: () => void
  private readonly fechar: () => void

  constructor(pararPedidos: () => void, fechar: () => void) {
    this.pararPedidos = pararPedidos
    this.fechar = fechar
  }

  pedir(temAudio: boolean) {
    if (this.fechando || this.concluida) return
    this.fechando = true
    this.pararPedidos()
    if (temAudio) this.timer = setTimeout(() => this.audioTerminou(), TETO_DESPEDIDA_MS)
    else this.fechar()
  }

  audioTerminou() {
    if (!this.fechando || this.concluida) return
    this.cancelar()
    this.fechar()
  }

  cancelar() {
    this.concluida = true
    if (this.timer !== null) clearTimeout(this.timer)
    this.timer = null
  }
}
