import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/parear.ts'

// Fatia 1 (D454): navegacao/parear (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.parear.qr": "رمز الاقتران لـ {computador}",
  "navegacao.parear.semRede": "لم يُعثر على عنوان شبكة و«الوصول من أي مكان» متوقف. اتصل بشبكة Wi-Fi أو فعّله.",
  "navegacao.parear.mesmaRede": "«الوصول من أي مكان» متوقف، لذا لا يصل iPhone إلى {computador} إلا على الشبكة نفسها.",
  "navegacao.parear.valeAte": "صالح مرة واحدة، حتى {hora}",
  "navegacao.parear.novo": "رمز جديد",
  "navegacao.parear.copiar": "نسخ رابط الاقتران",
  "navegacao.parear.semHost": "مضيف هذا الحاسوب غير متصل.",
  "navegacao.parear.esteComputador": "هذا الحاسوب",
}
