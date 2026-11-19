import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/ajustes.ts'

export const textos: Traducao<typeof fonte> = {
  "conversa.pagina.titulo": "对话",
  "conversa.envio.titulo": "发送",
  "conversa.envio.atalho": "发送快捷键",
  "conversa.envio.atalhoDetalhe": "选择 Enter 何时发送或换行",
  "conversa.envio.modoEnter": "Enter",
  "conversa.envio.modoVariasLinhas": "多行时使用 {mod}+Enter",
  "conversa.envio.modoSempre": "始终使用 {mod}+Enter",
  "conversa.envio.rodapeEnter": "Enter 发送。Shift+Enter 换行。",
  "conversa.envio.rodapeVariasLinhas": "单行时 Enter 发送。多行时 Enter 换行，{mod}+Enter 发送。",
  "conversa.envio.rodapeSempre": "Enter 换行。{mod}+Enter 发送。",
  "conversa.avancado.titulo": "高级",
  "conversa.avancado.skillsMenu": "/ 菜单中的技能",
  "conversa.avancado.skillsMenuDetalhe": "在 / 命令菜单中显示机器人的技能",
  "conversa.fastMode": "快速模式",
  "conversa.fechar": "关闭",
}
