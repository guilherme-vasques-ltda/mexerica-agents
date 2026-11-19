import { useId, useState } from 'react'
import type { BotStore } from '../../../store/bot-store'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { VozPage } from '../voz/VozPage'
import { abaHorizontalPeloTeclado } from './abas'

const ABAS = [
  ['vivo', 'ajustes.aba.vivo'],
  ['ouvir', 'ajustes.aba.ouvir'],
  ['chaves', 'ajustes.aba.chaves'],
] as const
type AbaVoz = typeof ABAS[number][0]

/** As escolhas de áudio já existentes, por aparelho, usando o Hub do bot aberto. */
export function BotVoiceSection({ store }: { store: BotStore }) {
  const [aba, setAba] = useState<AbaVoz>('vivo')
  const id = useId()
  const t = useT()
  const idioma = superficieIdioma(useIdioma())
  return <div className="bot-settings-voice" {...idioma}>
    <div className="tab-choice bot-settings-voice-tabs" role="tablist" aria-label={t('ajustes.partesDe', { pagina: t('botSettings.abas.voice') })}>
      {ABAS.map(([key, label]) => <button key={key} type="button" role="tab" id={`${id}-${key}`} aria-controls={`${id}-panel`} aria-selected={aba === key} tabIndex={aba === key ? 0 : -1}
        className={`tab-choice-option${aba === key ? ' on' : ''}`} onClick={() => setAba(key)} onKeyDown={(e) => {
          if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return
          const next = abaHorizontalPeloTeclado(ABAS.map(([key]) => key), key, e.key, idioma.dir)
          if (!next) return
          e.preventDefault()
          setAba(next)
          document.getElementById(`${id}-${next}`)?.focus()
        }}>{t(label)}</button>)}
    </div>
    <div role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-${aba}`} tabIndex={0}>
      <VozPage reserva={store} aba={aba} />
    </div>
  </div>
}
