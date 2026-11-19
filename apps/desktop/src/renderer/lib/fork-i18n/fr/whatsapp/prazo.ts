import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/prazo.ts'

// DON-903: whatsapp/prazo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.prazo.secao': 'Sur cet ordinateur',
  'whatsapp.prazo.rotulo': 'Conserver les messages pendant',
  'whatsapp.prazo.detalhe': 'Ensuite, les messages disparaissent de cet ordinateur. Rien ne change sur le téléphone.',
  'whatsapp.prazo.dias.one': '{n} jour',
  'whatsapp.prazo.dias.many': '{n} de jours',
  'whatsapp.prazo.dias.other': '{n} jours',
}
