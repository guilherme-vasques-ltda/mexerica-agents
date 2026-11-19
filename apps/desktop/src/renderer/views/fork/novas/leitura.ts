// D432 (07/11; docs/reference/design/barrinha-novas-20261107): o "lido até aqui" por aparelho e as novas da conversa. Puro.
// Fonte única: o conjunto `unseen` desta visita. A linha "Novas", as marcas em accent e o "N novas" do botão leem
// ele. A marca guardada é o createdAt da última mensagem do bot lida em sequência (prefs.chatReadMarks), sem texto.

/** `at`: createdAt do Hub (só para ordem e marca, nunca contra o relógio local); `rev`: revisão do stream. */
export interface Seen { id: string; at: number; mine: boolean; rev: number }

export const markKey = (botId: string, sessionId: string | null) => `${botId}:${sessionId ?? '-'}`
/** Teto do mapa de marcas: passado dele, saem as de leitura mais antiga. */
export const MARKS_MAX = 300

/**
 * A marca ao abrir: a guardada neste aparelho; sem ela, a principal parte do `bot.unread` do Hub (as N respostas
 * mais novas) e a extra parte de "tudo lido" (o Hub não conta não lidas por sessão).
 */
export function openMark(messages: readonly Seen[], stored: number | undefined, principal: boolean, unread: number): number {
  if (stored !== undefined) return stored
  const theirs = messages.filter((m) => !m.mine)
  if (!principal || unread <= 0) return theirs.at(-1)?.at ?? 0
  const first = theirs.at(-Math.min(unread, theirs.length))
  return first ? first.at - 1 : 0
}

/** As novas ao abrir: do bot (nunca suas) depois da marca. */
export const newIds = (messages: readonly Seen[], mark: number): ReadonlySet<string> =>
  new Set(messages.filter((m) => !m.mine && m.at > mark).map((m) => m.id))

/**
 * Chegadas depois de abrir, pela fronteira de revisão do stream, sem relógio (R2-01 do #511). Carga (cache e
 * catch-up do Hub: `rev` até o do hello do stream atual, ou antes de qualquer hello) é como a carga inicial: depois
 * da marca, é nova. Ao vivo (`rev` acima do hello) só é nova com você fora do fim; no fim, você a acompanha. As
 * suas, nunca.
 */
export const isLive = (m: Pick<Seen, 'rev'>, streamRev: number | null) => streamRev !== null && m.rev > streamRev

export function withArrivals(unseen: ReadonlySet<string>, arrived: readonly Seen[], mark: number, following: boolean, streamRev: number | null): ReadonlySet<string> {
  const add = arrived.filter((m) => !m.mine && m.at > mark && !unseen.has(m.id) && (!isLive(m, streamRev) || !following))
  return add.length ? new Set([...unseen, ...add.map((m) => m.id)]) : unseen
}

/** A visita: a marca congelada ao abrir, as novas, a linha e as mensagens já conhecidas. */
export interface Visit { mark: number; unseen: ReadonlySet<string>; line: string | null; known: ReadonlySet<string> }

export function openVisit(messages: readonly Seen[], stored: number | undefined, principal: boolean, unread: number): Visit {
  const mark = openMark(messages, stored, principal, unread)
  const unseen = newIds(messages, mark)
  return { mark, unseen, line: messages.find((m) => unseen.has(m.id))?.id ?? null, known: new Set(messages.map((m) => m.id)) }
}

/** O passo da visita quando chegam mensagens; a mesma visita quando nada é novo. */
export function nextVisit(visit: Visit, messages: readonly Seen[], following: boolean, streamRev: number | null): Visit {
  const arrived = messages.filter((m) => !visit.known.has(m.id))
  if (!arrived.length) return visit
  const unseen = withArrivals(visit.unseen, arrived, visit.mark, following, streamRev)
  return { ...visit, unseen, line: visit.line ?? messages.find((m) => unseen.has(m.id))?.id ?? null, known: new Set(messages.map((m) => m.id)) }
}

/**
 * Até onde você leu em sequência: a marca avança só sobre respostas do bot que estiveram na tela, em ordem de
 * createdAt e sem lacuna; o que foi pulado (barrinha, arraste, Ir para o fim) segue novo. Com histórico mais antigo
 * ainda por carregar depois da marca (`gapBefore`), não avança: não se sabe o que há entre ela e o carregado.
 */
export function advanceContiguous(read: number, messages: readonly Seen[], seen: ReadonlySet<string>, gapBefore: boolean): number {
  if (gapBefore) return read
  const theirs = messages.filter((m) => !m.mine && m.at > read).sort((a, b) => a.at - b.at)
  let at = read
  for (const m of theirs) {
    if (!seen.has(m.id)) break
    at = m.at
  }
  return at
}

/** Quantas novas ainda não passaram pela tela e estão abaixo dela (`below`: createdAt da última à vista). */
export const countBelow = (unseen: ReadonlySet<string>, seen: ReadonlySet<string>, messages: readonly Seen[], below: number) =>
  messages.filter((m) => unseen.has(m.id) && !seen.has(m.id) && m.at > below).length

/** A nova marca, só se avançou; o mapa fica no teto, sem as de leitura mais antiga. null quando nada muda. */
export function advanceMark(all: Readonly<Record<string, number>>, key: string, at: number, max = MARKS_MAX): Record<string, number> | null {
  if ((all[key] ?? -1) >= at) return null
  const next = Object.entries({ ...all, [key]: at }).sort((a, b) => b[1] - a[1]).slice(0, max)
  return Object.fromEntries(next)
}

/** Tira as marcas das sessões extras deste bot que não estão mais abertas (arquivadas ou apagadas). */
export function forgetSessions(all: Readonly<Record<string, number>>, botId: string, open: ReadonlySet<string>): Record<string, number> | null {
  const gone = Object.keys(all).filter((k) => k.startsWith(`${botId}:`) && k !== markKey(botId, null) && !open.has(k.slice(botId.length + 1)))
  if (!gone.length) return null
  const next = { ...all }
  for (const k of gone) delete next[k]
  return next
}

/** Grava a marca durante a visita, no máximo uma vez a cada `ms` (e já ao sair ou fechar). */
export function markSaver(write: (at: number) => void, ms: number, timers: { set: (fn: () => void, ms: number) => unknown; clear: (t: unknown) => void } = { set: (fn, ms) => setTimeout(fn, ms), clear: (t) => clearTimeout(t as ReturnType<typeof setTimeout>) }) {
  let pending: number | null = null
  let timer: unknown = null
  const flush = () => {
    if (timer !== null) timers.clear(timer)
    timer = null
    if (pending !== null) write(pending)
    pending = null
  }
  return {
    push(at: number) {
      pending = at
      if (timer === null) timer = timers.set(flush, ms)
    },
    flush,
  }
}

/** Ao abrir: as novas que não cabem numa tela abrem na linha; senão, no fim (com a linha à vista). */
export const openAt = (fromLineToEnd: number, viewport: number): 'line' | 'end' => (fromLineToEnd > viewport ? 'line' : 'end')

/**
 * O que a lista faz quando o conteúdo muda: guardar o lugar ao carregar antigas, descer quando você segue o fim; fora
 * do fim, nunca pula (a pílula mostra as novas).
 */
export function afterChange(following: boolean, keepingPlace: boolean): 'restore' | 'bottom' | 'stay' {
  if (keepingPlace && !following) return 'restore'
  return following ? 'bottom' : 'stay'
}

/** A pílula: só fora do fim; com novas abaixo, a contagem. */
export const jumpPill = (following: boolean, hasItems: boolean, below: number) => ({ shown: !following && hasItems, count: following ? 0 : below })
