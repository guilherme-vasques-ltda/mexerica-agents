// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { validOrigin, type OriginDraft, type OriginKind } from './guard-hub.ts'
import { editOrigin, type EditRange } from './composer-segments.ts'
import { registrarEstadoEmMemoria } from './estado-so-em-memoria.ts'

/** FNV-1a sobre unidades UTF-16; detecta rascunho divergente, não atesta autoria. */
function textHash(text: string): string {
  let hash = 0x811c9dc5
  for (let i = 0; i < text.length; i++) hash = Math.imul(hash ^ text.charCodeAt(i), 0x01000193)
  return (hash >>> 0).toString(16).padStart(8, '0')
}

/** Drafts saved before 2.20.1 were keyed `["<bot>",null]`; a thread's (`["<bot>","<root>"]`) is dropped. */
function chatOf(key: string): string | null {
  let old: unknown
  try { old = JSON.parse(key) } catch { return key }
  if (!Array.isArray(old)) return key
  return old.length === 2 && typeof old[0] === 'string' && old[1] === null ? old[0] : null
}

/** Process-local attachment bytes; never included in text/origin persistence. */
export interface DraftFile { id: string; name: string; data: Uint8Array }
const NO_FILES: DraftFile[] = []

/** Fork (UI-01): o rascunho de uma extra (texto e anexos) tem chave própria; a principal continua com o id da conversa. */
const SESSION_SEP = '\u241f'
export const draftKey = (bot: string, session: string | null = null) => (session === null || session === bot ? bot : `${bot}${SESSION_SEP}${session}`)

/** Text belongs to a chat, independent of a mounted editor or transcript cache. */
export class ComposerDrafts {
  private values = new Map<string, string>()
  private attachments = new Map<string, DraftFile[]>()
  private fileLifetimes = new Map<string, object>()
  private origins = new Map<string, OriginDraft>()
  private listeners = new Set<() => void>()
  private retired = false

  private storage: Pick<Storage, 'getItem' | 'setItem'>
  private key: string
  private originKey: string
  private onSaveError?: () => void
  /** #753: rascunho escrito e anexos (só em memória); a recarga da web pergunta antes (shared/estado-so-em-memoria.ts). */
  private readonly pararConsultaMemoria = registrarEstadoEmMemoria(() => this.retired ? {} : {
    rascunho: [...this.values.values()].some(texto => texto.trim() !== ''),
    soEmMemoria: this.attachments.size > 0,
  })

  constructor(storage: Pick<Storage, 'getItem' | 'setItem'>, key: string, onSaveError?: () => void) {
    this.storage = storage
    this.key = key
    this.originKey = key.startsWith('codync-drafts-v1-') ? key.replace('codync-drafts-v1-', 'codync-draft-origins-v1-') : `${key}-origins-v1`
    this.onSaveError = onSaveError
    try {
      const saved: unknown = JSON.parse(storage.getItem(key) ?? 'null')
      if (Array.isArray(saved)) {
        for (const row of saved) {
          if (Array.isArray(row) && row.length === 2 && typeof row[0] === 'string' && (typeof row[1] === 'string' || (row[1] && typeof row[1] === 'object' && typeof row[1].text === 'string'))) {
            const chat = chatOf(row[0])
            if (chat) {
              const text = typeof row[1] === 'string' ? row[1] : row[1].text
              this.values.set(chat, text)
            }
          }
        }
      }
    } catch { /* A damaged or unavailable store must not prevent typing. */ }
    try {
      const saved: unknown = JSON.parse(storage.getItem(this.originKey) ?? 'null')
      if (Array.isArray(saved)) {
        for (const row of saved) {
          if (!Array.isArray(row) || row.length !== 2 || typeof row[0] !== 'string') continue
          const chat = chatOf(row[0]), metadata = row[1]
          const text = chat ? this.values.get(chat) : undefined
          if (chat && text !== undefined && metadata && typeof metadata === 'object'
            && metadata.textLength === text.length && metadata.textHash === textHash(text) && validOrigin(metadata.origin, text)) {
            this.origins.set(chat, { ...metadata.origin, draftId: crypto.randomUUID(), revision: metadata.origin.revision + 1 })
          }
        }
      }
    } catch { /* Sem metadados íntegros, o texto restaurado permanece desconhecido. */ }
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => { this.listeners.delete(listener) }
  }

  get(bot: string) {
    return this.values.get(bot) ?? ''
  }

  files(bot: string): DraftFile[] { return this.attachments.get(bot) ?? NO_FILES }

  setFiles(bot: string, files: DraftFile[]) {
    if (this.retired) return
    if (files.length) this.attachments.set(bot, files)
    else {
      this.attachments.delete(bot)
      this.fileLifetimes.delete(bot)
    }
    this.notify()
  }

  /** Capture before an async read; deletion/reset/retirement cannot revive that draft. */
  fileGuard(bot: string): () => boolean {
    let lifetime = this.fileLifetimes.get(bot)
    if (!lifetime) { lifetime = {}; this.fileLifetimes.set(bot, lifetime) }
    return () => !this.retired && this.fileLifetimes.get(bot) === lifetime
  }

  origin(bot: string) { return this.origins.get(bot) }

  edit(bot: string, text: string, kind: OriginKind, range?: EditRange) {
    const origin = editOrigin(this.get(bot), text, this.origin(bot), kind, range)
    this.set(bot, text, origin)
  }

  set(bot: string, text: string, origin?: OriginDraft) {
    if (this.retired) return
    if (text && origin && validOrigin(origin, text)) this.origins.set(bot, origin)
    else this.origins.delete(bot)
    if (text) this.values.set(bot, text)
    else this.values.delete(bot)
    this.save()
  }

  removeBot(bot: string) {
    if (this.retired) return
    this.attachments.delete(bot)
    this.fileLifetimes.delete(bot)
    // Fork (UI-01): as extras do bot saem junto (texto, origem e anexos).
    for (const key of new Set([...this.values.keys(), ...this.attachments.keys(), ...this.fileLifetimes.keys()])) {
      if (!key.startsWith(`${bot}${SESSION_SEP}`)) continue
      this.values.delete(key)
      this.origins.delete(key)
      this.attachments.delete(key)
      this.fileLifetimes.delete(key)
    }
    this.values.delete(bot)
    this.origins.delete(bot)
    this.save()
  }

  clear() {
    if (this.retired) return
    this.attachments.clear()
    this.fileLifetimes.clear()
    this.origins.clear()
    this.values.clear()
    this.save()
  }

  retire() {
    this.retired = true
    this.pararConsultaMemoria()
    this.attachments.clear()
    this.fileLifetimes.clear()
    this.notify()
  }

  private save() {
    // Save on each edit, including clearing after submission; no unmount/debounce race.
    try {
      // A versão anterior continua lendo strings; metadados não duplicam o corpo.
      this.storage.setItem(this.key, JSON.stringify([...this.values]))
      this.storage.setItem(this.originKey, JSON.stringify([...this.origins].map(([bot, origin]) => {
        const text = this.get(bot)
        return [bot, { textLength: text.length, textHash: textHash(text), origin }]
      })))
    } catch { this.onSaveError?.() }
    this.notify()
  }

  private notify() { for (const listener of this.listeners) listener() }
}
