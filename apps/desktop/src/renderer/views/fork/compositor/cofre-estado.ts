// MX-277: em que ponto está o "Mover para o cofre" do aviso de senha colada. Puro e sem valor: só o estado da janela
// segura (aberta, guardado, falhou). O trecho escondido nunca passa por aqui.
import { ENTRY_LOGIN_NOVO, type AvisoEstado, type MotivoRecusa, type RespostaAbrir } from '../../../../shared/fork-cofre-seguro.ts'

export type FaseCofre = 'abrindo' | 'aberta' | 'guardado' | MotivoRecusa

/** A resposta do main ao pedido de abrir a janela. */
export const faseAoResponder = (resposta: RespostaAbrir): FaseCofre => (resposta.ok ? 'aberta' : resposta.motivo)

/**
 * O aviso do main sobre a janela do login novo (outros pedidos são ignorados). Guardado é definitivo para este cartão;
 * fechar a janela sem guardar tira o cartão.
 */
export function faseAoAviso(atual: FaseCofre | null, aviso: AvisoEstado): FaseCofre | null {
  if (aviso.entryId !== ENTRY_LOGIN_NOVO) return atual
  if (aviso.estado === 'salva') return 'guardado'
  if (aviso.estado === 'aberta') return atual === 'guardado' ? atual : 'aberta'
  return atual === 'guardado' ? atual : null
}
