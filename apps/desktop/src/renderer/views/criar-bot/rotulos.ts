// DON-903 na criação de bot: rótulos de dados conhecidos (funções da biblioteca e catálogo do motor) pelo idioma
// escolhido. IDs e valores gravados não mudam; o que não é conhecido aparece como veio (texto do usuário ou do motor).
import { papelDoTitulo, type Funcao } from '@shared/fork-bot-funcao'
import { chaveDoCatalogo } from '@shared/fork-criar-bot'
import type { Tradutor } from '../../lib/fork-i18n/estado'
import { CATALOGOS, type ChaveTexto } from '../../lib/fork-i18n/index'

type ChavePapel = Extract<ChaveTexto, `botRoles.${string}.nome`>
const ehChavePapel = (chave: string): chave is ChavePapel => chave in CATALOGOS['pt-BR']

/** O rótulo de uma função da biblioteca pelo ID estável; null se o ID não tiver chave. */
export function rotuloDoPapel(t: Tradutor, id: string): string | null {
  const chave = `botRoles.${id}.nome`
  return ehChavePapel(chave) ? t(chave) : null
}

/** O nome de uma função na tela: a do dono como foi escrita; a da biblioteca, traduzida. */
export const rotuloDaFuncao = (t: Tradutor, f: Funcao) => (f.propria ? f.nome : rotuloDoPapel(t, f.id.replace(/^exemplo:/, '')) ?? f.nome)

/** Um `title` gravado na tela: exemplo da biblioteca traduzido; texto do dono como está. */
export function rotuloDoTitulo(t: Tradutor, title: string): string {
  const id = papelDoTitulo(title)
  return (id && rotuloDoPapel(t, id)) || title
}

/** Nome ou valor de uma opção do modelo: o conhecido traduzido, o resto como o motor mandou. */
export function rotuloDoCatalogo(t: Tradutor, texto: string): string {
  const chave = chaveDoCatalogo(texto)
  return chave ? t(`botCreate.catalogo.${chave}`) : texto
}
