import type { AccountService } from '../account'
import { RelayLink, type RelayLinkOptions } from './relay-socket.ts'

export type AccountRelay = { kind: 'started'; link: RelayLink; stop(): void } | { kind: 'notEligible' | 'superseded' }

/** The factory owns one captured session and never accepts a seal from IPC or a renderer. */
export async function openAccountRelay(account: AccountService, options: RelayLinkOptions): Promise<AccountRelay> {
  const seal = account.captureCloudSession()
  if (!seal || !account.state.cloudURL || options.cloudURL !== account.state.cloudURL) return { kind: 'notEligible' }
  const binding = account.verifiedDeviceBinding(seal)
  if (!binding || binding.deviceKey !== Buffer.from(options.signer.publicKey).toString('base64url')) return { kind: 'notEligible' }
  let link: RelayLink | null = null
  let cancelled = false
  const current = () => !cancelled && account.isCloudSessionCurrent(seal)
  const stop = () => { cancelled = true; account.off('epoch', onEpoch); link?.stop() }
  const onEpoch = () => { if (!current()) stop() }
  account.on('epoch', onEpoch)
  try {
    await account.tokenForCloud(seal)
    if (!current()) { stop(); return { kind: 'superseded' } }
    link = new RelayLink({ ...options, isAuthorized: current,
      onState: (state) => { if (current()) options.onState?.(state) },
      onChannel: (channel) => { if (!channel || current()) options.onChannel?.(channel) },
    })
    if (!current()) { stop(); return { kind: 'superseded' } }
    link.start()
    if (!current()) { stop(); return { kind: 'superseded' } }
    return { kind: 'started', link, stop }
  } catch {
    stop()
    return { kind: 'superseded' }
  }
}
