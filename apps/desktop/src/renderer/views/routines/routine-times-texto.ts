// Os textos dos horários do editor de rotina, no idioma do app: "9h", "Dias úteis às 9h e às 15h", a conta
// "2 horários × 5 dias = 10 vezes por semana" e as mensagens leigas de erro. Chaves em `rotinas.editor.*` (fork-i18n).
import type { AppLanguage } from '../../../shared/fork-idioma.ts'
import { localeDe, traduzir, traduzirPlural } from '../../lib/fork-i18n/index.ts'
import { txWeekdays } from '../rotinas-central/editor-textos.ts'
import { ALL_DAYS, WEEKDAYS, partsOf, type TimesProblem } from './routine-times.ts'

/** "9h" / "9h30" (pt-BR); cada idioma tem a sua forma na chave `horaCheia`/`horaComMin`. */
export function horaTexto(idioma: AppLanguage, time: string): string {
  const { hour, minute } = partsOf(time)
  return minute === 0 ? traduzir(idioma, 'rotinas.editor.horaCheia', { h: hour }) : traduzir(idioma, 'rotinas.editor.horaComMin', { h: hour, m: String(minute).padStart(2, '0') })
}

/** "às 9h"; à meia-noite e à 1h o português diz "à" ("à 0h"), e o espanhol "a la 1:00 h". */
function comPreposicao(idioma: AppLanguage, time: string): string {
  const { hour } = partsOf(time)
  const singular = idioma === 'pt-BR' ? hour <= 1 : idioma === 'es' ? hour === 1 : false
  return traduzir(idioma, singular ? 'rotinas.editor.agendaAsUm' : 'rotinas.editor.agendaAs', { hora: horaTexto(idioma, time) })
}

const lista = (idioma: AppLanguage, itens: string[]) => new Intl.ListFormat(localeDe(idioma), { style: 'long', type: 'conjunction' }).format(itens)

/** "Todo dia", "Dias úteis" ou os dias por extenso ("Seg, qua e sex"). */
export function diasTexto(idioma: AppLanguage, days: readonly number[]): string {
  if (days.length === ALL_DAYS.length) return traduzir(idioma, 'rotinas.editor.repetir.daily')
  if (days.join() === WEEKDAYS.join()) return traduzir(idioma, 'rotinas.editor.repetir.weekdays')
  const locale = localeDe(idioma)
  const nomes = txWeekdays(idioma).filter((d) => days.includes(d.id)).map((d, i) => (i === 0 ? d.label : d.label.toLocaleLowerCase(locale)))
  return lista(idioma, nomes)
}

/** A agenda por extenso: "Dias úteis às 9h e às 15h". */
export function agendaTexto(idioma: AppLanguage, days: readonly number[], times: readonly string[]): string {
  return traduzir(idioma, 'rotinas.editor.agendaTexto', { dias: diasTexto(idioma, days), horas: lista(idioma, times.map((time) => comPreposicao(idioma, time))) })
}

/** "2 horários × 5 dias = 10 vezes por semana" (a tela só mostra com 2 ou mais horários). */
export function vezesTexto(idioma: AppLanguage, horarios: number, dias: number): string {
  return traduzir(idioma, 'rotinas.editor.vezesSemana', {
    horarios: traduzirPlural(idioma, 'rotinas.editor.nHorarios', horarios),
    dias: traduzirPlural(idioma, 'rotinas.editor.nDias', dias),
    vezes: traduzirPlural(idioma, 'rotinas.editor.nVezes', horarios * dias),
  })
}

export const nHorariosTexto = (idioma: AppLanguage, n: number) => traduzirPlural(idioma, 'rotinas.editor.nHorarios', n)

export const horasTexto = (idioma: AppLanguage, times: readonly string[]) => lista(idioma, times.map((time) => horaTexto(idioma, time)))

/** A mensagem de um erro dos horários, para o leigo. */
export function problemaTexto(idioma: AppLanguage, problem: TimesProblem, quantos: number): string {
  switch (problem.kind) {
    case 'empty':
      return traduzir(idioma, quantos > 1 ? 'rotinas.editor.preenchaCada' : 'rotinas.editor.escolhaHorario')
    case 'repeat':
      return traduzir(idioma, 'rotinas.editor.repetido', { horas: horasTexto(idioma, problem.times) })
    case 'noDay':
      return traduzir(idioma, 'rotinas.editor.escolhaDia')
  }
}
