import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/prazo.ts'

// DON-903: whatsapp/prazo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.prazo.secao': 'On this computer',
  'whatsapp.prazo.rotulo': 'Keep messages for',
  'whatsapp.prazo.detalhe': 'After that, the messages are removed from this computer. Nothing changes on your phone.',
  'whatsapp.prazo.dias.one': '{n} day',
  'whatsapp.prazo.dias.other': '{n} days',
}
