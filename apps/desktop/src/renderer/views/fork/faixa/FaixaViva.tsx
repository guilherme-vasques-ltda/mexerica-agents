import type { ReactNode } from 'react'
import { SubscriptionPaused } from '../../../lib/observable'

/**
 * A faixa da esquerda fica viva com as telas que cobrem a janela (Mission Board, Rotinas, Times, Ajustes, Extensões).
 * Essas camadas pausam as assinaturas da árvore coberta (MX-4, C11), e a faixa mora nessa árvore: sem isto, o item
 * ativo congelava no lugar aberto primeiro (a torre ficava marcada depois de ir para Conectores ou Rotinas).
 */
export function FaixaViva({ children }: { children: ReactNode }) {
  return <SubscriptionPaused.Provider value={false}>{children}</SubscriptionPaused.Provider>
}
