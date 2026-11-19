import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/sistema/conversa.ts'

// Fatia 10b (D454): sistema/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "sistema.conversa.espera.semHora": "في الانتظار · فور التوفر",
  "sistema.conversa.espera.comHora": "في الانتظار · يُستأنف عند {hora}",
  "sistema.conversa.espera.retomada": "يُستأنف عند {hora}",
  "sistema.conversa.espera.parou": "توقف بسبب حد الباقة؛ {volta}",
  "sistema.conversa.espera.parouSemHora": "توقف بسبب حد الباقة؛ لم يُبلغ المزوّد متى يعود",
  "sistema.conversa.rapido.ligar": "شغّله لردود أسرع واستخدام أكبر",
  "sistema.conversa.rapido.desligar": "أوقفه وعُد إلى السرعة العادية",
  "sistema.conversa.rapido.ligarTodos": "شغّله لكل البوتات هنا: ردود أسرع واستخدام أكبر",
  "sistema.conversa.rapido.desligarTodos": "أوقفه لكل البوتات هنا وعُد إلى السرعة العادية",
  "sistema.conversa.rapido.recusado": "أوقفه الوكيل: {motivo}. أوقفه وشغّله مجددًا لتحاول مرة أخرى.",
  "sistema.conversa.rapido.recusadoBot": "{bot}: أوقفه الوكيل: {motivo}. أوقفه وشغّله مجددًا لتحاول مرة أخرى.",
  "sistema.conversa.rapido.indisponivel": "غير متاح",
  "sistema.conversa.parteIndisponivel": "تعذّر تحميل هذا الجزء. إذا استمر ذلك، فاحفظ ما كتبته ثم أعد تحميل الصفحة أو أعد فتح التطبيق.",
  "sistema.conversa.tentarParte": "حاول مجددًا",
}
