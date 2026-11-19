// UI01b / D372: rótulo do modelo da conversa aberta, no cabeçalho, só leitura (o seletor é da C9).
import type { Bot } from '@shared/models'
import { useModel } from '../../../lib/observable'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { BotStore } from '../../../store/bot-store'
import { extrasAllowed } from './ganchos'
import { motorVisivel, rotuloModelo } from './rotulo'
import './rotulo-modelo.css'

export function RotuloModelo({ store, chat, sessionId }: { store: BotStore; chat: Bot | null; sessionId: string | null }) {
  const model = useModel(store.sessions)
  const t = useT()
  const idioma = useIdioma()
  if (!extrasAllowed(store, chat)) return null
  const view = rotuloModelo(model.can(chat.id, 'engine'), model.of(chat.id), chat.id, sessionId)
  if (view.kind === 'hidden') return null
  const surface = { ...superficieIdioma(idioma), className: 'rotulo-modelo idioma-superficie' }
  if (view.kind === 'unavailable') {
    const text = t('sessions.engine.unavailable')
    return (
      <span {...surface} data-state="unavailable" role="note" title={t('sessions.engine.unavailableHint')} aria-label={t('sessions.engine.label', { modelo: text })}>
        {text}
      </span>
    )
  }
  const { engine } = view
  const modelo = engine.model ?? t('sessions.engine.defaultModel')
  const origem = t(engine.source === 'own' ? 'sessions.engine.own' : 'sessions.engine.botDefault')
  const partes = [motorVisivel(engine.backend), modelo, ...(engine.fastMode ? [t('sessions.engine.fast')] : [])].join(' · ')
  return (
    <span {...surface} data-state={engine.source} role="note" title={t('sessions.engine.hint')} aria-label={`${t('sessions.engine.label', { modelo: partes })} · ${origem}`}>
      <bdi className="rm-modelo bidi-isolado">{partes}</bdi>
      <span className="rm-origem">{origem}</span>
    </span>
  )
}
