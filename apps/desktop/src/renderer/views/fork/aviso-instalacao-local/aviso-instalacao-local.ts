/** O aviso vale só para o Hub deste computador: sem sinal de que é ele, não aparece. */
export function ehHubDesteComputador(local: { computer: { id: string } } | null, store: { computer: { id: string } }): boolean {
  return local !== null && local.computer.id === store.computer.id
}

/**
 * O aviso de criação vale só quando o overview do Hub deste computador chegou e cobre todos os bots que a tela
 * pode escolher, e nenhuma rotina existente (ligada ou desligada) tem gatilho cron.
 */
export function avisaPrimeiraAgendada(input: {
  hubLocal: boolean
  visao: { routines: { triggers: { type: string }[] }[]; teams: { botIds: string[] }[] } | null
  botsDaTela: { id: string }[]
}): boolean {
  const { hubLocal, visao, botsDaTela } = input
  if (!hubLocal || visao === null) return false
  const cobertos = new Set(visao.teams.flatMap((team) => team.botIds))
  if (!botsDaTela.every((bot) => cobertos.has(bot.id))) return false
  return !visao.routines.some((rotina) => rotina.triggers.some((gatilho) => gatilho.type === 'cron'))
}
