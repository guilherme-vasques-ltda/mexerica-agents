import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/conversa.ts'

// DON-903: whatsapp/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // WhatsApp R8: prazo de guarda e "Apagar esta conversa deste computador".
  'whatsapp.conversa.mais': 'مزيد من خيارات المحادثة',
  'whatsapp.conversa.apagar': 'حذف هذه المحادثة من هذا الكمبيوتر',
  'whatsapp.conversa.apagarTitulo': 'حذف هذه المحادثة من هذا الكمبيوتر؟',
  'whatsapp.conversa.apagarContato': 'يحذف الرسائل مع {nome} التي تحفظها Mexerica على هذا الكمبيوتر. لا يتغير شيء على هاتفك ولا في واتساب {nome}. لا يمكن التراجع عن ذلك.',
  'whatsapp.conversa.apagarGrupo': 'يحذف رسائل المجموعة {nome} التي تحفظها Mexerica على هذا الكمبيوتر. لا يتغير شيء على هاتفك ولا في واتساب أعضاء المجموعة. لا يمكن التراجع عن ذلك.',
  'whatsapp.conversa.apagarAcao': 'حذف المحادثة',
  'whatsapp.conversa.wacliFicou': 'تعذّر تأكيد أن المحادثة خرجت؛ حاول الحذف مرة أخرى.',
  'whatsapp.conversa.semConexao': 'لا يوجد اتصال بكمبيوتر الروبوتات الخاصة بك.',
  "whatsapp.conversa.rascunho.titulo": "مسودة رد",
  "whatsapp.conversa.rascunho.revisao": "راجع النص قبل الإرسال. تبقى المسودة متاحة لمدة تصل إلى 90 ثانية دون تجديد.",
  "whatsapp.conversa.rascunho.encerrado": "أُغلقت المسودة. لم يُرسل شيء.",
  "whatsapp.conversa.rascunho.conta": "حساب الإرسال",
  "whatsapp.conversa.rascunho.respostaA": "رد على",
  "whatsapp.conversa.rascunho.externo": "محتوى خارجي · الرسالة الأصلية وليست تعليمات",
  "whatsapp.conversa.rascunho.texto": "نص المسودة",
  "whatsapp.conversa.rascunho.erro": "تعذر فتح المسودة.",
  "whatsapp.conversa.rascunho.semPrivado": "المحتوى الخاص غير متاح. لم يُرسل شيء.",
  "whatsapp.conversa.rascunho.enviar": "إرسال",
  "whatsapp.conversa.rascunho.agoraNao": "ليس الآن",
  "whatsapp.conversa.rascunho.naFila": "في طابور الإرسال. يُرسل عند دوره ضمن الحدود.",
  "whatsapp.conversa.rascunho.envioNota": "الإرسال يضع النص في طابور الخروج؛ يُرسل عند دوره مع مراعاة الحد وساعات الهدوء.",
  "whatsapp.conversa.rascunho.erroEnviar": "تعذّر الإرسال الآن. المسودة ما زالت هنا؛ حاول مجددًا.",
}
