// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { BotAvatarConfig } from './fork-avatar-config'
import type { AvatarMoodState } from './fork-avatar-mood'
import type { BotStatus, UsageProvider } from './models'

// The notch island: a black shape at the top of the screen (in the notch on a
// MacBook) that shows working bots and takes approvals. The chat window owns the stores and
// sends an `IslandSummary`; the island window only draws it (main/island.ts, views/island/).

/** One tool call in a bot's ticker. */
export interface IslandStep {
  /** O texto exibido (MX-141: verbo em linguagem simples no idioma do app). */
  title: string
  /** O título do motor como veio, para o hover; ausente quando não havia título. */
  original?: string
  /** The ACP tool kind: execute | edit | read | search | fetch | … */
  kind: string
}

/** One bot as the island shows it (`ref` = computerId/botId). */
export interface IslandBot {
  ref: string
  name: string
  shape: string
  avatarConfig?: BotAvatarConfig
  avatarMood?: AvatarMoodState
  avatarLabel?: string
  /** Cor hexadecimal do avatar normalizado. */
  color: string
  /** A `BotStatus`, or a newer one this app doesn't know. */
  status: BotStatus | string
  /** MB-1b: turno em andamento (`isWorking`); Precisa de você só por pergunta, sem turno, não é trabalho. */
  working: boolean
  /** The agent it runs, as the app names it ("Claude Code"). */
  backend: string
  /** Its working folder's name. */
  folder: string
  /** What it's doing, that it needs you, or its last message. */
  detail: string
  /** Its latest tool calls, oldest first (only while it works). */
  steps: IslandStep[]
  /** Why it stopped, when it failed. */
  error: string | null
  /** Duração do turno congelada por `observeTurns` no instante da transição (ms); null/ausente = desconhecida. */
  duracaoMs?: number | null
  /**
   * Visibilidade da conversa no instante da transição, em três estados: `true` à vista, `false`
   * fora da vista provado, `null`/ausente = desconhecido. O fail-safe do "Terminou" só notifica com `false`.
   */
  watching?: boolean | null
}

export interface IslandOption {
  id: string
  label: string
  /** allow_once | allow_always | reject_once | reject_always, or another the agent sent. */
  kind: string
}

/** A pending approval card: the conversation holding it and the bot asking. */
export interface IslandPermission {
  ref: string
  entryId: string
  bot: Pick<IslandBot, 'name' | 'shape' | 'color' | 'backend' | 'avatarConfig' | 'avatarMood' | 'avatarLabel'>
  headline: string
  title: string
  options: IslandOption[]
  /** The option on its way to the host. */
  answering: string | null
}

export interface IslandSummary {
  bots: IslandBot[]
  permissions: IslandPermission[]
  /** This computer's plan limits (Claude, Codex), as the menu bar shows them. */
  usage: UsageProvider[]
}

/** The display the island sits on: the notch's size in points, or null at a plain top edge. */
export interface IslandScreen {
  notch: { width: number; height: number } | null
}

/** The island's shape in its window, in points: the part that takes the mouse. */
export interface IslandRect {
  x: number
  y: number
  width: number
  height: number
}

/** One `NSScreen`: its frame's x and width, the safe area's top inset and the menu bar on each side of the notch. */
export interface NotchScreen {
  x: number
  width: number
  top: number
  left: number
  right: number
}

/** Closed shapes, then the open views (640 pt wide, under a header of tabs). */
export type IslandMode = 'hidden' | 'peek' | 'compact' | 'overview' | 'empty' | 'approval' | 'finished' | 'failed' | 'message' | 'usage'
/** The header's choice: `auto` shows what needs you first (an approval, a finished or failed bot). */
export type IslandTab = 'auto' | 'bots' | 'message' | 'usage'
/** A bot that just stopped, shown until it times out (done) or is dismissed (failed). */
export interface IslandNotice {
  kind: 'finished' | 'failed'
  ref: string
}

/** How long a bot that just finished stays in the island. */
export const FINISHED_MS = 5200
/**
 * MX-251: o banner "Terminou" só fura a tela por turno longo; resposta curta de conversa em
 * andamento não interrompe (decisão do dono: notificação a cada mensagem é spam).
 */
export const LONG_TURN_MS = 60_000
/** Extra room around the shape that still counts as on it, so a click at its edge is never lost. */
export const HIT_MARGIN = 8
/** The window holding the island: the largest shape fits, the rest is transparent and lets clicks through. */
export const PANEL = { width: 720, height: 300 }
/** The open island's width. */
export const OPEN_WIDTH = 640

/**
 * Every open view has the same height (for a 32 pt notch; a taller one adds its extra), so
 * switching tabs or alerts never moves the black shape: only what's inside changes.
 */
const OPEN_HEIGHT = 196
/** The resting bar at a plain top edge (no notch). */
const BAR = 26

export const isActive = (b: Pick<IslandBot, 'status'>) => b.status === 'working' || b.status === 'needsInput'
/** O "N trabalhando" da ilha: só turnos em andamento; a pergunta sem turno fica no Precisa de você. */
export const workingCount = (bots: readonly Pick<IslandBot, 'working'>[]) => bots.filter((b) => b.working).length
export const isOpenMode = (mode: IslandMode) => mode !== 'hidden' && mode !== 'peek' && mode !== 'compact'

export function islandMode(
  summary: IslandSummary,
  { open, peek, tab, deferred, notice }: { open: boolean; peek: boolean; tab: IslandTab; deferred: ReadonlySet<string>; notice: IslandNotice | null },
): IslandMode {
  if (!open) {
    // Alerts open the island by themselves.
    if (summary.permissions.some((p) => !deferred.has(p.entryId))) return 'approval'
    if (notice) return notice.kind
    if (peek) return 'peek'
    return summary.bots.some(isActive) ? 'compact' : 'hidden'
  }
  if (tab === 'message' || tab === 'usage') return tab
  if (tab === 'auto') {
    if (summary.permissions.length) return 'approval'
    if (notice) return notice.kind
  }
  return summary.bots.some(isActive) ? 'overview' : 'empty'
}

/**
 * Bots that just stopped working since `before` (their last statuses by ref): done or failed. A runtime that turns
 * `unavailable` (runtime-audiencia-01) is neither, and what follows it never counts as an end of that work.
 */
export function transitions(before: ReadonlyMap<string, string>, bots: IslandBot[]): IslandNotice[] {
  return bots.flatMap((b): IslandNotice[] => {
    const was = before.get(b.ref)
    if (was !== 'working' && was !== 'needsInput') return []
    if (b.status === 'idle') {
      // MX-251: "terminou" só interrompe quando é CERTO que o dono não estava olhando a conversa
      // (watching === false) E o turno foi longo. `duracaoMs`/`watching` chegam congelados por
      // `observeTurns` do instante da transição; qualquer dúvida (visibilidade desconhecida — null,
      // duração desconhecida, sem working observado) o fail-safe é não notificar.
      if (b.watching !== false || b.duracaoMs == null || b.duracaoMs < LONG_TURN_MS) return []
      return [{ kind: 'finished', ref: b.ref }]
    }
    if (b.status === 'error') return [{ kind: 'failed', ref: b.ref }]
    return []
  })
}

/** Um `startedAt` em epoch ms: finito, >= 0 e não futuro; qualquer outro valor é duração desconhecida. */
const validStart = (v: unknown, agora: number): number | null =>
  typeof v === 'number' && Number.isFinite(v) && v >= 0 && v <= agora ? v : null

const ativo = (status: string) => status === 'working' || status === 'needsInput'

/**
 * MX-251 (revisão): converte os sinais crus de visibilidade da janela para o tri-estado "à
 * vista?". `true` só com foco E aba visível provados; `false` só com fora da vista provado
 * (sem foco, aba oculta ou em pré-renderização); `null` para tudo fora do conjunto conhecido —
 * `hasFocus` indisponível ou não booleano, `visibilityState` ausente/undefined/não suportado.
 * Desconhecido nunca vira "fora da vista": sem prova, sem "Terminou".
 */
export function janelaVista(foco: unknown, visibilidade: unknown): boolean | null {
  if (foco === false || visibilidade === 'hidden' || visibilidade === 'prerender') return false
  return foco === true && visibilidade === 'visible' ? true : null
}

/**
 * O que a janela de conversa guarda de cada bot entre resumos. O Hub limpa `started_at` na mesma
 * atualização em que o bot fica idle, então a duração do turno só existe medida pela app: âncora
 * monotônica local (`performance.now`) na 1ª observação do turno, mais o decorrido que o Hub já
 * reportava naquele instante (`Date.now() - startedAt`). Na saída de atividade, `duracaoMs` e
 * `watching` são congelados — trocar de conversa ou focar a janela depois não muda o registro.
 */
export interface TurnSeen {
  status: string
  /** `performance.now()` da 1ª observação do turno; null quando o turno nunca foi visto ativo. */
  ancora: number | null
  /** `Date.now() - startedAt` válido na 1ª observação; null = duração desconhecida. */
  elapsed: number | null
  /** Duração congelada na transição (`elapsed` + monotônico decorrido); null = desconhecida. */
  duracaoMs: number | null
  /** Visibilidade tri-estado: true à vista, false fora da vista provado, null desconhecido. */
  watching: boolean | null
  /** Já saiu de working/needsInput: `duracaoMs` e `watching` ficam congelados até o próximo turno. */
  ended: boolean
}

/**
 * Atualiza `seen` com uma varredura dos bots, feita na hora em que a mudança é observada na fonte
 * (não quando o resumo agregado sai 150 ms depois). Relógios injetáveis para teste.
 */
export function observeTurns(
  seen: Map<string, TurnSeen>,
  bots: readonly { ref: string; status: string; startedAt?: number | null; watching?: boolean | null }[],
  relogios: { date?: () => number; perf?: () => number } = {},
): void {
  const dateNow = relogios.date ?? (() => Date.now())
  const perfNow = relogios.perf ?? (() => performance.now())
  const alive = new Set(bots.map((b) => b.ref))
  for (const ref of seen.keys()) if (!alive.has(ref)) seen.delete(ref)
  for (const b of bots) {
    const prev = seen.get(b.ref)
    if (ativo(b.status)) {
      if (prev && !prev.ended) {
        // Mesmo turno: âncora e elapsed são da 1ª observação; só a visibilidade acompanha.
        seen.set(b.ref, { ...prev, status: b.status, watching: b.watching ?? null })
      } else {
        const start = validStart(b.startedAt, dateNow())
        seen.set(b.ref, {
          status: b.status,
          ancora: perfNow(),
          elapsed: start === null ? null : dateNow() - start,
          duracaoMs: null,
          watching: b.watching ?? null,
          ended: false,
        })
      }
    } else if (prev && !prev.ended) {
      // Transição observada: congela duração e visibilidade DESTE instante.
      const duracaoMs = prev.elapsed === null || prev.ancora === null ? null : prev.elapsed + (perfNow() - prev.ancora)
      seen.set(b.ref, { status: b.status, ancora: null, elapsed: null, duracaoMs, watching: b.watching ?? null, ended: true })
    } else {
      // Sem working observado (app recém-aberta, Hub antigo): duração desconhecida, registro congelado.
      seen.set(b.ref, {
        status: b.status,
        ancora: null,
        elapsed: null,
        duracaoMs: prev?.duracaoMs ?? null,
        watching: prev ? prev.watching : (b.watching ?? null),
        ended: true,
      })
    }
  }
}

/** The bot in the big slot: one that needs you, else one working, else the first. */
export function focusOf(bots: IslandBot[]) {
  return bots.find((b) => b.status === 'needsInput') ?? bots.find(isActive) ?? bots[0] ?? null
}

/** The other bots, working and waiting first, at most `limit`. */
export function othersOf(bots: IslandBot[], focus: IslandBot | null, limit: number) {
  const rest = bots.filter((b) => b.ref !== focus?.ref)
  return [...rest.filter(isActive), ...rest.filter((b) => !isActive(b))].slice(0, limit)
}

/** The header's height: a taller notch pushes the content down so nothing sits behind the camera. */
export const contentTop = (screen: IslandScreen) => 36 + Math.max(0, (screen.notch?.height ?? 32) - 32)

/** The shape's size and bottom corner radius for `mode`. */
export function islandFrame(mode: IslandMode, screen: IslandScreen) {
  const notch = screen.notch
  const bar = notch?.height ?? BAR
  const closedRadius = notch ? 14 : bar / 2
  switch (mode) {
    case 'hidden':
      // At a plain top edge only a thin strip under the edge wakes it.
      return notch ? { width: notch.width, height: notch.height, radius: closedRadius } : { width: 160, height: 0, radius: 0 }
    case 'peek':
      return { width: notch ? notch.width + 64 : 120, height: bar, radius: closedRadius }
    case 'compact':
      return { width: notch ? notch.width + 104 : 240, height: bar, radius: closedRadius }
    default:
      return { width: OPEN_WIDTH, height: OPEN_HEIGHT + contentTop(screen) - 36, radius: 30 }
  }
}

/** The shape centered at the top of a `windowWidth`-wide window. */
export function islandRect(frame: { width: number; height: number }, windowWidth: number): IslandRect {
  return { x: (windowWidth - frame.width) / 2, y: 0, width: frame.width, height: frame.height }
}

/** The part of the window that takes the mouse: the shape with `HIT_MARGIN` around it, inside the window. */
export function hitRect(rect: IslandRect): IslandRect {
  if (rect.width <= 0) return { x: 0, y: 0, width: 0, height: 0 }
  const x = Math.max(0, Math.floor(rect.x - HIT_MARGIN))
  const y = Math.max(0, Math.floor(rect.y - HIT_MARGIN))
  return { x, y, width: Math.ceil(rect.x + rect.width + HIT_MARGIN) - x, height: Math.ceil(rect.y + rect.height + HIT_MARGIN) - y }
}

/** Whether a point in window coordinates is on the island. The window's top edge is the screen's, so above it counts too. */
export function hits(rect: IslandRect, x: number, y: number) {
  const r = hitRect(rect)
  return r.width > 0 && x >= r.x && x <= r.x + r.width && y <= r.y + r.height && (r.y === 0 || y >= r.y)
}

/** The notch of the display at `bounds`, from the screens `NSScreen` reports (same points and x axis). */
export function notchOf(screens: NotchScreen[], bounds: { x: number; width: number }) {
  const s = screens.find((s) => s.x === bounds.x && s.width === bounds.width && s.top > 0)
  const width = s ? s.width - s.left - s.right : 0
  return s && width > 0 ? { width, height: s.top } : null
}

/** The setting's name: a notch on the Mac, the top edge elsewhere. */
/** D454 (fatia 10b): a chave do rótulo; o menu da barra traduz no idioma do app. */
export const islandLabel = (platform: string): 'sistema.barra.mostrarEntalhe' | 'sistema.barra.mostrarTopo' => (platform === 'darwin' ? 'sistema.barra.mostrarEntalhe' : 'sistema.barra.mostrarTopo')
