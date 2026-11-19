// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { ptBR } from './fork-i18n/pt-BR.ts'
import { useSyncExternalStore } from 'react'
import { isSystemDark, resolvedDark, useResolvedDark } from './appearance'

/**
 * A media query read from one MediaQueryList and listened to on another: Chromium shares a list's
 * `matches` with its change detection, so a read while the window is hidden can swallow the `change`
 * and memoized canvases (avatars) keep the old appearance's ink. Showing or focusing the window
 * checks again, so a missed change never outlives the next look at the window.
 */
function mediaQuery(media: string) {
  const reads = window.matchMedia(media)
  const events = window.matchMedia(media)
  return {
    get: () => reads.matches,
    subscribe: (cb: () => void) => {
      events.addEventListener('change', cb)
      document.addEventListener('visibilitychange', cb)
      window.addEventListener('focus', cb)
      return () => {
        events.removeEventListener('change', cb)
        document.removeEventListener('visibilitychange', cb)
        window.removeEventListener('focus', cb)
      }
    },
  }
}

const motionQuery = mediaQuery('(prefers-reduced-motion: reduce)')

/**
 * The screen's pixel ratio, for canvases' backing size. It changes when the window moves to another
 * screen or the Mac sleeps with the lid closed; a canvas drawn meanwhile would stay blurry at 1x.
 * The query names the current ratio, so it is renewed on every change.
 */
const pixelRatio = {
  get: () => window.devicePixelRatio || 1,
  subscribe: (cb: () => void) => {
    let query: MediaQueryList | null = null
    const changed = () => {
      watch()
      cb()
    }
    const watch = () => {
      query?.removeEventListener('change', changed)
      query = window.matchMedia(`(resolution: ${pixelRatio.get()}dppx)`)
      query.addEventListener('change', changed)
    }
    watch()
    document.addEventListener('visibilitychange', changed)
    window.addEventListener('focus', changed)
    return () => {
      query?.removeEventListener('change', changed)
      document.removeEventListener('visibilitychange', changed)
      window.removeEventListener('focus', changed)
    }
  },
}

/** The app's theme: Aparência's choice, or the Mac's with Sistema (lib/appearance.ts). */
export function useDark() {
  return useResolvedDark()
}

export function useReduceMotion() {
  return useSyncExternalStore(motionQuery.subscribe, motionQuery.get)
}

export function usePixelRatio() {
  return useSyncExternalStore(pixelRatio.subscribe, pixelRatio.get)
}

export const isDark = resolvedDark
/** The Mac's own appearance, for what the system draws (the menu bar's icon), whatever the app's theme. */
export { isSystemDark }
export const reduceMotion = motionQuery.get

/** Palette values for drawing code (canvas) that can't read CSS variables; they match tokens.css (`ink`). */
export const palette = (dark: boolean) => ({
  text: dark ? [244, 243, 241] : [23, 22, 26],
  warning: '#F0A030',
  danger: dark ? '#F0A7A7' : '#C23A2B',
})

export interface Swatch {
  id: string
  label: string
  hex: string
}

export const AVATAR_COLORS: Swatch[] = [
  { id: 'black', label: ptBR['ajustes.corAvatar.black'], hex: '#2B2B2B' },
  { id: 'brown', label: ptBR['ajustes.corAvatar.brown'], hex: '#936439' },
  { id: 'red', label: ptBR['ajustes.corAvatar.red'], hex: '#FF263C' },
  { id: 'orange', label: ptBR['ajustes.corAvatar.orange'], hex: '#FF6700' },
  { id: 'yellow', label: ptBR['ajustes.corAvatar.yellow'], hex: '#FF9800' },
  { id: 'green', label: ptBR['ajustes.corAvatar.green'], hex: '#00C972' },
  { id: 'cyan', label: ptBR['ajustes.corAvatar.cyan'], hex: '#00BCA6' },
  { id: 'blue', label: ptBR['ajustes.corAvatar.blue'], hex: '#1084FE' },
  { id: 'violet', label: ptBR['ajustes.corAvatar.violet'], hex: '#9159FE' },
  { id: 'magenta', label: ptBR['ajustes.corAvatar.magenta'], hex: '#FF309B' },
  { id: 'gray', label: ptBR['ajustes.corAvatar.gray'], hex: '#777777' },
]

export const AVATAR_SHAPES = ['blob', 'pebble', 'squircle', 'tablet', 'wedge', 'hex', 'cloud', 'teardrop']

export function avatarColor(id: string) {
  return (AVATAR_COLORS.find((c) => c.id === id) ?? AVATAR_COLORS[7]!).hex
}

/** `#RRGGBB` → `rgba(r, g, b, a)`. */
export function withAlpha(hex: string, alpha: number) {
  const n = parseInt(hex.slice(1), 16)
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`
}
