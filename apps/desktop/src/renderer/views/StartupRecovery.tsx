import { isSSHConnecting } from '@shared/ssh-startup'
import { useT } from '../lib/fork-i18n/estado'
import { Button, Spinner } from '../components/Controls'
import { Icon } from '../components/Icon'
import { Text } from '../components/Text'
import { useApp } from '../store/context'
import { HostStateView } from './HostStateView'
import { SSHRow } from './settings/SSHRow'
import { useSSH } from './settings/ssh-model'
import './startup-recovery.css'

/** Saved remote computers remain visible while no computer is ready for a chat. */
export function StartupRecovery({ manage }: { manage: () => void }) {
  const t = useT()
  const app = useApp()
  const ssh = useSSH()
  const connecting = ssh.state.profiles.some((p) => isSSHConnecting(ssh.status(p.id)))
  return (
    <div className="startup-recovery">
      <div className="detail-drag drag" />
      <div className="startup-recovery-content">
        <div className="startup-recovery-title" role="status">
          {connecting ? <Spinner size={22} /> : <Icon name="desktopcomputer" size={22} color="var(--context-ink-2, var(--ink-2))" />}
          <Text size="title2" weight="semibold">{t(connecting ? 'navegacao.janela.vazio.conectando' : 'navegacao.janela.remotos')}</Text>
        </div>
        <Text as="div" tone="secondary">{t('navegacao.janela.remotosDetalhe')}</Text>
        <div className="startup-recovery-profiles">
          {ssh.state.profiles.map((p) => (
            <SSHRow key={p.id} profile={p} status={ssh.status(p.id)} attached={ssh.state.attachments.some((a) => a.profileId === p.id)} edit={manage} />
          ))}
        </div>
        <Button kind="secondary" onClick={manage}>{t('navegacao.janela.gerenciarComputadores')}</Button>
        {app.host.state.kind !== 'running' ? <HostStateView compact /> : null}
      </div>
    </div>
  )
}
