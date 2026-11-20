import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/instrucoes.ts'

// Instruções do dono (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "instrucoes.pagina": "التعليمات",
  "instrucoes.pagina.lead": "اكتب مرة واحدة كيف تريد أن تعمل روبوتاتك. تسري على الجميع دون تعديل كل روبوت على حدة.",
  "instrucoes.geral.titulo": "لكل روبوتاتي",
  "instrucoes.geral.ajuda": "تسري على كل روبوت في أي Space على هذا الكمبيوتر.",
  "instrucoes.geral.exemplo": "مثال: إجابات قصيرة وبلغة بسيطة. لا تسألني عن تفاصيل التنفيذ: اختر الخيار المعقول وأخبرني في سطر واحد.",
  "instrucoes.space.titulo": "في هذا الـSpace",
  "instrucoes.space.ajuda": "تسري فقط على روبوتات الـSpace المفتوح، إضافةً إلى نص كل الروبوتات.",
  "instrucoes.space.exemplo": "مثال: العمل هنا للعملاء: نبرة رسمية وبلا عامية.",
  "instrucoes.contador": "{n}/{max}",
  "instrucoes.estado.salvo": "تم الحفظ. تسري من الرسالة التالية لكل روبوت، دون إعادة تشغيل.",
  "instrucoes.estado.vazio": "فارغ. لا يُضاف شيء إلى تعليمات الروبوتات.",
  "instrucoes.estado.naoSalvo": "غير محفوظ.",
  "instrucoes.estado.salvando": "جارٍ الحفظ…",
  "instrucoes.estado.lendo": "جارٍ قراءة التعليمات…",
  "instrucoes.estado.conflito": "تغيّر النص في مكان آخر. تعيد المحاولة قراءته وتحفظ نصك فوق النسخة الجديدة.",
  "instrucoes.acao.salvar": "حفظ",
  "instrucoes.acao.tentarDeNovo": "إعادة المحاولة",
  "instrucoes.falha.envio": "تعذّر الحفظ الآن. حاول مرة أخرى.",
  "instrucoes.falha.leitura": "تعذّرت قراءة التعليمات الآن.",
  "instrucoes.falha.limite": "تجاوز النص حد الأحرف.",
  "instrucoes.falha.invalido": "في النص حرف غير مرئي أو حرف تحكم. احذفه وحاول مرة أخرى.",
  "instrucoes.falha.spaceSumiu": "هذا الـSpace لم يعد موجودًا.",
  "instrucoes.falha.soDono": "المالك وحده، على هذا الكمبيوتر، يغيّر التعليمات.",
  "instrucoes.falha.hubAntigo": "حدّث الـHub لاستخدام التعليمات.",
  "instrucoes.semConexao": "لا يوجد اتصال بالـHub. تظهر التعليمات عند عودته.",
  "instrucoes.ordem.titulo": "الترتيب:",
  "instrucoes.ordem.texto": "ما تكتبه في الروبوت نفسه أهم من «في هذا الـSpace»، وهذا أهم من «لكل روبوتاتي». هذه إرشادات: لا تمنح إذنًا ولا أداة ولا نموذجًا ولا حسابًا.",
}
