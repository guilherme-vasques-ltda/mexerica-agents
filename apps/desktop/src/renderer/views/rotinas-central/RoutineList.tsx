// A Lista da tela central: as rotinas de todos os bots (filtradas), em três grupos — agendadas pela
// próxima vez, sob demanda (webhook/evento) e pausadas —, com o bot, a agenda (texto do Hub), o estado e
// o botão de ligar/pausar.
import type { Bot } from '@shared/models'
import type { Ref } from 'react'
import { BotAvatar } from '../../components/Avatar'
import { Switch } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { timeLabel } from '../../lib/routine-calendar'
import { isActiveStatus, isOnDemand, type OverviewRoutine, type OverviewRun } from './model'
import { botName } from './parts'
import { RunsOnBadge } from './RunsOnBadge'
import { awaitingRelease } from '../routines/routine-duplicate'
import { runOutcomeText } from '../routines/run-outcome'
import { idioma, texts } from './texts'
import { localeDe } from '../../lib/fork-i18n/index.ts'

function nextLabel(at: number) {
  const date = new Date(at)
  const day = date.toLocaleDateString(localeDe(idioma()), { weekday: 'short', day: '2-digit', month: '2-digit' }).replace('.', '')
  return texts.dayAt(day, timeLabel(at, idioma()))
}

export function RoutineList({ globalPause, routines, runs, bots, invalid, busy, disabled, onOpen, onToggle, copyId, copyRow }: {
  globalPause: boolean | null
  routines: OverviewRoutine[]
  runs: OverviewRun[]
  bots: Map<string, Bot>
  /** Agendas que o Hub não conseguiu projetar (cron ou fuso inválido). */
  invalid: Set<string>
  busy: string | null
  disabled: boolean
  onOpen: (routine: OverviewRoutine) => void
  onToggle: (routine: OverviewRoutine, enabled: boolean) => void
  copyId?: string | null
  copyRow?: Ref<HTMLButtonElement>
}) {
  const scheduled = routines.filter((r) => r.enabled && !isOnDemand(r)).sort((a, b) => (a.nextRunAt ?? Infinity) - (b.nextRunAt ?? Infinity) || a.name.localeCompare(b.name))
  const onDemand = routines.filter((r) => r.enabled && isOnDemand(r)).sort((a, b) => a.name.localeCompare(b.name))
  const paused = routines.filter((r) => !r.enabled).sort((a, b) => a.name.localeCompare(b.name))
  // As execuções vêm da mais antiga para a mais nova.
  const lastRun = new Map<string, OverviewRun>()
  for (const run of runs) lastRun.set(run.routineId, run)

  /** A segunda linha: o que acontece agora, um problema, ou quando roda. */
  const status = (routine: OverviewRoutine): { text: string; tone: 'secondary' | 'danger' | 'accent' | 'warning'; detail?: string } => {
    const run = lastRun.get(routine.id)
    if (run && isActiveStatus(run.status)) return { text: texts.status(run.status) ?? run.status, tone: 'accent' }
    if (routine.lastError || invalid.has(routine.id)) return { text: texts.needsAttention, tone: 'danger' }
    const outcome = run ? runOutcomeText(run.status, run.outcome, idioma()) : null
    const next = run && routine.enabled && globalPause === false && routine.nextRunAt ? ` · ${texts.nextRun(nextLabel(routine.nextRunAt))}` : ''
    // I-023 R2: a frase do desfecho (o que falta, por que falhou, "Segundo o bot") vai na dica e no leitor de tela.
    const detail = outcome?.sentence || undefined
    if (outcome?.tone === 'danger') return { text: `${texts.lastFailed}${next}`, tone: 'danger', detail }
    if (outcome?.tone === 'attention') return { text: `${texts.lastNeedsOwner}${next}`, tone: 'warning', detail }
    if (outcome?.unconfirmed) return { text: `${texts.lastUnconfirmed}${next}`, tone: 'secondary', detail }
    if (awaitingRelease(routine)) return { text: texts.awaitingRelease, tone: 'warning' }
    if (!routine.enabled) return { text: texts.paused, tone: 'secondary' }
    if (globalPause !== false) return { text: globalPause ? texts.globalPaused : texts.pauseUnknown, tone: 'secondary' }
    if (isOnDemand(routine)) return { text: texts.onDemand, tone: 'secondary' }
    return { text: routine.nextRunAt ? texts.nextRun(nextLabel(routine.nextRunAt)) : texts.noNextRun, tone: 'secondary' }
  }

  const section = (title: string, list: OverviewRoutine[]) =>
    list.length ? (
      <section className="rc-list-section" aria-label={title}>
        <h3 className="rc-list-heading">{title}<span className="rc-count">{list.length}</span></h3>
        <ul className="rc-list">
          {list.map((routine) => {
            const bot = bots.get(routine.botId)
            const line = status(routine)
            return (
              <li key={routine.id} className={`rc-row ${routine.enabled ? '' : 'paused'}`}>
                {bot ? <BotAvatar bot={bot} size={30} animated={false} /> : <span className="rc-row-avatar" aria-hidden />}
                <button ref={routine.id === copyId ? copyRow : undefined} className="rc-row-main" onClick={() => onOpen(routine)}>
                  <span className="rc-row-title">
                    <span className="rc-row-name" dir="auto">{routine.name}</span>
                    {isOnDemand(routine) ? <span className="rc-pill"><Icon name="link" size={9} />{routine.triggers.some((t) => t.type === 'webhook') ? texts.webhook : texts.event}</span> : null}
                    <RunsOnBadge runsOn={routine.runsOn} />
                  </span>
                  <span className="rc-row-summary"><bdi dir="auto">{botName(bots, routine.botId)}</bdi> · {routine.summary}</span>
                  <span className={`rc-row-status tone-${line.tone}`} title={line.detail}>
                    {line.text}
                    {line.detail ? <span className="rc-sr-only">. {line.detail}</span> : null}
                  </span>
                </button>
                <Switch className="rc-row-switch" on={routine.enabled} label={texts.toggle(routine.name)} disabled={disabled || busy !== null} onChange={(on) => onToggle(routine, on)} />
              </li>
            )
          })}
        </ul>
      </section>
    ) : null

  return (
    <div className="rc-list-wrap">
      {section(texts.sections.scheduled, scheduled)}
      {section(texts.sections.onDemand, onDemand)}
      {section(texts.sections.paused, paused)}
    </div>
  )
}
