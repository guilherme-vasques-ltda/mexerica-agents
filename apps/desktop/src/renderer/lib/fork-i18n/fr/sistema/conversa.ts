import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/sistema/conversa.ts'

// Fatia 10b (D454): sistema/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "sistema.conversa.espera.semHora": "En attente · dès que possible",
  "sistema.conversa.espera.comHora": "En attente · reprise à {hora}",
  "sistema.conversa.espera.retomada": "reprise à {hora}",
  "sistema.conversa.espera.parou": "Arrêt à cause de la limite du forfait ; {volta}",
  "sistema.conversa.espera.parouSemHora": "Arrêt à cause de la limite du forfait ; le fournisseur n’a pas indiqué quand il revient",
  "sistema.conversa.rapido.ligar": "Activez-le pour des réponses plus rapides et plus d’utilisation",
  "sistema.conversa.rapido.desligar": "Désactivez-le et revenez à la vitesse standard",
  "sistema.conversa.rapido.ligarTodos": "Activez-le pour tous les bots d’ici : réponses plus rapides et plus d’utilisation",
  "sistema.conversa.rapido.desligarTodos": "Désactivez-le pour tous les bots d’ici et revenez à la vitesse standard",
  "sistema.conversa.rapido.recusado": "L’agent l’a désactivé : {motivo}. Désactivez-le puis réactivez-le pour réessayer.",
  "sistema.conversa.rapido.recusadoBot": "{bot} : l’agent l’a désactivé : {motivo}. Désactivez-le puis réactivez-le pour réessayer.",
  "sistema.conversa.rapido.indisponivel": "indisponible",
  "sistema.conversa.parteIndisponivel": "Cette partie ne s’est pas chargée. Si cela continue, enregistrez ce que vous avez écrit, puis rechargez la page ou rouvrez l’app.",
  "sistema.conversa.tentarParte": "Réessayer",
}
