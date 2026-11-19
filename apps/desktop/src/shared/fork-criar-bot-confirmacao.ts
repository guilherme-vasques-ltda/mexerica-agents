/** D543: confirmar uma referência concreta, nunca um padrão que possa mudar depois. */
export interface ContextoConta {
  hubId: string | null | undefined
  epoch: number
  backend: string
  accountId: string | null
  spaceId: string | null
}
export interface ConfirmacaoConta extends ContextoConta { hubId: string; accountId: string; spaceId: string }

export function confirmarConta(atual: ContextoConta): ConfirmacaoConta | null {
  return atual.hubId && atual.accountId && atual.spaceId ? { ...atual, hubId: atual.hubId, accountId: atual.accountId, spaceId: atual.spaceId } : null
}
export function contaConfirmadaPara(confirmada: ConfirmacaoConta | null, atual: ContextoConta, enviada: string | null): boolean {
  return !!confirmada && !!enviada && confirmada.accountId === enviada && enviada === atual.accountId
    && confirmada.hubId === atual.hubId && confirmada.epoch === atual.epoch
    && confirmada.backend === atual.backend && confirmada.spaceId === atual.spaceId
}
export function reconciliarConfirmacao(confirmada: ConfirmacaoConta | null, atual: ContextoConta): ConfirmacaoConta | null {
  return contaConfirmadaPara(confirmada, atual, atual.accountId) ? confirmada : null
}
