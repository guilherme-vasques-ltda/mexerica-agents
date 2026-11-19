import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/skills.ts'

// Fatia 2 (D454): conversa/skills (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "conversa.skills.motivo": "السبب",
  "conversa.skills.agoraNao": "ليس الآن",
  "conversa.skills.examinarRascunho": "مراجعة المسودة",
  "conversa.skills.soPessoal": "المهارات متاحة في «الشخصي» فقط. لا يمكن لهذه المساحة الفتح أو الحفظ في الكتالوج الشخصي.",
  "conversa.skills.indicacao.aria": "اقتراح مهارة",
  "conversa.skills.indicacao.de": "اقتراح من {bot}",
  "conversa.skills.indicacao.referencia": "المرجع المقترح",
  "conversa.skills.indicacao.pista": "هذا مجرد دليل للبحث بالاسم أو المعرّف. لا يؤكد الوجود أو الإصدار أو التثبيت أو الاستخدام من قِبل بوت. المراجعة تفتح بحثًا فقط.",
  "conversa.skills.indicacao.soPessoal": "المهارات متاحة في «الشخصي» فقط. لا يمكن لهذه المساحة البحث في الكتالوج الشخصي.",
  "conversa.skills.indicacao.examinar": "مراجعة الاقتراح",
  "conversa.skills.recomendacao.aria": "توصية مهارة",
  "conversa.skills.recomendacao.de": "توصية من {bot}",
  "conversa.skills.recomendacao.quando": "متى تُستخدم",
  "conversa.skills.recomendacao.instrucoes": "التعليمات المقترحة",
  "conversa.skills.recomendacao.nada": "اقتراح هذه المسودة وفتحها لا يحفظ شيئًا ولا يفعّل مهارات ولا يوافق على إجراءات.",
  "conversa.skills.rascunho.revise": "مسودة نصية. راجعها قبل الحفظ؛ لن يُفعَّل أي بوت.",
}
