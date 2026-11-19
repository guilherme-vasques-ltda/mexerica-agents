// MX-147: o texto de atividade que as telas mostram. Código conhecido sai do catálogo no idioma de `t` (a view que
// recebe `t` e `idioma` de `useT()`/`useIdioma()` re-renderiza ao trocar o idioma); sem código válido, o `activity`
// livre do Hub, intacto. MX-253 (decisão do dono = A): o título da ferramenta (`tool`) deixa de sair cru e passa pela mesma
// tradução em linguagem simples dos passos (`stepText`, MX-141): "Read src/pace.js" lê "Lendo src/pace.js". O código não
// traz o tipo do passo, então vale 'other'. O `activity` livre (Hub sem código) segue intacto: não se sabe que é ferramenta.
import type { ActivityCode } from '../../shared/fork-activity-code.ts'
import type { AppLanguage } from '../../shared/fork-idioma.ts'
import { waitingLabel } from '../../shared/subscription.ts'
import type { Tradutor } from './fork-i18n/index.ts'
import { stepText } from '../views/thread/cards/step-text.ts'

const FIXED_KEY = {
  writing_reply: 'navegacao.barra.atividade.escrevendo',
  thinking: 'navegacao.barra.atividade.pensando',
  working: 'navegacao.barra.atividade.trabalhando',
  starting_agent: 'navegacao.barra.atividade.iniciando',
  resuming_session: 'navegacao.barra.atividade.retomando',
  continuing: 'navegacao.barra.atividade.continuando',
  // Como a frase legada "Typing…" (#712, DonnabotThinkingMark): o mesmo texto no Hub novo e no antigo.
  typing: 'navegacao.barra.atividade.escrevendo',
  starting: 'navegacao.barra.atividade.iniciando',
  summarizing: 'navegacao.barra.atividade.resumindo',
  resuming_work: 'navegacao.barra.atividade.retomandoTrabalho',
  waiting_admission: 'navegacao.barra.atividade.aguardandoAdmissao',
  waiting_session: 'navegacao.barra.atividade.aguardandoSessao',
  subscription_paused: 'navegacao.barra.atividade.assinaturaSuspensa',
  setup_failed: 'navegacao.barra.atividade.preparoFalhou',
  routine_setup_failed: 'navegacao.barra.atividade.rotinaPreparoFalhou',
  routine_interrupted: 'navegacao.barra.atividade.rotinaInterrompida',
  start_failed: 'navegacao.barra.atividade.naoIniciou',
} as const

export type WithActivity = { activity?: string | null; activityCode?: ActivityCode | null; resumeAt?: number | null }

/** O texto do código sem o prefixo do membro, ou `null` quando não dá para reproduzi-lo com segurança. */
function bodyText(t: Tradutor, idioma: AppLanguage, code: ActivityCode, resumeAt: number | null | undefined): string | null {
  switch (code.kind) {
    case 'needs_approval': return t('sessions.detail.needs.permission', { texto: code.title })
    case 'tool': return stepText(t, { title: code.title, kind: 'other' })
    case 'downloading': return t('navegacao.barra.atividade.baixando', { item: code.item })
    case 'installing': return t('navegacao.barra.atividade.instalando', { item: code.item })
    // O horário vem do `resumeAt` do bot, como na espera de sempre; ausente = "assim que liberar". Outra coisa
    // (valor não numérico, data inválida) não se reproduz com segurança: volta ao texto do Hub.
    case 'subscription_wait':
      if (resumeAt == null) return waitingLabel(t, idioma, { resumeAt: null })
      return typeof resumeAt === 'number' && Number.isFinite(resumeAt) && !Number.isNaN(new Date(resumeAt).getTime()) ? waitingLabel(t, idioma, { resumeAt }) : null
    default: return t(FIXED_KEY[code.kind])
  }
}

/** O título da ferramenta como o Hub mandou, para a dica (`title`) de quem mostra a atividade traduzida; `''` sem ele. */
export function activityOriginal(source: WithActivity): string {
  return source.activityCode?.kind === 'tool' ? source.activityCode.title : ''
}

/** O texto do código no idioma de `t` (com o nome do membro, num grupo), ou `null` se não dá para reproduzi-lo. */
export function activityCodeText(t: Tradutor, idioma: AppLanguage, code: ActivityCode, resumeAt?: number | null): string | null {
  const body = bodyText(t, idioma, code, resumeAt)
  return body !== null && code.member ? t('navegacao.barra.atividade.membro', { nome: code.member, texto: body }) : body
}

/**
 * Atividade para mostrar: o código traduzido, senão o texto do Hub; `''` quando não há nada (quem chama põe o seu
 * padrão). Onde a #712 diz o pedido pelo estado (BotRow, DonnaCard, ChatRows), quem chama nem lê a atividade.
 */
export function activityText(t: Tradutor, idioma: AppLanguage, source: WithActivity): string {
  const code = source.activityCode ? activityCodeText(t, idioma, source.activityCode, source.resumeAt) : null
  return code ?? source.activity ?? ''
}
