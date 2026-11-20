// Peças comuns da tela central "Rotinas": cor do bot, rótulos acessíveis, estados e a faixa
// "Sob demanda".
import type { CSSProperties, ReactNode } from 'react'
import type { Bot } from '@shared/models'
import { Button, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { dayLabel, timeLabel } from '../../lib/routine-calendar'
import { botAccent } from '@shared/fork-avatar-config'
import { isActiveStatus, isOldHub, type CalendarItem, type OverviewRoutine } from './model'
import { RunsOnBadge, runsOnName } from './RunsOnBadge'
import { outcomeLabel, runOutcomeText } from '../routines/run-outcome'
import { errorText } from '../routines/routine'
import { awaitingRelease } from '../routines/routine-duplicate'
import { idioma, texts } from './texts'

/** A cor do bot (a do avatar), para os cartões tingirem por CSS (`--bot`). */
export const botStyle = (bot: Bot | undefined): CSSProperties => ({ '--bot': bot ? botAccent(bot) : 'var(--text-tertiary, var(--ink-3))' } as CSSProperties)

export const botName = (bots: Map<string, Bot>, id: string) => bots.get(id)?.name ?? texts.deletedBot

/** O estado que o cartão mostra: o da execução, "Pausada", ou nada (a próxima vez, ligada). */
export function itemStatus(item: CalendarItem): string | null {
  if (item.run) return runOutcomeText(item.run.status, item.run.outcome, idioma()).label
  return item.pauseUnknown ? texts.pauseUnknown : item.globalPaused ? texts.globalPaused : item.paused ? (awaitingRelease(item.routine) ? texts.awaitingRelease : texts.paused) : null
}

/** Para leitores de tela: nome, bot, dia e hora, o estado e onde roda (D84, quando houver). */
export function itemLabel(item: CalendarItem, bot: string): string {
  const status = item.run ? outcomeLabel(runOutcomeText(item.run.status, item.run.outcome, idioma())) : itemStatus(item)
  const where = runsOnName(item.routine.runsOn)
  return `${texts.item(item.routine.name, bot, dayLabel(item.at, idioma()), timeLabel(item.at, idioma()))}${status ? `, ${status}` : ''}${where ? `, ${texts.runsOnLabel(where)}` : ''}`
}

export function itemClass(item: CalendarItem): string {
  const status = item.run?.status
  // I-023: o tom vem do desfecho; "precisa de você" tem o tom de atenção, sem confirmação fica neutro.
  const tone = item.run ? runOutcomeText(item.run.status, item.run.outcome, idioma()).tone : null
  return [
    item.paused ? 'paused' : '',
    item.run ? 'ran' : '',
    status && isActiveStatus(status) ? 'active' : '',
    tone === 'danger' ? 'failed' : '',
    tone === 'attention' ? 'attention' : '',
    tone === 'ok' ? 'ok' : '',
    status === 'cancelled' ? 'cancelled' : '',
  ].filter(Boolean).join(' ')
}

/** Carregando, erro (com "Tentar de novo") ou Hub antigo, no lugar do conteúdo. */
export function LoadState({ loading, error, retry }: { loading: boolean; error: string | null; retry: () => void }) {
  if (error) {
    return (
      <div className="rc-state" role="alert">
        <Icon name="exclamationmark.triangle" size={18} color="var(--danger)" />
        <strong>{isOldHub(error) ? texts.oldHub : texts.loadError}</strong>
        {isOldHub(error) ? null : <span className="rc-state-detail">{errorText(error)}</span>}
        <Button kind="secondary" onClick={retry}>{texts.retry}</Button>
      </div>
    )
  }
  if (!loading) return null
  return (
    <div className="rc-state" aria-live="polite">
      <Spinner size={16} />
      <span>{texts.loading}</span>
    </div>
  )
}

/** Rotinas sem horário (webhook/evento) e as frequentes demais para a grade, acima do calendário. */
export function OnDemandBand({ onDemand, manual, frequent, bots, onOpen }: {
  onDemand: OverviewRoutine[]
  manual: OverviewRoutine[]
  frequent: OverviewRoutine[]
  bots: Map<string, Bot>
  onOpen: (routine: OverviewRoutine) => void
}) {
  if (!onDemand.length && !manual.length && !frequent.length) return null
  const chips = (routines: OverviewRoutine[], icon: string) =>
    routines.map((routine) => (
      <button
        key={routine.id}
        className={`rc-chip ${routine.enabled ? '' : 'paused'}`}
        style={botStyle(bots.get(routine.botId))}
        title={manual.includes(routine) ? texts.manual : routine.summary}
        aria-label={`${routine.name}, ${botName(bots, routine.botId)}, ${manual.includes(routine) ? texts.manual : routine.summary}${routine.enabled ? '' : `, ${awaitingRelease(routine) ? texts.awaitingRelease : texts.paused}`}${runsOnName(routine.runsOn) ? `, ${texts.runsOnLabel(runsOnName(routine.runsOn)!)}` : ''}`}
        onClick={() => onOpen(routine)}
      >
        <Icon name={icon} size={10} />
        <span className="rc-chip-name" dir="auto">{routine.name}</span>
        <RunsOnBadge runsOn={routine.runsOn} compact />
        <span className="rc-chip-detail"><bdi dir="auto">{botName(bots, routine.botId)}</bdi> · {manual.includes(routine) ? texts.manual : routine.summary}</span>
      </button>
    ))
  return (
    <div className="rc-band">
      {onDemand.length ? <BandRow label={texts.onDemand} hint={texts.onDemandHint}>{chips(onDemand, 'link')}</BandRow> : null}
      {manual.length ? <BandRow label={texts.manual} hint={texts.manualHint}>{chips(manual, 'play.circle')}</BandRow> : null}
      {frequent.length ? <BandRow label={texts.frequent} hint={texts.frequentHint}>{chips(frequent, 'clock.arrow.circlepath')}</BandRow> : null}
    </div>
  )
}

function BandRow({ label, hint, children }: { label: string; hint: string; children: ReactNode }) {
  return (
    <div className="rc-band-row" role="group" aria-label={`${label}: ${hint}`}>
      <span className="rc-band-label" title={hint}>{label}</span>
      <div className="rc-band-chips">{children}</div>
    </div>
  )
}
