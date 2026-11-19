// MX-142: preferências LOCAIS deste aparelho na área Tarefas (nada vai ao Hub): a vista (Lista padrão ou Quadro) e o
// último Projeto usado na Nova tarefa, por Space. Só ids e um enum: nenhum conteúdo de tarefa fica em disco.

export type TaskViewMode = 'list' | 'board'

// Nome do item no localStorage deste aparelho (identificador público, não é segredo).
const PREFS_ITEM = 'mexerica.tarefas.v1'

interface Prefs { view: Record<string, TaskViewMode>; lastProject: Record<string, string> }

function load(): Prefs {
  try {
    const raw = globalThis.localStorage?.getItem(PREFS_ITEM)
    const v = raw ? JSON.parse(raw) as Partial<Prefs> : {}
    return { view: typeof v.view === 'object' && v.view ? v.view : {}, lastProject: typeof v.lastProject === 'object' && v.lastProject ? v.lastProject : {} }
  } catch {
    return { view: {}, lastProject: {} }
  }
}

function save(p: Prefs) {
  try { globalThis.localStorage?.setItem(PREFS_ITEM, JSON.stringify(p)) } catch { /* sem armazenamento: vale só nesta sessão */ }
}

export function viewModeFor(spaceId: string): TaskViewMode {
  return load().view[spaceId] === 'board' ? 'board' : 'list'
}

export function rememberViewMode(spaceId: string, mode: TaskViewMode) {
  const p = load()
  p.view[spaceId] = mode
  save(p)
}

export function lastProjectFor(spaceId: string): string | null {
  const id = load().lastProject[spaceId]
  return typeof id === 'string' ? id : null
}

export function rememberLastProject(spaceId: string, projectId: string) {
  const p = load()
  p.lastProject[spaceId] = projectId
  save(p)
}
