import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/sistema/conversa.ts'

// Fatia 10b (D454): sistema/conversa (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "sistema.conversa.espera.semHora": "等待中 · 一旦可用即继续",
  "sistema.conversa.espera.comHora": "等待中 · {hora} 恢复",
  "sistema.conversa.espera.retomada": "{hora} 恢复",
  "sistema.conversa.espera.parou": "因套餐额度用完而停止；{volta}",
  "sistema.conversa.espera.parouSemHora": "因套餐额度用完而停止；提供方未告知何时恢复",
  "sistema.conversa.rapido.ligar": "开启后回复更快，用量也更多",
  "sistema.conversa.rapido.desligar": "关闭并恢复标准速度",
  "sistema.conversa.rapido.ligarTodos": "为这里的所有机器人开启：回复更快，用量也更多",
  "sistema.conversa.rapido.desligarTodos": "为这里的所有机器人关闭并恢复标准速度",
  "sistema.conversa.rapido.recusado": "代理已将其关闭：{motivo}。关闭后再开启即可重试。",
  "sistema.conversa.rapido.recusadoBot": "{bot}：代理已将其关闭：{motivo}。关闭后再开启即可重试。",
  "sistema.conversa.rapido.indisponivel": "不可用",
  "sistema.conversa.parteIndisponivel": "此部分未能加载。若仍如此，请先保存已写内容，再刷新页面或重新打开应用。",
  "sistema.conversa.tentarParte": "重试",
}
