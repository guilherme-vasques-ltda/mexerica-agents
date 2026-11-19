// Encaixe das abas de sessão (Barra05, passo 14), puro para testar sem DOM. As abas entram inteiras enquanto
// couberem, na ordem; a aberta nunca sai (o lugar dela é reservado). Se sobrar espaço, a próxima entra cortada
// para ocupá-lo; as demais vão para "···". Mesma regra do protótipo (`fitTabs`).

export interface TabFit {
  /** Índices mostrados, na ordem original. */
  shown: number[]
  /** A aba cortada para ocupar a sobra (índice e largura máxima), se houver. */
  cut: { index: number; width: number } | null
  /** Quantas ficaram para "···". */
  hidden: number
}

/**
 * `widths`: largura natural de cada aba (já limitada a 200 px). `room`: o espaço das abas na barra.
 * `gap`: o que cada aba soma além dela (traço e espaçamento). `more`: a largura do "···".
 */
export function fitTabs(widths: readonly number[], room: number, active: number, { gap, more, minCut = 104 }: { gap: number; more: number; minCut?: number }): TabFit {
  const all = widths.map((_, i) => i)
  const total = widths.reduce((sum, w) => sum + w + gap, 0)
  if (total <= room) return { shown: all, cut: null, hidden: 0 }
  let left = room - more - (active >= 0 && active < widths.length ? widths[active]! + gap : 0)
  const shown = new Set<number>(active >= 0 && active < widths.length ? [active] : [])
  let next = -1
  for (const i of all) {
    if (i === active) continue
    if (next < 0 && widths[i]! + gap <= left) {
      shown.add(i)
      left -= widths[i]! + gap
    } else if (next < 0) next = i
  }
  let cut: TabFit['cut'] = null
  if (next >= 0 && left - gap >= minCut) {
    shown.add(next)
    cut = { index: next, width: Math.min(widths[next]!, left - gap - 6) }
  }
  return { shown: all.filter((i) => shown.has(i)), cut, hidden: widths.length - shown.size }
}
