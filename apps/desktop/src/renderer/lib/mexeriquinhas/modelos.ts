// SPDX-License-Identifier: Apache-2.0
// D526: catálogo nomeado do dono. Metadados visuais, sem criar bots ou delegação no app.
import catalogo from './catalogo.pt-BR.json' with { type: 'json' };
import { baseVisualPorId, type BaseVisualId } from './bases.ts';
import type { AvatarConfig } from './pecas.ts';

export type CategoriaId =
  | 'diretoria' | 'produtividade' | 'conteudo' | 'tech' | 'e-commerce' | 'operacao'
  | 'marketing-ads' | 'criadores' | 'atendimento-vendas' | 'financas-pessoas'
  | 'mercado-financeiro' | 'design-3d' | 'negocio-gestao' | 'vida-pessoal'
  | 'profissoes' | 'comunidade' | 'bem-estar';
export type GrupoId = 'pomarca' | 'colheitor' | 'tesourango' | 'holofruta' | 'chiperica' | 'direto-com-berga';

export interface Categoria {
  id: CategoriaId;
  nome: string;
  nota?: string;
}
export interface Grupo {
  id: GrupoId;
  nome: string;
}
export interface Modelo {
  id: string;
  nome: string;
  papel: string;
  categoriaId: CategoriaId;
  grupoId: GrupoId;
  /** Hierarquia do catálogo para a prévia, sem qualquer autoridade de runtime. */
  reportaA: string | null;
  baseId: BaseVisualId;
  destaque?: boolean;
  aliases?: readonly string[];
  config: AvatarConfig;
}

type Entrada = Omit<Modelo, 'config'> & { config?: Partial<AvatarConfig> };
export const CATALOGO_FONTE = catalogo.fonte;
export const CATEGORIAS: readonly Categoria[] = catalogo.categorias as Categoria[];
export const GRUPOS: readonly Grupo[] = catalogo.grupos as Grupo[];

/** D528: nomes mantidos; conjuntoDoModelo escolhe um conjunto WebP pronto. Config é só metadado visual. */
export const MODELOS: readonly Modelo[] = (catalogo.modelos as Entrada[]).map((entrada) => {
  const { config, ...modelo } = entrada;
  const base = baseVisualPorId(modelo.baseId)!;
  return { ...modelo, config: { ...base.config, ...config, nome: modelo.nome } };
});

export function modeloPorId(id: string): Modelo | undefined {
  const chave = id.trim().toLowerCase();
  return MODELOS.find((m) => m.id === chave || m.aliases?.includes(chave));
}

export function modelosDaCategoria(id: CategoriaId): readonly Modelo[] {
  return MODELOS.filter((m) => m.categoriaId === id);
}
