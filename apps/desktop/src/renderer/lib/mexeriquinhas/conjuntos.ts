// SPDX-License-Identifier: Apache-2.0
import { BASES_VISUAIS } from './bases.ts';
import type { Modelo } from './modelos.ts';

export interface ConjuntoAvatar { id: string; nome: string; baseUrl?: string }
const DIRETORIA = ['berga', 'pomarca', 'colheitor', 'tesourango', 'holofruta', 'chiperica'] as const;
export const CONJUNTOS: readonly ConjuntoAvatar[] = [
  { id: 'base', nome: 'Mexeriquinha' },
  ...BASES_VISUAIS.map(({ id, nome }) => ({ id, nome })),
  ...DIRETORIA.map((id) => ({ id, nome: id[0]!.toUpperCase() + id.slice(1) })),
];
export function conjuntoPorId(id: string): ConjuntoAvatar | undefined {
  return CONJUNTOS.find((c) => c.id === id.trim().toLowerCase());
}
export function conjuntoDoModelo(modelo: Modelo): ConjuntoAvatar {
  return conjuntoPorId(DIRETORIA.some((id) => id === modelo.id) ? modelo.id : modelo.baseId)!;
}
