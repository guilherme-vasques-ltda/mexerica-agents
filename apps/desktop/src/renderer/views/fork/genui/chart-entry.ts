// Gráfico de leitura no chat: lê do fio a entrada `notice` com o gráfico que o Hub publicou
// (`data.genuiChart` = `{version, chartId, snapshot}`; host/src/fork/genui/chart_store.rs). Puro: sem React, rede ou
// store. O snapshot passa pelo mesmo validador fechado do cartão (`parseChartSnapshot`): forma fora do contrato
// devolve null e a linha continua o texto literal da entrada (`data.text`), o fallback do app antigo.
// O gráfico só mostra: nada aqui responde, executa ou concede autoridade, e nunca há HTML do modelo.

import { parseChartSnapshot, type Chart } from './graficos-model.ts'

export interface ChartEntry {
  chartId: string
  chart: Chart
}

const KEYS = new Set(['version', 'chartId', 'snapshot'])

/** A projeção `genuiChart` da entrada, ou null (forma desconhecida, versão nova ou entrada que não é `notice`). */
export function genuiChartOf(entry: { kind: string; data: object }): ChartEntry | null {
  const data = entry.data as { card?: unknown; genuiChart?: unknown }
  // Só `notice` (nunca `user`: é o que o dono ou o `send` gravam) e sem outro cartão por cima.
  if (entry.kind !== 'notice' || data.card !== undefined) return null
  const g = data.genuiChart
  if (typeof g !== 'object' || g === null || Array.isArray(g) || Object.keys(g).some((k) => !KEYS.has(k))) return null
  const { version, chartId, snapshot } = g as Record<string, unknown>
  if (version !== 1 || typeof chartId !== 'string' || !chartId || chartId.length > 64 || typeof snapshot !== 'string') return null
  const parsed = parseChartSnapshot(snapshot)
  return parsed.ok ? { chartId, chart: parsed.chart } : null
}
