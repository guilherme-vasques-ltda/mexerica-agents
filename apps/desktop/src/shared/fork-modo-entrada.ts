/** Como a pessoa escolheu usar o app na entrada guiada. Perguntado uma vez e gravado pelo processo principal. */
export type ModoEntrada = 'local' | 'conta'

/** `infraestrutura` só existe com conta; "na nossa nuvem" ainda não é uma escolha possível. */
export interface EscolhaEntrada {
  readonly modo: ModoEntrada
  readonly infraestrutura: 'propria' | null
}

export function escolhaEntradaValida(valor: unknown): EscolhaEntrada | null {
  if (!valor || typeof valor !== 'object' || Array.isArray(valor)) return null
  const { modo, infraestrutura } = valor as Record<string, unknown>
  if (modo === 'local' && infraestrutura === null) return { modo, infraestrutura }
  if (modo === 'conta' && infraestrutura === 'propria') return { modo, infraestrutura }
  return null
}
