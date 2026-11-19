import { useState } from 'react'
import { Button } from '../../components/Controls'
import { useJanelaBaixa } from '../../lib/janela-baixa'
import { useT } from '../../lib/fork-i18n/estado'
import type { RoutinesPauseModel } from '../../store/routines-pause'
import { errorText } from '../routines/routine'

/** Apresenta apenas o estado confirmado pelo Hub. Em janela baixa (E4), uma linha com "Saiba mais". */
export function GlobalPause({ model, offline }: { model: RoutinesPauseModel; offline: boolean }) {
  const { paused, busy, loading, error } = model
  const baixa = useJanelaBaixa()
  const [aberta, setAberta] = useState(false)
  const t = useT()
  const EXPLICACAO = t('rotinas.pausa.explicacao')
  const estado = t(offline ? 'rotinas.pausa.indisponivel' : busy ? 'rotinas.pausa.atualizando' : loading ? 'rotinas.pausa.consultando' : paused === null ? 'rotinas.pausa.indisponivel' : paused ? 'rotinas.pausa.pausadas' : 'rotinas.pausa.valeParaTodas')
  const pausar = <Button kind="secondary" disabled={busy || paused === null || offline} onClick={() => void model.setPaused(!paused)}>
    {t(paused ? 'rotinas.pausa.retomar' : 'rotinas.pausa.pausar')}
  </Button>
  if (!baixa) return <div className="rc-global-pause">
    {pausar}
    <span>{estado} {EXPLICACAO}</span>
    {error ? <span role="alert">{errorText(error)}</span> : null}
  </div>
  // A mesma linha aberta ou fechada: o foco fica em "Saiba mais"/"Mostrar menos".
  return <div className="rc-global-pause">
    <span className={`rc-global-pause-line${aberta ? ' is-open' : ''}`}>
      {pausar}
      <span id="rc-global-pause-text" className="rc-global-pause-state" title={aberta ? undefined : `${estado} ${EXPLICACAO}`}>{estado}{aberta ? ` ${EXPLICACAO}` : null}</span>
      <button type="button" className="rc-global-pause-more" aria-expanded={aberta} aria-controls="rc-global-pause-text" onClick={() => setAberta((a) => !a)}>{t(aberta ? 'rotinas.pausa.mostrarMenos' : 'rotinas.pausa.saibaMais')}</button>
    </span>
    {error ? <span role="alert">{errorText(error)}</span> : null}
  </div>
}
