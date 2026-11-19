import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/ajustes.ts'

export const textos: Traducao<typeof fonte> = {
  "conversa.pagina.titulo": "المحادثة",
  "conversa.envio.titulo": "الإرسال",
  "conversa.envio.atalho": "اختصار الإرسال",
  "conversa.envio.atalhoDetalhe": "اختر متى يرسل Enter أو يبدأ سطرًا جديدًا",
  "conversa.envio.modoEnter": "Enter",
  "conversa.envio.modoVariasLinhas": "{mod}+Enter لعدة أسطر",
  "conversa.envio.modoSempre": "{mod}+Enter دائمًا",
  "conversa.envio.rodapeEnter": "Enter يرسل. Shift+Enter يبدأ سطرًا جديدًا.",
  "conversa.envio.rodapeVariasLinhas": "Enter يرسل سطرًا واحدًا. عند وجود عدة أسطر، يبدأ Enter سطرًا جديدًا ويرسل {mod}+Enter.",
  "conversa.envio.rodapeSempre": "Enter يبدأ سطرًا جديدًا. {mod}+Enter يرسل.",
  "conversa.avancado.titulo": "متقدم",
  "conversa.avancado.skillsMenu": "المهارات في قائمة /",
  "conversa.avancado.skillsMenuDetalhe": "يعرض مهارات البوت في قائمة أوامر /",
  "conversa.fastMode": "الوضع السريع",
  "conversa.fechar": "إغلاق",
}
