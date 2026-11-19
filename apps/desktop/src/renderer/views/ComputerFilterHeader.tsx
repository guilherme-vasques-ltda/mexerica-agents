// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { Computer } from '@shared/models'
import { sshSummary, sshStatusDetail } from '@shared/ssh-startup'
import { DropdownMenu } from '../components/Controls'
import { Icon } from '../components/Icon'
import type { MenuItem } from '../components/Overlay'
import { Text } from '../components/Text'
import { useApp } from '../store/context'
import { useSSH } from './settings/ssh-model'
import type { Connection } from '../store/bot-store'
import { idiomaAtual, superficieIdioma, tradutor, useIdioma, useT, type Tradutor } from '../lib/fork-i18n/estado'
import { isolado, ltr } from '@shared/bidi'
import { rotuloConexao } from './settings/parts'

/** An empty exclusion list means all computers, including newly paired ones. */
export function computerSelection(all: string[], hidden: string) {
  const excluded = new Set(hidden.split(',').filter(Boolean))
  const visible = all.filter((id) => !excluded.has(id))
  const shown = visible.length ? visible : all
  const encoded = (ids: Set<string>) => [...ids].sort().join(',')
  return {
    shown,
    toggling(id: string) {
      if (!all.includes(id)) return encoded(new Set(all.filter((x) => !shown.includes(x))))
      const next = new Set(shown)
      if (next.has(id)) {
        if (next.size > 1) next.delete(id)
      } else next.add(id)
      return encoded(new Set(all.filter((x) => !next.has(x))))
    },
    only(id: string) {
      return all.includes(id) ? encoded(new Set(all.filter((x) => x !== id))) : ''
    },
  }
}

// D454 (fatia 4): o resumo e o menu dos computadores no idioma escolhido.
function summary(connections: Connection[], t: Tradutor = tradutor(idiomaAtual())) {
  if (!connections.length) return t('ajustes.filtro.computadores')
  const online = connections.filter((c) => c.kind === 'online').length
  if (online === connections.length) return t.plural('ajustes.filtro.conectados', online)
  if (online > 0) return t('ajustes.filtro.parcial', { online, total: connections.length })
  if (connections.some((c) => c.kind === 'connecting')) return t('ajustes.estado.conectando')
  if (connections.every((c) => c.kind === 'unauthorized')) return t('ajustes.filtro.semAcesso')
  if (connections.every((c) => c.kind === 'unpaired')) return t('ajustes.filtro.naoPareado')
  return t.plural('ajustes.filtro.desconectados', connections.length)
}

/** The computers' choices (show all, each one, only one, reconnect, manage): the header's menu, and Barra04's search filter. */
export function useComputerMenu(hidden: string, setHidden: (v: string) => void, manage: () => void) {
  const app = useApp()
  const ssh = useSSH()
  const t = useT()
  const idioma = useIdioma()
  const pending = ssh.state.profiles.filter((p) => !ssh.state.attachments.some((a) => a.profileId === p.id))
  const pendingSummary = sshSummary(pending.map((p) => ssh.status(p.id)), idioma)
  const selection = computerSelection(app.computers.map((c) => c.id), hidden)
  const stores = selection.shown.map((id) => app.store(id)).filter((s) => !!s)
  const online = stores.filter((s) => s.shownConnection.kind === 'online').length
  const text = pendingSummary
    ? (online ? t('ajustes.filtro.comPendentes', { conectados: t.plural('ajustes.filtro.conectados', online), pendentes: pendingSummary }) : pendingSummary)
    : summary(stores.map((s) => s.shownConnection), t)
  const title = (c: Computer) => {
    const store = app.store(c.id)
    return store ? `${isolado(store.hostName)} · ${rotuloConexao(store, t)}` : isolado(c.name)
  }
  const items = (): MenuItem[] => {
    const list: MenuItem[] = [{ title: t('ajustes.filtro.todos'), selected: selection.shown.length === app.computers.length, action: () => setHidden('') }]
    for (const c of app.computers) list.push({ title: title(c), selected: selection.shown.includes(c.id), action: () => setHidden(selection.toggling(c.id)) })
    for (const p of pending) {
      const status = ssh.status(p.id)
      const detail = sshStatusDetail(status, false, idioma)
      list.push({ title: `${p.name ? isolado(p.name) : ltr(p.host)} · ${detail}`, action: manage })
    }
    if (app.computers.length > 1) app.computers.forEach((c, i) => list.push({ title: t('ajustes.filtro.so', { nome: isolado(c.name) }), divider: i === 0, action: () => setHidden(selection.only(c.id)) }))
    if (stores.length) list.push({ title: t('ajustes.filtro.reconectar'), icon: 'arrow.clockwise', divider: true, action: () => stores.forEach((s) => s.restartStream()) })
    if (app.host.state.kind === 'notInstalled') list.push({ title: window.codync.platform === 'darwin' ? t('ajustes.filtro.instalarMac') : t('ajustes.filtro.instalar'), icon: 'desktopcomputer', action: () => window.codync.host.install() })
    if (app.host.state.kind === 'failed') list.push({ title: t('ajustes.filtro.tentarDeNovo'), icon: 'arrow.clockwise', action: () => window.codync.host.restart() })
    list.push({ title: t('ajustes.filtro.gerenciar'), icon: 'desktopcomputer', action: manage })
    return list
  }
  return { text, items }
}

/** The roster's connection summary, with details and filters on demand. */
export function ComputerFilterHeader({ hidden, setHidden, manage, compact = false, onMenuOpenChange }: { hidden: string; setHidden: (v: string) => void; manage: () => void; compact?: boolean; onMenuOpenChange?: (open: boolean) => void }) {
  const { text, items } = useComputerMenu(hidden, setHidden, manage)
  const t = useT()
  const idioma = useIdioma()
  return (
    <DropdownMenu items={items} title={text} language={superficieIdioma(idioma)} className={`computer-filter ${compact ? 'computer-filter-action' : ''}`} onOpenChange={onMenuOpenChange}>
      {compact ? <Icon name="desktopcomputer" size={12} weight="medium" /> : <Text truncate className="idioma-superficie" {...superficieIdioma(idioma)} aria-label={t('ajustes.filtro.rotulo', { filtro: text })}>{text}</Text>}
      <Icon name="chevron.down" size={8} weight="semibold" />
    </DropdownMenu>
  )
}
