import type { Bot } from '@shared/models'
import './funcao-etiqueta.css'

/** D133: a função ao lado do nome na barra lateral (`db-tag`, D165). Grupos e bots sem função não mostram nada. */
export function FuncaoEtiqueta({ bot }: { bot: Pick<Bot, 'kind' | 'title'> }) {
  if (bot.kind === 'group' || !bot.title) return null
  return <span className="funcao-etiqueta" title={bot.title}>{bot.title}</span>
}
