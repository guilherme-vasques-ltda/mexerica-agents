import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/voz.ts'

// DON-903: whatsapp/voz (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.voz.transcrito': 'Transcribed by {provedor}',
  'whatsapp.voz.transcritoProvedor': 'Transcribed by your provider',
  'whatsapp.voz.semFala': 'No speech to transcribe',
  'whatsapp.voz.transcrevendo': 'Transcribing…',
  'whatsapp.voz.longa': 'Too long to transcribe',
  'whatsapp.voz.semChave': 'No transcript: add the key in Settings → Voice',
  'whatsapp.voz.formato': "This format can't be transcribed",
  'whatsapp.voz.falhou': "Couldn't transcribe",
  'whatsapp.voz.transcrever': 'Transcribe received voice messages',
  'whatsapp.voz.transcreverDetalhe': 'Each voice message you receive is sent to the transcription provider you chose (OpenAI or Gemini) to become text, then deleted from this computer. Only the text is kept.',
}
