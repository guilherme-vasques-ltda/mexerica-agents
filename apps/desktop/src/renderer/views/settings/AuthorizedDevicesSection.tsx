// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useCallback, useEffect, useState } from 'react'
import type { AuthorizedDevice } from '@shared/models'
import { CardSection, IconButton, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Dialog } from '../../components/Overlay'
import { HStack, IconSlot, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useModel } from '../../lib/observable'
import type { BotStore } from '../../store/bot-store'
import { deviceIcon, errorText, liveClient, relativeDay } from './parts'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../lib/fork-i18n/estado'
import { isolado } from '@shared/bidi'

/** The devices that can use a computer this app manages, each one revocable. */
export function AuthorizedDevicesSection({ store }: { store: BotStore }) {
  const t = useT()
  const idioma = useIdioma()
  useModel(store)
  const [devices, setDevices] = useState<AuthorizedDevice[] | { error: string } | null>(null)
  const [confirmRevoke, setConfirmRevoke] = useState<AuthorizedDevice | null>(null)
  const online = store.connection.kind === 'online'
  const name = store.hostName

  const loadDevices = useCallback(async () => {
    const client = liveClient(store)
    if (!client) return
    try {
      setDevices((await client.call<{ devices: AuthorizedDevice[] }>('devices')).devices)
    } catch (e) {
      setDevices({ error: errorText(e) })
    }
  }, [store])

  // Also when it changes account: the devices approved through the old one lose access.
  useEffect(() => void loadDevices(), [loadDevices, online, store.accessRequests.length, store.cloud?.owner?.userId])

  const revoke = async (device: AuthorizedDevice) => {
    try {
      await (await store.ready()).call('revokeDevice', { key: device.key })
    } catch (e) {
      store.setError(errorText(e))
    }
    await loadDevices()
  }

  return (
    <>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <CardSection title={t('ajustes.disp.titulo', { nome: isolado(name) })}>
        {devices === null
          ? [<Spinner key="loading" />]
          : 'error' in devices
            ? [<Text key="error" tone="danger" className="settings-appear">{devices.error}</Text>]
            : [
                ...(devices.length === 0
                  ? [<Text key="none" tone="secondary">{t('ajustes.disp.vazio')}</Text>]
                  : []),
                ...devices.map((device) => (
                  <HStack key={device.key} gap={8} className="settings-row settings-appear">
                    <IconSlot width={18} tone="secondary">
                      <Icon name={deviceIcon(device.platform)} size={13} />
                    </IconSlot>
                    <VStack grow className="settings-stack">
                      <span dir="auto">{device.name}</span>
                      <Text size="caption" tone="tertiary">{deviceDetail(device, t)}</Text>
                    </VStack>
                    <IconButton title={t('ajustes.disp.revogar')} icon="xmark.circle" onClick={() => setConfirmRevoke(device)} />
                  </HStack>
                )),
              ]}
      </CardSection>
      </div>
      <Dialog
        open={confirmRevoke !== null}
        title={confirmRevoke?.name ? t('ajustes.disp.revogarTitulo', { nome: isolado(confirmRevoke.name) }) : t('ajustes.disp.revogarTituloSemNome')}
        message={t('ajustes.disp.revogarMensagem')}
        actions={confirmRevoke ? [{ title: t('ajustes.disp.revogar'), destructive: true, action: () => void revoke(confirmRevoke) }] : []}
        cancel={t('ajustes.cancelar')}
        language={superficieIdioma(idioma)}
        onClose={() => setConfirmRevoke(null)}
      />
    </>
  )
}

function deviceDetail(device: AuthorizedDevice, t: Tradutor) {
  const parts = [device.source === 'account' ? t('ajustes.disp.pelaConta') : t('ajustes.disp.porQr')]
  if (device.connected) parts.push(t('ajustes.disp.conectado'))
  else if (device.lastSeenAt) parts.push(t('ajustes.disp.visto', { quando: relativeDay(new Date(device.lastSeenAt)) }))
  return parts.join(' · ')
}
