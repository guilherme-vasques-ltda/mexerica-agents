import { useCallback, useEffect, useState } from 'react'
import { useT } from '../lib/fork-i18n/estado'
import type { CloudStatus, PairingInfo } from '@shared/models'
import { LoopbackTransport } from '../client/host-client'
import { useApp } from '../store/context'
import { PairingCode } from './fork/sob-demanda/telas'

/** The pairing window: this computer's code, without a mirror of its bots. */
export function PairingWindow() {
  const t = useT()
  // By value: the host's snapshot is a new object on each of its changes (every few seconds), and
  // each request issues a new code.
  const { baseURL, token } = useApp().host.local ?? {}
  const [cloudOn, setCloudOn] = useState(false)
  // Sem `hello` ainda, o nome genérico sai do catálogo ao desenhar.
  const [name, setName] = useState<string | null>(null)
  // The window stays open between uses: each "Pair iPhone…" shows it again, with a new code.
  const [shown, setShown] = useState(0)
  useEffect(() => window.codync.app.onCommand((c) => {
    if (c.kind === 'reviewApprovals') setShown((n) => n + 1)
  }), [])

  const requestCode = useCallback(async () => {
    if (!baseURL || !token) throw Object.assign(new Error(), { name: 'PairingWithoutHost' }) // o SEM_HOST do PairingCode, sem carregá-lo
    const transport = new LoopbackTransport(baseURL, token)
    const [pairing, status, hello] = await Promise.all([
      transport.call('pairing', {}, 20_000) as Promise<PairingInfo>,
      transport.call('cloudStatus', {}, 20_000).catch(() => null) as Promise<CloudStatus | null>,
      transport.call('hello', {}, 20_000).catch(() => null) as Promise<{ name?: string } | null>,
    ])
    setCloudOn(status?.enabled === true)
    if (hello?.name) setName(hello.name)
    return pairing
  // R1 do #575: sem `t` nas dependências. O `t` muda a cada desenho e cada pedido gera um código novo.
  }, [baseURL, token])

  return <PairingCode name={name ?? t('navegacao.parear.esteComputador')} cloudOn={cloudOn} requestCode={requestCode} renew={shown} />
}
