import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/voz.ts'

// DON-903: whatsapp/voz (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.voz.transcrito': 'نسخه {provedor}',
  'whatsapp.voz.transcritoProvedor': 'نسخه مزوّدك',
  'whatsapp.voz.semFala': 'لا يوجد كلام لنسخه',
  'whatsapp.voz.transcrevendo': 'جارٍ النسخ…',
  'whatsapp.voz.longa': 'طويل جدًا للنسخ',
  'whatsapp.voz.semChave': 'لا يوجد نص: أضف المفتاح في الإعدادات ← الصوت',
  'whatsapp.voz.formato': 'لا يمكن نسخ هذا التنسيق',
  'whatsapp.voz.falhou': 'تعذّر النسخ',
  'whatsapp.voz.transcrever': 'نسخ الرسائل الصوتية المستلمة',
  'whatsapp.voz.transcreverDetalhe': 'تُرسل كل رسالة صوتية مستلمة إلى مزوّد النسخ الذي اخترته (OpenAI أو Gemini) لتحويلها إلى نص، ثم تُحذف من هذا الكمبيوتر. يُحفظ النص فقط.',
}
