import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/numero.ts'

// DON-903: whatsapp/numero (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // WhatsApp D374: o número do bot escondido e o olhinho.
  'whatsapp.numero.escondido': 'الرقم مخفي، ينتهي بـ {fim}',
  'whatsapp.numero.mostrar': 'إظهار الرقم',
  'whatsapp.numero.esconder': 'إخفاء الرقم',
}
