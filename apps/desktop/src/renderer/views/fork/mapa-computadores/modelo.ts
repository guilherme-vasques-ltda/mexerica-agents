// Modelo puro da demonstração do mapa de computadores: só projeta dados já recebidos. Os quatro fatos são
// independentes; nenhum deriva do outro.

export type Rota = 'configurada' | 'nao_configurada' | 'desconhecida'
export type Tunel = 'em_uso' | 'parado' | 'desconhecido'
export type Identidade = 'verificada' | 'negada' | 'desconhecida'
export type Autorizacao = 'confirmada' | 'negada' | 'desconhecida'

export interface Fatos {
  rota: Rota
  tunel: Tunel
  identidade: Identidade
  autorizacao: Autorizacao
}

export type ChaveNome = 'mapaComputadores.nome.esta' | 'mapaComputadores.nome.atlas' | 'mapaComputadores.nome.boreal' | 'mapaComputadores.nome.cedro' | 'mapaComputadores.nome.dunas' | 'mapaComputadores.nome.fjord'

export interface Computador {
  id: string
  nomeChave: ChaveNome
  papel: 'esta' | 'demo'
  posicao: { x: number; y: number }
  fatos: Fatos
  /** Amostra crua, como chegaria de fora: pode vir inválida, velha ou ausente. Só `projetarAmostra` a lê. */
  amostra: unknown
}

export type Filtro = 'todos' | 'tunel_em_uso' | 'sem_autorizacao'

export const PRAZO_AMOSTRA_MS = 5 * 60_000
const LIMIAR_DISCO_PCT = 90

export type ProjecaoAmostra =
  | { estado: 'fresca'; cpu: number; memoria: number; disco: number; alertaDisco: boolean }
  | { estado: 'obsoleta' | 'ausente' | 'invalida' | 'futura' }

/** Amostra só vale com números válidos, horário não futuro e dentro do prazo. Relógio inválido rejeita tudo. */
export function projetarAmostra(bruta: unknown, agora: number): ProjecaoAmostra {
  if (!Number.isFinite(agora)) return { estado: 'invalida' }
  if (bruta === undefined || bruta === null) return { estado: 'ausente' }
  if (typeof bruta !== 'object') return { estado: 'invalida' }
  const { cpu, memoria, disco, coletadaEm } = bruta as Record<string, unknown>
  const numeros = [cpu, memoria, disco].every((v) => typeof v === 'number' && Number.isFinite(v) && v >= 0 && v <= 100)
  const instante = typeof coletadaEm === 'string' ? Date.parse(coletadaEm) : NaN
  if (!numeros || !Number.isFinite(instante)) return { estado: 'invalida' }
  if (instante > agora) return { estado: 'futura' }
  if (agora - instante > PRAZO_AMOSTRA_MS) return { estado: 'obsoleta' }
  return { estado: 'fresca', cpu: cpu as number, memoria: memoria as number, disco: disco as number, alertaDisco: (disco as number) >= LIMIAR_DISCO_PCT }
}

export function filtrar(computadores: readonly Computador[], filtro: Filtro): Computador[] {
  if (filtro === 'tunel_em_uso') return computadores.filter((c) => c.fatos.tunel === 'em_uso')
  if (filtro === 'sem_autorizacao') return computadores.filter((c) => c.fatos.autorizacao !== 'confirmada')
  return [...computadores]
}

export type Tom = 'ok' | 'atencao' | 'neutro'

/** Cada fato sai com seu próprio estado e tom; a cor nunca é o único sinal. */
export function projetarFatos(fatos: Fatos): { fato: keyof Fatos; estado: string; tom: Tom }[] {
  return [
    { fato: 'rota', estado: fatos.rota, tom: fatos.rota === 'configurada' ? 'ok' : 'neutro' },
    { fato: 'tunel', estado: fatos.tunel, tom: fatos.tunel === 'em_uso' ? 'ok' : 'neutro' },
    { fato: 'identidade', estado: fatos.identidade, tom: fatos.identidade === 'verificada' ? 'ok' : fatos.identidade === 'negada' ? 'atencao' : 'neutro' },
    { fato: 'autorizacao', estado: fatos.autorizacao, tom: fatos.autorizacao === 'confirmada' ? 'ok' : fatos.autorizacao === 'negada' ? 'atencao' : 'neutro' },
  ]
}
