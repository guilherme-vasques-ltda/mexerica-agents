import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/limites.ts'

// DON-903: conversa/limites (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'conversa.limites.titulo': 'Limites d’utilisation',
  'conversa.limites.retomar': 'Reprendre après la limite',
  'conversa.limites.retomarDetalhe': 'Quand la limite du compte est réinitialisée, le bot poursuit la demande interrompue.',
  'conversa.limites.soneca': 'Mise en veille jusqu’à la réinitialisation de la limite',
  'conversa.limites.sonecaDetalhe': 'La conversation attend l’heure de réinitialisation, sans tentative avant ni alerte entre-temps.',
  'conversa.limites.rodape': 'S’applique à tous les bots de ce Hub. Dans chaque conversation, « Ne pas reprendre » annule uniquement cette attente.',
  'conversa.limites.carregando': 'Lecture des réglages…',
  'conversa.limites.semConexao': 'Pas de connexion au Hub. Les réglages s’afficheront à son retour.',
  'conversa.limites.hubAntigo': 'Mettez à jour le Hub pour modifier les limites d’utilisation.',
  'conversa.limites.falhaLer': 'Impossible de lire les réglages pour le moment.',
  'conversa.limites.falhaGravar': 'Impossible de confirmer l’enregistrement. Vérifiez les réglages.',
  'conversa.limites.tentarDeNovo': 'Réessayer',
  'conversa.limites.naoRetomar': 'Ne pas reprendre',
  'conversa.limites.retomarAgora': 'Reprendre maintenant',
  'conversa.limites.falhaCancelar': 'Impossible de confirmer l’annulation. Vérifiez la conversation avant de réessayer.',
  'conversa.limites.falhaRetomar': 'Impossible de reprendre maintenant. Réessayez.',
}
