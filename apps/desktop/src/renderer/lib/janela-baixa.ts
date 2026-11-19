import { useSyncExternalStore } from 'react'

/**
 * Auditoria D249, E4: abaixo de 600 px de altura (a janela mínima é 760×500), a conversa, Times e
 * Rotinas se compactam. A mesma consulta vale no CSS (`@media (height < 600px)`); de 600 px para
 * cima nada muda.
 */
export const JANELA_BAIXA = '(height < 600px)'

const assinar = (aviso: () => void) => {
  const consulta = window.matchMedia(JANELA_BAIXA)
  consulta.addEventListener('change', aviso)
  return () => consulta.removeEventListener('change', aviso)
}

/** Verdadeiro enquanto a janela estiver abaixo de 600 px de altura. */
export function useJanelaBaixa() {
  return useSyncExternalStore(assinar, () => window.matchMedia(JANELA_BAIXA).matches, () => false)
}
