import { originDraft, validOrigin, type OriginDraft, type OriginKind, type OriginSegment } from './guard-hub.ts'

export interface EditRange { start: number; end: number }

/** Menor substituição entre os textos, sem partir pares substitutos. */
function changedRange(before: string, after: string): EditRange {
  let start = 0, end = before.length, afterEnd = after.length
  while (start < Math.min(end, afterEnd) && before[start] === after[start]) start++
  if (start > 0 && /[\uD800-\uDBFF]/.test(before[start - 1]!) && /[\uDC00-\uDFFF]/.test(before[start] ?? after[start] ?? '')) start--
  while (end > start && afterEnd > start && before[end - 1] === after[afterEnd - 1]) { end--; afterEnd-- }
  if (end < before.length && /[\uDC00-\uDFFF]/.test(before[end]!) && /[\uD800-\uDBFF]/.test(before[end - 1] ?? '')) end++
  return { start, end }
}

function matchesRange(before: string, after: string, { start, end }: EditRange): boolean {
  const added = after.length - before.length + end - start
  return Number.isInteger(start) && Number.isInteger(end) && start >= 0 && end >= start && end <= before.length && added >= 0
    && before.slice(0, start) === after.slice(0, start) && before.slice(end) === after.slice(start + added)
}

/** Seleção anterior ao input; IME substitui a composição desde seu início, não desde o caret final. */
export function inputRange(before: string, after: string, inputType: string, range?: EditRange, composition?: EditRange): EditRange {
  // Undo/redo não ressuscita evidência antiga de digitação.
  if (inputType.startsWith('history')) return { start: 0, end: before.length }
  if (inputType.startsWith('delete')) {
    const removed = before.length - after.length
    const anchored = range && (range.start !== range.end ? range
      : inputType.endsWith('Backward') ? { start: range.start - removed, end: range.start }
      : inputType.endsWith('Forward') ? { start: range.start, end: range.start + removed } : undefined)
    return anchored && matchesRange(before, after, anchored) ? anchored : changedRange(before, after)
  }
  const declared = composition ?? range
  return declared && matchesRange(before, after, declared) ? declared : changedRange(before, after)
}

/** Uma substituição ancorada na seleção real; repetição de palavras não troca sua origem. */
export function editOrigin(before: string, after: string, origin: OriginDraft | undefined, kind: OriginKind, range?: EditRange): OriginDraft {
  const old = validOrigin(origin, before) ? origin : originDraft(before)
  const { start, end } = range && matchesRange(before, after, range) ? range : changedRange(before, after)
  const added = after.length - before.length + end - start
  const delta = added - (end - start)
  const segments: OriginSegment[] = old.segments.flatMap((s) => [
    ...(s.start < start ? [{ ...s, end: Math.min(start, s.end) }] : []),
    ...(s.end > end ? [{ ...s, start: Math.max(end, s.start) + delta, end: s.end + delta }] : []),
  ])
  if (added) segments.push({ start, end: start + added, kind })
  segments.sort((a, b) => a.start - b.start)
  const merged: OriginSegment[] = []
  for (const s of segments) {
    const last = merged.at(-1)
    if (last?.end === s.start && last.kind === s.kind) last.end = s.end
    else merged.push(s)
  }
  const result: OriginDraft = { ...old, revision: old.revision + 1, textLength: after.length, segments: merged }
  return validOrigin(result, after) ? result : { ...originDraft(after), draftId: old.draftId, revision: old.revision + 1 }
}

/** Sem beforeinput confiável (ou undo/redo), o trecho alterado permanece desconhecido. */
export function inputOrigin(inputType: string, trusted: boolean): OriginKind {
  if (!trusted) return 'unknown'
  if (['insertText', 'insertLineBreak', 'insertParagraph', 'insertCompositionText', 'insertFromComposition'].includes(inputType)) return 'typed'
  if (['insertFromPaste', 'insertFromDrop', 'insertFromYank'].includes(inputType)) return 'pasted'
  return 'unknown'
}

/** Estado dos eventos nativos, compartilhado pelo compositor e pela caixa rápida. */
export class ComposerInput {
  composing = false
  private composition: EditRange | undefined
  private pending: { kind: OriginKind; type: string; range: EditRange } | undefined

  before(type: string, trusted: boolean, range: EditRange, isComposing = false) {
    if (!this.composing && !isComposing && !type.includes('Composition')) this.composition = undefined
    this.pending = { kind: inputOrigin(type, trusted), type, range }
  }

  startComposition(range: EditRange) {
    this.composing = true
    this.composition = range
    this.pending = undefined
  }

  endComposition() { this.composing = false }

  change(before: string, after: string) {
    const edit = this.pending
    this.pending = undefined
    const anchor = this.composing || edit?.type.includes('Composition') ? this.composition : undefined
    const range = inputRange(before, after, edit?.type ?? '', edit?.range, anchor)
    if (anchor) this.composition = { start: range.start, end: range.end + after.length - before.length }
    return { kind: edit?.kind ?? 'unknown', range }
  }
}
