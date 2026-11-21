import { STALE_AFTER_MS, type LiveClosedReason, type LiveState } from '../../../../shared/fork-browser-live.ts'
import type { ChaveTexto } from '../../../lib/fork-i18n/index'

/** O que a faixa de controle diz. Só decide o estado; os textos vêm do catálogo. */
export type FaixaAoVivo =
  | { kind: 'abrindo' }
  | { kind: 'bot' }
  | { kind: 'assumindo' }
  | { kind: 'pessoa' }
  | { kind: 'fechada'; reason: LiveClosedReason }

export function faixaDoEstado(state: LiveState): FaixaAoVivo {
  if (state.status === 'closed') return { kind: 'fechada', reason: state.reason ?? 'closed' }
  if (state.status === 'connecting') return { kind: 'abrindo' }
  if (state.switching) return { kind: 'assumindo' }
  return { kind: state.controller === 'person' ? 'pessoa' : 'bot' }
}

/** Segundos de atraso a mostrar; `null` quando o quadro é recente ou ainda não há quadro. */
export function atrasoDoQuadro(idadeMs: number | null): number | null {
  if (idadeMs === null || idadeMs <= STALE_AFTER_MS) return null
  return Math.floor(idadeMs / 1000)
}

/** A caixa do quadro só leva clique com a pessoa no controle e o quadro em dia. */
export function aceitaClique(faixa: FaixaAoVivo, atraso: number | null, temQuadro: boolean): boolean {
  return faixa.kind === 'pessoa' && atraso === null && temQuadro
}

const MOTIVOS = {
  unsupported: 'ajustes.navegadorAoVivo.fechada.unsupported',
  denied: 'ajustes.navegadorAoVivo.fechada.denied',
  connectionLost: 'ajustes.navegadorAoVivo.fechada.connectionLost',
  unavailable: 'ajustes.navegadorAoVivo.fechada.unavailable',
  closed: 'ajustes.navegadorAoVivo.fechada.closed',
} as const satisfies Record<LiveClosedReason, ChaveTexto>

export const chaveDoMotivo = (reason: LiveClosedReason) => MOTIVOS[reason]
