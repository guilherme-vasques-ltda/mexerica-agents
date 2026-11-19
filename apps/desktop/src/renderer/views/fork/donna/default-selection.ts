// Abertura no Berga (dono, 03/11/2026; docs/features/janela-do-mac.md#ao-abrir): o que o lado da conversa abre
// quando não há conversa válida. Regras puras, sem React nem store; use-default-selection.ts as aplica.

export interface Ref { computerId: string; botId: string; spaceId?: string }
export interface Row { ref: Ref; group: boolean }

export interface DefaultHub {
  computerId: string
  /** `principal.protectedId`: a identidade confirmada ou, antes da consulta terminar, a guardada deste Hub. */
  spaceId?: string
  principalId: string | null
  phase: 'unknown' | 'loading' | 'ready' | 'error'
  /** Online ou conectando: a consulta da principal ainda pode chegar. */
  reachable: boolean
}

export interface DefaultInput {
  selection: Ref | null
  /** O que a barra e a paleta listam: bots visíveis dos computadores mostrados, mais a principal oculta antiga. */
  items: readonly Row[]
  /** As linhas como a barra desenha, em ordem (sem a principal). */
  rows: readonly Row[]
  /** O Hub do trilho (railStore), senão o primeiro computador online da barra. */
  hub: DefaultHub | null
  connecting: boolean
  /**
   * A lista vazia vale como ausência de bots: `emptyWait(...) === 0` (5 s online sem bot visível em todos os
   * computadores mostrados, ou zero explícito no `hello`). Antes disso, o painel fica neutro.
   */
  settled: boolean
  /** A seleção atual veio da pessoa (clique, notificação, Buscar), mesmo que repita a referência automática. */
  manual: boolean
  /**
   * A pessoa abriu a seleção fora da lista (bandeja ou ilha com o computador escondido no filtro) e o bot continua
   * existindo e visível no seu Hub. Um bot que sai da lista depois de aberto não conta.
   */
  pinned: boolean
}

export type DefaultSelection =
  | { kind: 'keep' }
  | { kind: 'select'; ref: Ref }
  /** Painel neutro: a principal ainda está sendo consultada, ou a lista vazia ainda não foi confirmada. */
  | { kind: 'wait' }
  /** Nenhum bot à vista enquanto conecta: "Conectando…", sem botão. */
  | { kind: 'connecting' }
  /** Ausência de bots confirmada: a tela de boas-vindas com Novo bot. */
  | { kind: 'empty' }

/** O que `emptyWait` lê de um computador (BotStore). */
export interface WatchedStore {
  connection: { kind: string }
  mismatch: unknown
  emptyWatch: { remaining(now: number): number | null }
}

/**
 * Quanto falta para a lista vazia valer como ausência de bots em todos os computadores mostrados
 * (store/fork-empty-roster.ts): 5 s online sem bot visível, ou zero explícito no `hello`. null enquanto algum não está
 * online (ou tem versão incompatível): aí a lista vazia não confirma nada.
 */
export function emptyWait(stores: readonly WatchedStore[], now: number): number | null {
  if (!stores.length) return null
  let wait = 0
  for (const store of stores) {
    const left = store.connection.kind === 'online' && !store.mismatch ? store.emptyWatch.remaining(now) : null
    if (left === null) return null
    wait = Math.max(wait, left)
  }
  return wait
}

const same = (a: Ref, b: Ref) => a.computerId === b.computerId && a.botId === b.botId && (a.spaceId ?? null) === (b.spaceId ?? null)

export function defaultSelection({ selection, items, rows, hub, connecting, settled, manual, pinned }: DefaultInput): DefaultSelection {
  const listed = (ref: Ref) => items.some((item) => same(item.ref, ref))
  const donna = hub?.principalId ? { computerId: hub.computerId, botId: hub.principalId, ...(hub.spaceId ? { spaceId: hub.spaceId } : {}) } : null
  const donnaListed = donna && listed(donna) ? donna : null
  if (selection && pinned) return { kind: 'keep' }
  if (selection && listed(selection)) {
    // A consulta confirmou outra principal no mesmo Hub enquanto a seleção ainda era a automática: troca (decisão 2).
    // Outro Hub no trilho (o da seleção caiu e outro está online) não é principal alterada: a conversa em cache fica.
    const stale = !manual && !!donnaListed && hub?.phase === 'ready' && selection.computerId === hub.computerId && !same(selection, donnaListed)
    return stale ? { kind: 'select', ref: donnaListed! } : { kind: 'keep' }
  }
  if (donnaListed) return { kind: 'select', ref: donnaListed }
  if (!items.length) return connecting ? { kind: 'connecting' } : settled ? { kind: 'empty' } : { kind: 'wait' }
  if (hub?.reachable && (hub.phase === 'unknown' || hub.phase === 'loading')) return { kind: 'wait' }
  // Sem principal: a primeira conversa direta com bot como a barra desenha; grupo só se não houver bot.
  const first = rows.find((row) => !row.group) ?? items.find((row) => !row.group) ?? rows[0] ?? items[0]!
  return { kind: 'select', ref: first.ref }
}

/** O que o gancho lembra da seleção entre renders. */
export interface SelectionTrack {
  /** `AppModel.selectionSeq` visto por último. */
  seq: number
  key: string | null
  manual: boolean
  /** Aberta pela pessoa fora da lista e ainda não vista nela. */
  outside: boolean
}

/**
 * O que vale na primeira montagem da barra: uma seleção que já existe e não veio da escolha automática (por exemplo,
 * `openBot` de uma notificação enquanto o acesso ao computador estava na tela) é da pessoa, e fica fixada se estiver
 * fora da lista (computador escondido no filtro).
 */
export function initialTrack(now: { seq: number; key: string | null; listed: boolean; autoSeq: number }): SelectionTrack {
  const manual = now.key !== null && now.seq !== now.autoSeq
  return { seq: now.seq, key: now.key, manual, outside: manual && !now.listed }
}

/**
 * Cada `select` conta (`seq`), inclusive o da mesma conversa: só o da escolha automática (`autoSeq`) deixa de ser
 * manual. Uma seleção que muda sem `select` (computador removido) nunca é da pessoa. Vista na lista, deixa de ser
 * "fora da lista": se sair dela depois, volta para a Donna.
 */
export function trackSelection(prev: SelectionTrack, now: { seq: number; key: string | null; listed: boolean; autoSeq: number }): SelectionTrack {
  if (now.seq !== prev.seq) {
    const manual = now.seq !== now.autoSeq && now.key !== null
    return { seq: now.seq, key: now.key, manual, outside: manual && !now.listed }
  }
  if (now.key !== prev.key) return { seq: now.seq, key: now.key, manual: false, outside: false }
  return now.listed && prev.outside ? { ...prev, outside: false } : prev
}
