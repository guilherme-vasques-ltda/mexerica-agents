import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/ilha.ts'

// Fatia 1 (D454): navegacao/ilha (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.ilha.botAssistente": "{bot}، بوت مساعد",
  "navegacao.ilha.abrirConversa": "فتح المحادثة",
  "navegacao.ilha.nadaMostrar": "لا عمل لعرضه.",
  "navegacao.ilha.nadaRodando": "لا شيء قيد التشغيل الآن.",
  "navegacao.ilha.indisponiveis": "هناك بوتات حالتها غير متاحة.",
  "navegacao.ilha.mensagemBot": "رسالة إلى بوت",
  "navegacao.ilha.terminou": "{motor} · انتهى",
  "navegacao.ilha.parou": "توقف {bot}.",
  "navegacao.ilha.outroBot": "اختيار بوت آخر",
  "navegacao.ilha.mensagemPara": "رسالة إلى {bot}",
  "navegacao.ilha.mensagemParaPlaceholder": "رسالة إلى {bot}…",
  "navegacao.ilha.enviar": "إرسال",
  "navegacao.ilha.mostrarBots": "إظهار البوتات",
  "navegacao.ilha.aba.bots": "البوتات",
  "navegacao.ilha.trabalhandoBadge": "يعمل",
  "navegacao.ilha.pronto": "جاهز",
  "navegacao.ilha.falhou": "فشل",
  "navegacao.ilha.abrir": "فتح {bot}",
  "navegacao.ilha.usoAria": "الاستخدام: {provedor} {janela} {pct} بالمئة",
  "navegacao.ilha.semUso": "لا استخدام بعد.",
  "navegacao.ilha.semUsoDetalhe": "تظهر حدود Claude Code وCodex هنا بعد أن يستخدمها بوت.",
  "navegacao.ilha.ok": "حسنًا",
  "navegacao.ilha.trabalhando.zero": "لا أحد يعمل",
  "navegacao.ilha.trabalhando.one": "واحد يعمل",
  "navegacao.ilha.trabalhando.two": "اثنان يعملان",
  "navegacao.ilha.trabalhando.few": "{n} يعملون",
  "navegacao.ilha.trabalhando.many": "{n} يعمل",
  "navegacao.ilha.trabalhando.other": "{n} يعمل",
}
