import type { BotEditor, BotEditorErrorCode, BotEditorPatch, BotEditorProfile, BotEditorSave } from '../../../../shared/fork-bot-editor.ts'
import type { BotStore } from '../../../store/bot-store'
import type { Bot } from '../../../../shared/models'
import { camposAvatarDoBot, type BotAvatarConfig } from '../../../../shared/fork-avatar-config.ts'
import { motorPatch } from './engine-account.ts'
import { chaveDoErroDaDonna } from '../../../lib/fork-donna-erros.ts'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'

export interface EditorTransport {
  read(botId: string): Promise<BotEditor>
  save(request: BotEditorSave): Promise<BotEditor>
}

export interface EditorState {
  botId: string
  editor: BotEditor | null
  draft: BotEditorProfile | null
  online: boolean
  loading: boolean
  saving: boolean
  dirty: boolean
  paused: boolean
  /** A chave do catálogo do erro atual; a tela traduz ao mostrar (R1 do #572). */
  error: ChaveTexto | null
  conflict: boolean
}

const fields = ['name', 'description', 'avatarConfig', 'mexeriquinha', 'title', 'backend', 'model', 'modelOptions', 'fastMode', 'browserPreference', 'forkEditor'] as const
const equal = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)
/** Fase 2a: objetos de avatar iguais com chaves em outra ordem; ausente e null são o mesmo "sem avatar". */
const canonico = (v: unknown): string => v && typeof v === 'object'
  ? Array.isArray(v) ? `[${v.map(canonico).join(',')}]` : `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canonico((v as Record<string, unknown>)[k])}`).join(',')}}`
  : JSON.stringify(v ?? null)
export const fieldEqual = (field: typeof fields[number], a: unknown, b: unknown) => {
  if (field === 'forkEditor') {
    const id = (v: unknown) => v && typeof v === 'object' && 'engineAccountId' in v ? v.engineAccountId ?? null : null
    return id(a) === id(b)
  }
  if (field === 'modelOptions') {
    const sorted = (v: unknown) => v && typeof v === 'object' ? Object.entries(v).sort(([a], [b]) => a.localeCompare(b)) : []
    return equal(sorted(a), sorted(b))
  }
  if (field === 'avatarConfig' || field === 'mexeriquinha') return canonico(a) === canonico(b)
  if (field === 'fastMode') return (a ?? false) === (b ?? false)
  return String(a ?? '').trim() === String(b ?? '').trim()
}
const sameProfile = (a: BotEditorProfile, b: BotEditorProfile) => fields.every((field) => fieldEqual(field, a[field], b[field]))

// BotConfig serializado por bot_editor/api.rs::version, nos campos disponíveis no mirror.
// normalizeBot descarta permissionChosenByUser; a autoria ainda não chega ao mirror.
// Escolha explícita com autonomia igual em outro cliente pode mudar só a versão; ver backlog.
const configurationFields = [
  'id', 'kind', 'members', 'name', 'description', 'ownerPreferences', 'avatarColor', 'avatarShape', 'avatarConfig', 'mexeriquinha', 'title',
  'backend', 'forkEditor', 'command', 'cwd', 'permission', 'model', 'modelOptions', 'fastMode', 'pinned', 'position',
  'hidden', 'notify', 'connectors', 'logins', 'skills', 'computer', 'browserPreference', 'manageTeam', 'createdBy',
  'sharedMemory', 'harnessMemory', 'autoName', 'createdAt',
] as const satisfies readonly (keyof Bot)[]

/** Só configuração versionada e principal invalidam a leitura; progresso do runtime fica de fora. */
export const botSettingsSignature = (bot: Partial<Bot> | undefined, primaryBotId: string | null) =>
  JSON.stringify([bot ? configurationFields.map((field) => bot[field]) : null, primaryBotId])

function confirmedDraft(current: BotEditorProfile, submitted: BotEditorProfile, saved: BotEditorProfile) {
  const draft = { ...current }
  for (const field of fields) {
    // Não aparar o texto controlado: um espaço/Enter final pode ser a próxima palavra/linha.
    if (equal(current[field], submitted[field]) && !fieldEqual(field, saved[field], submitted[field])) {
      Object.assign(draft, { [field]: saved[field] })
    }
  }
  return draft
}
// Fatia 3 (D454) e R1 do #572: o estado guarda a CHAVE do catálogo (bots.ajustes.erro.*); a tela traduz ao mostrar.
const messages: Record<BotEditorErrorCode, ChaveTexto> = {
  bot_not_found: 'bots.ajustes.erro.naoEncontrado',
  bot_deleted: 'bots.ajustes.erro.apagado',
  settings_conflict: 'bots.ajustes.erro.conflito',
  not_allowed: 'bots.ajustes.erro.semPermissao',
  invalid_field: 'bots.ajustes.erro.campo',
  folder_unavailable: 'bots.ajustes.erro.pasta',
  model_unavailable: 'bots.ajustes.erro.modelo',
  command_not_allowed: 'bots.ajustes.erro.comando',
  save_failed: 'bots.ajustes.erro.salvar',
}

export function editorError(error: unknown, operation: 'save' | 'load' = 'save'): { error: ChaveTexto; conflict: boolean } {
  const raw = typeof error === 'object' && error !== null && 'message' in error ? String(error.message) : String(error)
  const code = raw.replace(/^Error: /, '').split(':')[0] as BotEditorErrorCode
  // D366/D368: recusa do Berga fixo pelo texto do catálogo.
  const donna = chaveDoErroDaDonna(raw.replace(/^Error: /, ''))
  if (donna) return { error: donna, conflict: false }
  if (code === 'invalid_field' && /^invalid_field: (motor|modelo|opções|conta)\b/.test(raw.replace(/^Error: /, ''))) {
    return { error: 'bots.ajustes.erro.selecao', conflict: false }
  }
  return { error: operation === 'load' && !(code in messages) ? 'bots.ajustes.erro.carregar' : messages[code] ?? messages.save_failed, conflict: code === 'settings_conflict' }
}

/** Rascunho de UI apenas. O Hub valida e versiona; nenhum BotDraft integral é enviado. */
export class BotSettingsStore {
  private transport: EditorTransport
  private state: EditorState = { botId: '', editor: null, draft: null, online: true, loading: false, saving: false, dirty: false, paused: false, error: null, conflict: false }
  private listeners = new Set<() => void>()
  private generation = 0
  private timer: ReturnType<typeof setTimeout> | undefined
  private flight: Promise<boolean> | null = null
  private reading: Promise<void> | null = null
  private refreshPending = false
  private lastSubmitted: BotEditorProfile | null = null
  private changingMotor = false

  constructor(transport: EditorTransport) { this.transport = transport }
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  getSnapshot = () => this.state
  private update(patch: Partial<EditorState>) {
    this.state = { ...this.state, ...patch }
    this.state.dirty = !!this.state.draft && !!this.state.editor && !sameProfile(this.state.draft, this.state.editor.profile)
    for (const listener of this.listeners) listener()
  }

  async open(botId: string) {
    this.cancelTimer()
    this.generation++
    this.flight = null
    this.reading = null
    this.refreshPending = false
    this.lastSubmitted = null
    this.changingMotor = false
    this.update({ botId, editor: null, draft: null, loading: false, saving: false, error: null, conflict: false, paused: false })
    await this.refresh()
  }

  setOnline(online: boolean) {
    if (online === this.state.online) return
    this.cancelTimer()
    this.update({ online, paused: online ? this.state.dirty && this.state.paused : this.state.dirty || this.state.paused })
    if (online) void this.refresh()
  }

  /** A assinatura de configuração e a reconexão relêem o DTO; progresso do runtime não o invalida. */
  refresh(): Promise<void> {
    if (!this.state.online || !this.state.botId) return Promise.resolve()
    if (this.flight) { this.refreshPending = true; return Promise.resolve() }
    if (this.reading) return this.reading
    const generation = this.generation
    this.update({ loading: true })
    const read = (async () => {
      try {
        const editor = await this.transport.read(this.state.botId)
        if (generation !== this.generation) return
        const confirmed = this.lastSubmitted && sameProfile(editor.profile, this.lastSubmitted)
        if (confirmed) {
          const draft = confirmedDraft(this.state.draft!, this.lastSubmitted!, editor.profile)
          this.lastSubmitted = null
          this.update({ editor, draft, conflict: false, error: null, paused: this.state.paused && !sameProfile(draft, editor.profile) })
        } else if (this.state.dirty && this.state.editor?.settingsVersion !== editor.settingsVersion) {
          this.update({ conflict: true, error: messages.settings_conflict })
        } else {
          const draft = this.state.dirty ? this.state.draft : this.state.draft ? confirmedDraft(this.state.draft, this.state.draft, editor.profile) : { ...editor.profile }
          this.update({ editor, draft, error: this.state.dirty ? this.state.error : null })
        }
      } catch (error) {
        if (generation === this.generation) this.update(editorError(error, 'load'))
      } finally {
        if (generation === this.generation) { this.reading = null; this.update({ loading: false }) }
      }
    })()
    this.reading = read
    return read
  }

  change<K extends keyof BotEditorPatch>(field: K, value: BotEditorPatch[K], immediate = false) {
    if (!this.state.online || !this.state.draft || !this.state.editor?.permissions.canEdit || !this.state.editor.capabilities.editableFields.includes(field)) return
    this.update({ draft: { ...this.state.draft, [field]: value }, error: this.state.conflict ? this.state.error : null })
    this.cancelTimer()
    if (this.state.conflict || this.state.paused) return
    if (immediate) void this.flush()
    else this.timer = setTimeout(() => { void this.flush() }, 600)
  }

  /**
   * Fase 2a: a aparência ocupa dois campos (avatarConfig legado e mexeriquinha v2) que mudam juntos, num rascunho só,
   * para o mesmo patch levar os dois. Hub que não edita algum dos campos devolvidos não recebe nada.
   */
  changeAvatar(value: BotAvatarConfig, immediate = true) {
    let campos: Partial<BotEditorPatch>
    try { campos = camposAvatarDoBot(value) } catch { return }
    const editor = this.state.editor
    if (!this.state.online || !this.state.draft || !editor?.permissions.canEdit) return
    if (!(Object.keys(campos) as (keyof BotEditorPatch)[]).every((field) => editor.capabilities.editableFields.includes(field))) return
    // Legado vem sem `mexeriquinha`: o rascunho também a perde, e o patch só com avatarConfig faz o Hub limpar a v2.
    const { mexeriquinha: _antiga, ...resto } = this.state.draft
    this.update({ draft: { ...resto, ...campos }, error: this.state.conflict ? this.state.error : null })
    this.cancelTimer()
    if (this.state.conflict || this.state.paused) return
    // Um save em voo não leva esta troca; o próximo começa assim que ele termina.
    if (immediate) void (this.flight ? this.flight.then(() => this.flush()) : this.flush())
    else this.timer = setTimeout(() => { void this.flush() }, 600)
  }

  private cancelTimer() { clearTimeout(this.timer); this.timer = undefined }

  /** Troca confirmada pela pessoa: não publica motor/retirada da conta no rascunho antes do save. */
  async saveMotor(backend: string | null): Promise<boolean> {
    if (this.changingMotor) return false
    const generation = this.generation
    this.changingMotor = true
    try {
      if (!await this.flush() || generation !== this.generation) return false
      const { editor, draft, online, conflict, paused, botId } = this.state
      if (!editor || !draft || !online || conflict || paused || !editor.permissions.canEdit) return false
      // Comando próprio continua no fluxo legado; aqui só se confirma a retirada explícita da conta.
      const requested: BotEditorPatch = backend === null ? { forkEditor: { engineAccountId: null } } : motorPatch(backend)
      const patch: BotEditorPatch = {}
      for (const field of fields) {
        if (field in requested && !fieldEqual(field, requested[field], editor.profile[field])) Object.assign(patch, { [field]: requested[field] })
      }
      if (!Object.keys(patch).length) return true
      if (Object.keys(patch).some((field) => !editor.capabilities.editableFields.includes(field as keyof BotEditorPatch))) {
        this.update(editorError(new Error('invalid_field: motor não editável neste Hub.'))) // textos-ok: código técnico interno, a view mostra o texto do catálogo
        return false
      }
      const run = (async () => {
        this.update({ saving: true, error: null })
        try {
          const saved = await this.transport.save({ botId, expectedVersion: editor.settingsVersion, patch })
          if (generation !== this.generation) return false
          this.update({ editor: saved, draft: confirmedDraft(this.state.draft!, draft, saved.profile) })
          return true
        } catch (error) {
          if (generation === this.generation) this.update(editorError(error))
          return false
        }
      })()
      this.flight = run
      try { return await run }
      finally {
        if (generation === this.generation) {
          this.flight = null
          this.update({ saving: false })
          if (this.refreshPending) { this.refreshPending = false; void this.refresh() }
          // Edições de identidade feitas durante o pedido conservam seu autosave.
          if (this.state.dirty && !this.state.error) this.timer = setTimeout(() => { void this.flush() }, 600)
        }
      }
    } finally { if (generation === this.generation) this.changingMotor = false }
  }

  /** Um save por vez. Uma resposta só confirma valores que não foram reeditados. */
  flush(): Promise<boolean> {
    this.cancelTimer()
    if (this.flight) return this.flight
    const generation = this.generation
    const run = (async () => {
      if (this.reading) await this.reading
      while (generation === this.generation && this.state.dirty) {
        const { editor, draft, online, conflict, paused, botId } = this.state
        if (!editor || !draft || !online || conflict || paused || !editor.permissions.canEdit) return false
        const submitted = { ...draft }
        const patch: BotEditorPatch = {}
        for (const field of editor.capabilities.editableFields) {
          // Sem mexeriquinha no rascunho (escolha legada), o patch não leva a chave: o avatarConfig legado limpa a v2 no Hub.
          if (field === 'mexeriquinha' && draft.mexeriquinha === undefined) continue
          if (!fieldEqual(field, draft[field], editor.profile[field])) Object.assign(patch, { [field]: draft[field] })
        }
        if (!Object.keys(patch).length) return false
        this.lastSubmitted = submitted
        this.update({ saving: true, error: null })
        try {
          const saved = await this.transport.save({ botId, expectedVersion: editor.settingsVersion, patch })
          if (generation !== this.generation) return false
          const current = confirmedDraft(this.state.draft!, submitted, saved.profile)
          this.lastSubmitted = null
          this.update({ editor: saved, draft: current })
          if (!this.state.dirty) this.update({ paused: false })
        } catch (error) {
          if (generation === this.generation) this.update(editorError(error))
          return false
        }
      }
      return generation === this.generation && !this.state.dirty
    })()
    this.flight = run
    void run.finally(() => {
      if (generation !== this.generation) return
      this.flight = null
      this.update({ saving: false })
      if (this.refreshPending) { this.refreshPending = false; void this.refresh() }
    })
    return run
  }

  async retry() {
    if (this.state.conflict) return false
    this.update({ error: null, paused: false })
    await this.refresh()
    return this.flush()
  }

  async discard() {
    this.cancelTimer()
    if (this.flight) await this.flight
    this.lastSubmitted = null
    this.update({ draft: this.state.editor ? { ...this.state.editor.profile } : null, error: null, conflict: false, paused: false })
    await this.refresh()
  }

  pause() { this.cancelTimer() }
}

/** A ponte existente autentica a intenção do cliente; a view só observa este store. */
export function createBotSettingsStore(store: BotStore) {
  return new BotSettingsStore({
    read: async (botId) => (await store.ready()).call<BotEditor>('donnaBotEditor', { botId }),
    save: async (request) => (await store.ready()).call<BotEditor>('donnaBotEditorSave', request),
  })
}
