import type { SessionState } from './estado'
import './ldot.css'

/** Ponto de estado de 7 px (8 px no Agora), igual nas abas, na árvore do painel e no Agora (passo 16). */
export function LDot({ state, big = false }: { state: SessionState; big?: boolean }) {
  return <span className={`ldot s-${state}${big ? ' big' : ''}`} aria-hidden />
}
