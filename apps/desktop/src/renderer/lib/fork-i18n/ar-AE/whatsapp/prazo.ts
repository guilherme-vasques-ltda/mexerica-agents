import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/prazo.ts'

// DON-903: whatsapp/prazo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.prazo.secao': 'على هذا الكمبيوتر',
  'whatsapp.prazo.rotulo': 'الاحتفاظ بالرسائل لمدة',
  'whatsapp.prazo.detalhe': 'بعد ذلك، تختفي الرسائل من هذا الكمبيوتر. لا يتغير شيء على الهاتف.',
  'whatsapp.prazo.dias.zero': '{n} يوم',
  'whatsapp.prazo.dias.one': 'يوم واحد',
  'whatsapp.prazo.dias.two': 'يومان',
  'whatsapp.prazo.dias.few': '{n} أيام',
  'whatsapp.prazo.dias.many': '{n} يومًا',
  'whatsapp.prazo.dias.other': '{n} يوم',
}
