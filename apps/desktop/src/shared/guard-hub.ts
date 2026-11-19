/** 22.1: informação declarada pelo app, sem prova de pessoa nem autorização. Offsets UTF-16. */
export type OriginKind = 'typed' | 'pasted' | 'quote' | 'attachment' | 'voice' | 'generated' | 'unknown'
export interface OriginSegment { start: number; end: number; kind: OriginKind }
export interface OriginDraft { schema: 1; draftId: string; revision: number; textLength: number; segments: OriginSegment[] }
export interface OriginReport extends OriginDraft {
  channel: 'desktopComposer' | 'desktopVoice'; deviceId: string; interaction: 'send'
}
export interface ContentOrigin {
  schema: 1; mode: 'observe'; status: 'appReported' | 'unknown'; report: OriginReport | null
  ingress?: { caller: 'local' } | { caller: 'device'; device: string } | null
  segments: OriginSegment[]; attachments: { id: string; kind: 'attachment' }[]; reply: { id: string; kind: 'quote' } | null
}

export function originDraft(text: string, kind: OriginKind = 'unknown'): OriginDraft {
  return { schema: 1, draftId: crypto.randomUUID(), revision: 1, textLength: text.length,
    segments: text ? [{ start: 0, end: text.length, kind }] : [] }
}

export function validOrigin(value: unknown, text: string): value is OriginDraft {
  if (!value || typeof value !== 'object') return false
  const v = value as OriginDraft
  if (v.schema !== 1 || typeof v.draftId !== 'string' || !/^[0-9a-f-]{36}$/i.test(v.draftId) || !Number.isSafeInteger(v.revision) || v.revision < 1 || v.textLength !== text.length || !Array.isArray(v.segments) || v.segments.length > 4096) return false
  let cursor = 0
  for (const s of v.segments) {
    if (!s || s.start !== cursor || !Number.isInteger(s.end) || s.end <= cursor || s.end > text.length || !['typed', 'pasted', 'quote', 'voice', 'generated', 'unknown'].includes(s.kind)) return false
    // Não partir um par substituto no meio.
    if (s.end < text.length && /[\uD800-\uDBFF]/.test(text[s.end - 1]!) && /[\uDC00-\uDFFF]/.test(text[s.end]!)) return false
    cursor = s.end
  }
  return cursor === text.length
}

/** Apenas rebaixa citações reconhecíveis; nunca promove texto a instrução. */
function quotes(text: string, segments: OriginSegment[]): OriginSegment[] {
  let closeAt = -1, lineQuote = false, cursor = 0
  let segmentStart = -1, segmentText = ''
  const closings = new Map<string, number>()
  const result: OriginSegment[] = []
  const internalApostrophe = (at: number) => (text[at] === "'" || text[at] === '‘' || text[at] === '’')
    && /[\p{L}\p{N}]$/u.test(text.slice(Math.max(0, at - 2), at))
    && /^[\p{L}\p{N}]/u.test(text.slice(at + 1, at + 3))
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!
    while (segments[cursor] && segments[cursor]!.end <= i) { cursor++; closeAt = -1 }
    const segment = segments[cursor]!
    if (segmentStart !== segment.start) {
      segmentStart = segment.start
      segmentText = text.slice(segment.start, segment.end)
      closings.clear()
    }
    if (i === 0 || text[i - 1] === '\n') lineQuote = /^\s*>/.test(text.slice(i).split('\n', 1)[0]!)
    const apostrophe = c === "'" || c === '‘' || c === '’'
    const straight = c === '"' || c === '`'
    const wordBefore = (apostrophe || straight) && /[\p{L}\p{N}]$/u.test(text.slice(Math.max(0, i - 2), i))
    const opens = apostrophe ? !wordBefore && i + 1 < text.length && !/\s/u.test(text[i + 1]!) : straight ? !wordBefore : c === '“'
    if (closeAt < i && opens && !lineQuote) {
      const close = c === '“' ? '”' : c === '‘' ? '’' : c
      let at = closings.get(close)
      if (at === undefined || (at >= 0 && at <= i)) {
        // Cada busca avança no segmento; -1 evita repetir a varredura sem fechamento.
        let offset = segmentText.indexOf(close, i + 1 - segmentStart)
        while (offset >= 0 && internalApostrophe(offset + segmentStart)) offset = segmentText.indexOf(close, offset + 1)
        at = offset < 0 ? -1 : offset + segmentStart
        closings.set(close, at)
      }
      // Uma aspa solta (ou fechamento em outro segmento) não rebaixa o restante.
      if (at >= 0) closeAt = at
    }
    const quoted = closeAt >= i || lineQuote
    const source = segments[cursor]?.kind ?? 'unknown'
    const kind = (source === 'typed' || source === 'voice') && quoted ? 'quote' : source
    const last = result.at(-1)
    if (last?.kind === kind) last.end++
    else result.push({ start: i, end: i + 1, kind })
  }
  return result
}

/** Recorta junto com trim, mantendo a revisão original e os offsets do texto enviado. */
export function originReport(text: string, origin: OriginDraft | undefined, deviceId: string, channel: OriginReport['channel'] = 'desktopComposer'): OriginReport {
  const draft = validOrigin(origin, text) ? origin : originDraft(text)
  const start = text.length - text.trimStart().length, end = text.trimEnd().length
  const segments = draft.segments.flatMap((s) => {
    const a = Math.max(start, s.start), b = Math.min(end, s.end)
    return a < b ? [{ start: a - start, end: b - start, kind: s.kind }] : []
  })
  return { ...draft, textLength: text.trim().length, segments: quotes(text.trim(), segments), deviceId, channel, interaction: 'send' }
}

/** Ponte legada: digitação e ditado ao vivo são fala do dono; outras origens são externas. */
export function originPasted(text: string, origin: OriginDraft | undefined, pasted: readonly string[] = []): string[] {
  const segments = validOrigin(origin, text) ? origin.segments : originDraft(text).segments
  const external = segments.filter((s) => s.kind !== 'typed' && s.kind !== 'voice')
  const values = [...new Set([...external.map((s) => text.slice(s.start, s.end)).sort((a, b) => b.length - a.length), ...pasted])].filter(Boolean)
  // Simula evidence::typed, acompanhando quais caracteres externos sobrevivem.
  // CRLF vira LF; a marca da quebra normalizada conserva qualquer origem externa.
  const marks = segments.map((s) => (s.kind === 'typed' || s.kind === 'voice' ? 't' : 'e').repeat(s.end - s.start)).join('')
  let from = 0
  const normalized: string[] = []
  let remaining = text.replace(/\r\n/g, (_match, at: number) => {
    normalized.push(marks.slice(from, at), marks.slice(at, at + 2).includes('e') ? 'e' : 't')
    from = at + 2
    return '\n'
  })
  let surviving = normalized.join('') + marks.slice(from)
  for (const value of values) {
    // Mesmo trim de str::trim no Hub: inclui U+0085 e preserva U+FEFF.
    const trimmed = value.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '')
    if (!trimmed || !text.includes(trimmed)) continue
    const needle = trimmed.replace(/\r\n/g, '\n')
    from = 0
    const next: string[] = []
    remaining = remaining.replaceAll(needle, (_match, at: number) => {
      next.push(surviving.slice(from, at), 'ttt')
      from = at + needle.length
      return '\n\u0001\n'
    })
    surviving = next.join('') + surviving.slice(from)
  }
  for (let at = surviving.indexOf('e'); at >= 0; at = surviving.indexOf('e', at + 1)) {
    if (!/\p{White_Space}/u.test(remaining[at]!)) return [...new Set([text, ...values])]
  }
  return values
}

/** Identifica a instalação emissora, não o Hub de destino; não atesta a pessoa. */
export function originDevice(storage: Pick<Storage, 'getItem' | 'setItem'> = localStorage): string {
  const key = 'donnabot-origin-device-v1'
  try {
    const saved = storage.getItem(key)
    if (saved && /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(saved)) return saved
    const id = crypto.randomUUID()
    storage.setItem(key, id)
    return id
  } catch { return crypto.randomUUID() }
}
