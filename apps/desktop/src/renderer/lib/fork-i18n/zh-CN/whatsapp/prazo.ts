import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/prazo.ts'

// DON-903: whatsapp/prazo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.prazo.secao': '在这台电脑上',
  'whatsapp.prazo.rotulo': '消息保留时长',
  'whatsapp.prazo.detalhe': '超过此时长后，消息会从这台电脑上删除。手机上不会有任何变化。',
  'whatsapp.prazo.dias.other': '{n} 天',
}
