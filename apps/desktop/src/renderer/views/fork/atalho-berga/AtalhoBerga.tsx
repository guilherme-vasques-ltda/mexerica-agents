import { Button, CardSection, Switch } from '../../../components/Controls'
import { useT } from '../../../lib/fork-i18n/estado'
import { prefs, usePref } from '../../../lib/prefs'
import { spotlightKeyboardSettings } from '@shared/fork-berga-shortcut'

/** D548: preferência deste aparelho, sem sincronizar com o Hub ou modificar o sistema. */
export function AtalhoBerga() {
  const t = useT()
  const [on, setOn] = usePref(prefs.commandSpaceBerga)
  if (window.codync.platform !== 'darwin') return null
  return <CardSection title={t('ajustes.berga.titulo')} footer={t('ajustes.berga.escopo')}>
    <Switch on={on} onChange={setOn}>{t('ajustes.berga.opcao')}</Switch>
    {on ? <div>
      <p role="status">{t('ajustes.berga.spotlight')}</p>
      <Button onClick={() => window.codync.app.openSettings(spotlightKeyboardSettings)}>{t('ajustes.berga.abrir')}</Button>
    </div> : null}
  </CardSection>
}
