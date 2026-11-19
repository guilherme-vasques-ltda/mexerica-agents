import { useEffect, useState, useSyncExternalStore, type ReactNode } from 'react'
import type { Bot } from '@shared/models'
import type { BotStore } from '../../../store/bot-store'
import type { RosterItem } from '../../../store/app-model'
import type { MenuItem } from '../../../components/Overlay'
import { AnchoredMenu } from '../../../components/Overlay'
import { Icon } from '../../../components/Icon'
import { spaceAllows } from '../../../lib/fork-space-capabilities'
import { idiomaAtual, superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { tradutor } from '../../../lib/fork-i18n/index'
import { collapseKey, groupOf, groupRoster, renameIntent, UNGROUPED } from '../../../lib/bot-groups'
import { useModel } from '../../../lib/observable'
import { usePref } from '../../../lib/prefs'
import { gruposRecolhidos, migrarRecolhimento } from '../../bot-groups/prefs'

let editing: { store: BotStore; id: string } | null = null
const listeners = new Set<() => void>()
function editName(value: typeof editing) { editing = value; listeners.forEach((f) => f()) }
const subscribe = (f: () => void) => { listeners.add(f); return () => { listeners.delete(f) } }
const current = () => editing
const tNow = () => tradutor(idiomaAtual())

export async function createAgentGroup(store: BotStore, botId?: string) {
  try {
    const result = await store.botGroups.addGenerated(tNow()('grupos.nomePadrao'), botId)
    if (result.created) editName({ store, id: result.created })
  } catch { /* modelo publica o aviso */ }
}
export function agentsAddItems(store: BotStore, newAgent?: () => void, library?: () => void): MenuItem[] {
  const t = tNow()
  return [
    ...(newAgent ? [{ title: t('times.quadro.barra.grupo.novoAgente'), icon: 'plus', action: newAgent }] : []),
    ...(store.botGroups.snapshot ? [{ title: t('grupos.menu.novo'), icon: 'folder', disabled: !spaceAllows(store.spaceId, 'teams'), detail: store.spaceId ? t('spaces.selector.unavailable') : undefined, action: () => { void createAgentGroup(store) } }] : []),
    ...(library ? [{ title: t('times.quadro.barra.grupo.biblioteca'), icon: 'book.closed', divider: true, action: library }] : []),
  ]
}
export function moveToGroupItems(store: BotStore, bot: Bot): MenuItem[] {
  const snapshot = store.botGroups.snapshot
  if (!snapshot || !spaceAllows(store.spaceId, 'teams')) return []
  const t = tNow(), currentGroup = groupOf(snapshot, bot.id)
  const options: MenuItem[] = snapshot.groups.filter((g) => g.id !== currentGroup).map((g, index) => ({
    title: t('grupos.bot.mover', { grupo: g.name }), icon: 'folder', divider: index === 0,
    action: () => { void store.botGroups.edit({ op: 'place', botId: bot.id, groupId: g.id }).catch(() => {}) },
  }))
  if (currentGroup) options.push({ title: t('grupos.bot.tirar'), divider: snapshot.groups.length < 2, action: () => { void store.botGroups.edit({ op: 'place', botId: bot.id, groupId: null }).catch(() => {}) } })
  options.push({ title: t('grupos.bot.novoGrupo', { bot: bot.name }), action: () => { void createAgentGroup(store, bot.id) } })
  return options
}

export function AgentGroups({ store, items, compact, filtering, dropTarget, renderRow }: {
  store: BotStore; items: RosterItem[]; compact: boolean; filtering: boolean; dropTarget: string | null; renderRow: (item: RosterItem) => ReactNode
}) {
  const model = useModel(store.botGroups), idioma = useIdioma(), t = useT()
  const [collapsed, setCollapsed] = usePref(gruposRecolhidos)
  const active = useSyncExternalStore(subscribe, current, current)
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null)
  const [name, setName] = useState('')
  const hubId = store.hello?.hostId, spaceId = store.spaceId ?? store.spaces?.personalSpaceId
  useEffect(() => {
    if (!hubId || !spaceId || !model.snapshot || !model.matchesOrigin(hubId, spaceId)) return
    const saved = gruposRecolhidos.get()
    const next = migrarRecolhimento(saved, store.computer.id, hubId, spaceId, model.snapshot.groups.map((group) => group.id))
    if (next !== saved) setCollapsed(next)
  }, [collapsed, hubId, spaceId, model.snapshot, store.computer.id, setCollapsed])
  if (!model.snapshot) return <div className="idioma-superficie" {...superficieIdioma(idioma)}>{items.map(renderRow)}</div>
  const keyFor = (id: string) => collapseKey(store.hello?.hostId ?? store.computer.id, store.spaceId ?? store.spaces?.personalSpaceId ?? '', id)
  const legacyKey = (id: string) => JSON.stringify([store.computer.id, id])
  const isClosed = (id: string) => collapsed.includes(keyFor(id)) || collapsed.includes(legacyKey(id)) || collapsed.includes(id)
  const toggle = (id: string) => { const key = keyFor(id); const rest = collapsed.filter((v) => v !== id && v !== key && v !== legacyKey(id)); setCollapsed(isClosed(id) ? rest : [...rest, key]) }
  const start = (id: string, value: string) => { setName(value); editName({ store, id }) }
  const save = (id: string) => { editName(null); const intent = model.snapshot && renameIntent(model.snapshot, id, name); if (intent) void model.edit(intent).catch(() => {}) }
  const errorKeys = { name_invalid: 'grupos.erro.nomeInvalido', name_taken: 'grupos.erro.nomeUsado', group_missing: 'grupos.erro.grupoSumiu', bot_missing: 'grupos.erro.botSumiu', migrating: 'grupos.erro.atualizando' } as const
  const errorKey = model.error?.code && errorKeys[model.error.code as keyof typeof errorKeys]
  return <div className="times-sidebar idioma-superficie" {...superficieIdioma(idioma)}>
    {model.error ? <p className="agent-group-empty" role="alert">{errorKey ? t(errorKey) : model.error.message || t('grupos.erro.generico')}</p> : null}
    {groupRoster(items, model.snapshot).map((group) => {
      const ungrouped = group.id === UNGROUPED, open = filtering || !isClosed(group.id)
      if (filtering && !group.items.length) return null
      const isEditing = active?.store === store && active.id === group.id
      const menuItems: MenuItem[] = [
        { title: t('grupos.menu.renomear'), action: () => start(group.id, group.name) },
        { title: t(isClosed(group.id) ? 'grupos.menu.mostrar' : 'grupos.menu.recolher'), action: () => toggle(group.id) },
        { title: t('grupos.menu.apagar'), destructive: true, action: () => { void model.edit({ op: 'remove', id: group.id }).catch(() => {}) } },
      ]
      return <div key={group.id} className={`agent-group${dropTarget === group.id ? ' is-over' : ''}`} data-bot-group={group.id} data-computer-id={store.computer.id}>
        <div className="agent-group-head">
          {isEditing && !ungrouped ? <input autoFocus className="agent-group-input" aria-label={t('grupos.nome.rotulo')} defaultValue={group.name} onChange={(e) => setName(e.target.value)} onFocus={(e) => { setName(e.target.value); e.target.select() }} onBlur={() => save(group.id)} onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); if (e.key === 'Escape') editName(null) }} /> :
            <button className="agent-group-label" aria-expanded={open} onClick={() => { if (!filtering) toggle(group.id) }} onDoubleClick={() => { if (!ungrouped && spaceAllows(store.spaceId, 'teams')) start(group.id, group.name) }} onContextMenu={(e) => { if (!ungrouped && spaceAllows(store.spaceId, 'teams')) { e.preventDefault(); setMenu({ id: group.id, x: e.clientX, y: e.clientY }) } }}>
              <span className="agent-group-name" dir="auto">{compact ? (ungrouped ? '·' : [...group.name][0]) : ungrouped ? t('grupos.foraDeGrupo') : group.name}</span>
              {!compact && !ungrouped ? <span className="agent-group-count">{t.plural('grupos.contagem', group.items.length)}</span> : null}<span className="agent-group-rule" /><Icon name="chevron.down" size={9} className="agent-group-chevron" />
            </button>}
        </div>
        {open ? <>{group.items.map(renderRow)}{!ungrouped && !group.items.length && !compact ? <p className="agent-group-empty">{t('grupos.vazio')} {t('grupos.dica')}</p> : null}</> : null}
        {menu?.id === group.id ? <AnchoredMenu open onClose={() => setMenu(null)} point={{ x: menu.x, y: menu.y }} items={() => menuItems} language={superficieIdioma(idioma)} /> : null}
      </div>
    })}
  </div>
}
