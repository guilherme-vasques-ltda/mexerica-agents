import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/mailInbox.ts'

export const textos: Traducao<typeof fonte> = {
  "mailInbox.title": "Boîte de lecture seule",
  "mailInbox.fixtureNotice": "Démonstration avec des messages fictifs ; aucune connexion à un compte réel.",
  "mailInbox.refresh": "Actualiser",
  "mailInbox.loading": "Chargement…",
  "mailInbox.notConnected": "Aucune boîte n’est connectée.",
  "mailInbox.reconnect": "La connexion doit être autorisée à nouveau.",
  "mailInbox.unavailable": "La boîte est indisponible pour le moment.",
  "mailInbox.mailboxes": "Boîtes",
  "mailInbox.noMailboxes": "Aucune boîte disponible à la lecture.",
  "mailInbox.empty": "Cette boîte est vide.",
  "mailInbox.noSubject": "Sans objet",
  "mailInbox.thread": "Fil de discussion",
  "mailInbox.chooseThread": "Choisissez une conversation à lire.",
  "mailInbox.unknownSender": "Expéditeur inconnu",
  "mailInbox.declaredDomain": "Domaine déclaré",
  "mailInbox.truncated": "Le contenu a été raccourci.",
  "mailInbox.loadMore": "Charger plus",
  "mailInbox.error": "Impossible de charger la boîte.",
  "mailInbox.contextUnavailable": "Sélectionnez un Space connecté pour ouvrir la boîte en lecture seule.",
  "mailInbox.fixtureMailbox": "Support fictif",
  "mailInbox.fixtureSubject": "Demande d’exemple",
  "mailInbox.fixtureSender": "Personne d’exemple",
  "mailInbox.fixtureBody": "Message fictif. https://example.invalid/link doit rester du texte inerte.",
}
