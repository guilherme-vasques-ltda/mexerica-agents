// Donnabot (DON-917): o que o editor de rotina do Codync (`views/routines/`) precisa além das chaves do catálogo
// (fork-i18n, módulo `rotinas`; cada lugar do editor chama `t('rotinas.editor.…')`). Antes era `editor-pt.ts`: um
// mapa do texto em inglês do upstream para o pt-BR, em que um texto sem tradução aparecia como veio. Agora o idioma
// é o do app e a falta de tradução cai no inglês e depois no pt-BR (CADEIA_FALLBACK do catálogo). A agenda por
// extenso vem do Hub: `resumo` é pt-BR (calendar.rs) e `summary` é o inglês do upstream (`agendaDoHub`).
import type { AppLanguage } from '../../../shared/fork-idioma.ts'
import { localeDe, traduzirPlural } from '../../lib/fork-i18n/index.ts'
import type { RoutineSchedulePreview } from '../routines/routine'

/** Os dias da semana (segunda a domingo, ids do cron) com nome curto e completo do Intl, no idioma atual. */
export function txWeekdays(idioma: AppLanguage): { id: number; label: string; title: string }[] {
  const locale = localeDe(idioma)
  const curto = new Intl.DateTimeFormat(locale, { weekday: 'short' })
  const longo = new Intl.DateTimeFormat(locale, { weekday: 'long' })
  // 2 de novembro de 2026 é uma segunda-feira; o cron conta domingo como 0.
  return [1, 2, 3, 4, 5, 6, 0].map((id) => {
    const dia = new Date(2026, 10, id === 0 ? 8 : id + 1)
    const label = curto.format(dia).replace('.', '')
    return { id, label: label.charAt(0).toLocaleUpperCase(locale) + label.slice(1), title: longo.format(dia) }
  })
}

/** O tempo limite como se fala ("1 hour", "30 minutos", "90 secondes"); outro texto como foi digitado. */
export function txDuration(text: string, idioma: AppLanguage): string {
  const value = text.trim()
  if (!/^\d+$/.test(value) || Number(value) === 0) return `${value} s`
  const seconds = Number(value)
  if (seconds % 3600 === 0) return traduzirPlural(idioma, 'rotinas.editor.duracao.horas', seconds / 3600)
  if (seconds % 60 === 0) return traduzirPlural(idioma, 'rotinas.editor.duracao.minutos', seconds / 60)
  return traduzirPlural(idioma, 'rotinas.editor.duracao.segundos', seconds)
}

/** A agenda por extenso que o Hub manda: `resumo` só existe em pt-BR; nos outros idiomas, o `summary` em inglês. */
export function agendaDoHub(preview: Pick<RoutineSchedulePreview, 'summary' | 'resumo'>, idioma: AppLanguage): string {
  return idioma === 'pt-BR' ? (preview.resumo ?? preview.summary) : preview.summary
}
