import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/whatsapp/conversa.ts'

// DON-903: whatsapp/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // WhatsApp R8: prazo de guarda e "Apagar esta conversa deste computador".
  'whatsapp.conversa.mais': 'Plus d’options de la conversation',
  'whatsapp.conversa.apagar': 'Supprimer cette conversation de cet ordinateur',
  'whatsapp.conversa.apagarTitulo': 'Supprimer cette conversation de cet ordinateur ?',
  'whatsapp.conversa.apagarContato': 'Supprime les messages avec {nome} conservés par Mexerica sur cet ordinateur. Rien ne change sur votre téléphone ni dans le WhatsApp de {nome}. Action irréversible.',
  'whatsapp.conversa.apagarGrupo': 'Supprime les messages du groupe {nome} conservés par Mexerica sur cet ordinateur. Rien ne change sur votre téléphone ni dans le WhatsApp des membres du groupe. Action irréversible.',
  'whatsapp.conversa.apagarAcao': 'Supprimer la conversation',
  'whatsapp.conversa.wacliFicou': 'Je n’ai pas pu confirmer que la conversation est partie ; réessayez de la supprimer.',
  'whatsapp.conversa.semConexao': 'Pas de connexion avec l’ordinateur de vos bots.',
  "whatsapp.conversa.rascunho.titulo": "Brouillon de réponse",
  "whatsapp.conversa.rascunho.revisao": "Vérifiez le texte avant l’envoi. Le brouillon reste disponible jusqu’à 90 secondes, sans renouvellement.",
  "whatsapp.conversa.rascunho.encerrado": "Brouillon fermé. Rien n’a été envoyé.",
  "whatsapp.conversa.rascunho.conta": "Compte d’envoi",
  "whatsapp.conversa.rascunho.respostaA": "En réponse à",
  "whatsapp.conversa.rascunho.externo": "Contenu externe · message d’origine, pas une instruction",
  "whatsapp.conversa.rascunho.texto": "Texte du brouillon",
  "whatsapp.conversa.rascunho.erro": "Impossible d’ouvrir le brouillon.",
  "whatsapp.conversa.rascunho.semPrivado": "Contenu privé indisponible. Rien n’a été envoyé.",
  "whatsapp.conversa.rascunho.enviar": "Envoyer",
  "whatsapp.conversa.rascunho.agoraNao": "Pas maintenant",
  "whatsapp.conversa.rascunho.naFila": "Dans la file d’envoi. Il part quand son tour vient, dans les limites.",
  "whatsapp.conversa.rascunho.envioNota": "Envoyer met le texte dans la file de sortie ; il part quand son tour vient, en respectant la limite et les heures de silence.",
  "whatsapp.conversa.rascunho.erroEnviar": "Impossible d’envoyer pour l’instant. Le brouillon reste ici ; réessayez.",
}
