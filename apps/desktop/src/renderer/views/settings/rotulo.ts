// D454 (fatia 4): rótulos vindos de listas de dados (temas, cores, abas…) chegam como `ChaveTexto` ampla, sem
// parâmetros. O tradutor tipado exige os parâmetros de cada chave; aqui ele recebe só a chave.
import { createElement, Fragment, type ReactNode } from 'react'
import type { ChaveTexto } from '../../lib/fork-i18n/index.ts'
import { superficieIdioma, useIdioma, type Tradutor } from '../../lib/fork-i18n/estado'

export const semParametros = (t: Tradutor) => t as unknown as (chave: ChaveTexto) => string

/** Marcador de parâmetro: um caractere de uso privado que nenhum texto do catálogo tem. */
const MARCA = ''
/** O valor de parâmetro que `comNos` troca pelo elemento (`t('x', { app: no('app') })`). */
export const no = (nome: string) => `${MARCA}${nome}${MARCA}`

/**
 * Uma frase traduzida inteira com elementos no lugar de alguns parâmetros (um nome em negrito, por exemplo), sem
 * cortar a frase em pedaços: a ordem das palavras continua sendo a de cada idioma.
 */
export function comNos(texto: string, nos: Record<string, ReactNode>): ReactNode[] {
  return texto.split(new RegExp(`${MARCA}(\\w+)${MARCA}`)).map((parte, i) => (i % 2 === 1 ? createElement(Fragment, { key: i }, nos[parte] ?? parte) : parte))
}

/**
 * Um trecho migrado dentro de uma tela que ainda mistura legado: declara `lang`/`dir` do idioma escolhido só nele,
 * sem caixa própria (`display: contents`), e o resto da tela continua no limite legado pt-BR/ltr.
 */
export function Superficie({ children }: { children: ReactNode }) {
  const idioma = useIdioma()
  return createElement('div', { className: 'idioma-superficie', ...superficieIdioma(idioma), style: { display: 'contents' } }, children)
}
