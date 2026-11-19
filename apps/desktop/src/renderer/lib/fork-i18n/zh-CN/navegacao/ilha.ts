import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/ilha.ts'

// Fatia 1 (D454): navegacao/ilha (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.ilha.botAssistente": "{bot}，助理机器人",
  "navegacao.ilha.abrirConversa": "打开对话",
  "navegacao.ilha.nadaMostrar": "没有可显示的工作。",
  "navegacao.ilha.nadaRodando": "当前没有在运行的任务。",
  "navegacao.ilha.indisponiveis": "部分机器人状态不可用。",
  "navegacao.ilha.mensagemBot": "给机器人发消息",
  "navegacao.ilha.terminou": "{motor} · 已完成",
  "navegacao.ilha.parou": "{bot} 已停止。",
  "navegacao.ilha.outroBot": "选择其他机器人",
  "navegacao.ilha.mensagemPara": "给 {bot} 发消息",
  "navegacao.ilha.mensagemParaPlaceholder": "给 {bot} 发消息…",
  "navegacao.ilha.enviar": "发送",
  "navegacao.ilha.mostrarBots": "显示机器人",
  "navegacao.ilha.aba.bots": "机器人",
  "navegacao.ilha.trabalhandoBadge": "工作中",
  "navegacao.ilha.pronto": "完成",
  "navegacao.ilha.falhou": "失败",
  "navegacao.ilha.abrir": "打开 {bot}",
  "navegacao.ilha.usoAria": "用量：{provedor} {janela} 百分之 {pct}",
  "navegacao.ilha.semUso": "暂无用量。",
  "navegacao.ilha.semUsoDetalhe": "机器人使用 Claude Code 和 Codex 后，其限额会显示在这里。",
  "navegacao.ilha.ok": "好",
  "navegacao.ilha.trabalhando.other": "{n} 个工作中",
}
