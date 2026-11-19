import './working-dots.css'

/**
 * D169 (28/10, pedido do dono): the bot at work, shown as the bot — three soft dots under its face in the live bubble
 * (`face`), or before the activity when the Hub doesn't say which bot it is (`inline`, a channel). They only exist while
 * the indicator does (the bot really working in this chat); still with reduced motion. Decoration: the activity's
 * own text ("Iniciando…", "Escrevendo…") is what is read.
 */
export function WorkingDots({ placement }: { placement: 'face' | 'inline' }) {
  return (
    <span className={`working-dots is-${placement}`} aria-hidden>
      <span />
      <span />
      <span />
    </span>
  )
}
