import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/voz.ts'

// DON-903: whatsapp/voz (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.voz.transcrito': '由 {provedor} 转写',
  'whatsapp.voz.transcritoProvedor': '由你的服务商转写',
  'whatsapp.voz.semFala': '没有可转写的语音',
  'whatsapp.voz.transcrevendo': '正在转写…',
  'whatsapp.voz.longa': '太长，无法转写',
  'whatsapp.voz.semChave': '无转写：请在“设置 → 语音”中添加密钥',
  'whatsapp.voz.formato': '此格式无法转写',
  'whatsapp.voz.falhou': '无法转写',
  'whatsapp.voz.transcrever': '转写收到的语音消息',
  'whatsapp.voz.transcreverDetalhe': '收到的每条语音消息都会发送给你选择的转写服务商（OpenAI 或 Gemini）转成文字，随后从这台电脑删除。只保留文字。',
}
