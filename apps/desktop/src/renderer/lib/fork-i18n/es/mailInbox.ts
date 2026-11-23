import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/mailInbox.ts'

export const textos: Traducao<typeof fonte> = {
  "mailInbox.title": "Bandeja de solo lectura",
  "mailInbox.fixtureNotice": "Demostración con mensajes ficticios; no conecta con una cuenta real.",
  "mailInbox.refresh": "Actualizar",
  "mailInbox.loading": "Cargando…",
  "mailInbox.notConnected": "No hay ningún buzón conectado.",
  "mailInbox.reconnect": "La conexión necesita autorización de nuevo.",
  "mailInbox.unavailable": "El buzón no está disponible ahora.",
  "mailInbox.mailboxes": "Buzones",
  "mailInbox.noMailboxes": "No hay buzones disponibles para leer.",
  "mailInbox.empty": "Este buzón está vacío.",
  "mailInbox.noSubject": "Sin asunto",
  "mailInbox.thread": "Conversación",
  "mailInbox.chooseThread": "Elige una conversación para leer.",
  "mailInbox.unknownSender": "Remitente desconocido",
  "mailInbox.declaredDomain": "Dominio declarado",
  "mailInbox.truncated": "El contenido se acortó.",
  "mailInbox.loadMore": "Cargar más",
  "mailInbox.error": "No se pudo cargar el buzón.",
  "mailInbox.contextUnavailable": "Selecciona un Space conectado para abrir la bandeja de solo lectura.",
  "mailInbox.fixtureMailbox": "Soporte ficticio",
  "mailInbox.fixtureSubject": "Solicitud de ejemplo",
  "mailInbox.fixtureSender": "Persona de ejemplo",
  "mailInbox.fixtureBody": "Mensaje ficticio. https://example.invalid/link debe seguir como texto inerte.",
}
