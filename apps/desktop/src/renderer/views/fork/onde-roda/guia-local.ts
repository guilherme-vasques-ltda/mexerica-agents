import type { WorkplaceGuide } from '../../../../shared/fork-workplaces.ts'

export type GuideContent = { kind: 'loading' } | { kind: 'error' } | { kind: 'ready'; guide: WorkplaceGuide }
interface GuideSource { readonly generation: number; readGuide(): Promise<WorkplaceGuide | null> }

/** Fechar, reabrir e trocar de origem invalidam o carregamento anterior. */
export class GuideReading {
  private revision = 0
  cancel() { this.revision++ }
  async read(source: GuideSource, apply: (content: GuideContent) => void) {
    const revision = ++this.revision
    const generation = source.generation
    try {
      const guide = await source.readGuide()
      if (this.revision !== revision || source.generation !== generation) return
      apply(guide ? { kind: 'ready', guide } : { kind: 'error' })
    } catch {
      if (this.revision === revision && source.generation === generation) apply({ kind: 'error' })
    }
  }
}
