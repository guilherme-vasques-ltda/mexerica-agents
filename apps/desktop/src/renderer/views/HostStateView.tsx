// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { Button } from '../components/Controls'
import { Icon } from '../components/Icon'
import { Text } from '../components/Text'
import { useApp } from '../store/context'
import { useT } from '../lib/fork-i18n/estado'

const isMac = window.codync.platform === 'darwin'

/** What stands between this computer and the chat, with the one action that fixes it. */
export function HostStateView({ compact = false }: { compact?: boolean }) {
  const app = useApp()
  const t = useT()
  const state = app.host.state
  switch (state.kind) {
    // D76: nada a resolver neste computador; o chat abre nos computadores SSH (showsChat).
    case 'disabled':
      return null
    case 'notInstalled':
      return <EmptyState compact={compact} icon="desktopcomputer" title={t(isMac ? 'navegacao.janela.hub.configurarMac' : 'navegacao.janela.hub.configurar')} message={t('marca.hub.configurar')} action={[t('navegacao.janela.hub.instalar'), () => window.codync.host.install()]} />
    case 'missingBinary':
      return <EmptyState compact={compact} icon="desktopcomputer.trianglebadge.exclamationmark" title={t('navegacao.janela.hub.semCopia')} message={window.codync.appScheme === 'donnabot-dev' ? t('marca.hub.devSemHub') : t('marca.hub.semHub')} />
    case 'failed':
      return <EmptyState compact={compact} icon="desktopcomputer.trianglebadge.exclamationmark" title={t('navegacao.janela.hub.parado')} message={state.message} action={[t('navegacao.janela.tentarDeNovo'), () => window.codync.host.restart()]} />
    default:
      return <EmptyState compact={compact} icon="desktopcomputer" title={t('navegacao.janela.hub.iniciando')} message={t('navegacao.janela.hub.segundos')} />
  }
}

/** A quiet placeholder for an empty screen: icon, title, a line of help and an optional action. */
export function EmptyState({ icon, title, message, action, compact = false }: { icon: string; title: string; message: string; action?: [string, () => void]; compact?: boolean }) {
  return (
    <div className={`empty-state ${compact ? 'local-recovery' : ''}`}>
      <Icon name={icon} size={compact ? 22 : 36} weight="light" color="var(--text-tertiary, var(--ink-3))" />
      <Text as="div" size={compact ? 'callout' : 'title3'} weight="semibold" tone="text">{title}</Text>
      <Text as="div" size="callout" tone="secondary" className="empty-state-message">{message}</Text>
      {action ? (
        <Button kind={compact ? 'secondary' : 'primary'} onClick={action[1]} className="empty-state-action">
          {action[0]}
        </Button>
      ) : null}
    </div>
  )
}

