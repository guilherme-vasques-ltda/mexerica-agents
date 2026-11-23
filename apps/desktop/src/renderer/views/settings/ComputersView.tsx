// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useCallback, useState } from 'react'
import { forkFeatures } from '@shared/fork-features'
import type { AccountApproval, PairingInfo } from '@shared/models'
import { DONNABOT_HUB_HOST, newSSHProfile, SSH_STORAGE_STATUS, type SSHProfile } from '@shared/ssh'
import { Button, CardSection, IconButton, Switch } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Dialog, ModalHeader, Sheet, useDismiss } from '../../components/Overlay'
import { HStack, Spacer, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useModel } from '../../lib/observable'
import { useAccount } from '../../store/account'
import type { CloudComputer } from '../../store/cloud'
import type { BotStore } from '../../store/bot-store'
import { useApp } from '../../store/context'
import { ComputerBadge } from '../ComputerBadge'
import { CloudLinkSection } from '../fork/cloud/CloudLinkSection'
import { PairingCode } from '../fork/sob-demanda/telas'
import { AuthorizedDevicesSection } from './AuthorizedDevicesSection'
import { errorText, liveClient, relativeDay, Reveal, SectionHeader, statusText, isMac } from './parts'
import { useSSH } from './ssh-model'
import { capacidades } from '@shared/fork-capacidades'
import { SSHRow } from './SSHRow'
import { ChavesNoHubFolha } from '../fork/voz/VozPage'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../lib/fork-i18n/estado'
import { textoDoStatus, textoSSH } from '@shared/ssh-startup'
import { isolado, ltr } from '@shared/bidi'

/**
 * Fork (D171): com `cloudAccess`, o acesso pela nuvem tem a própria seção (views/fork/cloud) e os
 * controles da ponte legada, inertes nesse caminho, não aparecem. Sem ela, a tela é a de antes.
 */
const cloudAccess = forkFeatures.cloudAccess

/**
 * Computers this app manages (itself and SSH), with their account, relay and authorized devices;
 * SSH profiles; and the account's other computers.
 */
export function ComputersView() {
  const app = useApp()
  const t = useT()
  const idioma = useIdioma()
  const account = useAccount()
  const sshState = useSSH()
  const sshStorage = sshState.state.status[SSH_STORAGE_STATUS]
  const cap = capacidades(window.codync)
  const cloud = useModel(app.cloud)
  const [editing, setEditing] = useState<{ profile: SSHProfile; isNew: boolean } | null>(null)
  const [removingCopy, setRemovingCopy] = useState<CloudComputer | null>(null)
  const managed = app.managedStores
  const managedIds = new Set(managed.map((s) => s.computer.id))
  const others = cloud.computers.filter((c) => !managedIds.has(c.computerId))

  return (
    <div className="settings-computers idioma-superficie" {...superficieIdioma(idioma)}>
      <VStack gap={26} className="settings-computers-content">
        {managed.map((store) => (
          <ManagedComputerCard key={store.computer.id} store={store} ssh={sshState.isSSH(store.computer.id)} />
        ))}

        {cap.ssh ? <div>
          <SectionHeader title={t('ajustes.comp.porSsh')}>
            <IconButton title={t('ajustes.comp.adicionarSsh')} icon="plus" onClick={() => {
              const bridge = window.codync
              const port = bridge.readHostPort ? bridge.readHostPort() : bridge.hostPort
              if (typeof port === 'number') setEditing({ profile: newSSHProfile(port), isNew: true })
            }} />
          </SectionHeader>
          {sshStorage?.kind === 'failed' ? <Text as="div" size="callout" tone="attention">{textoDoStatus(sshStorage, idioma)}</Text> : null}
          <Reveal show={!sshStorage && sshState.state.profiles.length === 0}>
            <Text as="div" size="callout" tone="secondary">
              {isMac ? t('ajustes.comp.sshVazioMac') : t('ajustes.comp.sshVazio')}
            </Text>
          </Reveal>
          {sshState.state.profiles.map((profile) => (
            <SSHRow key={profile.id} profile={profile} status={sshState.status(profile.id)} edit={() => setEditing({ profile, isNew: false })} />
          ))}
        </div> : null}

        {cloudAccess ? (
          <CloudLinkSection />
        ) : (
          <div>
            <SectionHeader title={t('ajustes.comp.outros')}>
              {account.isSignedIn ? <IconButton title={t('ajustes.comp.atualizar')} icon="arrow.clockwise" onClick={() => void cloud.refresh()} /> : null}
            </SectionHeader>
            {!account.isSignedIn ? (
              <Text as="div" size="callout" tone="secondary">{t('ajustes.comp.entreParaVer')}</Text>
            ) : others.length === 0 ? (
              <Text as="div" size="callout" tone="secondary">{t('ajustes.comp.semOutros')}</Text>
            ) : null}
            {others.map((computer) => (
              <AccountComputerRow key={computer.computerId} computer={computer} olderCopy={cloud.isOlderCopy(computer)} onRemove={() => setRemovingCopy(computer)} />
            ))}
          </div>
        )}
      </VStack>

      {cap.ssh ? <Sheet language={superficieIdioma(idioma)} open={editing !== null} onClose={() => setEditing(null)} width={460}>
        {editing ? <SSHProfileEditor key={editing.profile.id} profile={editing.profile} isNew={editing.isNew} /> : null}
      </Sheet> : null}
      <Dialog
        open={removingCopy !== null}
        title={removingCopy?.name ? t('ajustes.comp.removerCopiaTitulo', { nome: isolado(removingCopy.name) }) : t('ajustes.comp.removerCopiaTituloSemNome')}
        message={removingCopy?.name ? t('ajustes.comp.removerCopiaMensagem', { nome: isolado(removingCopy.name) }) : t('ajustes.comp.removerCopiaMensagemSemNome')}
        actions={removingCopy ? [{ title: t('ajustes.comp.remover'), destructive: true, action: () => void cloud.removeFromAccount(removingCopy.computerId) }] : []}
        cancel={t('ajustes.cancelar')}
        language={superficieIdioma(idioma)}
        onClose={() => setRemovingCopy(null)}
      />
    </div>
  )
}


/** This computer or an SSH computer: relay, account membership, pairing and authorized devices. */
function ManagedComputerCard({ store, ssh: overSSH }: { store: BotStore; ssh: boolean }) {
  const app = useApp()
  const t = useT()
  const idioma = useIdioma()
  const account = useAccount()
  useModel(store)
  const [busy, setBusy] = useState(false)
  const [showPairing, setShowPairing] = useState(false)
  const [showVoice, setShowVoice] = useState(false)
  const [confirmUnclaim, setConfirmUnclaim] = useState(false)
  const [confirmAutoApproval, setConfirmAutoApproval] = useState(false)
  const cloud = store.cloud
  const owner = cloud?.owner ?? null
  const autoApproval = cloud?.approval === 'auto'
  const online = store.connection.kind === 'online'
  // A host that needs an update doesn't give its cloud status: nothing there to show or change.
  const manageable = online && !store.mismatch
  const name = store.hostName

  const run = (work: () => Promise<void>) => {
    setBusy(true)
    void work().finally(() => setBusy(false))
  }

  const setApproval = (approval: AccountApproval) =>
    run(async () => {
      try {
        await (await store.ready()).call('setApproval', { approval })
      } catch (e) {
        store.setError(errorText(e))
      }
    })

  const cloudLine = !cloud?.enabled
    ? t('ajustes.comp.nuvemDesligada')
    : (cloud.lastError ?? (cloud.connected ? t('ajustes.comp.nuvemOk') : t('ajustes.comp.nuvemConectando')))

  const accountLine = (
    <HStack key="account" gap={8} className="settings-row settings-appear">
      <Icon name={owner ? 'person.crop.circle.badge.checkmark' : 'person.crop.circle.badge.plus'} size={13} color="var(--context-ink-2, var(--ink-2))" />
      <Text size="callout" grow>
        {owner
          ? owner.userId === account.user?.userId
            ? t('ajustes.comp.naSuaConta')
            : owner.email ? t('ajustes.comp.naConta', { email: ltr(owner.email) }) : t('ajustes.comp.naContaOutra')
          : account.isSignedIn
            ? t('ajustes.comp.foraDaConta')
            : t('ajustes.comp.entreParaAdicionar')}
      </Text>
      {owner ? (
        <Button kind="secondary" onClick={() => setConfirmUnclaim(true)} disabled={busy || !manageable}>
          {t('ajustes.comp.removerDaConta')}
        </Button>
      ) : account.isSignedIn ? (
        <Button kind="secondary" onClick={() => run(async () => void (await app.cloud.claim(store)))} disabled={busy || !manageable}>
          {busy ? t('ajustes.comp.adicionando') : t('ajustes.comp.adicionarConta')}
        </Button>
      ) : null}
    </HStack>
  )

  return (
    <VStack gap={28}>
      <CardSection>
        {[
          <div key="header" className="settings-row">
            <ComputerBadge computer={store.computer} size={26} />
            <VStack grow className="settings-stack">
              <Text size="headline" dir="auto">{name}</Text>
              <Text size="caption" tone="secondary">
                {overSSH ? t('ajustes.comp.porSsh') : isMac ? t('ajustes.grupo.esteMac') : t('ajustes.grupo.esteComputador')} · {statusText(store, t)}
              </Text>
            </VStack>
            {overSSH ? (
              // This computer's voice settings are their own Settings page.
              <IconButton title={t('ajustes.comp.chavesVoz')} icon="waveform" selected={showVoice} disabled={!online} onClick={() => setShowVoice((s) => !s)} />
            ) : null}
            <IconButton title={t('ajustes.comp.parear')} icon="qrcode" selected={showPairing} disabled={!online} onClick={() => setShowPairing((s) => !s)} />
          </div>,
          // Until the host gives its cloud status, there's no way to tell its relay or account.
          // Fork: com `cloudAccess`, posse e vínculo ficam na seção "Pela nuvem"; estes são da ponte legada.
          ...(cloud && !cloudAccess
            ? [
                <Switch key="cloud" className="settings-appear" on={cloud.enabled} disabled={busy || !manageable} onChange={(on) => run(() => app.cloud.setCloud(store, on))}>
                  <span className="settings-stack">
                    <span>{t('ajustes.comp.alcancar')}</span>
                    <Text size="caption" tone={cloud.lastError ? 'attention' : 'secondary'}>{cloudLine}</Text>
                  </span>
                </Switch>,
                accountLine,
              ]
            : []),
          ...(owner
            ? [
                <Switch
                  key="approval"
                  on={autoApproval}
                  disabled={busy || !manageable}
                  onChange={(on) => (on ? setConfirmAutoApproval(true) : setApproval('code'))}
                >
                  <span className="settings-stack settings-appear">
                    <span>{t('ajustes.comp.pularCodigo')}</span>
                    <Text size="caption" tone={autoApproval ? 'attention' : 'secondary'}>
                      {autoApproval ? t('ajustes.comp.entramSozinhos') : t('ajustes.comp.cadaDispositivo')}
                    </Text>
                  </span>
                </Switch>,
              ]
            : []),
        ]}
      </CardSection>

      <AuthorizedDevicesSection store={store} />

      <Sheet language={superficieIdioma(idioma)} open={showVoice} onClose={() => setShowVoice(false)}>
        <ChavesNoHubFolha store={store} />
      </Sheet>
      <Sheet language={superficieIdioma(idioma)} open={showPairing} onClose={() => setShowPairing(false)} width={340}>
        <ModalHeader closeTitle={t('ajustes.fechar')} icon="qrcode" title={t('ajustes.comp.parearCom', { nome: isolado(name) })} />
        <PairingPanel store={store} />
      </Sheet>
      <Dialog
        open={confirmAutoApproval}
        title={t('ajustes.comp.semCodigoTitulo')}
        message={t('marca.nuvem.controleTotal', { nome: isolado(name) })}
        actions={[{ title: t('ajustes.comp.pularConferencia'), destructive: true, action: () => setApproval('auto') }]}
        cancel={t('ajustes.cancelar')}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirmAutoApproval(false)}
      />
      <Dialog
        open={confirmUnclaim}
        title={t('ajustes.comp.removerTitulo', { nome: isolado(name) })}
        message={t('ajustes.comp.removerMensagem')}
        actions={[{ title: t('ajustes.comp.removerDaConta'), destructive: true, action: () => run(() => app.cloud.unclaim(store)) }]}
        cancel={t('ajustes.cancelar')}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirmUnclaim(false)}
      />
    </VStack>
  )
}

/** An account computer this app isn't authorized on, or one it uses under an earlier identity (`olderCopy`). */
function AccountComputerRow({ computer, olderCopy, onRemove }: { computer: CloudComputer; olderCopy: boolean; onRemove: () => void }) {
  const t = useT()
  return (
    <div className="settings-card settings-appear">
      <div className="settings-row">
        <span className={olderCopy ? 'settings-off' : undefined}>
          <ComputerBadge computer={{ id: computer.computerId, name: computer.name, signKey: computer.signKey, urls: [], device: computer.device ?? null }} size={22} />
        </span>
        <div className="settings-stack">
          <Text size="callout" weight="medium" tone={olderCopy ? 'secondary' : undefined} dir="auto">{computer.name}</Text>
          <Text size="caption" tone={olderCopy ? 'tertiary' : 'secondary'}>{accountComputerStatus(computer, olderCopy, t)}</Text>
        </div>
        <Spacer />
        {olderCopy ? <IconButton title={t('ajustes.comp.removerDaConta')} icon="trash" onClick={onRemove} /> : null}
      </div>
    </div>
  )
}

/** "Older copy", not the phones' "Older copy of this computer": on the desktop "this computer" reads as this Mac. */
function accountComputerStatus(computer: CloudComputer, olderCopy: boolean, t: Tradutor) {
  if (!olderCopy) return computer.online ? t('ajustes.estado.conectado') : t('ajustes.estado.desconectado')
  return computer.lastSeenAt ? t('ajustes.comp.copiaAntigaVista', { quando: relativeDay(new Date(computer.lastSeenAt)) }) : t('ajustes.comp.copiaAntiga')
}

/** "Pair iPhone" for a computer this app manages, through its store. */
function PairingPanel({ store }: { store: BotStore }) {
  useModel(store)
  const t = useT()
  const requestCode = useCallback(async () => {
    const client = liveClient(store)
    if (!client) throw new Error(t('ajustes.comp.naoConectado', { nome: isolado(store.hostName) }))
    return client.call<PairingInfo>('pairing')
  }, [store, t])
  return <PairingCode name={store.hostName} cloudOn={store.cloud?.enabled === true} requestCode={requestCode} />
}

// MARK: SSH

/** Add or change an SSH computer. Only fields ssh takes as separate arguments; nothing goes through a shell. */
function SSHProfileEditor({ profile: initial, isNew }: { profile: SSHProfile; isNew: boolean }) {
  const dismiss = useDismiss()
  const [profile, setProfile] = useState(initial)
  const [port, setPort] = useState(initial.port?.toString() ?? '')
  const [remotePort, setRemotePort] = useState(String(initial.remotePort))
  const [user, setUser] = useState(initial.user ?? '')
  const [problem, setProblem] = useState<string | null>(null)
  const bridge = window.codync.ssh
  const t = useT()
  const idioma = useIdioma()

  const save = async () => {
    const isInt = (s: string) => /^-?\d+$/.test(s)
    const portText = port.trim()
    if (portText && !isInt(portText)) return setProblem(t('ajustes.comp.portaNumero'))
    const remote = remotePort.trim()
    if (!isInt(remote)) return setProblem(t('marca.ssh.portaNumero'))
    const u = user.trim()
    const p: SSHProfile = {
      ...profile,
      host: profile.host.trim(),
      name: profile.name.trim(),
      user: u || null,
      port: portText ? Number(portText) : null,
      remotePort: Number(remote),
    }
    const found = await bridge.save(p)
    if (found) return setProblem(textoSSH(found, idioma))
    dismiss()
  }

  // Host, usuário e porta são técnicos (da esquerda para a direita, também em árabe); o nome é do usuário (dir="auto").
  const field = (label: string, value: string, set: (v: string) => void, prompt: string, dir: 'ltr' | 'auto' = 'ltr') => (
    <HStack gap={12} className="settings-row">
      <Text tone="text">{label}</Text>
      <input className="settings-field" dir={dir} aria-label={label} placeholder={prompt} value={value} spellCheck={false} onChange={(e) => set(e.target.value)} />
    </HStack>
  )

  return (
    // Return in a field saves (the default action).
    <VStack className="settings-ssh-editor idioma-superficie" {...superficieIdioma(idioma)} onKeyDown={(e) => e.key === 'Enter' && e.target instanceof HTMLInputElement && void save()}>
      <ModalHeader closeTitle={t('ajustes.fechar')} icon="terminal" title={isNew ? t('ajustes.comp.adicionarSsh') : t('ajustes.comp.editarSsh')} trailing={<IconButton title={isNew ? t('ajustes.comp.adicionar') : t('ajustes.comp.salvar')} icon="checkmark" onClick={() => void save()} />} />
      <VStack gap={16} className="settings-ssh-editor-body">
        <CardSection>
          {field(t('ajustes.comp.nome'), profile.name, (name) => setProfile({ ...profile, name }), t('ajustes.comp.opcional'), 'auto')}
          {field(t('ajustes.comp.host'), profile.host, (host) => setProfile({ ...profile, host }), t('ajustes.comp.hostExemplo', { exemplo: ltr(DONNABOT_HUB_HOST) }))}
          {field(t('ajustes.comp.usuario'), user, setUser, t('ajustes.comp.doSshConfig'))}
          {field(t('ajustes.comp.portaSsh'), port, setPort, t('ajustes.comp.doSshConfig'))}
          <div className="settings-row">
            <span>{t('ajustes.comp.chave')}</span>
            <Spacer />
            {profile.identityFile ? <Text tone="secondary" className="bidi-isolado" dir="ltr">{profile.identityFile.split('/').pop()}</Text> : <Text tone="secondary">{t('ajustes.comp.agentePadrao')}</Text>}
            <IconButton
              title={t('ajustes.comp.escolherChave')}
              icon="key"
              onClick={() => void bridge.chooseKey().then((path) => path && setProfile((p) => ({ ...p, identityFile: path })))}
            />
            {profile.identityFile ? (
              <HStack as="span" align="stretch" className="settings-appear">
                <IconButton title={t('ajustes.comp.usarAgente')} icon="xmark.circle" onClick={() => setProfile({ ...profile, identityFile: null })} />
              </HStack>
            ) : null}
          </div>
          {field(t('marca.ssh.porta'), remotePort, setRemotePort, '')}
        </CardSection>
        <Text size="caption" tone="secondary">
          {t('marca.ssh.tunel')}
        </Text>
        <Reveal show={problem !== null}>
          <Text size="caption" tone="danger">{problem}</Text>
        </Reveal>
      </VStack>
    </VStack>
  )
}
