import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/contrato.ts'

// DON-903: contrato (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'contrato.titulo': "مع عقد",
  'contrato.resultado': "النتيجة المتوقعة",
  'contrato.aceite': "كيفية التحقق",
  'contrato.escopo': "أين يمكنه العمل",
  'contrato.limites': "الحدود",
  'contrato.regra': "قاعدة التفويض المتبعة",
  'contrato.nota': "اقترحه البوت الذي طلب. يوجّه التسليم ولا يمنح أي إذن ولا يغيّر الحدود.",
}
