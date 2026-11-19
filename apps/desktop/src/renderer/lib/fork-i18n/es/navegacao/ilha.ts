import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/navegacao/ilha.ts'

// Fatia 1 (D454): navegacao/ilha (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "navegacao.ilha.botAssistente": "{bot}, bot asistente",
  "navegacao.ilha.abrirConversa": "Abrir conversación",
  "navegacao.ilha.nadaMostrar": "No hay trabajo que mostrar.",
  "navegacao.ilha.nadaRodando": "Nada en ejecución ahora.",
  "navegacao.ilha.indisponiveis": "Hay bots con estado no disponible.",
  "navegacao.ilha.mensagemBot": "Mensaje para un bot",
  "navegacao.ilha.terminou": "{motor} · Terminó",
  "navegacao.ilha.parou": "{bot} se detuvo.",
  "navegacao.ilha.outroBot": "Elegir otro bot",
  "navegacao.ilha.mensagemPara": "Mensaje para {bot}",
  "navegacao.ilha.mensagemParaPlaceholder": "Mensaje para {bot}…",
  "navegacao.ilha.enviar": "Enviar",
  "navegacao.ilha.mostrarBots": "Mostrar bots",
  "navegacao.ilha.aba.bots": "Bots",
  "navegacao.ilha.trabalhandoBadge": "Trabajando",
  "navegacao.ilha.pronto": "Listo",
  "navegacao.ilha.falhou": "Falló",
  "navegacao.ilha.abrir": "Abrir {bot}",
  "navegacao.ilha.usoAria": "Uso: {provedor} {janela} {pct} por ciento",
  "navegacao.ilha.semUso": "Aún no hay uso.",
  "navegacao.ilha.semUsoDetalhe": "Los límites de Claude Code y Codex aparecen aquí después de que un bot los use.",
  "navegacao.ilha.ok": "Aceptar",
  "navegacao.ilha.trabalhando.one": "{n} trabajando",
  "navegacao.ilha.trabalhando.many": "{n} trabajando",
  "navegacao.ilha.trabalhando.other": "{n} trabajando",
}
