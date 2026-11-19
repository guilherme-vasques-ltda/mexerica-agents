// D169 (28/10, pedido do dono): the app's mark no longer stands for the bot at work (WorkingDots does, under the
// bot's face). What stays here is the activity text the Hub sends, shown in the app's language (fatia 1, D454).
// Uma autorização só (#712): o mesmo mapa da main, só por igualdade com a string inteira da atividade (sem corte por
// ":" nem prefixo de membro), mais "Typing…". Era .tsx; virou .ts para o teste rodar no Node.
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/index.ts'
import { activityText, type WithActivity } from '../../../lib/fork-atividade.ts'

// As frases fixas que o Hub manda em inglês → chaves do catálogo (navegacao.barra.atividade.*).
const activities = {
  'Writing a reply…': 'navegacao.barra.atividade.escrevendo',
  'Typing…': 'navegacao.barra.atividade.escrevendo',
  'Thinking…': 'navegacao.barra.atividade.pensando',
  'Working…': 'navegacao.barra.atividade.trabalhando',
  'Starting agent…': 'navegacao.barra.atividade.iniciando',
  'Resuming session…': 'navegacao.barra.atividade.retomando',
  'Continuing…': 'navegacao.barra.atividade.continuando',
} as const

/** A frase fixa do Hub, se a atividade for ela inteira; senão o texto como veio (vazio: "Trabalhando…"). */
export const activityIn = (lang: AppLanguage, activity: string) => Object.hasOwn(activities, activity)
  ? traduzir(lang, activities[activity as keyof typeof activities])
  : activity || traduzir(lang, 'navegacao.barra.atividade.trabalhando')

/**
 * Atividade do chat (MX-147): o código traduzido, senão a frase fixa legada (#712), senão o texto livre do Hub (nome
 * real de ferramenta). Só se chama trabalhando; pedindo você, o balão diz o pedido pelo estado.
 */
export const workingActivity = (t: Tradutor, idioma: AppLanguage, bot: WithActivity) =>
  activityText(t, idioma, { ...bot, activity: activityIn(idioma, bot.activity ?? '') })
