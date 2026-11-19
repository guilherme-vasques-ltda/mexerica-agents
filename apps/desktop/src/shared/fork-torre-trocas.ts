// MEX-1391 (F6 fatia B, regra (o).2): a seção "Trocas de conta" do Mission Board — um cartão por troca de conta do
// fallback (origem, destino, motivo, hora). Lê `accountSwitches` do snapshot (`host/src/fork/fallback_contas/torre.rs`).
// Só rótulos seguros das contas; nenhum texto de conversa. Item desconhecido é ignorado, nunca vira outro motivo.

export type MotivoTroca = 'perto_do_limite' | 'esgotada' | 'voltou'

export interface TrocaDeConta {
  id: string
  at: number
  botId: string
  fromLabel: string
  toLabel: string
  reason: MotivoTroca
  /** Quando a conta de origem volta, se o Hub sabe e ainda é futuro. */
  backAt: number | null
}

export interface TrocasDeConta { items: TrocaDeConta[]; omitted: number }

const MOTIVOS = new Set<MotivoTroca>(['perto_do_limite', 'esgotada', 'voltou'])
const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown) => (typeof v === 'string' && v ? v : null)
const instante = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : null)

function item(v: unknown): TrocaDeConta | null {
  if (!isObj(v)) return null
  const id = str(v.id)
  const botId = str(v.botId)
  const fromLabel = str(v.fromLabel)
  const toLabel = str(v.toLabel)
  const at = instante(v.at)
  if (!id || !botId || !fromLabel || !toLabel || at === null) return null
  if (typeof v.reason !== 'string' || !MOTIVOS.has(v.reason as MotivoTroca)) return null
  return { id, at, botId, fromLabel, toLabel, reason: v.reason as MotivoTroca, backAt: instante(v.backAt) }
}

/** Lista inválida vira indisponível; item desconhecido é ignorado. */
export function parseTrocasDeConta(v: unknown): TrocasDeConta | null {
  if (!isObj(v) || !Array.isArray(v.items)) return null
  const omitted = v.omitted
  if (typeof omitted !== 'number' || !Number.isInteger(omitted) || omitted < 0) return null
  return { items: v.items.map(item).filter((i): i is TrocaDeConta => i !== null), omitted }
}
