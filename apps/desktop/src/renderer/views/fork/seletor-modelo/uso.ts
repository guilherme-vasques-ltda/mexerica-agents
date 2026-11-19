// DON-907 C9 (D375): quantos tokens a troca releria no mesmo provedor: o `used` do medidor de contexto (C2) desta
// conversa e sessão, nesta conexão. Sem medida (medidor oculto, Hub sem `donnaContext`, outra conexão): null, e a
// troca vai direto, sem pergunta.

/** O que se lê do medidor: a leitura atual de uma conversa (`sessionId` null = a principal). */
export interface Medidor { of(botId: string, sessionId: string | null): { used: number } | null }

export function usoDaConversa(medidor: Medidor, botId: string, sessionId: string | null): number | null {
  const used = medidor.of(botId, sessionId)?.used
  return typeof used === 'number' && Number.isFinite(used) ? used : null
}
