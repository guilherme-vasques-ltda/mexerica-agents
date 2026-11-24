export type RoutineStep = { id: string; title: string; state: 'done' | 'waiting' | 'uncertain' | 'pending' }

export const savedRun: RoutineStep[] = [
  { id: 'read', title: 'Ler mensagem de exemplo', state: 'done' },
  { id: 'draft', title: 'Preparar rascunho', state: 'done' },
  { id: 'review', title: 'Revisar antes de enviar', state: 'waiting' },
  { id: 'send', title: 'Enviar mensagem', state: 'pending' },
]

export class RoutineDemoSession {
  private intentionRecorded = false
  readonly enabled: boolean
  readonly steps: RoutineStep[]

  constructor(enabled = false, steps: RoutineStep[] = savedRun.map((step) => ({ ...step }))) {
    this.enabled = enabled
    this.steps = steps
  }

  continueOnce() {
    if (!this.enabled || this.intentionRecorded || this.steps.some((step) => step.state === 'uncertain')) return false
    const current = this.steps.find((step) => step.state !== 'done')
    if (!current || current.state === 'uncertain') return false
    this.intentionRecorded = true
    current.state = 'done'
    return true
  }

  get recorded() { return this.intentionRecorded }
}
