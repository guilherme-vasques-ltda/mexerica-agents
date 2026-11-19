import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/voz.ts'

// DON-903: whatsapp/voz (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'whatsapp.voz.transcrito': 'Transcrit par {provedor}',
  'whatsapp.voz.transcritoProvedor': 'Transcrit par votre fournisseur',
  'whatsapp.voz.semFala': 'Aucune parole à transcrire',
  'whatsapp.voz.transcrevendo': 'Transcription…',
  'whatsapp.voz.longa': 'Trop long pour être transcrit',
  'whatsapp.voz.semChave': 'Pas de transcription : ajoutez la clé dans Réglages → Voix',
  'whatsapp.voz.formato': 'Ce format ne peut pas être transcrit',
  'whatsapp.voz.falhou': 'Impossible de transcrire',
  'whatsapp.voz.transcrever': 'Transcrire les messages vocaux reçus',
  'whatsapp.voz.transcreverDetalhe': 'Chaque message vocal reçu est envoyé au fournisseur de transcription choisi (OpenAI ou Gemini) pour être converti en texte, puis supprimé de cet ordinateur. Seul le texte est conservé.',
}
