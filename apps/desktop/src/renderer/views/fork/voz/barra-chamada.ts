// Auditoria D249 (F1): a barra da chamada diz o estado em uma palavra e o tempo desde o início da chamada.
// Módulo puro (sem imports), para o CallView e os testes; os outros textos da chamada seguem em textos-chamada.ts.
/**
 * Auditoria D249 (F1): o estado da chamada em uma palavra, na barra, ao lado do tempo. A falha continua dizendo
 * "Não consigo ouvir" na barra, com o motivo na linha de avisos.
 */
export const ESTADO_CHAMADA = {
  encerrada: 'voz.chamada.barra.encerrada',
  conectando: 'voz.chamada.barra.conectando',
  falando: 'voz.chamada.barra.falando',
  falha: 'voz.chamada.barra.falha',
  mudo: 'voz.chamada.barra.mudo',
  pensando: 'voz.chamada.barra.pensando',
  ouvindo: 'voz.chamada.barra.ouvindo',
} as const // Fatia 6b (D454): chaves do catálogo voz/chamada; a barra traduz ao desenhar.
export type EstadoDaBarra = keyof typeof ESTADO_CHAMADA

export function estadoDaBarra(c: { encerrada: boolean; fase: string | null; mudo: boolean; trabalhando: boolean }): EstadoDaBarra {
  if (c.encerrada) return 'encerrada'
  if (c.fase === null || c.fase === 'starting') return 'conectando'
  if (c.fase === 'speaking') return 'falando'
  if (c.fase === 'failed') return 'falha'
  if (c.mudo) return 'mudo'
  return c.trabalhando ? 'pensando' : 'ouvindo'
}

/** O tempo desde o início da chamada (não do motor): "00:42", "12:05". A chamada tem teto de 30 minutos. */
export function tempoDaChamada(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000))
  return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`
}

/**
 * O que a barra mostra: até o fim efetivo da chamada (limite, falha sem reserva, fim pela voz), o tempo para no
 * instante em que `finish` encerrou, o mesmo que vai para o registro da chamada; antes disso, até agora.
 */
export function tempoMostrado(inicio: number, agora: number, fim: number | null): string {
  return tempoDaChamada((fim ?? agora) - inicio)
}
