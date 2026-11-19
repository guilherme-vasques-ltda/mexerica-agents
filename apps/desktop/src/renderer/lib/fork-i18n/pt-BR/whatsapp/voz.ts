// DON-903: whatsapp/voz (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos = {
  // R3b: bolha de voz do mini WhatsApp.
  'whatsapp.voz.transcrito': 'Transcrito por {provedor}',
  'whatsapp.voz.transcritoProvedor': 'Transcrito pelo seu provedor',
  'whatsapp.voz.semFala': 'Sem fala para transcrever',
  'whatsapp.voz.transcrevendo': 'Transcrevendo…',
  'whatsapp.voz.longa': 'Longa demais para transcrever',
  'whatsapp.voz.semChave': 'Sem transcrição: falta a chave em Ajustes → Voz',
  'whatsapp.voz.formato': 'Formato que não dá para transcrever',
  'whatsapp.voz.falhou': 'Não deu para transcrever',
  // Ajustes do bot → WhatsApp: o interruptor diz para onde vai o áudio.
  'whatsapp.voz.transcrever': 'Transcrever áudios recebidos',
  'whatsapp.voz.transcreverDetalhe': 'Cada áudio recebido é enviado ao provedor de transcrição escolhido (OpenAI ou Gemini) para virar texto e apagado deste computador em seguida. Só o texto fica guardado.',
} as const
