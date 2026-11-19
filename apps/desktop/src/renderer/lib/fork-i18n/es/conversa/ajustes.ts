import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/ajustes.ts'

export const textos: Traducao<typeof fonte> = {
  "conversa.pagina.titulo": "Conversación",
  "conversa.envio.titulo": "Envío",
  "conversa.envio.atalho": "Atajo de envío",
  "conversa.envio.atalhoDetalhe": "Elige cuándo Enter envía o inserta una nueva línea",
  "conversa.envio.modoEnter": "Enter",
  "conversa.envio.modoVariasLinhas": "{mod}+Enter para varias líneas",
  "conversa.envio.modoSempre": "Siempre {mod}+Enter",
  "conversa.envio.rodapeEnter": "Enter envía. Shift+Enter inserta una nueva línea.",
  "conversa.envio.rodapeVariasLinhas": "Enter envía una sola línea. Con varias líneas, Enter inserta una nueva línea y {mod}+Enter envía.",
  "conversa.envio.rodapeSempre": "Enter inserta una nueva línea. {mod}+Enter envía.",
  "conversa.avancado.titulo": "Avanzado",
  "conversa.avancado.skillsMenu": "Skills en el menú /",
  "conversa.avancado.skillsMenuDetalhe": "Muestra las skills del bot en el menú de comandos /",
  "conversa.fastMode": "Modo rápido",
  "conversa.fechar": "Cerrar",
}
