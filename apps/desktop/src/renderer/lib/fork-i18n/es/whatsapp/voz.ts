import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/voz.ts'

// DON-903: whatsapp/voz (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.voz.transcrito': 'Transcrito por {provedor}',
  'whatsapp.voz.transcritoProvedor': 'Transcrito por tu proveedor',
  'whatsapp.voz.semFala': 'No hay voz para transcribir',
  'whatsapp.voz.transcrevendo': 'Transcribiendo…',
  'whatsapp.voz.longa': 'Demasiado largo para transcribir',
  'whatsapp.voz.semChave': 'Sin transcripción: falta la clave en Ajustes → Voz',
  'whatsapp.voz.formato': 'Este formato no se puede transcribir',
  'whatsapp.voz.falhou': 'No se pudo transcribir',
  'whatsapp.voz.transcrever': 'Transcribir audios recibidos',
  'whatsapp.voz.transcreverDetalhe': 'Cada audio recibido se envía al proveedor de transcripción elegido (OpenAI o Gemini) para convertirlo en texto y luego se borra de este ordenador. Solo se guarda el texto.',
}
