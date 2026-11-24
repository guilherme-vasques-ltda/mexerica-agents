// O editor de rotina do Codync (`RoutineEditorView`) a partir da tela central: abre uma rotina, ou
// cria uma nova depois de escolher o bot (o editor precisa de um). Um horário da agenda vira o
// começo da agenda: toda semana, naquele dia e hora.
import type { Bot } from '@shared/models'
import { BotAvatar } from '../../components/Avatar'
import { ModalHeader, Sheet } from '../../components/Overlay'
import { dayLabel, timeLabel } from '../../lib/routine-calendar'
import type { Routine } from '../routines/routine'
import { RoutineEditorView } from '../routines/RoutineEditorView'
import { weeklyAt } from './model'
import { idioma, texts } from './texts'
import { superficieIdioma } from '../../lib/fork-i18n/estado'

/** O que está aberto: uma rotina, ou uma nova (com bot e horário quando vieram da agenda). */
export interface EditorTarget {
  /** Um por abertura (a `key` do editor). */
  id: string
  routine: Routine | null
  botId: string | null
  at: number | null
}

export function RoutineSheet({ target, setTarget, bots, saved, duplicated, avisaCriacao = false }: {
  target: EditorTarget | null
  setTarget: (target: EditorTarget | null) => void
  /** Os bots que podem ter rotinas. */
  bots: Bot[]
  saved: () => void
  duplicated: (target: EditorTarget, routine: Routine) => void
  avisaCriacao?: boolean
}) {
  const bot = target?.botId ? bots.find((b) => b.id === target.botId) : undefined
  const when = target?.at ? texts.dayTime(dayLabel(target.at, idioma()), timeLabel(target.at, idioma())) : null
  const onSaved = (current: EditorTarget, routine: Routine | null) => {
    saved()
    // Uma rotina nova por webhook continua aberta: a URL e a chave só existem agora.
    if (!current.routine && routine?.triggers.some((t) => t.type === 'webhook')) setTarget({ ...current, routine })
    else setTarget(null)
  }
  return (
    <Sheet language={superficieIdioma(idioma())} open={target !== null} onClose={() => setTarget(null)} width={540}>
      {target && target.botId ? (
        <RoutineEditorView
          key={target.id}
          botId={target.botId}
          avisaCriacao={avisaCriacao}
          routine={target.routine}
          initialTriggers={target.at ? weeklyAt(target.at) : undefined}
          subtitle={bot ? texts.forBot(bot.name) : undefined}
          saved={(routine) => onSaved(target, routine)}
          duplicated={(routine) => duplicated(target, routine)}
        />
      ) : target ? (
        // Portal no body (Sheet): declara o idioma da tela (DON-917).
        <div className="rc-pick idioma-superficie" {...superficieIdioma(idioma())}>
          <ModalHeader icon="clock" title={texts.newRoutineTitle} subtitle={when ? texts.chooseBotAt(when) : texts.chooseBot} closeTitle={texts.close} />
          {bots.length ? (
            <div className="rc-pick-list" role="list">
              {bots.map((b) => (
                <div role="listitem" key={b.id}>
                  <button className="rc-pick-bot" onClick={() => setTarget({ ...target, botId: b.id })}>
                    <BotAvatar bot={b} size={32} animated={false} />
                    <span className="rc-bot-text">
                      <span className="rc-bot-name" dir="auto">{b.name}</span>
                      <span className="rc-bot-role">{b.description || texts.bot}</span>
                    </span>
                  </button>
                </div>
              ))}
            </div>
          ) : (
            <p className="rc-pick-empty">{texts.noAgentBots}</p>
          )}
        </div>
      ) : null}
    </Sheet>
  )
}
