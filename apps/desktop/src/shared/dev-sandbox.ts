// `npm run dev` in a linked worktree runs a sandbox beside the main checkout's Donnabot Dev: its own
// host data (`~/.mexerica-dev-<name>`), port, app data and host process (tools/dev.mjs).

export const SANDBOX_PREFIX = '.mexerica-dev-'

/** The sandbox port range: clear of Codync (19222/19223, sandboxes 19300–19399) and Donnabot (19422/19423). */
export const SANDBOX_PORTS = { first: 19430, last: 19499 }

/** A worktree folder name made safe for a data folder and a window title. */
export const sandboxName = (folder: string) => folder.replace(/[^A-Za-z0-9._-]+/g, '-')

/** Ports for sandboxes, lowest first, without the ones other sandboxes hold. */
export function sandboxPorts(claimed: Iterable<number>): number[] {
  const taken = new Set(claimed)
  return Array.from({ length: SANDBOX_PORTS.last - SANDBOX_PORTS.first + 1 }, (_, i) => SANDBOX_PORTS.first + i).filter((port) => !taken.has(port))
}

/** `Donnabot Dev (feat-x)` for `~/.mexerica-dev-feat-x`; any other host data folder shows by its name. */
export function sandboxLabel(name: string, home: string) {
  const folder = home.split(/[\\/]/).filter(Boolean).pop() ?? home
  return `${name} (${folder.startsWith(SANDBOX_PREFIX) ? folder.slice(SANDBOX_PREFIX.length) : folder})`
}

/** Host and app environments of `npm run dev` sandboxes: the app is handed the very port the host serves on. */
export function sandboxEnvironments(base: Record<string, string | undefined>, options: { home: string; port: number; cloud: boolean; cdp: number | null }) {
  const { home, port, cloud, cdp } = options
  return {
    hostArgs: ['serve', '--port', String(port)],
    hostEnv: { ...base, MEXERICA_HOME: home, ...(cloud ? {} : { CODYNC_CLOUD: 'off' }) },
    appEnv: { ...base, MEXERICA_HOME: home, MEXERICA_PORT: String(port), ...(cdp ? { CODYNC_REMOTE_DEBUG: String(cdp) } : {}) },
  }
}
