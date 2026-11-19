import type { Entry, MemoryOrigin, MemoryWrite } from '../../../../shared/models'
import type { MemoryEntry, MemoryView } from '../../../store/memory-store'

/** Fixture opt-in do Vite demo, importada manualmente. Nunca usada no app. */
interface DemoHost {
  call(method: string, body: Record<string, unknown>): Promise<unknown>
  add(entry: Pick<Entry, 'botId' | 'kind' | 'data'>, id?: string): Entry
  emitEntry(entry: Entry): void
}

const provenance = (kind: MemoryOrigin['kind'], detail?: string): MemoryOrigin => ({
  schema: 1, kind, external: kind === 'external',
  sources: [{ messageId: 'memauto-fixture-human', kind, ...(detail ? { detail } : {}) }],
})

export function installMemoryFixture(host: DemoHost, botId = 'relay') {
  const original = host.call.bind(host)
  const notices = new Map<string, Entry>()
  const requests: string[] = []
  let undoMode: 'success' | 'timeout' | 'changed' = 'success'
  let memoryFailure = false
  const now = Date.now()
  const fact = (id: number, text: string, origin?: MemoryOrigin): MemoryEntry => ({
    id, bot: botId, state: 'active', text, origin, author: 'bot', authorBot: botId,
    pinned: false, evidence: [], createdAt: now, updatedAt: now, rev: id,
  })
  const view: MemoryView = {
    shared: true, yours: { entries: [], used: 0, cap: 400 },
    bot: { entries: [fact(1, 'A Clara é alérgica a amendoim.', provenance('user')), fact(2, 'Prefere opções de aniversário perto de casa.', provenance('conversation')), fact(3, 'Reunião da escola: 14/11, às 19h.', provenance('external', 'e-mail')), fact(4, 'Lembrança antiga sem prova de origem.')], used: 42, cap: 1200 },
    dreams: [], projectMemory: null, memoryRev: 1,
  }
  view.bot.entries[0]!.nature = 'said'
  Object.assign(view.bot.entries[1]!, { nature: 'inferred', natureBot: botId })
  Object.assign(view.bot.entries[2]!, { nature: 'conflict', natureBot: botId, conflictText: 'Reunião da escola: 15/11, às 19h.', previousVersion: { text: 'Reunião da escola: 14/11, às 19h.', origin: provenance('external', 'e-mail'), nature: 'unknown', at: 0, rev: 2 } })
  Object.assign(view.bot.entries[3]!, { updatedAt: 0, rev: undefined })
  host.call = async (method, body) => {
    if (method === 'memory' && body.botId === botId) {
      if (memoryFailure) throw new Error('Fixture: consulta não confirmada')
      return structuredClone(view)
    }
    if (method === 'undoMemory' && notices.has(String(body.entryId))) {
      requests.push(String(body.entryId))
      if (undoMode === 'timeout') throw new Error('Fixture: confirmação não recebida')
      const previous = notices.get(String(body.entryId))!
      if (previous.data.memory?.status === 'undone' || previous.data.memory?.status === 'changed') return { entry: previous }
      previous.data = { memory: { ...previous.data.memory, status: undoMode === 'changed' ? 'changed' : 'undone', origin: provenance('external', 'site') } }
      host.emitEntry(previous)
      return { entry: previous }
    }
    return original(method, body)
  }
  const addNotice = (memory: MemoryWrite, id: string) => {
    const entry = host.add({ botId, kind: 'notice', data: { memory } }, id)
    notices.set(id, entry)
    return entry
  }
  host.add({ botId, kind: 'user', data: { text: 'Lembra que a Clara é alérgica a amendoim? E resuma o e-mail da escola.', status: 'sent' } }, 'memauto-fixture-human')
  host.add({ botId, kind: 'agent', data: { text: 'Vou considerar a alergia ao procurar opções. A escola marcou a reunião para 14/11, às 19h.', final: true, author: botId } }, 'memauto-fixture-answer')
  addNotice({ action: 'remember', status: 'done', rev: 1, at: now, jobId: 'fixture', text: 'A Clara é alérgica a amendoim.', origin: provenance('user'), nature: 'said' }, 'memory-auto-fixture-user')
  addNotice({ action: 'remember', status: 'done', rev: 2, at: now, jobId: 'fixture', text: 'Reunião da escola: 14/11, às 19h.', origin: provenance('external', 'e-mail'), nature: 'conflict', natureBot: botId, conflictText: 'Reunião da escola: 15/11, às 19h.', previous: 'Reunião da escola: 14/11, às 19h.', previousOrigin: provenance('external', 'e-mail'), previousNature: 'unknown', previousAt: 0, previousRev: 1 }, 'memory-auto-fixture-external')
  addNotice({ action: 'remember', status: 'done', rev: 3, at: now, jobId: 'fixture', text: 'Prefere opções perto de casa.', origin: provenance('conversation'), nature: 'inferred', natureBot: botId }, 'memory-auto-fixture-inferred')
  return {
    requests, view,
    setUndoMode(mode: typeof undoMode) { undoMode = mode },
    /** D259: metadados sintéticos para prova visual; não prova produção de causas no Hub. */
    setNonSaved(value: unknown) { Object.assign(view, { nonSaved: value }) },
    setMemoryFailure(fail: boolean) { memoryFailure = fail },
    addNotice,
    restore() { host.call = original },
  }
}
