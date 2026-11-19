// DON-908 (C8): os textos de um total, iguais no balão (`contexto.custo.*`) e na página Uso (`conversa.custo.*`).
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { formatarNumero } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'
import { dinheiro, parcial, partes, tokens, type Totais } from './modelo.ts'

export type Prefixo = 'contexto.custo' | 'conversa.custo'

/** O valor da página Uso: o dinheiro informado pelo motor ou "Sem preço"; nunca um valor inventado. O balão, sem
 * dinheiro, mostra só os tokens (sem linha de valor). */
export const valorDe = (t: Tradutor, idioma: AppLanguage, totais: Totais) => dinheiro(idioma, totais.costs) ?? t('conversa.custo.semPreco')

/** As partes de tokens conhecidas ("Entrada 12 mil · Saída 3 mil"); nenhuma: "Sem tokens registrados". */
export function tokensDe(t: Tradutor, idioma: AppLanguage, p: Prefixo, totais: Totais): string {
  const lista = partes(totais).map(({ parte, n }) => t(`${p}.parte.${parte}`, { n: tokens(idioma, n) }))
  return lista.length ? lista.join(' · ') : t(`${p}.semTokens`)
}

/** "Parcial: 2 de 9 turnos sem preço", só com dinheiro e turnos de fora. */
export const parcialDe = (t: Tradutor, idioma: AppLanguage, p: Prefixo, totais: Totais) =>
  parcial(totais) ? t(`${p}.parcial`, { n: formatarNumero(idioma, totais.unpricedTurns), total: formatarNumero(idioma, totais.turns) }) : null
