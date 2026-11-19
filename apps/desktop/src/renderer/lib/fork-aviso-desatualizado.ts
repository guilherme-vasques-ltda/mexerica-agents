// Aviso de computador desatualizado: o que o cartão mostra em cada fase, sem React nem store (testável no Node).
// O ponto de integração com o estado é `avisoDoStore`, que lê `store.desatualizado`: a mesma evidência de
// mismatch que o VersionGate produz (versão ilegível do computador já chega como `encontrada: null`).

/** Quem precisa atualizar: o computador (o app é novo demais para ele) ou este app (o computador é novo demais). */
export type AlvoAviso = 'computador' | 'app'

export interface AvisoDesatualizado {
  alvo: AlvoAviso
  /** A versão que o computador roda, quando se sabe. */
  encontrada: string | null
  /** A versão mínima que falta alcançar. */
  minima: string
  /** Uma nova checagem está em andamento ("Verificando de novo…"). */
  verificando: boolean
}

/** `oculto`: nada; `desatualizado`: o aviso; `voltou`: a confirmação curta de que tudo voltou ao normal. */
export type FaseAviso = 'oculto' | 'desatualizado' | 'voltou'

/** Quanto tempo a confirmação "voltou ao normal" fica antes de sumir sozinha. */
export const VOLTOU_MS = 4000

/** O que o store entrega: só o getter `desatualizado`, para o ponto de integração não depender do resto. */
export interface FonteDoAviso {
  desatualizado: { alvo: 'app' | 'hub'; encontrada: string | null; minimo: string } | null
}

export function avisoDoStore(fonte: FonteDoAviso, verificando = false): AvisoDesatualizado | null {
  const d = fonte.desatualizado
  if (!d) return null
  return { alvo: d.alvo === 'app' ? 'app' : 'computador', encontrada: d.encontrada, minima: d.minimo, verificando }
}

/**
 * A fase seguinte. Só vira `voltou` quem estava mostrando o aviso, o computador segue conectado e o aviso acabou:
 * uma queda de conexão (sem aviso e sem conexão) some em silêncio, sem anunciar uma atualização que não houve.
 */
export function proximaFase(anterior: FaseAviso, aviso: AvisoDesatualizado | null, conectado: boolean): FaseAviso {
  if (aviso) return 'desatualizado'
  if (anterior === 'desatualizado') return conectado ? 'voltou' : 'oculto'
  return anterior
}
