import { useRef, useState } from 'react'
import type { Space } from '@shared/fork-spaces'
import { isWorking } from '@shared/models'
import type { AppModel } from '../../../store/app-model'
import type { BotStore } from '../../../store/bot-store'
import { useIdioma, useT, superficieIdioma } from '../../../lib/fork-i18n/estado'
import { AnchoredMenu, type MenuItem } from '../../../components/Overlay'
import { Icon } from '../../../components/Icon'
import { SpaceTile } from './SpaceTile'
import { settingsScreen } from '../ajustes/tela'
import { routinesScreen } from '../../rotinas-central/navigation'
import { timesScreen } from '../times/navigation'
import { torreScreen } from '../torre/navigation'
import { extensoesScreen } from '../extensoes/tela'
import './space-selector.css'

export function SpaceSelector({ app, root, active, onOpen, onPersonal, onCreate, onEdit, open, setOpen }: {
  app: AppModel; root: BotStore; active: Space | null; onOpen: (id: string) => void; onPersonal: () => void
  onCreate: () => void; onEdit: (id: string) => void; open: boolean; setOpen: (open: boolean) => void
}) {
  const t = useT()
  const language = superficieIdioma(useIdioma())
  const [editing, setEditing] = useState(false)
  const anchor = useRef<HTMLButtonElement>(null)
  const rows = [...(root.spaces?.spaces ?? [])].sort((a, b) => a.position - b.position)
  const supported = root.hello?.spacesContract === 1
  const choose = (id: string) => {
    for (const screen of [settingsScreen, routinesScreen, timesScreen, torreScreen, extensoesScreen]) screen.close(false)
    setOpen(false); setEditing(false)
    if (id === root.spaces?.personalSpaceId) onPersonal()
    else onOpen(id)
  }
  const items = (): MenuItem[] => editing ? rows.filter(s => s.kind === 'space').map(s => ({
    title: s.name, leading: s.tile ? <SpaceTile tile={s.tile} size={26} /> : undefined,
    action: () => { setOpen(false); setEditing(false); onEdit(s.id) },
  })) : [
    ...rows.map((s, i): MenuItem => {
      const store = app.spaceMirrors.store(root.computer.id, s.id)
      const unread = store?.roster.reduce((n, bot) => n + bot.unread, 0) ?? 0
      const working = store?.roster.some(isWorking)
      return { title: s.kind === 'personal' ? t('navegacao.barra.donna.pessoal') : s.name,
        leading: s.tile ? <SpaceTile tile={s.tile} size={26} /> : <Icon name="person.fill" size={20} />,
        selected: active ? active.id === s.id : s.kind === 'personal', disabled: !supported && s.kind !== 'personal',
        detail: [working ? '●' : '', unread ? String(unread) : '', i < 9 ? `⌘${i + 1}` : ''].filter(Boolean).join(' · '), action: () => choose(s.id) }
    }),
    { title: t('spaces.selector.new'), icon: 'plus', divider: true, disabled: !supported, action: onCreate },
    { title: t('spaces.selector.edit'), icon: 'slider.horizontal.3', disabled: !supported || rows.length < 2, action: () => { requestAnimationFrame(() => { setEditing(true); setOpen(true) }) } },
    ...(!supported ? [{ title: t('spaces.selector.old'), disabled: true, action: () => {} }] : []),
  ]
  return <div className="space-selector no-drag idioma-superficie" {...language}>
    <button className="space-selector-current" ref={anchor} aria-haspopup="menu" aria-expanded={open} aria-label={t('spaces.selector.label')}
      onClick={() => { setEditing(false); setOpen(!open) }}>
      {active?.tile ? <SpaceTile tile={active.tile} size={18} /> : <Icon name="person.fill" size={15} />}
      <span>{active?.name ?? t('navegacao.barra.donna.pessoal')}</span><Icon name="chevron.up.chevron.down" size={10} />
    </button>
    <button className="space-selector-create" aria-label={t('spaces.selector.new')} disabled={!supported} onClick={onCreate}><Icon name="plus" size={13} /></button>
    <AnchoredMenu open={open} anchor={anchor} onClose={() => setOpen(false)} items={items} language={language} />
  </div>
}
