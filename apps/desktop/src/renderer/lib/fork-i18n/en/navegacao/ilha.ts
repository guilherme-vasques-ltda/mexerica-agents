import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/ilha.ts'

// Fatia 1 (D454): navegacao/ilha (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.ilha.botAssistente": "{bot}, assistant bot",
  "navegacao.ilha.abrirConversa": "Open conversation",
  "navegacao.ilha.nadaMostrar": "No work to show.",
  "navegacao.ilha.nadaRodando": "Nothing running right now.",
  "navegacao.ilha.indisponiveis": "Some bots have an unavailable status.",
  "navegacao.ilha.mensagemBot": "Message a bot",
  "navegacao.ilha.terminou": "{motor} · Finished",
  "navegacao.ilha.parou": "{bot} stopped.",
  "navegacao.ilha.outroBot": "Pick another bot",
  "navegacao.ilha.mensagemPara": "Message {bot}",
  "navegacao.ilha.mensagemParaPlaceholder": "Message {bot}…",
  "navegacao.ilha.enviar": "Send",
  "navegacao.ilha.mostrarBots": "Show bots",
  "navegacao.ilha.aba.bots": "Bots",
  "navegacao.ilha.trabalhandoBadge": "Working",
  "navegacao.ilha.pronto": "Done",
  "navegacao.ilha.falhou": "Failed",
  "navegacao.ilha.abrir": "Open {bot}",
  "navegacao.ilha.usoAria": "Usage: {provedor} {janela} {pct} percent",
  "navegacao.ilha.semUso": "No usage yet.",
  "navegacao.ilha.semUsoDetalhe": "Claude Code and Codex limits show up here once a bot uses them.",
  "navegacao.ilha.ok": "OK",
  "navegacao.ilha.trabalhando.one": "{n} working",
  "navegacao.ilha.trabalhando.other": "{n} working",
}
