import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/memoria/sugestoes.ts'

// DON-903: memoria/sugestoes (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // D389: Sugestões na Memória do bot (views/fork/memoria-auto) e o contador do cartão.
  'memoria.sugestoes.esperando': "اقتراحات بانتظارك: {n}",
  'memoria.sugestoes.aba': "الاقتراحات",
  'memoria.sugestoes.lead': "ما تريد الروبوتات الاحتفاظ به. لا شيء منه في الذاكرة حتى توافق عليه.",
  'memoria.sugestoes.vazio': "لا توجد اقتراحات بانتظارك.",
  'memoria.sugestoes.aprovar': "موافقة",
  'memoria.sugestoes.descartar': "تجاهل",
  'memoria.sugestoes.acao.remember': "تذكّر جديد",
  'memoria.sugestoes.acao.revise': "تعديل تذكّر",
  'memoria.sugestoes.acao.archive': "أرشفة تذكّر",
  'memoria.sugestoes.acao.confirm': "تأكيد: أنت قلت هذا",
  'memoria.sugestoes.acao.reclassify': "تأكيد من رسالتك",
  'memoria.sugestoes.acao.note': "ملاحظة {arquivo}",
  'memoria.sugestoes.removerNota': "إزالة هذه الملاحظة",
  'memoria.sugestoes.compartilhada': "لكل روبوتاتك",
  'memoria.sugestoes.mudou': "تغيّرت الذاكرة بعد هذا الاقتراح. لم يُستبدل شيء.",
  'memoria.sugestoes.semMudanca': "كان موجودًا في الذاكرة بالفعل.",
  'memoria.sugestoes.erro': "تعذّر الإكمال. حاول مرة أخرى.",
}
