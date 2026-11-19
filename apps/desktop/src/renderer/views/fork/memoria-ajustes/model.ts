// D389: Ajustes → Memória. O modelo da memória é a mesma seleção `memory_consolidation` do Bot de fundo
// (BackgroundStore, functionModels/saveFunctionModel); aqui só o modo de aprender, sem DOM nem React.
import { Observable } from '../../../lib/observable.ts'
import type { BackgroundState, Draft, FunctionRow } from '../bot-de-fundo/model.ts'

export type Learning = 'auto' | 'suggest' | 'off'
export interface LearningState { mode: Learning; pending: number }
export type LearningIssue = 'unavailable' | 'save'

export const LEARNINGS: readonly Learning[] = ['auto', 'suggest', 'off']
const isLearning = (v: unknown): v is Learning => typeof v === 'string' && (LEARNINGS as readonly string[]).includes(v)

/** Resposta de `memoryLearning`/`saveMemoryLearning`; fora do contrato, nada é presumido. */
export function readLearning(value: unknown): LearningState {
  const v = value as { mode?: unknown; pending?: unknown } | null
  if (!v || typeof v !== 'object' || !isLearning(v.mode) || typeof v.pending !== 'number' || !Number.isSafeInteger(v.pending) || v.pending < 0) throw new Error('learning_invalid')
  return { mode: v.mode, pending: v.pending }
}

export type ModelFamily = 'haiku' | 'sonnet' | 'opus'
/** A família pelo ID do catálogo da conta (`claude-haiku-5-5`, `haiku`); outra família mostra o próprio ID. */
export function modelFamily(id: string): ModelFamily | null {
  const lower = id.toLowerCase()
  return lower.includes('haiku') ? 'haiku' : lower.includes('sonnet') ? 'sonnet' : lower.includes('opus') ? 'opus' : null
}

/** A troca de modelo no cartão: vazio volta ao automático do Hub (Haiku, esforço baixo). */
export function modelPatch(modelId: string): Partial<Pick<Draft, 'mode' | 'modelId'>>[] {
  return modelId ? [{ mode: 'custom' }, { modelId }] : [{ mode: 'automatic' }]
}

/** Motivos do Hub com texto próprio no cartão; os demais usam o aviso genérico. */
const UNAVAILABLE = {
  codex_subscription_unavailable: 'memoria.modelo.indisponivel.codex',
  api_coming_soon: 'memoria.modelo.indisponivel.api',
} as const
export type ModelNoticeKey = 'memoria.modelo.salvo' | 'memoria.modelo.salvoSemUso' | 'memoria.modelo.indisponivel' | (typeof UNAVAILABLE)[keyof typeof UNAVAILABLE]

/** Avisos do cartão pela resolução relida do Hub: "salvo" só promete uso quando a escolha está disponível.
 * Enquanto relê (`loading`), a resolução ainda é a da escolha anterior: nenhum aviso (R1-01). */
export function modelNotices(resolution: FunctionRow['resolution'], state: Pick<BackgroundState, 'dirty' | 'conflict' | 'aviso' | 'loading'>): { saved: ModelNoticeKey | null; warning: ModelNoticeKey | null } {
  if (state.dirty || state.loading) return { saved: null, warning: null }
  const reason = resolution.status === 'unavailable' ? resolution.unavailable.reason : null
  const warning = reason === null ? null : Object.hasOwn(UNAVAILABLE, reason) ? UNAVAILABLE[reason as keyof typeof UNAVAILABLE] : 'memoria.modelo.indisponivel'
  const saved = !state.conflict && state.aviso ? (warning ? 'memoria.modelo.salvoSemUso' : 'memoria.modelo.salvo') : null
  return { saved, warning }
}

interface Transport { call<T>(method: string, body?: unknown): Promise<T> }

/** O modo é do Hub, igual para todos os bots; só o acesso local lê e grava (como o Bot de fundo). */
export class LearningStore extends Observable {
  value: LearningState | null = null
  issue: LearningIssue | null = null
  busy = false
  private request = 0
  private readonly client: () => Promise<Transport>

  constructor(client: () => Promise<Transport>) { super(); this.client = client }

  async load() {
    const request = ++this.request
    try {
      const value = readLearning(await (await this.client()).call('memoryLearning', {}))
      if (request !== this.request) return
      this.value = value
      this.issue = null
    } catch {
      if (request !== this.request) return
      this.value = null
      this.issue = 'unavailable'
    }
    this.changed()
  }

  async save(mode: Learning) {
    if (this.busy || !this.value || this.value.mode === mode) return
    const request = ++this.request
    this.busy = true
    this.issue = null
    this.changed()
    try {
      const value = readLearning(await (await this.client()).call('saveMemoryLearning', { mode }))
      if (request === this.request) this.value = value
    } catch {
      // Resultado incerto: relê o estado do Hub em vez de presumir a escolha.
      if (request === this.request) this.issue = 'save'
    } finally {
      this.busy = false
      this.changed()
    }
    if (this.issue === 'save') {
      const keep = this.issue
      await this.load()
      if (this.value) { this.issue = keep; this.changed() }
    }
  }
}
