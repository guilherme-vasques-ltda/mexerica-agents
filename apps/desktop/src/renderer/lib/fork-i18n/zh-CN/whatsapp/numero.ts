import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/numero.ts'

// DON-903: whatsapp/numero (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // WhatsApp D374: o número do bot escondido e o olhinho.
  'whatsapp.numero.escondido': '号码已隐藏，尾号 {fim}',
  'whatsapp.numero.mostrar': '显示号码',
  'whatsapp.numero.esconder': '隐藏号码',
}
