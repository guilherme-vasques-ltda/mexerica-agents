import type { Point } from './board-layout.ts'

// Preferências do Quadro neste aparelho, por Hub e space: zoom, deslocamento, Travar, Rotas, itens recolhidos e a
// disposição arrastada. Não vão ao Hub e não são permissão nem pausa: Travar só impede arrastar no quadro.

export interface BoardPrefs {
  zoom: number | null
  pan: Point | null
  locked: boolean
  showRoutes: boolean
  collapsed: string[]
  offsets: Record<string, Point>
}

export const DEFAULT_BOARD_PREFS: BoardPrefs = { zoom: null, pan: null, locked: false, showRoutes: true, collapsed: [], offsets: {} }

/** O mínimo de `Storage` usado aqui (localStorage no app; um mapa nos testes). */
export interface PrefsStorage { getItem(key: string): string | null; setItem(key: string, value: string): void }

export const boardPrefsKey = (hostId: string, spaceId: string) => `donnaTimesBoard.v1:${hostId}:${spaceId}`

const isPoint = (v: unknown): v is Point => !!v && typeof v === 'object' && Number.isFinite((v as Point).x) && Number.isFinite((v as Point).y)

/** Lê o que estiver guardado; qualquer campo inválido volta ao padrão, sem perder os outros. */
export function readBoardPrefs(storage: PrefsStorage | null, key: string): BoardPrefs {
  let raw: unknown = null
  try {
    const text = storage?.getItem(key)
    raw = text ? JSON.parse(text) : null
  } catch {
    raw = null
  }
  if (!raw || typeof raw !== 'object') return { ...DEFAULT_BOARD_PREFS, collapsed: [], offsets: {} }
  const v = raw as Record<string, unknown>
  const offsets: Record<string, Point> = {}
  if (v.offsets && typeof v.offsets === 'object') {
    for (const [id, p] of Object.entries(v.offsets as Record<string, unknown>).slice(0, 1024)) if (isPoint(p)) offsets[id] = { x: p.x, y: p.y }
  }
  return {
    zoom: typeof v.zoom === 'number' && Number.isFinite(v.zoom) ? v.zoom : null,
    pan: isPoint(v.pan) ? { x: v.pan.x, y: v.pan.y } : null,
    locked: v.locked === true,
    showRoutes: v.showRoutes !== false,
    collapsed: Array.isArray(v.collapsed) ? v.collapsed.filter((k): k is string => typeof k === 'string').slice(0, 1024) : [],
    offsets,
  }
}

export function writeBoardPrefs(storage: PrefsStorage | null, key: string, prefs: BoardPrefs) {
  try {
    storage?.setItem(key, JSON.stringify(prefs))
  } catch {}
}
