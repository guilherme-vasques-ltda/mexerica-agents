import type { ReactNode } from 'react'
import { isGroup, type Bot, type Entry } from '@shared/models'
import { BotAvatar } from '../../../components/Avatar'
import { useStoreRef as useStore } from '../../../store/context'
import { ChartCard } from './ChartCard'
import { genuiChartOf, type ChartEntry } from './chart-entry'

// Gráfico de leitura no chat: o cartão na lista de mensagens. ChatRows (herdado) só chama `genuiChartRow`; o resto
// é deste módulo. Não há resposta nem I/O: o gráfico só mostra o que o Hub guardou, e reabrir a conversa lê a
// mesma entrada, sem chamar fornecedor.

/** Gancho de ChatRows: o gráfico quando a entrada é `genuiChart` válida; senão null e a linha segue o texto literal. */
export function genuiChartRow(entry: Entry, chat: Bot | null, groupStart: boolean): ReactNode {
  const card = genuiChartOf(entry)
  if (!card) return null
  return <div className={groupStart ? 'chat-row group-start' : 'chat-row'}><GenuiChartRow entry={entry} card={card} chat={chat} /></div>
}

function GenuiChartRow({ entry, card, chat }: { entry: Entry; card: ChartEntry; chat: Bot | null }) {
  const store = useStore()
  const author = entry.data.author ? store.bots.get(entry.data.author) : chat && !isGroup(chat) ? chat : undefined
  return <GenuiChartMessage card={card} name={store.authorName(entry.data.author ?? chat?.id)} face={author ? <BotAvatar bot={author} size={28} animated={false} /> : null} />
}

/** Sem store: o balão do bot com o gráfico real. Também serve à prévia com dados fictícios. */
export function GenuiChartMessage({ card, name, face }: { card: ChartEntry; name?: string; face?: ReactNode }) {
  return (
    <div className="message agent with-face genui-chart-message">
      <span className="message-face" aria-hidden>{face}</span>
      <div className="message-body" role="group" aria-label={name}>
        <div className="bubble agent">
          <ChartCard chart={card.chart} />
        </div>
      </div>
    </div>
  )
}
