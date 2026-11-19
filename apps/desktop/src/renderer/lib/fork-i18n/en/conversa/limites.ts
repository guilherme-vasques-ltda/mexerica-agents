import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/limites.ts'

// DON-903: conversa/limites (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'conversa.limites.titulo': 'Usage limits',
  'conversa.limites.retomar': 'Resume after the limit',
  'conversa.limites.retomarDetalhe': 'When the account limit resets, the bot continues the request that stopped.',
  'conversa.limites.soneca': 'Snooze until the limit resets',
  'conversa.limites.sonecaDetalhe': 'The conversation waits for the reset time, with no earlier tries and no alerts meanwhile.',
  'conversa.limites.rodape': 'Applies to every bot on this Hub. In each conversation, “Don’t resume” cancels only that wait.',
  'conversa.limites.carregando': 'Reading settings…',
  'conversa.limites.semConexao': 'No connection to the Hub. Settings show up when it’s back.',
  'conversa.limites.hubAntigo': 'Update the Hub to change usage limits.',
  'conversa.limites.falhaLer': 'Couldn’t read the settings right now.',
  'conversa.limites.falhaGravar': 'Couldn’t confirm the save. Check the settings.',
  'conversa.limites.tentarDeNovo': 'Try again',
  'conversa.limites.naoRetomar': 'Don’t resume',
  'conversa.limites.retomarAgora': 'Resume now',
  'conversa.limites.falhaCancelar': 'Couldn’t confirm the cancellation. Check the conversation before trying again.',
  'conversa.limites.falhaRetomar': 'Couldn’t resume right now. Try again.',
}
