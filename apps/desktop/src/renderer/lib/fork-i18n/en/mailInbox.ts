import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/mailInbox.ts'

export const textos: Traducao<typeof fonte> = {
  "mailInbox.title": "Read-only inbox",
  "mailInbox.fixtureNotice": "Demonstration with fictional messages; it doesn’t connect to a real account.",
  "mailInbox.refresh": "Refresh",
  "mailInbox.loading": "Loading…",
  "mailInbox.notConnected": "No mailbox is connected.",
  "mailInbox.reconnect": "The connection needs authorization again.",
  "mailInbox.unavailable": "The mailbox is unavailable right now.",
  "mailInbox.mailboxes": "Mailboxes",
  "mailInbox.noMailboxes": "No mailbox is available for reading.",
  "mailInbox.empty": "This mailbox is empty.",
  "mailInbox.noSubject": "No subject",
  "mailInbox.thread": "Thread",
  "mailInbox.chooseThread": "Choose a conversation to read.",
  "mailInbox.unknownSender": "Unknown sender",
  "mailInbox.declaredDomain": "Declared domain",
  "mailInbox.truncated": "The content was shortened.",
  "mailInbox.loadMore": "Load more",
  "mailInbox.error": "The inbox couldn’t be loaded.",
  "mailInbox.contextUnavailable": "Select a connected Space to open the read-only inbox.",
  "mailInbox.fixtureMailbox": "Fictional support",
  "mailInbox.fixtureSubject": "Example request",
  "mailInbox.fixtureSender": "Example Person",
  "mailInbox.fixtureBody": "Fictional message. https://example.invalid/link must remain inert text.",
}
