// Uma autorização só: o cartão é o único lugar de decidir; o balão do bot só conta o que a pessoa escolheu.
import type { Entry } from '@shared/models'

/** What the person chose on the latest approval card of the running turn. */
export type ApprovalDecision = 'permitiu' | 'recusou' | null
/** The part of a chat item this reads (ChatItem fits). */
type DecisionItem = { kind: string; entry?: Entry }

/**
 * What the person chose on the newest approval card of the current turn (after the last message
 * of theirs), for the bot's live bubble. Pending, cancelled or expired cards, and turns without a card, say nothing.
 */
export function lastDecision(items: readonly DecisionItem[]): ApprovalDecision {
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i]
    const e = item.entry
    if (item.kind !== 'entry' || !e) continue
    if (e.kind === 'user') return null
    if (e.kind !== 'permission') continue
    if (e.data.status !== 'answered') return null
    const allow = !!e.data.options?.find((o) => o.optionId === e.data.selected)?.kind.startsWith('allow')
    // A tela: "Permitir" sem efeito no Hub não liberou nada (o cartão diz "Sem liberação"); o balão não afirma nada.
    if (allow && e.data.toolKind === 'screen_access' && e.data.screenAccess?.effect !== true) return null
    return allow ? 'permitiu' : 'recusou'
  }
  return null
}

/** O status curto do balão: com a aprovação aberta e o cartão na tela, "esperando"; senão, o que a pessoa escolheu. */
export type StatusDoBalao = 'esperando' | Exclude<ApprovalDecision, null> | null
export function statusDoBalao(aprovacaoAberta: boolean, cartaoNaTela: boolean, decisao: ApprovalDecision): StatusDoBalao {
  if (aprovacaoAberta) return cartaoNaTela ? 'esperando' : null
  return decisao
}
