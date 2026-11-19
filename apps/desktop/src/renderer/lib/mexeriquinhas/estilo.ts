// SPDX-License-Identifier: Apache-2.0
import type { AvatarConfig } from './pecas.ts';
import type { ConjuntoAvatar } from './conjuntos.ts';

/** Escolha exclusivamente visual; não carrega papel ou instruções de bot. */
export type EstiloAvatar = 'simples' | 'pelucia';
export type EscolhaAvatar =
  | { estilo: 'simples'; nome: string; aparencia: AvatarConfig }
  | { estilo: 'pelucia'; nome: string; conjunto: ConjuntoAvatar };
