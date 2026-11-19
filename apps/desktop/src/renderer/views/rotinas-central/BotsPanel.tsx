// Adaptado de OpenMausBot (Apache-2.0, commit 615fe65e17f88d99f12d34682750d9ae2fa402bc),
// src/components/routines/CalendarSidebar.tsx (alterado): CSS e controles do Donnabot, a função do
// bot é a tag do Hub (D169, a descrição fica para o leitor), um clique filtra pelo bot e "Agendar" é a
// alternativa de teclado ao arrastar.
import { useMemo, useState, type DragEvent } from 'react'
import type { Bot } from '@shared/models'
import { BotAvatar } from '../../components/Avatar'
import { SearchField } from '../../components/Controls'
import { matchesWords, searchWords } from '../../lib/busca'
import { BOT_DRAG_TYPE, dragging } from './model'
import { texts } from './texts'
import { FuncaoEtiqueta } from '../criar-bot/FuncaoEtiqueta'

export function BotsPanel({ bots, selected, onSelect, onSchedule, scheduleWhen, disabled }: {
  bots: Bot[]
  /** O bot do filtro, se houver: a linha fica marcada. */
  selected: string | null
  onSelect: (botId: string) => void
  onSchedule: (botId: string) => void
  /** Quando "Agendar" agenda: a próxima hora cheia (`null`) ou um dia e hora. */
  scheduleWhen: { dia: string; hora: string } | null
  disabled: boolean
}) {
  const [query, setQuery] = useState('')
  const shown = useMemo(() => {
    const words = searchWords(query)
    return bots.filter((bot) => matchesWords(words, [bot.name, bot.description]))
  }, [bots, query])

  const beginDrag = (e: DragEvent, bot: Bot) => {
    e.dataTransfer.effectAllowed = 'copy'
    e.dataTransfer.setData(BOT_DRAG_TYPE, bot.id)
    e.dataTransfer.setData('text/plain', bot.name)
    dragging.bot = bot
  }

  return (
    <section aria-labelledby="rc-bots-heading" className="rc-bots">
      <div className="rc-bots-heading">
        <span id="rc-bots-heading">{texts.myBots}</span>
        <span className="rc-count" aria-label={texts.botCount(bots.length)}>{bots.length}</span>
      </div>
      <SearchField value={query} onChange={setQuery} prompt={texts.searchBots} clearLabel={texts.clearSearch} inputProps={{ 'aria-label': texts.searchBots }} />
      <div role="list" aria-label={texts.botList} className="rc-bot-list">
        {shown.map((bot) => (
          <div
            key={bot.id}
            role="listitem"
            className={`rc-bot ${selected === bot.id ? 'selected' : ''}`}
            draggable={!disabled}
            onDragStart={(e) => beginDrag(e, bot)}
            onDragEnd={() => (dragging.bot = null)}
            title={texts.dragBot(bot.name)}
          >
            <span className="rc-grip" aria-hidden />
            <button className="rc-bot-main" aria-pressed={selected === bot.id} aria-label={texts.onlyBot(bot.name)} aria-description={bot.description || undefined} onClick={() => onSelect(bot.id)}>
              <BotAvatar bot={bot} size={22} animated={false} />
              <span className="rc-bot-text">
                <span className="rc-bot-name" dir="auto">{bot.name}</span>
                <FuncaoEtiqueta bot={bot} />
              </span>
            </button>
            <button className="rc-bot-schedule" disabled={disabled} aria-label={texts.scheduleBot(bot.name, scheduleWhen)} title={texts.scheduleBot(bot.name, scheduleWhen)} onClick={() => onSchedule(bot.id)}>
              {texts.schedule}
            </button>
          </div>
        ))}
        {shown.length === 0 ? <p className="rc-bots-empty">{bots.length === 0 ? texts.noBots : texts.noMatch}</p> : null}
      </div>
      <p className="rc-hint">{texts.dragHint}</p>
    </section>
  )
}
