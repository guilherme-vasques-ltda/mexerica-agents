import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/instrucoes.ts'

// Instruções do dono (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "instrucoes.pagina": "Instructions",
  "instrucoes.pagina.lead": "Écrivez une fois comment vous voulez que vos bots travaillent. Cela vaut pour tous, sans modifier bot par bot.",
  "instrucoes.geral.titulo": "Pour tous mes bots",
  "instrucoes.geral.ajuda": "Vaut pour chaque bot de n'importe quel Space de cet ordinateur.",
  "instrucoes.geral.exemplo": "Ex. : Réponses courtes, en langage simple. Ne me demandez pas de détails d'exécution : choisissez le défaut raisonnable et dites-le-moi en une ligne.",
  "instrucoes.space.titulo": "Dans ce Space",
  "instrucoes.space.ajuda": "Vaut seulement pour les bots du Space ouvert, en plus du texte de tous les bots.",
  "instrucoes.space.exemplo": "Ex. : Ici le travail est pour des clients : ton formel, sans argot.",
  "instrucoes.contador": "{n}/{max}",
  "instrucoes.estado.salvo": "Enregistré. Vaut dès le prochain message de chaque bot, sans rien redémarrer.",
  "instrucoes.estado.vazio": "Vide. Rien n'est ajouté aux instructions des bots.",
  "instrucoes.estado.naoSalvo": "Non enregistré.",
  "instrucoes.estado.salvando": "Enregistrement…",
  "instrucoes.estado.lendo": "Lecture des instructions…",
  "instrucoes.estado.conflito": "Le texte a changé ailleurs. Réessayer le relit et enregistre votre texte sur la version récente.",
  "instrucoes.acao.salvar": "Enregistrer",
  "instrucoes.acao.tentarDeNovo": "Réessayer",
  "instrucoes.falha.envio": "Impossible d'enregistrer maintenant. Réessayez.",
  "instrucoes.falha.leitura": "Impossible de lire les instructions maintenant.",
  "instrucoes.falha.limite": "Le texte dépasse la limite de caractères.",
  "instrucoes.falha.invalido": "Le texte contient un caractère invisible ou de contrôle. Supprimez-le et réessayez.",
  "instrucoes.falha.spaceSumiu": "Ce Space n'existe plus.",
  "instrucoes.falha.soDono": "Seul le propriétaire, sur cet ordinateur, change les instructions.",
  "instrucoes.falha.hubAntigo": "Mettez à jour le Hub pour utiliser les instructions.",
  "instrucoes.semConexao": "Pas de connexion au Hub. Les instructions apparaissent à son retour.",
  "instrucoes.ordem.titulo": "Ordre :",
  "instrucoes.ordem.texto": "ce que vous écrivez sur le bot lui-même prime sur « Dans ce Space », qui prime sur « Pour tous mes bots ». Ce sont des consignes : elles n'accordent ni permission, ni outil, ni modèle, ni compte. N'écrivez pas de mots de passe ni de clés ici : les bots lisent ce texte.",
}
