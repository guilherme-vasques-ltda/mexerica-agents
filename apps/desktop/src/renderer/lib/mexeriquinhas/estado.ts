// SPDX-License-Identifier: Apache-2.0
import rotulos from './rotulos.pt-BR.json' with { type: 'json' };

// Estado do avatar: cinco estados, iguais para todos. Função pura, com `now` explícito.

export type EstadoId = 'dormindo' | 'livre' | 'trabalhando' | 'precisa-resposta' | 'erro';

export interface Estado {
  id: EstadoId;
  nome: string;
  /** Rótulo alternativo do mesmo estado (Trabalhando também aparece como "Ocupado"). */
  nomeAlternativo?: string;
  selo: 'zzz' | '?' | 'x' | null;
}

export const ESTADOS: readonly Estado[] = [
  { id: 'dormindo', nome: rotulos.estados['dormindo'], selo: 'zzz' },
  { id: 'livre', nome: rotulos.estados['livre'], selo: null },
  { id: 'trabalhando', nome: rotulos.estados['trabalhando'], nomeAlternativo: rotulos.ocupado, selo: null },
  { id: 'precisa-resposta', nome: rotulos.estados['precisa-resposta'], selo: '?' },
  { id: 'erro', nome: rotulos.estados['erro'], selo: 'x' },
];

/** Inatividade a partir da qual o bot dorme (estritamente maior). */
export const INATIVIDADE_MS = 8 * 60 * 60 * 1000;

export interface SinaisAtividade {
  erro?: boolean;
  precisaResposta?: boolean;
  ocupado?: boolean;
  /** Epoch em ms da última atividade. Ausente/nulo = sem registro. */
  ultimaAtividade?: number | null;
}

/**
 * Precedência: erro > precisa-resposta > trabalhando > dormindo (> 8 h parado) > livre.
 * Sem registro de atividade ou com horário no futuro, fica livre.
 */
export function resolveState(s: SinaisAtividade, now: number): EstadoId {
  if (s.erro) return 'erro';
  if (s.precisaResposta) return 'precisa-resposta';
  if (s.ocupado) return 'trabalhando';
  const ultima = s.ultimaAtividade;
  if (typeof ultima === 'number' && Number.isFinite(ultima) && now - ultima > INATIVIDADE_MS) {
    return 'dormindo';
  }
  return 'livre';
}

export function estadoPorId(id: unknown): Estado {
  return ESTADOS.find((e) => e.id === id) ?? ESTADOS[1]!;
}
