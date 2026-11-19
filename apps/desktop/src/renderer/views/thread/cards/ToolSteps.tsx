import { useId, useState } from 'react'
import { Icon } from '../../../components/Icon'
import { Text } from '../../../components/Text'
import { liveSummary, stepsSummary, type Step, type TurnSteps } from './tool-steps'
import { stepText } from './step-text'
import { useT } from '../../../lib/fork-i18n/estado'
import './tool-steps.css'

// D165 fase F + D169 (components/ToolSteps): what the bot did, from the Hub's own tool entries. A pill that
// folds the list ("4 passos · Comando, Edição · 41 s"), and the list in a `group` block under it: open while
// it works ("Trabalhando · 2 passos"), folded once the reply arrives. No time per step and no read receipt:
// the Hub has no contract for them yet (D169 B1/B3), so nothing is inferred.

export function ToolSteps({ steps, live = false }: { steps: TurnSteps; live?: boolean }) {
  const [open, setOpen] = useState(live)
  const t = useT()
  const listId = useId()
  const running = steps.steps.find((s) => s.state === 'running')
  const failed = steps.steps.some((s) => s.state === 'failed')
  return (
    <div className={`tool-steps${live ? ' is-live' : ''}`}>
      <button className="tool-steps-toggle" aria-expanded={open} aria-controls={listId} onClick={() => setOpen((o) => !o)}>
        <Icon name="chevron.down" size={11} weight="semibold" className="tool-steps-chevron" />
        <span className="tool-steps-summary">{live ? liveSummary(t, steps) : stepsSummary(t, steps)}</span>
        {failed ? <span className="tool-steps-flag" title={t('conversa.cartoes.passos.falhou')}><Icon name="exclamationmark.triangle" size={11} color="var(--danger)" /></span> : null}
      </button>
      {!open && live && running ? <div className="tool-steps-list is-peek"><StepRow step={running} /></div> : null}
      <ol id={listId} className="tool-steps-list" hidden={!open}>
        {steps.steps.map((s) => <li key={s.id}><StepRow step={s} /></li>)}
      </ol>
    </div>
  )
}

function StepRow({ step }: { step: Step }) {
  const t = useT()
  // MX-141: o texto exibido é em linguagem simples; o título original do motor fica na dica (hover).
  const text = stepText(t, step)
  const title = step.title || text
  return (
    <span className="tool-step" data-state={step.state}>
      <span className="tool-step-mark" role="img" aria-label={t(`conversa.cartoes.passos.estado.${step.state}`)}>
        {step.state === 'done' ? <Icon name="checkmark" size={9} weight="bold" /> : step.state === 'failed' ? <Icon name="xmark" size={9} weight="bold" /> : null}
      </span>
      {/* D249: one line with an ellipsis; the whole title stays in the DOM (copy, screen reader) and in `title` on hover. */}
      <Text size="body" tone={step.state === 'failed' ? 'danger' : 'text'} truncate grow className="selectable" title={title}>{text}</Text>
    </span>
  )
}
