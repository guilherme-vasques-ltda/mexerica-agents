// `donnaUsageHistory` (só leitura e só do dono), sem DOM nem React: o histórico das COTAS do provedor (percentual
// usado por janela), nunca tokens nem dinheiro. A UI não soma, não interpola e não converte: bucket ausente é lacuna,
// nunca 0; `accountId` ausente (Hub antigo) ou null é conta não identificada, nunca inferida. Nada vai para o mirror.
import { INVALIDA } from './modelo.ts'

export const RPC_HISTORICO = 'donnaUsageHistory'
export const FAIXAS_HISTORICO = ['24h', '7d', '30d'] as const
export type FaixaHistorico = (typeof FAIXAS_HISTORICO)[number]

export interface BlocoCota { start: number; percent: number }
/** `null`: o Hub não soube prever (poucas amostras ou recorte); `exhaustsAt` null: sem previsão de esgotar. */
export interface PrevisaoCota { exhausted: boolean; exhaustsAt: number | null }
export interface JanelaHistorico {
  provider: string
  accountId: string | null
  windowId: string
  resetsAt: number | null
  buckets: BlocoCota[]
  forecast: PrevisaoCota | null
}
export interface HistoricoCotas {
  range: FaixaHistorico
  bucketMs: number
  measuredAt: number
  truncated: boolean
  windows: JanelaHistorico[]
}

const invalida = (): never => { throw new Error(INVALIDA) }
const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : invalida())
const inteiro = (v: unknown): number => (typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : invalida())
const talvez = (v: unknown): number | null => (v === null ? null : inteiro(v))
const id = (v: unknown): string => (typeof v === 'string' && v ? v : invalida())

export function lerHistorico(raw: unknown, faixa: FaixaHistorico): HistoricoCotas {
  const o = obj(raw)
  if (o.range !== faixa || typeof o.truncated !== 'boolean' || !Array.isArray(o.windows)) invalida()
  const bucketMs = inteiro(o.bucketMs)
  if (bucketMs === 0) invalida()
  return {
    range: faixa,
    bucketMs,
    measuredAt: inteiro(o.measuredAt),
    truncated: o.truncated as boolean,
    windows: (o.windows as unknown[]).map((w) => {
      const j = obj(w)
      if (!Array.isArray(j.buckets)) invalida()
      const previsao = j.forecast === null ? null : obj(j.forecast)
      if (previsao && typeof previsao.exhausted !== 'boolean') invalida()
      let anterior = -1
      return {
        provider: id(j.provider),
        accountId: j.accountId === undefined || j.accountId === null ? null : id(j.accountId),
        windowId: id(j.windowId),
        resetsAt: talvez(j.resetsAt ?? null),
        buckets: (j.buckets as unknown[]).map((b) => {
          const x = obj(b)
          const percent = x.percent
          const start = inteiro(x.start)
          if (typeof percent !== 'number' || !Number.isFinite(percent) || percent < 0) invalida()
          if (start <= anterior) invalida()
          anterior = start
          return { start, percent: percent as number }
        }),
        forecast: previsao ? { exhausted: previsao.exhausted as boolean, exhaustsAt: talvez(previsao.exhaustsAt ?? null) } : null,
      }
    }),
  }
}

/** Agrupa por conta sem somar nem misturar: a ordem do Hub dentro de cada conta e a primeira aparição entre as contas. */
export function porConta(janelas: readonly JanelaHistorico[]): { accountId: string | null; janelas: JanelaHistorico[] }[] {
  const grupos = new Map<string | null, JanelaHistorico[]>()
  for (const j of janelas) grupos.set(j.accountId, [...(grupos.get(j.accountId) ?? []), j])
  return [...grupos].map(([accountId, lista]) => ({ accountId, janelas: lista }))
}
