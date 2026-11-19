import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/prazo.ts'

// DON-903: whatsapp/prazo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.prazo.secao': 'En esta computadora',
  'whatsapp.prazo.rotulo': 'Guardar los mensajes durante',
  'whatsapp.prazo.detalhe': 'Después de ese plazo, los mensajes desaparecen de esta computadora. En el teléfono no cambia nada.',
  'whatsapp.prazo.dias.one': '{n} día',
  'whatsapp.prazo.dias.many': '{n} de días',
  'whatsapp.prazo.dias.other': '{n} días',
}
