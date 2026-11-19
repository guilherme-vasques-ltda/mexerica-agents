/** O que o processo principal sabe da conta (a projeção de `account.state`). */
export interface AccountPresence {
  ready: boolean
  busy: boolean
  user: unknown
}

/** Conta de verdade: pronta, sem ação pendente e com usuário. O pareamento e a nuvem exigem isto sempre. */
export function accountConfirmed(account: AccountPresence): boolean {
  return account.ready && !account.busy && account.user !== null
}

/**
 * Ilha e resumos da bandeja (dono, 05/11/2026, opção B, extensão do PR #278): a versão gerada sem
 * sistema de login (`cloudAccess` desligado) os mostra direto; com login, só com a conta confirmada.
 */
export function canPresentSurfaces(account: AccountPresence, controls: boolean): boolean {
  return !controls || accountConfirmed(account)
}
