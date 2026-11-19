import type { SpacePrincipalAppearance } from '@shared/fork-spaces'
import type { AparenciaMexeriquinha } from '@shared/fork-mexeriquinha-config'

// D543 E1 (docs/reference/redesenho/propostas/spaces-principal-excluir-20261110): as seis
// personalidades do Berga são combinações de peças do editor de avatar que o app já tem
// (lib/mexeriquinhas, estilo Simples). A lista é trocável: o Hub só valida `aparencia`.
// Sem arte nova e sem Pelúcia 3D — as pelúcias do Berga ainda não existem na main.

export interface BergaFigure {
  id: 'simples' | 'elegante' | 'despojado' | 'brabo' | 'praieiro' | 'curioso'
  olhos: AparenciaMexeriquinha['olhos']
  roupa: AparenciaMexeriquinha['roupa']
  acessorio: AparenciaMexeriquinha['acessorio']
  objeto: AparenciaMexeriquinha['objeto']
}

export const BERGA_FIGURES: readonly BergaFigure[] = [
  { id: 'simples', olhos: 'ponto', roupa: 'nenhuma', acessorio: 'nenhum', objeto: 'nenhum' },
  { id: 'elegante', olhos: 'ponto', roupa: 'blazer', acessorio: 'nenhum', objeto: 'gravata' },
  { id: 'despojado', olhos: 'piscando', roupa: 'moletom', acessorio: 'nenhum', objeto: 'nenhum' },
  { id: 'brabo', olhos: 'oculos-escuros', roupa: 'moletom', acessorio: 'capuz', objeto: 'nenhum' },
  { id: 'praieiro', olhos: 'oculos-escuros', roupa: 'nenhuma', acessorio: 'flor', objeto: 'nenhum' },
  { id: 'curioso', olhos: 'oculos-redondos', roupa: 'colete', acessorio: 'nenhum', objeto: 'nenhum' },
]

/** As seis cores do seletor de avatar; `cor` vai dentro de `aparencia`. */
export const BERGA_CORES = ['laranja', 'lima', 'limao', 'toranja', 'roxo', 'celeste'] as const
export type BergaCor = (typeof BERGA_CORES)[number]

export function bergaAppearance(figure: BergaFigure, cor: BergaCor): SpacePrincipalAppearance {
  return {
    v: 2,
    estilo: 'simples',
    aparencia: { cor, olhos: figure.olhos, roupa: figure.roupa, acessorio: figure.acessorio, objeto: figure.objeto },
  }
}
