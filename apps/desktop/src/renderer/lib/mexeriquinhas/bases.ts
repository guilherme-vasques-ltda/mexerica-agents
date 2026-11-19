// SPDX-License-Identifier: Apache-2.0
// Quinze combinações visuais reutilizadas pelo catálogo nomeado (D526).

import rotulos from './rotulos.pt-BR.json' with { type: 'json' };
import type { AvatarConfig } from './pecas.ts';

export type BaseVisualId =
  | 'ceo'
  | 'assistente'
  | 'financeiro'
  | 'marketing'
  | 'developer'
  | 'pesquisa'
  | 'vendas'
  | 'compras'
  | 'rh'
  | 'suporte'
  | 'designer'
  | 'amazon'
  | 'dono'
  | 'chefe'
  | 'hacker';

export interface BaseVisual {
  id: BaseVisualId;
  nome: string;
  papel: string;
  aliases?: readonly string[];
  config: AvatarConfig;
}

const m = (
  id: BaseVisualId,
  config: Omit<AvatarConfig, 'nome'>,
  aliases?: readonly string[],
): BaseVisual => ({ id, ...rotulos.modelos[id], config: { ...config, nome: rotulos.modelos[id].nome }, ...(aliases ? { aliases } : {}) });

export const BASES_VISUAIS: readonly BaseVisual[] = [
  m('ceo', { cor: 'laranja', olhos: 'ponto', roupa: 'blazer', acessorio: 'coroa', objeto: 'gravata' }),
  m('assistente', { cor: 'laranja', olhos: 'ponto', roupa: 'jaleco', acessorio: 'fone', objeto: 'nenhum' }),
  m('financeiro', { cor: 'lima', olhos: 'oculos-redondos', roupa: 'colete', acessorio: 'nenhum', objeto: 'moeda' }, ['cfo']),
  m('marketing', { cor: 'toranja', olhos: 'oculos-escuros', roupa: 'moletom', acessorio: 'nenhum', objeto: 'nenhum' }),
  m('developer', { cor: 'roxo', olhos: 'ponto', roupa: 'moletom', acessorio: 'fone', objeto: 'notebook' }, ['dev']),
  m('pesquisa', { cor: 'limao', olhos: 'ponto', roupa: 'jaleco', acessorio: 'nenhum', objeto: 'lupa' }),
  m('vendas', { cor: 'celeste', olhos: 'piscando', roupa: 'blazer', acessorio: 'nenhum', objeto: 'nenhum' }),
  m('compras', { cor: 'lima', olhos: 'ponto', roupa: 'avental', acessorio: 'bone', objeto: 'carrinho' }),
  m('rh', { cor: 'toranja', olhos: 'brilhantes', roupa: 'polo', acessorio: 'flor', objeto: 'coracao' }),
  m('suporte', { cor: 'limao', olhos: 'ponto', roupa: 'blazer', acessorio: 'fone', objeto: 'ferramenta' }),
  m('designer', { cor: 'roxo', olhos: 'ponto', roupa: 'avental', acessorio: 'boina', objeto: 'pincel' }),
  m('amazon', { cor: 'celeste', olhos: 'ponto', roupa: 'polo', acessorio: 'capacete', objeto: 'caixa' }),
  m('dono', { cor: 'laranja', olhos: 'ponto', roupa: 'blazer', acessorio: 'cartola', objeto: 'chave' }),
  m('chefe', { cor: 'toranja', olhos: 'ponto', roupa: 'colete', acessorio: 'fone', objeto: 'megafone' }),
  m('hacker', { cor: 'laranja', olhos: 'oculos-verdes', roupa: 'moletom', acessorio: 'capuz', objeto: 'nenhum' }),
];

/** Busca por id ou alias (sem diferenciar maiúsculas). */
export function baseVisualPorId(id: string): BaseVisual | undefined {
  const k = id.trim().toLowerCase();
  return BASES_VISUAIS.find((mo) => mo.id === k || mo.aliases?.includes(k));
}
