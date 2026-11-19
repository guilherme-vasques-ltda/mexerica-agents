import type { ReactNode } from 'react'
import { usePresence } from '../../components/Overlay'
import { Spacer } from '../../components/Stack'
import { Text } from '../../components/Text'
import type { BotStore } from '../../store/bot-store'
import { idiomaAtual, tradutor, type Tradutor } from '../../lib/fork-i18n/estado'
import { formatarData } from '../../lib/fork-i18n/index.ts'
import './settings.css'

export const isMac = window.codync.platform === 'darwin'
/** "this Mac" on macOS; "this computer" elsewhere. */
export const thisMac = isMac ? 'this Mac' : 'this computer'

/** The icon for a device by its account platform: phones, Macs, and other desktops. */
export function deviceIcon(platform: string | null | undefined) {
  return platform === 'ios' ? 'iphone' : platform === 'macos' ? 'laptopcomputer' : 'desktopcomputer'
}

/** Keeps `children` mounted while it fades and folds away. */
export function Reveal({ show, children }: { show: boolean; children: ReactNode }) {
  const { mounted, shown } = usePresence(show)
  if (!mounted) return null
  return (
    <div className={`settings-reveal ${shown ? 'shown' : ''}`}>
      <div>{children}</div>
    </div>
  )
}

/** A section's title row (the CardSection heading). */
export function SectionHeader({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="settings-section-header">
      <Text as="h3" size="body" weight="semibold" tone="text">{title}</Text>
      <Spacer />
      {children}
    </div>
  )
}

export const errorText = (e: unknown) => (e instanceof Error ? e.message : typeof e === 'object' && e && 'message' in e ? String(e.message) : String(e))

/** "3:27 PM", "Yesterday", "Monday" or "Sep 16" (kit's `RelativeTime.day`), no idioma escolhido (D454). */
export function relativeDay(date: Date, now = new Date(), idioma = idiomaAtual()) {
  const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
  const days = Math.round((day(now) - day(date)) / 86_400_000)
  if (days === 0) return formatarData(idioma, date, { hour: '2-digit', minute: '2-digit' })
  if (days === 1) return tradutor(idioma)('ajustes.data.ontem')
  if (now.getTime() - date.getTime() < 6 * 86_400_000) return formatarData(idioma, date, { weekday: 'long' })
  return formatarData(idioma, date, { month: 'short', day: 'numeric' })
}

/** One short line for a computer's connection, used next to its name (D454: no idioma da tela). */
export function statusText(store: BotStore, t: Tradutor = tradutor(idiomaAtual())) {
  const c = store.connection
  switch (c.kind) {
    case 'online':
      return store.mismatch ? t('ajustes.estado.precisaAtualizar') : t('ajustes.estado.conectado')
    case 'connecting':
      return t('ajustes.estado.conectando')
    case 'computerOffline':
      return c.lastSeen ? t('ajustes.estado.desconectadoVisto', { quando: relativeDay(new Date(c.lastSeen)) }) : t('ajustes.estado.desconectado')
    case 'offline':
      return t('ajustes.estado.foraDeAlcance')
    case 'unauthorized':
      return c.message
    case 'unpaired':
      return t('ajustes.estado.naoPareado')
  }
}

/** O estado de conexão mostrado (o `connectionLabel` da store, que ainda é legado) pelo catálogo, no idioma da tela. */
export function rotuloConexao(store: BotStore, t: Tradutor) {
  switch (store.shownConnection.kind) {
    case 'online': return store.mismatch ? t('ajustes.estado.precisaAtualizar') : t('ajustes.estado.conectado')
    case 'connecting': return t('ajustes.estado.conectando')
    case 'computerOffline':
    case 'offline': return t('ajustes.estado.desconectado')
    case 'unauthorized': return t('ajustes.estado.semAcesso')
    case 'unpaired': return t('ajustes.estado.naoPareado')
  }
}

/** The live client, or null while the computer isn't connected (kit's `store.client`). */
export const liveClient = (store: BotStore) => (store.connection.kind === 'online' ? store.client : null)

/** "123 456" for a 6-digit code. */
export const spacedCode = (code: string) => (code.length === 6 ? `${code.slice(0, 3)} ${code.slice(3)}` : code)
