import type { HostClient } from '../client/host-client'
import type { Entry, MemoryAvailability, MemoryNature, MemoryOrigin, MemoryVersion } from '@shared/models'
import { Observable } from '../lib/observable'
import { MemoryUndoState } from '../views/fork/memoria-auto/undo-state'
import { readNonSaved, type MemoryNonSaved, type NonSavedHistory } from '../views/fork/memoria-auto/non-saved'
import { readOpeningOmitted, type OpeningOmittedHistory } from '../views/fork/memoria-auto/opening-omitted'
import { readSuggestions, suggestionOutcome, type MemorySuggestion, type SuggestionOutcome } from '../views/fork/memoria-auto/sugestoes'
import type { ChaveTexto } from '../lib/fork-i18n/index.ts'
import { ErroComChave, mensagemDoErro, type Mensagem } from '../views/fork/bot-settings/mensagem'

/** One memory entry (the host's `codync_mem::store::Entry`). */
export interface MemoryEntry {
  id: number
  /** The bot whose own layer holds it; null for what all bots share. */
  bot: string | null
  state: 'active' | 'archived' | string
  text: string
  /** user · bot · dream · imported */
  author: string
  authorBot: string | null
  pinned: boolean
  evidence: string[]
  createdAt: number
  updatedAt: number
  origin?: MemoryOrigin | null
  rev?: number
  nature?: MemoryNature
  natureBot?: string | null
  conflictText?: string | null
  previousVersion?: MemoryVersion | null
}

export interface MemoryLayer { entries: MemoryEntry[]; used: number; cap: number }

/** One dreaming run: what it added and archived; it can be undone. */
export interface DreamRun {
  id: number
  at: number
  undone: boolean
  added: { id: number; text: string }[]
  archived: { id: number; text: string }[]
  /** Long-form files it added a line to. */
  files?: { name: string; shared: boolean; version: number }[]
}

/** A long-form memory file as the host last recorded it (`codync_mem::files::File`). */
export interface MemoryFile {
  bot: string | null
  name: string
  /** SHA-256 of what it holds; a save is made over it. */
  hash: string
  description: string | null
  tokens: number
  deleted: boolean
  updatedAt: number
  origin?: MemoryOrigin | null
  rev?: number
  nature?: MemoryNature
  natureBot?: string | null
  conflictText?: string | null
}

export interface MemoryFiles {
  yours: { files: MemoryFile[]; folder: string }
  bot: { files: MemoryFile[]; folder: string }
  /** Tokens one file may hold, and files one folder may hold. */
  cap: number
  folderCap: number
}

export interface MemoryFileVersion {
  id: number
  /** null: the file was deleted. */
  content: string | null
  /** user · bot · dream · computer */
  author: string
  authorBot: string | null
  /** edit · delete · restore · undo · dream */
  cause: string
  at: number
  origin?: MemoryOrigin | null
  rev?: number
  nature?: MemoryNature
  natureBot?: string | null
  conflictText?: string | null
}

/** A file as it is on disk now, with its recent versions. */
export interface MemoryFileDetail {
  file: string
  shared: boolean
  content: string | null
  hash: string | null
  path: string
  versions: MemoryFileVersion[]
}

export interface MemoryView {
  /** Ausente em computadores antigos; ocorrências passadas, não estado atual. */
  nonSaved?: MemoryNonSaved[]
  /** O que ficou de fora da última abertura e por quê; ausente em computadores antigos. */
  openingOmitted?: unknown
  automatic?: { status: string; availability?: MemoryAvailability | null; jobId: string } | null
  sessionDiagnostic?: { marked: boolean; at: number | null; reason: string | null } | null
  discardedDreamChanges?: { at: number; archive: number; review: number; move: number; facts?: number } | null
  /** Whether this bot uses what all bots share. */
  shared: boolean
  yours: MemoryLayer
  bot: MemoryLayer
  dreams: DreamRun[]
  /** Claude Code's own notes for the bot's project folder, read-only. */
  projectMemory: { path: string; text: string } | null
  memoryRev: number
  /** The long-form files of both layers. */
  files?: MemoryFiles
  /** D389: o que os bots aprendem (ausente em computadores antigos). */
  learning?: 'auto' | 'suggest' | 'off'
  /** D389: pendentes desta memória; não estão na memória até o dono aprovar. */
  suggestions?: unknown
}

/** Resultado de transporte fica separado da verdade durável do recibo. */
export class MemoryNoticeStore extends Observable {
  private readonly undoState: MemoryUndoState

  constructor(client: () => Promise<HostClient>) {
    super()
    this.undoState = new MemoryUndoState(async (entryId) => (await client()).call('undoMemory', { entryId }), () => this.changed())
  }

  state(entry: Entry) { return this.undoState.state(entry) }
  undo(entry: Entry) { return this.undoState.undo(entry) }
}

/** One bot's memory: fetches it, and changes it through the host. */
export class MemoryStore extends Observable {
  view: MemoryView | null = null
  nonSaved: NonSavedHistory = { state: 'loading' }
  openingOmitted: OpeningOmittedHistory = { state: 'loading' }
  /** `null`: o computador não informa sugestões. */
  suggestions: MemorySuggestion[] | null = null
  busy = false
  /** D507: o erro como dado (chave do catálogo ou o texto cru do Hub); a tela traduz ao desenhar. */
  error: Mensagem | null = null
  private request = 0

  constructor(readonly botId: string, private readonly client: () => Promise<HostClient>) { super() }

  async load() {
    const request = ++this.request
    this.nonSaved = { state: 'loading' }
    this.openingOmitted = { state: 'loading' }
    this.changed()
    try {
      const view = await (await this.client()).call<MemoryView>('memory', { botId: this.botId })
      // A host before 2.18.0 answers with its old memory listing, which has no layers or dreams.
      if (!Array.isArray(view.dreams)) throw new ErroComChave('marca.memoria.hubAntigo')
      if (request === this.request) {
        this.view = view
        this.nonSaved = readNonSaved(view.nonSaved)
        this.openingOmitted = readOpeningOmitted(view.openingOmitted)
        this.suggestions = readSuggestions(view.suggestions)
        this.error = null
      }
    } catch (error) {
      if (request === this.request) {
        this.error = message(error)
        this.nonSaved = { state: 'unknown' }
        this.openingOmitted = { state: 'unknown' }
      }
    }
    this.changed()
  }

  /** A long-form file as it is now, with its versions. */
  async file(name: string, shared: boolean): Promise<MemoryFileDetail | null> {
    try {
      return await (await this.client()).call<MemoryFileDetail>('memoryFile', { botId: this.botId, file: name, shared })
    } catch (error) {
      this.error = message(error)
      this.changed()
      return null
    }
  }

  /** Saves (`content`), deletes (neither) or brings back `version` of a file, over what was loaded (`base`). */
  changeFile(name: string, shared: boolean, base: string | null, change: { content?: string; version?: number } = {}) {
    const method = change.version !== undefined ? 'restoreMemoryFile' : change.content !== undefined ? 'saveMemoryFile' : 'deleteMemoryFile'
    return this.mutate(method, { file: name, shared, base, ...change })
  }

  /** D389: gesto do dono sobre uma sugestão. O erro vira resultado na própria linha, não no topo da tela. */
  async decideSuggestion(id: string, approve: boolean): Promise<SuggestionOutcome | null> {
    if (this.busy) return null
    this.busy = true
    this.changed()
    let outcome: SuggestionOutcome
    try {
      const result = await (await this.client()).call(approve ? 'approveMemorySuggestion' : 'discardMemorySuggestion', { botId: this.botId, id })
      outcome = suggestionOutcome(approve, result)
    } catch (error) {
      outcome = suggestionOutcome(approve, null, error)
    }
    // Resultado incerto ou não: a lista recarrega o estado confirmado no Hub.
    await this.load()
    this.busy = false
    this.changed()
    return outcome
  }

  /** A memory method for this bot; reloads after it. */
  async mutate(method: string, body: object = {}) {
    if (this.busy) return false
    this.busy = true
    this.error = null
    this.changed()
    try {
      await (await this.client()).call(method, { botId: this.botId, ...body })
      await this.load()
      return true
    } catch (error) {
      this.error = message(error)
      return false
    } finally {
      this.busy = false
      this.changed()
    }
  }
}

/** Who wrote an entry or a version, as a catalog key; `bot` is the bot's name, isolated where it is drawn. */
export interface RotuloAutor { chave: ChaveTexto; bot?: string }

/** Who wrote an entry, for its detail line. */
export function authorLabel(entry: MemoryEntry, botName: (id: string | null) => string): RotuloAutor {
  switch (entry.author) {
    case 'user': return { chave: 'memoria.painel.autor.voce' }
    case 'bot': return { chave: 'memoria.painel.autor.bot', bot: botName(entry.authorBot) }
    case 'dream': return { chave: 'memoria.painel.autor.consolidacao' }
    default: return { chave: 'memoria.painel.autor.migrada' }
  }
}

const AUTORES = { user: 'voce', bot: 'bot', dream: 'consolidacao' } as const
const CAUSAS = { delete: 'excluiu', restore: 'restaurou', undo: 'desfez' } as const

/** Who made a file's version and why, for its row: one whole sentence per author and cause. */
export function versionLabel(version: MemoryFileVersion, botName: (id: string | null) => string): RotuloAutor {
  const autor = AUTORES[version.author as keyof typeof AUTORES] ?? 'computador'
  const causa = CAUSAS[version.cause as keyof typeof CAUSAS] ?? 'alterou'
  const chave = `memoria.painel.versao.${autor}.${causa}` as ChaveTexto
  return autor === 'bot' ? { chave, bot: botName(version.authorBot) } : { chave }
}

function message(error: unknown): Mensagem { return mensagemDoErro(error, (e) => (e instanceof Error ? e.message : String(e))) }
