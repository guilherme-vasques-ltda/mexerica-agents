import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/ajustes.ts'

export const textos: Traducao<typeof fonte> = {
  "conversa.pagina.titulo": "Conversation",
  "conversa.envio.titulo": "Sending",
  "conversa.envio.atalho": "Send shortcut",
  "conversa.envio.atalhoDetalhe": "Choose when Enter sends or starts a new line",
  "conversa.envio.modoEnter": "Enter",
  "conversa.envio.modoVariasLinhas": "{mod}+Enter for multiple lines",
  "conversa.envio.modoSempre": "Always {mod}+Enter",
  "conversa.envio.rodapeEnter": "Enter sends. Shift+Enter starts a new line.",
  "conversa.envio.rodapeVariasLinhas": "Enter sends a single line. With more than one line, Enter starts a new line and {mod}+Enter sends.",
  "conversa.envio.rodapeSempre": "Enter starts a new line. {mod}+Enter sends.",
  "conversa.avancado.titulo": "Advanced",
  "conversa.avancado.skillsMenu": "Skills in the / menu",
  "conversa.avancado.skillsMenuDetalhe": "Shows the bot's skills in the / command menu",
  "conversa.fastMode": "Fast mode",
  "conversa.fechar": "Close",
}
