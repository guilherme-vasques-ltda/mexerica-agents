// Trabalho em andamento na página (#753, decisão do líder e adendo do coordenador). Uma nova versão não recarrega a aba
// sozinha enquanto houver:
// - `rascunho`: texto escrito numa caixa (fica no disco, mas recarregar no meio da escrita atrapalha);
// - `soEmMemoria`: anexo pendente, upload em curso ou envio sem confirmação. Fica só em memória de propósito (sem
//   disco, menos superfície de segurança) e se perde ao recarregar.
// Quem guarda esse estado registra uma consulta aqui; hoje nenhuma recarga automática consulta (a de nova versão da web foi removida, MX-417); fica para quando houver. Só leitura.

export interface Pendencia {
  readonly rascunho: boolean
  readonly soEmMemoria: boolean
}

export type ConsultaPendencia = () => Partial<Pendencia>

const consultas = new Set<ConsultaPendencia>()

/** Registra uma consulta; devolve o cancelamento (chamar ao aposentar o dono do estado). */
export function registrarEstadoEmMemoria(consulta: ConsultaPendencia): () => void {
  consultas.add(consulta)
  return () => { consultas.delete(consulta) }
}

/** A função única: o que há em andamento na página. Uma consulta que falha conta como tudo (na dúvida, não recarregar). */
export function pendenciaAtual(): Pendencia {
  let rascunho = false
  let soEmMemoria = false
  for (const consulta of consultas) {
    try {
      const p = consulta()
      rascunho ||= !!p.rascunho
      soEmMemoria ||= !!p.soEmMemoria
    } catch {
      return { rascunho: true, soEmMemoria: true }
    }
  }
  return { rascunho, soEmMemoria }
}

/** Há algo que se perde ao recarregar (anexo, upload em curso, envio sem confirmação)? */
export function haEstadoSoEmMemoria(): boolean {
  return pendenciaAtual().soEmMemoria
}

/** Há qualquer trabalho em andamento (rascunho ou estado só em memória)? Enquanto houver, nada recarrega sozinho. */
export function haTrabalhoEmAndamento(): boolean {
  const p = pendenciaAtual()
  return p.rascunho || p.soEmMemoria
}
