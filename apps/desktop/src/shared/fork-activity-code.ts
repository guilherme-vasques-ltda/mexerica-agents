// MX-147: o Hub mantém `activity` (texto legado, em inglês, português ou livre) e pode mandar `activityCode`, um
// objeto com `kind` estável. O app traduz os códigos que conhece e cai em `activity` para código ausente,
// desconhecido ou malformado. Puro e sem DOM: o catálogo fica em renderer/lib/fork-atividade.ts.
// Contrato alinhado com o Hub (orquestrador, 11/11): objeto plano `{ kind, ...parâmetros, member? }`.

/**
 * Estados sem parâmetro. `subscription_wait` também não traz horário: o app usa o `resumeAt` do próprio bot.
 * Um kind novo entra aqui e no mapa de renderer/lib/fork-atividade.ts.
 */
export const FIXED_ACTIVITY_KINDS = [
  'writing_reply', 'thinking', 'working', 'starting_agent', 'resuming_session', 'continuing', 'typing',
  'starting', 'summarizing', 'resuming_work', 'waiting_admission', 'waiting_session', 'subscription_wait',
  'subscription_paused', 'setup_failed', 'routine_setup_failed', 'routine_interrupted', 'start_failed',
] as const
export type FixedActivityKind = (typeof FIXED_ACTIVITY_KINDS)[number]

/** Kinds com um parâmetro de texto obrigatório. `tool` mantém o título livre da ferramenta, sem traduzir. */
export const TEXT_ACTIVITY_KINDS = { needs_approval: 'title', downloading: 'item', installing: 'item', tool: 'title' } as const
export type TextActivityKind = keyof typeof TEXT_ACTIVITY_KINDS

export type ActivityCode = (
  | { kind: FixedActivityKind }
  | { kind: 'needs_approval'; title: string }
  | { kind: 'tool'; title: string }
  | { kind: 'downloading'; item: string }
  | { kind: 'installing'; item: string }
) & {
  /** Num grupo, o nome de quem trabalha: o texto legado vem como "Nome: atividade". */
  member?: string
}

const FIXED = new Set<string>(FIXED_ACTIVITY_KINDS)

/** Parâmetro de texto obrigatório: string não vazia (só espaços não conta). */
const param = (v: unknown): string | null => (typeof v === 'string' && v.trim() ? v : null)

/**
 * O código como o app o entende, ou `null`. Campo extra é ignorado (Hub mais novo); kind desconhecido, objeto
 * malformado ou parâmetro ausente/vazio/não texto dão `null`, e quem mostra usa `activity` (que já traz o nome do
 * membro). Um `member` presente mas inválido também invalida o código, para o nome nunca se perder.
 */
export function parseActivityCode(raw: unknown): ActivityCode | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const o = raw as Record<string, unknown>
  const kind = o.kind
  if (typeof kind !== 'string') return null
  let member: string | undefined
  if (o.member !== undefined && o.member !== null) {
    const m = param(o.member)
    if (!m) return null
    member = m
  }
  const withMember = <T extends object>(code: T) => (member ? { ...code, member } : code)
  if (FIXED.has(kind)) return withMember({ kind: kind as FixedActivityKind })
  if (Object.hasOwn(TEXT_ACTIVITY_KINDS, kind)) {
    const name = TEXT_ACTIVITY_KINDS[kind as TextActivityKind]
    const value = param(o[name])
    return value ? withMember({ kind, [name]: value } as ActivityCode) : null
  }
  return null
}
