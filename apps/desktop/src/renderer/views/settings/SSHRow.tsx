// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { type SSHProfile, type SSHStatus } from '@shared/ssh'
import { Button, IconButton } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { HStack, IconSlot, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { Reveal } from './parts'
import { CAMINHO_HOST_DEV, COMANDO_IMPRESSAO_CHAVE, sshStatusDetail, textoDoStatus } from '@shared/ssh-startup'
import { isolado, ltr } from '@shared/bidi'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'

export function SSHRow({ profile, status, edit, attached = true }: { profile: SSHProfile; status: SSHStatus; attached?: boolean; edit: () => void }) {
  const bridge = window.codync.ssh
  const t = useT()
  const idioma = useIdioma()
  const dev = window.codync.appScheme === 'donnabot-dev'
  const target = `${profile.user ? `${profile.user}@` : ''}${profile.host}${profile.port !== null ? `:${profile.port}` : ''}`
  const line = (() => {
    switch (status.kind) {
      case 'idle':
        return t('ajustes.ssh.linha.naoConectado', { alvo: ltr(target) })
      case 'connecting':
        return textoDoStatus(status, idioma)
      case 'confirmHostKey':
        return t('ajustes.ssh.linha.confirme', { alvo: ltr(target) })
      case 'connected':
        return t('ajustes.ssh.linha.estado', { alvo: ltr(target), estado: sshStatusDetail(status, attached, idioma) })
      case 'retrying':
        return t('ajustes.ssh.tentandoDeNovo', { mensagem: isolado(textoDoStatus(status, idioma)) })
      case 'failed':
        return textoDoStatus(status, idioma)
      case 'notInstalled':
        return dev ? t('ajustes.ssh.linha.semHostDev', { host: ltr(profile.host), caminho: ltr(CAMINHO_HOST_DEV) }) : t('ajustes.ssh.linha.semHost', { host: ltr(profile.host) })
    }
  })()
  const problem = status.kind === 'retrying' || status.kind === 'failed' || status.kind === 'notInstalled'
  const active = status.kind === 'connected' || status.kind === 'connecting' || status.kind === 'retrying'
  return (
    <div className="settings-card settings-appear idioma-superficie" {...superficieIdioma(idioma)}>
      <div className="settings-row">
        <IconSlot width={22} tone="secondary">
          <Icon name="terminal" size={13} />
        </IconSlot>
        <VStack grow className="settings-stack">
          <Text size="callout" weight="medium" dir={profile.name ? 'auto' : 'ltr'}>{profile.name || profile.host}</Text>
          <Text size="caption" tone={problem ? 'attention' : 'secondary'}>{line}</Text>
        </VStack>
        {active ? (
          <IconButton title={t('ajustes.ssh.desconectar')} icon="stop.circle" onClick={() => bridge.disconnect(profile.id)} />
        ) : (
          <IconButton title={t('ajustes.ssh.conectar')} icon="arrow.clockwise" onClick={() => bridge.connect(profile.id)} />
        )}
        <IconButton title={t('ajustes.ssh.editar')} icon="pencil" onClick={edit} />
        <IconButton title={t('ajustes.ssh.remover')} icon="trash" onClick={() => bridge.remove(profile.id)} />
      </div>
      <Reveal show={status.kind === 'confirmHostKey'}>
        {status.kind === 'confirmHostKey' ? (
          <VStack gap={6}>
            <Text size="caption" tone="secondary">
              {t('ajustes.ssh.primeiraConexao', { nome: isolado(status.name) })}
            </Text>
            {/* O comando e as impressões são código: da esquerda para a direita e isolados, também em árabe. */}
            <code className="settings-mono selectable bidi-isolado" dir="ltr">{COMANDO_IMPRESSAO_CHAVE}</code>
            {status.fingerprints.map((f) => (
              <Text key={f} size="caption" mono className="selectable bidi-isolado" dir="ltr">{f}</Text>
            ))}
            <HStack align="stretch" gap={8}>
              <Button onClick={() => bridge.trustHostKey(profile.id)}>{t('ajustes.ssh.confiar')}</Button>
              <Button kind="secondary" onClick={() => bridge.disconnect(profile.id)}>{t('ajustes.ssh.cancelar')}</Button>
            </HStack>
          </VStack>
        ) : null}
      </Reveal>
    </div>
  )
}
