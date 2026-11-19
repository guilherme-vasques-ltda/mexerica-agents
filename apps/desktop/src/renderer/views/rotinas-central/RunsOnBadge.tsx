import { Icon } from '../../components/Icon'
import type { RunsOn } from './model'
import { texts } from './texts'

/** D84 depende dos computadores da D83: o selo fica desligado até eles existirem no Hub. */
export const SHOW_RUNS_ON = false

/** O nome de onde a rotina roda ("Hub", "Mac"), ou nada enquanto o selo estiver desligado. */
export function runsOnName(runsOn: RunsOn | null): string | null {
  if (!SHOW_RUNS_ON || !runsOn) return null
  return runsOn.kind === 'hub' ? texts.hub : runsOn.name
}

/**
 * "Roda em Hub" ou "Roda em Mac": onde a rotina roda. `compact` é só o ícone (cartões da grade,
 * entradas do Mês e fichas da faixa), com o nome no `title`; o rótulo acessível do item já diz.
 */
export function RunsOnBadge({ runsOn, compact = false }: { runsOn: RunsOn | null; compact?: boolean }) {
  const name = runsOnName(runsOn)
  if (!name || !runsOn) return null
  const icon = runsOn.kind === 'hub' ? 'server.rack' : 'laptopcomputer'
  if (compact) {
    return (
      <span className="rc-runs-on-icon" title={texts.runsOn(name)} aria-hidden>
        <Icon name={icon} size={9} />
      </span>
    )
  }
  return (
    <span className="rc-runs-on" title={texts.runsOn(name)}>
      <Icon name={icon} size={10} />
      {name}
    </span>
  )
}
