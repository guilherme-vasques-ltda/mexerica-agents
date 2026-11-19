// DON-907 C9a (D375): "Modelo trocado de X para Y. …" no chat; ao passar o mouse ou focar, o detalhe técnico.
import { useId } from 'react'
import type { Entry, ModelSwitch } from '@shared/models'
import { Text } from '../../../components/Text'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { detalheTroca, linhaTroca } from './detalhe'
import './troca-modelo.css'

export function LinhaTroca({ entry, change }: { entry: Entry; change: ModelSwitch }) {
  const tip = useId()
  const t = useT()
  const idioma = useIdioma()
  return (
    <div className="chat-notice idioma-superficie" {...superficieIdioma(idioma)}>
      <span className="troca-modelo" tabIndex={0} aria-describedby={tip}>
        <Text size="footnote" tone="secondary" className="chat-pill glass-raised">{linhaTroca(t, change, entry.data.text ?? '')}</Text>
        <span id={tip} role="tooltip" className="troca-modelo-tip">{detalheTroca(t, idioma, change)}</span>
      </span>
    </div>
  )
}
