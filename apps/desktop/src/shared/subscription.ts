import type { Bot } from './models'
import type { AppLanguage } from './fork-idioma.ts'
import { formatarData, type Tradutor } from '../renderer/lib/fork-i18n/index.ts'

/** A hora da retomada no formato do idioma do app, no fuso de quem está vendo a conversa. */
const hora = (idioma: AppLanguage, at: number) => formatarData(idioma, at, { hour: '2-digit', minute: '2-digit' })

/** D454 (fatia 10b, BACKLOG do #575): o texto e a hora no idioma do app. */
export function waitingLabel(t: Tradutor, idioma: AppLanguage, bot: Pick<Bot, 'resumeAt'>): string {
  if (bot.resumeAt == null) return t('sistema.conversa.espera.semHora')
  return t('sistema.conversa.espera.comHora', { hora: hora(idioma, bot.resumeAt) })
}

/** Só a retomada ("retomada às 15:30"), para quem já diz "Aguardando" de outro jeito. */
export function resumeLabel(t: Tradutor, idioma: AppLanguage, resumeAt: number): string {
  return t('sistema.conversa.espera.retomada', { hora: hora(idioma, resumeAt) })
}
