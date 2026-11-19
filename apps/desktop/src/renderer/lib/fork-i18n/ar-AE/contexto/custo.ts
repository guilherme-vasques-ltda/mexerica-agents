import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/contexto/custo.ts'

// DON-903: contexto/custo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-908 C8
  'contexto.custo.titulo': 'التكلفة التقديرية لهذه المحادثة',
  'contexto.custo.parcial': 'جزئي: {n} من {total} دورات بلا سعر',
  'contexto.custo.vazio': 'لا توجد دورات مسجّلة في هذه المحادثة منذ تحديث الـHub.',
  'contexto.custo.semTokens': 'لا توجد رموز مسجّلة',
  'contexto.custo.falha': 'التكلفة غير متاحة الآن.',
  'contexto.custo.nota': 'تقدير لما يعادله استخدام الـAPI. في الاشتراك لا تدفع مقابل كل رمز؛ ما يهم هو حد الحساب.',
  'contexto.custo.parte.entrada': 'الإدخال {n}',
  'contexto.custo.parte.cacheLido': 'قراءة الذاكرة المؤقتة {n}',
  'contexto.custo.parte.cacheGravado': 'كتابة الذاكرة المؤقتة {n}',
  'contexto.custo.parte.saida': 'الإخراج {n}',
}
