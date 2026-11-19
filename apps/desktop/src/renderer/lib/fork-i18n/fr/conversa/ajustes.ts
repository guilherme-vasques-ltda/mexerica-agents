import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/ajustes.ts'

export const textos: Traducao<typeof fonte> = {
  "conversa.pagina.titulo": "Conversation",
  "conversa.envio.titulo": "Envoi",
  "conversa.envio.atalho": "Raccourci d’envoi",
  "conversa.envio.atalhoDetalhe": "Choisissez quand Entrée envoie ou passe à la ligne",
  "conversa.envio.modoEnter": "Enter",
  "conversa.envio.modoVariasLinhas": "{mod}+Entrée pour plusieurs lignes",
  "conversa.envio.modoSempre": "Toujours {mod}+Entrée",
  "conversa.envio.rodapeEnter": "Entrée envoie. Maj+Entrée passe à la ligne.",
  "conversa.envio.rodapeVariasLinhas": "Entrée envoie une seule ligne. Avec plusieurs lignes, Entrée passe à la ligne et {mod}+Entrée envoie.",
  "conversa.envio.rodapeSempre": "Entrée passe à la ligne. {mod}+Entrée envoie.",
  "conversa.avancado.titulo": "Avancé",
  "conversa.avancado.skillsMenu": "Skills dans le menu /",
  "conversa.avancado.skillsMenuDetalhe": "Affiche les skills du bot dans le menu de commandes /",
  "conversa.fastMode": "Mode rapide",
  "conversa.fechar": "Fermer",
}
