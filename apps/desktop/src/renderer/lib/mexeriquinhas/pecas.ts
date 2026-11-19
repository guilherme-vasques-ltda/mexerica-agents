// SPDX-License-Identifier: Apache-2.0
import rotulos from './rotulos.pt-BR.json' with { type: 'json' };

// Catálogo das partes das Mexeriquinhas: ids estáveis, rótulos em PT-BR e cores.

export type CorId = 'laranja' | 'lima' | 'limao' | 'toranja' | 'roxo' | 'celeste';
export type OlhosId =
  | 'ponto'
  | 'sonolentos'
  | 'brilhantes'
  | 'oculos-redondos'
  | 'oculos-escuros'
  | 'piscando'
  | 'oculos-verdes';
export type RoupaId = 'nenhuma' | 'blazer' | 'colete' | 'moletom' | 'avental' | 'jaleco' | 'polo';
export type AcessorioId =
  | 'nenhum'
  | 'coroa'
  | 'fone'
  | 'capacete'
  | 'boina'
  | 'bone'
  | 'flor'
  | 'cartola'
  | 'capuz';
export type ObjetoId =
  | 'nenhum'
  | 'gravata'
  | 'moeda'
  | 'chave'
  | 'megafone'
  | 'notebook'
  | 'lupa'
  | 'pincel'
  | 'caixa'
  | 'carrinho'
  | 'coracao'
  | 'ferramenta';

/** Configuração declarativa de um avatar. Ids desconhecidos caem na opção base. */
export interface AvatarConfig {
  cor: CorId;
  olhos: OlhosId;
  roupa: RoupaId;
  acessorio: AcessorioId;
  objeto?: ObjetoId;
  /** Nome usado no rótulo acessível. */
  nome?: string;
}

export interface Opcao<T extends string> {
  id: T;
  nome: string;
  /** Extra = fora da grade principal de seis (usada por modelos específicos). */
  extra?: boolean;
}

export interface Cor extends Opcao<CorId> {
  base: string;
  clara: string;
  escura: string;
}

export const CORES: readonly Cor[] = [
  { id: 'laranja', nome: rotulos.cores['laranja'], base: '#f28a1e', clara: '#ffbf5c', escura: '#c45f0c' },
  { id: 'lima', nome: rotulos.cores['lima'], base: '#94c630', clara: '#c8ec6e', escura: '#62921a' },
  { id: 'limao', nome: rotulos.cores['limao'], base: '#f2c41e', clara: '#ffe57a', escura: '#c08f0a' },
  { id: 'toranja', nome: rotulos.cores['toranja'], base: '#ec5a5e', clara: '#ff9690', escura: '#b8343c' },
  { id: 'roxo', nome: rotulos.cores['roxo'], base: '#8d56d8', clara: '#bd92f4', escura: '#5f33a6' },
  { id: 'celeste', nome: rotulos.cores['celeste'], base: '#2f9cf0', clara: '#7cc8ff', escura: '#1a68bb' },
];

export const OLHOS: readonly Opcao<OlhosId>[] = [
  { id: 'ponto', nome: rotulos.olhos['ponto'] },
  { id: 'sonolentos', nome: rotulos.olhos['sonolentos'] },
  { id: 'brilhantes', nome: rotulos.olhos['brilhantes'] },
  { id: 'oculos-redondos', nome: rotulos.olhos['oculos-redondos'] },
  { id: 'oculos-escuros', nome: rotulos.olhos['oculos-escuros'] },
  { id: 'piscando', nome: rotulos.olhos['piscando'] },
  { id: 'oculos-verdes', nome: rotulos.olhos['oculos-verdes'], extra: true },
];

export const ROUPAS: readonly Opcao<RoupaId>[] = [
  { id: 'nenhuma', nome: rotulos.roupas['nenhuma'] },
  { id: 'blazer', nome: rotulos.roupas['blazer'] },
  { id: 'colete', nome: rotulos.roupas['colete'] },
  { id: 'moletom', nome: rotulos.roupas['moletom'] },
  { id: 'avental', nome: rotulos.roupas['avental'] },
  { id: 'jaleco', nome: rotulos.roupas['jaleco'] },
  { id: 'polo', nome: rotulos.roupas['polo'] },
];

export const ACESSORIOS: readonly Opcao<AcessorioId>[] = [
  { id: 'nenhum', nome: rotulos.acessorios['nenhum'] },
  { id: 'coroa', nome: rotulos.acessorios['coroa'] },
  { id: 'fone', nome: rotulos.acessorios['fone'] },
  { id: 'capacete', nome: rotulos.acessorios['capacete'] },
  { id: 'boina', nome: rotulos.acessorios['boina'] },
  { id: 'bone', nome: rotulos.acessorios['bone'] },
  { id: 'flor', nome: rotulos.acessorios['flor'] },
  { id: 'cartola', nome: rotulos.acessorios['cartola'], extra: true },
  { id: 'capuz', nome: rotulos.acessorios['capuz'], extra: true },
];

export const OBJETOS: readonly Opcao<ObjetoId>[] = [
  { id: 'nenhum', nome: rotulos.objetos['nenhum'] },
  { id: 'gravata', nome: rotulos.objetos['gravata'] },
  { id: 'moeda', nome: rotulos.objetos['moeda'] },
  { id: 'chave', nome: rotulos.objetos['chave'] },
  { id: 'megafone', nome: rotulos.objetos['megafone'] },
  { id: 'notebook', nome: rotulos.objetos['notebook'] },
  { id: 'lupa', nome: rotulos.objetos['lupa'] },
  { id: 'pincel', nome: rotulos.objetos['pincel'] },
  { id: 'caixa', nome: rotulos.objetos['caixa'] },
  { id: 'carrinho', nome: rotulos.objetos['carrinho'] },
  { id: 'coracao', nome: rotulos.objetos['coracao'] },
  { id: 'ferramenta', nome: rotulos.objetos['ferramenta'] },
];

export const CONFIG_BASE: AvatarConfig = {
  cor: 'laranja',
  olhos: 'ponto',
  roupa: 'nenhuma',
  acessorio: 'nenhum',
  objeto: 'nenhum',
};

function escolher<T extends string>(lista: readonly Opcao<T>[], id: unknown, base: T): T {
  return lista.some((o) => o.id === id) ? (id as T) : base;
}

/** Normaliza uma configuração vinda de fora: ids desconhecidos viram a opção base. */
export function normalizarConfig(c: Partial<AvatarConfig> | null | undefined): AvatarConfig {
  const v = c ?? {};
  const config: AvatarConfig = {
    cor: escolher(CORES, v.cor, 'laranja'),
    olhos: escolher(OLHOS, v.olhos, 'ponto'),
    roupa: escolher(ROUPAS, v.roupa, 'nenhuma'),
    acessorio: escolher(ACESSORIOS, v.acessorio, 'nenhum'),
    objeto: escolher(OBJETOS, v.objeto, 'nenhum'),
  };
  if (typeof v.nome === 'string' && v.nome.trim()) config.nome = v.nome.trim();
  return config;
}

export function corPorId(id: CorId): Cor {
  return CORES.find((c) => c.id === id) ?? CORES[0]!;
}
