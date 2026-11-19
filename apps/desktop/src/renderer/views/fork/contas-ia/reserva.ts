// MEX-1391/F6-B: regras puras da ordem de reserva ("Se esta conta acabar"), testáveis no Node. O Hub decide e confere tudo
// de novo (assinatura comprovada, mesmo motor, até 3 reservas); aqui só se monta a lista que a pessoa vê e edita.
import type { AccountId, Engine, FallbackRest, FallbackState, ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'

/** Até 3 reservas depois da conta do bot (o Hub recusa mais que isso). */
export const MAX_RESERVAS = 3

/** A ordem de reserva do Space para o motor; sem lista, vazia (nada troca sozinho). */
export function ordemDoMotor(state: FallbackState | null, backend: Engine): AccountId[] {
  return state?.orders.find((o) => o.backend === backend)?.accounts ?? []
}

/**
 * Só assinatura comprovada e conectada do mesmo motor entra como reserva; chave de API nunca aparece aqui. A conta também
 * precisa de vínculo com o Space aberto (um bot dele a usa): `noSpace` diz isso, e o Hub confere de novo ao salvar.
 */
export function candidatas(contas: readonly ManagedAccount[], backend: Engine, ordem: readonly AccountId[], propriaId: AccountId, noSpace: (conta: ManagedAccount) => boolean = () => true): ManagedAccount[] {
  return contas.filter((c) => c.backend === backend && c.billing === 'subscription' && c.state === 'connected' && c.id !== propriaId && !ordem.includes(c.id) && noSpace(c))
}

export function mover(ordem: readonly AccountId[], id: AccountId, delta: -1 | 1): AccountId[] {
  const i = ordem.indexOf(id)
  const j = i + delta
  if (i < 0 || j < 0 || j >= ordem.length) return [...ordem]
  const nova = [...ordem]
  nova.splice(i, 1)
  nova.splice(j, 0, id)
  return nova
}

export const tirar = (ordem: readonly AccountId[], id: AccountId): AccountId[] => ordem.filter((a) => a !== id)

export const acrescentar = (ordem: readonly AccountId[], id: AccountId): AccountId[] =>
  ordem.includes(id) || ordem.length >= MAX_RESERVAS ? [...ordem] : [...ordem, id]

/** O descanso, o bloqueio de 1 h ou o uso extra desta conta, como o Hub informou agora. */
export function situacaoDaConta(state: FallbackState | null, id: AccountId, now = Date.now()): FallbackRest | null {
  const r = state?.resting.find((x) => x.engineAccountId === id)
  if (!r) return null
  const vivo = (at: number | null) => at !== null && at > now
  return vivo(r.restUntil) || vivo(r.extraUntil) || vivo(r.blockedUntil) ? r : null
}

/** "Na Claude 2 até a Claude 1 voltar": as conversas que hoje rodam numa reserva por causa desta conta. */
export function emReservaPor(state: FallbackState | null, origemId: AccountId) {
  return state?.sessions.filter((s) => s.originAccountId === origemId) ?? []
}
