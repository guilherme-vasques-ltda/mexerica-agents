// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { ReactNode } from 'react'
import { Button, IconButton, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { useScreenViewed } from '../../store/analytics'
import { useComputerAccess } from './use-computer-access'
import appIcon from '../../../../resources/icon.svg'
import './computer-access.css'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { comNos, no } from '../settings/rotulo'
import { ltr } from '@shared/bidi'

/** Shared by onboarding and Settings. Permission requests remain explicit and local. */
export function ComputerAccessView({ onContinue }: { onContinue?: (ready: boolean) => void }) {
  const access = useComputerAccess()
  const t = useT()
  const idioma = useIdioma()
  useScreenViewed('computer_access')
  const { screen, step, busy } = access
  const mac = window.codync.platform === 'darwin'
  const windows = window.codync.platform === 'win32'
  const ready = step === 'ready'
  const canRequest = ['capture', 'input', 'portal', 'ready'].includes(step)
  const helperReady = screen?.enabled === true && screen.connected && step !== 'background'
  const completed = windows ? Number(ready) : Number(helperReady) + (mac
    ? Number(screen?.capture === true) + Number(screen?.input === true)
    : Number(screen?.capture === true && screen.input))
  const request = (permission: 'capture' | 'input' | 'portal', title: string) => (
    <Button kind={step === permission ? 'primary' : 'secondary'} disabled={!canRequest || busy !== null} onClick={() => void access.request(permission)}>
      {busy === permission ? <><Spinner /> {t('ajustes.acesso.esperandoPermissao')}</> : title}
    </Button>
  )
  return (
    <section className={`computer-access idioma-superficie ${onContinue ? 'onboarding' : ''}`} {...superficieIdioma(idioma)} aria-label={t('ajustes.acesso.configurar')}>
      <div className="computer-access-content">
        <header className="computer-access-header">
          <div className="computer-access-identity">
            <img src={appIcon} width={40} height={40} alt={t('marca.acesso.iconeAlt')} />
            <div><span>{t('ajustes.acesso.esteComputador')}</span><strong>{access.computerName}</strong></div>
          </div>
          <h1>{ready ? t('ajustes.acesso.pronto') : t('ajustes.acesso.titulo')}</h1>
          <p>{windows ? t('ajustes.acesso.introWindows') : t('ajustes.acesso.intro')}</p>
        </header>
        {step === 'host' ? <AccessNotice title={t('ajustes.acesso.esperandoComputador')} icon="desktopcomputer">
          {t('marca.acesso.reiniciar')}
        </AccessNotice> : null}
        {step === 'unavailable' ? <AccessNotice title={t('ajustes.acesso.semScreen', { app: ltr(access.helperName) })} icon="desktopcomputer.trianglebadge.exclamationmark">
          {t('marca.acesso.instalar')}
        </AccessNotice> : null}
        {access.error ? <AccessNotice title={t('ajustes.acesso.erroConfigurar')} icon="exclamationmark.circle" error>{access.error}</AccessNotice> : null}
        <div className="computer-access-section-label">
          <span>{windows ? t('ajustes.acesso.disponibilidade') : t('ajustes.acesso.permissoes')}</span>
          <span>{ready ? t('ajustes.acesso.tudoPronto') : t('ajustes.acesso.prontas', { feitas: completed, total: mac ? 3 : windows ? 1 : 2 })}<IconButton title={t('ajustes.acesso.conferir')} icon="arrow.clockwise" size={22} onClick={() => void access.refresh()} /></span>
        </div>
        <div className="computer-access-steps">
          {windows ? <AccessRow title={t('ajustes.acesso.controle')} icon="cursorarrow.motionlines" detail={t('ajustes.acesso.controleWindows')} done={ready} /> : <>
            <AccessRow title={access.helperName} icon="desktopcomputer" detail={t('ajustes.acesso.screenDetalhe')} done={helperReady} active={['enable', 'helper', 'background'].includes(step)} status={step === 'background' ? t('ajustes.acesso.precisaAprovacao') : undefined}>
              {['enable', 'helper'].includes(step) ? <Button disabled={busy !== null} onClick={() => void access.enable()}>{busy === 'enable' ? <><Spinner /> {t('ajustes.acesso.iniciando')}</> : step === 'helper' ? t('ajustes.acesso.iniciarDeNovo') : t('ajustes.acesso.ativarScreen', { app: ltr(access.helperName) })}</Button> : null}
              {step === 'background' ? <><p className="computer-access-inline-note">{t('marca.acesso.segundoPlano')}</p><Button onClick={() => access.openSettings('background')}>{t('ajustes.acesso.itensInicio')} <Icon name="arrow.up.right.square" size={10} /></Button></> : null}
            </AccessRow>
            {mac ? <>
              <AccessRow title={t('ajustes.acesso.gravacao')} icon="record.circle" detail={t('marca.acesso.gravar')} done={screen?.capture === true} active={step === 'capture'}>
                {step === 'capture' ? <SettingsHint app={access.helperName} /> : null}
                {canRequest ? request('capture', t('ajustes.acesso.permitirGravacao')) : null}
                {access.requested.has('capture') ? <SettingsLink action={() => access.openSettings('capture')} /> : null}
              </AccessRow>
              <AccessRow title={t('ajustes.acesso.controlarApps')} icon="cursorarrow.motionlines" detail={t('marca.acesso.controlar')} done={screen?.input === true} active={step === 'input'}>
                {step === 'input' ? <SettingsHint app={access.helperName} control /> : null}
                {canRequest ? request('input', t('ajustes.acesso.permitirControle')) : null}
                {access.requested.has('input') ? <SettingsLink action={() => access.openSettings('input')} /> : null}
              </AccessRow>
            </> : <AccessRow title={t('ajustes.acesso.compartilhar')} icon="cursorarrow.motionlines" detail={t('ajustes.acesso.compartilharDetalhe')} done={screen?.capture === true && screen.input} active={step === 'portal'}>
              {canRequest ? request('portal', t('ajustes.acesso.escolherTela')) : null}
            </AccessRow>}
          </>}
        </div>
        <ul className="computer-access-notes">
          <li><Icon name="lock.shield" size={13} /><p>{onContinue ? t('ajustes.acesso.notaOnboarding') : t('ajustes.acesso.notaAjustes')}</p></li>
          <li><Icon name="iphone" size={13} /><p>{t('marca.acesso.iphone')}</p></li>
        </ul>
        {onContinue ? <footer className="computer-access-footer">
          <Button kind={ready ? 'primary' : 'secondary'} onClick={() => onContinue(ready)}>{ready ? t('ajustes.acesso.continuar') : t('ajustes.acesso.depois')}{ready ? <Icon name="arrow.right" size={12} /> : null}</Button>
        </footer> : null}
      </div>
    </section>
  )
}

function AccessRow({ title, icon, detail, done, active = false, status, children }: {
  title: string; icon: string; detail: string; done: boolean; active?: boolean; status?: string; children?: ReactNode
}) {
  const t = useT()
  const label = done ? t('ajustes.acesso.feito') : status ?? (active ? t('ajustes.acesso.precisaAcesso') : t('ajustes.acesso.esperando'))
  return <div className={`computer-access-row ${done ? 'granted' : active ? 'current' : 'waiting'}`}>
    <span className="computer-access-badge" role="img" aria-label={label} title={label}>
      {done ? <Icon name="checkmark" size={12} weight="bold" /> : <Icon name={icon} size={15} />}
    </span>
    <div className="computer-access-row-body">
      <h2>{title}</h2>
      <p>{detail}</p>
      {!done ? <div className="computer-access-actions">{children}</div> : null}
    </div>
  </div>
}

function AccessNotice({ title, icon, error = false, children }: { title: string; icon: string; error?: boolean; children: ReactNode }) {
  return <div className={`computer-access-notice ${error ? 'error' : ''}`} role={error ? 'alert' : 'status'}>
    <Icon name={icon} size={16} /><div><strong>{title}</strong><p>{children}</p></div>
  </div>
}

function SettingsHint({ app, control = false }: { app: string; control?: boolean }) {
  const t = useT()
  // "Device Control and Data Access" é o nome do painel no macOS (em inglês, como o sistema o mostra).
  const macos = 'Device Control and Data Access' // textos-ok: nome do painel do macOS, igual em todo idioma
  return <p className="computer-access-inline-note">
    {comNos(t('ajustes.acesso.dicaProcure', { app: no('app') }), { app: <bdi dir="ltr"><strong>{app}</strong></bdi> })}
    {control ? <>{' '}{comNos(t('ajustes.acesso.dicaControle', { macos: no('macos'), acessibilidade: no('acessibilidade') }), { macos: <bdi dir="ltr"><strong>{macos}</strong></bdi>, acessibilidade: <strong>{t('ajustes.acesso.acessibilidade')}</strong> })}</> : null}
    {' '}{t('ajustes.acesso.dicaVolte')}
  </p>
}

function SettingsLink({ action }: { action: () => void }) {
  const t = useT()
  return <Button kind="secondary" onClick={action}>{t('ajustes.acesso.abrirAjustes')} <Icon name="arrow.up.right.square" size={10} /></Button>
}
