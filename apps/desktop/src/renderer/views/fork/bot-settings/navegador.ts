import type { Bot, ScreenState } from '../../../../shared/models'
import type { BrowserPreference, BrowserStatus } from '../../../../shared/fork-browser'
import type { BotEditorProfile } from '../../../../shared/fork-bot-editor'

/**
 * Seletor "Navegador" do editor do bot (entrega E1 + adendo E2, 10/11/2026): três opções —
 * Meu computador, Integrado ao app e Desligado. A preferência é do dono, versionada pelo
 * editor; escolher nunca liga "Usar o computador" e Desligado é só navegador — não o desliga (escopo revisto pelo dono em 10/11/2026).
 */

/** A escolha efetiva: o rascunho do editor quando existe; sem rascunho, o confirmado pelo Hub. */
export function preferenciaDoNavegador(
  draft: Pick<BotEditorProfile, 'browserPreference'> | null | undefined,
  bot: Pick<Bot, 'browserPreference'>,
): BrowserPreference {
  return draft?.browserPreference ?? bot.browserPreference
}

/**
 * O valor que o Hub confirmou: o perfil que a última gravação do editor devolveu;
 * sem editor lido, o mirror do bot. É a única fonte do estado "desligado" exibido —
 * um rascunho pendente ou uma gravação recusada/conflitada não desliga nada na tela.
 */
export function preferenciaConfirmadaDoNavegador(
  profile: Pick<BotEditorProfile, 'browserPreference'> | null | undefined,
  bot: Pick<Bot, 'browserPreference'>,
): BrowserPreference {
  return profile?.browserPreference ?? bot.browserPreference
}

/**
 * Nome do computador onde os bots trabalham, sempre o que o Hub projetou (`screenAccess`
 * disponível ou o `name` do hello, que é o host_name do Hub). Sem fonte, `null`: a tela
 * mostra o texto sem nome, nunca um nome inventado.
 */
export function nomeDoComputador(bot: Pick<Bot, 'screenAccess'>, helloName: string | null | undefined): string | null {
  return bot.screenAccess?.find((row) => row.available)?.name || helloName || null
}

export type SituacaoDoComputador = 'pronto' | 'semUso' | 'semTela'

/**
 * "Meu computador": a tela usável é a mesma condição do controle "Usar o computador"
 * (`screen.computerUse`, a que o Hub usa para aceitar ou recusar a chamada). Sem tela
 * admitida não existe "pronto", mesmo com o controle ligado — é o caso do Hub em servidor.
 */
export function situacaoDoComputador(
  bot: Pick<Bot, 'computer'>,
  screen: Pick<ScreenState, 'computerUse'> | null | undefined,
): SituacaoDoComputador {
  if (!screen?.computerUse) return 'semTela'
  return bot.computer ? 'pronto' : 'semUso'
}

export type MotivoDoIntegrado = 'backend_absent' | 'client_unsupported' | 'generico'

/** O motivo vem do código estável `reason`; erro de consulta ou resposta incompatível vira o genérico. */
export function motivoDoIntegrado(status: BrowserStatus | null | undefined): MotivoDoIntegrado {
  return status?.reason === 'backend_absent' || status?.reason === 'client_unsupported' ? status.reason : 'generico'
}
