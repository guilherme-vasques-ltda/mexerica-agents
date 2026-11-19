// Movimento do cartão de abas (Barra05; revisão A352), puro para testar sem DOM.

/** O que cada filho do cartão é, para a entrada em sequência. */
export type ItemKind = 'tab' | 'sep' | 'tail'

/**
 * A352-2: atraso de entrada de cada item, 26 ms entre ABAS (não entre filhos). O traço entra junto da aba que vem
 * depois dele; "···" e "+" entram como a próxima aba. A aba do bot, o botão ‹‹/›› e o rascunho ficam fora da lista.
 */
export function staggerDelays(kinds: readonly ItemKind[], step = 26): number[] {
  let tabs = 0
  return kinds.map((kind) => (kind === 'tab' ? tabs++ : tabs) * step)
}

/** Quando termina a entrada inteira: o atraso da última mais a duração (não os 320 ms da largura). */
export const staggerEnd = (delays: readonly number[], duration: number) => (delays.length ? Math.max(...delays) + duration : 0)

/**
 * A352-3: redirecionar no meio (recolher enquanto mostra, ou o contrário) parte do ponto atual; a duração encolhe
 * com a distância que falta, para o caminho de volta não ficar lento nem seco.
 */
export function redirectMs(current: number, target: number, span: number, full: number, min = 120) {
  if (!(span > 0)) return full
  return Math.round(Math.max(min, Math.min(full, (full * Math.abs(target - current)) / span)))
}

/** A352-3: cada clique vira a intenção, mesmo no meio de uma animação; vale a última. */
export function clickIntent(intent: boolean) {
  return !intent
}
