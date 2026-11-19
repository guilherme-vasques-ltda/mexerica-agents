import type { BotRef } from '../../../store/app-model'

/**
 * O clique no cartão da principal (DonnaCard). Sempre passa por `select`, inclusive com o Berga já aberto: assim a
 * escolha conta como da pessoa (`AppModel.selectionSeq`) e a abertura automática nunca a troca depois. A conversa
 * aberta continua a mesma (mesma chave, sem remontar).
 */
export function openPrincipal(app: { select(ref: BotRef): void }, ref: BotRef) {
  app.select(ref)
}
