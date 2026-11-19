// D165 fase F (components/DataTable): the table logic, pure so it is testable. Works on the table the
// bot sent in Markdown; nothing is fetched or changed.
import { matchesWords, searchWords } from '../../../lib/busca.ts'

export type SortDir = 'asc' | 'desc'
/** Reads a cell as a number for sorting/alignment; the default is the Markdown heuristic below. */
export type NumberReader = (cell: string) => number | null

/** A number as people write it in a table: 1.234,56 · 1,234.56 · 12% · R$ 40 · -3. */
export function numericValue(cell: string): number | null {
  const t = cell.replace(/[`*_~]/g, '').trim()
  if (!t) return null
  const m = /^(-|−)?\s*(?:R\$|US\$|\$|€|£)?\s*([\d.,\s]+)\s*(%|[a-zA-Z]{0,3})?$/.exec(t)
  if (!m) return null
  let n = m[2]!.replace(/\s/g, '')
  if (/,\d{1,2}$/.test(n) && n.includes('.')) n = n.replace(/\./g, '').replace(',', '.')
  else if (/,\d{1,2}$/.test(n)) n = n.replace(',', '.')
  else n = n.replace(/,/g, '')
  const v = Number(n)
  if (!Number.isFinite(v)) return null
  return m[1] ? -v : v
}

/**
 * D169 R5: a cell's line breaks. Markdown tables can't hold a newline, so bots write `<br>` (also `<br/>`,
 * `<br />`, any case). Only that tag splits the text; each piece stays plain text for the inline renderer,
 * and any other markup is left as literal text. Search, sort, copy and CSV keep the cell as it came.
 */
export function cellLines(cell: string, plain = false): string[] {
  // D273: `plain` (chart card's Ver dados) keeps the cell literal, `<br>` included.
  return plain ? [cell] : cell.split(/<br\s*\/?>/i)
}

/** Columns whose every filled cell is a number: aligned right, sorted as numbers. */
export function numericColumns(rows: readonly string[][], columns: number, parse: NumberReader = numericValue): boolean[] {
  return Array.from({ length: columns }, (_, c) => {
    const filled = rows.map((r) => r[c] ?? '').filter((v) => v.trim() !== '')
    return filled.length > 0 && filled.every((v) => parse(v) !== null)
  })
}

/** Rows that match the search in any cell (accents and case ignored), in the chosen order. */
export function visibleRows(rows: readonly string[][], query: string, sort: { column: number; dir: SortDir } | null, numeric: readonly boolean[], parse: NumberReader = numericValue): string[][] {
  const words = searchWords(query)
  const kept = rows.filter((r) => matchesWords(words, r))
  if (!sort) return kept
  const { column, dir } = sort
  const sign = dir === 'asc' ? 1 : -1
  return kept
    .map((row, index) => ({ row, index }))
    .sort((a, b) => {
      const x = a.row[column] ?? ''
      const y = b.row[column] ?? ''
      const diff = numeric[column]
        ? (parse(x) ?? -Infinity) - (parse(y) ?? -Infinity)
        : x.localeCompare(y, 'pt-BR', { sensitivity: 'base', numeric: true })
      return diff * sign || a.index - b.index
    })
    .map((r) => r.row)
}

const plain = (cell: string) => cell.replace(/\\\|/g, '|')

/** Export-only protection, shared by headers and data in both spreadsheet formats. */
function spreadsheetCell(s: string) {
  // Export accepts only decimal literals, including consistent PT/EN thousands separators.
  // Sort's numericValue also accepts currencies/units/percentages that may be expressions.
  const literal = /^-?(?:\d+(?:[.,]\d+)?|\d{1,3}(?:\.\d{3})+(?:,\d+)?|\d{1,3}(?:,\d{3})+(?:\.\d+)?)$/.exec(s)
  const number = literal?.[0] === s // Full cell: $ alone also matches before a final newline.
  return /^[\s\u0000-\u001f\u007f-\u009f]*[=+\-@\t\r]/.test(s) && !number ? `'${s}` : s
}

const quoteCell = (s: string) => /["\t\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s

/** Tab-separated, for pasting into a spreadsheet. */
export function toTSV(header: readonly string[], rows: readonly string[][], literal = false) {
  // No cell may inject another row/column. Quote literal quotes so a paste cannot reinterpret them.
  const clean = (v: string) => quoteCell(spreadsheetCell((literal ? v : plain(v)).replace(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]+/g, ' ')))
  return [header, ...rows].map((r) => r.map(clean).join('\t')).join('\n')
}

/** RFC 4180 CSV; neutralizes formula prefixes except for strict numeric literals. */
export function toCSV(header: readonly string[], rows: readonly string[][], literal = false) {
  const cell = (v: string) => {
    const s = spreadsheetCell(literal ? v : plain(v))
    return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s
  }
  return [header, ...rows].map((r) => r.map(cell).join(',')).join('\r\n') + '\r\n'
}
