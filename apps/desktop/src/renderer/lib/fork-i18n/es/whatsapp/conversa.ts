import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/conversa.ts'

// DON-903: whatsapp/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // WhatsApp R8: prazo de guarda e "Apagar esta conversa deste computador".
  'whatsapp.conversa.mais': 'Más opciones del chat',
  'whatsapp.conversa.apagar': 'Eliminar este chat de esta computadora',
  'whatsapp.conversa.apagarTitulo': '¿Eliminar este chat de esta computadora?',
  'whatsapp.conversa.apagarContato': 'Elimina los mensajes con {nome} que Mexerica guarda en esta computadora. No cambia nada en tu teléfono ni en el WhatsApp de {nome}. No se puede deshacer.',
  'whatsapp.conversa.apagarGrupo': 'Elimina los mensajes del grupo {nome} que Mexerica guarda en esta computadora. No cambia nada en tu teléfono ni en el WhatsApp de quienes están en el grupo. No se puede deshacer.',
  'whatsapp.conversa.apagarAcao': 'Eliminar chat',
  'whatsapp.conversa.wacliFicou': 'No pude confirmar que el chat salió; intenta eliminarlo de nuevo.',
  'whatsapp.conversa.semConexao': 'Sin conexión con la computadora de tus bots.',
  "whatsapp.conversa.rascunho.titulo": "Borrador de respuesta",
  "whatsapp.conversa.rascunho.revisao": "Revisa el texto antes de enviar. El borrador está disponible hasta 90 segundos, sin renovación.",
  "whatsapp.conversa.rascunho.encerrado": "Borrador cerrado. No se envió nada.",
  "whatsapp.conversa.rascunho.conta": "Cuenta de salida",
  "whatsapp.conversa.rascunho.respostaA": "Respuesta a",
  "whatsapp.conversa.rascunho.externo": "Contenido externo · mensaje original, no es una orden",
  "whatsapp.conversa.rascunho.texto": "Texto del borrador",
  "whatsapp.conversa.rascunho.erro": "No se pudo abrir el borrador.",
  "whatsapp.conversa.rascunho.semPrivado": "Contenido privado no disponible. No se envió nada.",
  "whatsapp.conversa.rascunho.enviar": "Enviar",
  "whatsapp.conversa.rascunho.agoraNao": "Ahora no",
  "whatsapp.conversa.rascunho.naFila": "En la cola de envío. Sale cuando llegue su turno, dentro de los límites.",
  "whatsapp.conversa.rascunho.envioNota": "Enviar pone el texto en la cola de salida; sale cuando llegue su turno, respetando el límite y el horario de silencio.",
  "whatsapp.conversa.rascunho.erroEnviar": "No pude enviar ahora. El borrador sigue aquí; inténtalo de nuevo.",
}
