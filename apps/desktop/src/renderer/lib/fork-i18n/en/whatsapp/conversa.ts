import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/conversa.ts'

// DON-903: whatsapp/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // WhatsApp R8: prazo de guarda e "Apagar esta conversa deste computador".
  'whatsapp.conversa.mais': 'More chat options',
  'whatsapp.conversa.apagar': 'Delete this chat from this computer',
  'whatsapp.conversa.apagarTitulo': 'Delete this chat from this computer?',
  'whatsapp.conversa.apagarContato': 'Deletes the messages with {nome} that Mexerica keeps on this computer. Nothing changes on your phone or in {nome}’s WhatsApp. This can’t be undone.',
  'whatsapp.conversa.apagarGrupo': 'Deletes the messages from the group {nome} that Mexerica keeps on this computer. Nothing changes on your phone or in the WhatsApp of the people in the group. This can’t be undone.',
  'whatsapp.conversa.apagarAcao': 'Delete chat',
  'whatsapp.conversa.wacliFicou': 'Couldn’t confirm the chat was removed; try deleting it again.',
  'whatsapp.conversa.semConexao': 'No connection to your bots’ computer.',
  "whatsapp.conversa.rascunho.titulo": "Reply draft",
  "whatsapp.conversa.rascunho.revisao": "Check the text before sending. The draft is available for up to 90 seconds, without renewal.",
  "whatsapp.conversa.rascunho.encerrado": "Draft closed. Nothing was sent.",
  "whatsapp.conversa.rascunho.conta": "Sending account",
  "whatsapp.conversa.rascunho.respostaA": "Reply to",
  "whatsapp.conversa.rascunho.externo": "External content · original message, not an instruction",
  "whatsapp.conversa.rascunho.texto": "Draft text",
  "whatsapp.conversa.rascunho.erro": "Could not open the draft.",
  "whatsapp.conversa.rascunho.semPrivado": "Private content unavailable. Nothing was sent.",
  "whatsapp.conversa.rascunho.enviar": "Send",
  "whatsapp.conversa.rascunho.agoraNao": "Not now",
  "whatsapp.conversa.rascunho.naFila": "In the send queue. It goes out when its turn comes, within the limits.",
  "whatsapp.conversa.rascunho.envioNota": "Send puts the text in the out queue; it goes out when its turn comes, respecting the limit and quiet hours.",
  "whatsapp.conversa.rascunho.erroEnviar": "Couldn’t send right now. The draft stays here; try again.",
}
