import { useRef, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import type { TimesBot, TimesGraph } from '@shared/fork-times'
import { Icon } from '../../../components/Icon'
import type { BoardAction } from './TimesBoard'
import { botsBelow, itemKey, type TreeItem } from './structure'
import { OwnerFace, TimesFace, useTimesTextos } from './parts'
import { frase, plural } from '../../../lib/fork-i18n/frase'

// A Lista (components/TimesView): a mesma estrutura do Quadro em linhas, no padrão WAI-ARIA de árvore. É o caminho
// para teclado e leitor de tela: ↑ ↓ andam pelas linhas visíveis, → abre ou entra, ← fecha ou sobe, Home e End,
// Enter mostra o detalhe, R Reporta a, M Mover e Shift+F10 o mesmo menu do quadro.

interface Row { item: TreeItem; key: string; level: number; parent: string | null; expanded: boolean | null }

function visibleRows(root: TreeItem, collapsed: ReadonlySet<string>): Row[] {
  const rows: Row[] = []
  const walk = (item: TreeItem, level: number, parent: string | null) => {
    const key = itemKey(item)
    const open = item.children.length ? !collapsed.has(key) || item.kind === 'owner' : null
    rows.push({ item, key, level, parent, expanded: open })
    if (open) for (const c of item.children) walk(c, level + 1, key)
  }
  walk(root, 1, null)
  return rows
}

export function TimesList({ root, graph, bots, primaryBotId, collapsed, selected, onSelect, onToggle, onAction }: {
  root: TreeItem
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  primaryBotId: string | null
  collapsed: ReadonlySet<string>
  selected: string | null
  onSelect: (key: string) => void
  onToggle: (key: string) => void
  onAction: (action: BoardAction, key: string, invoker: HTMLElement) => void
}) {
  const tree = useRef<HTMLDivElement>(null)
  const rows = visibleRows(root, collapsed)
  const stop = selected && rows.some((r) => r.key === selected) ? selected : rows[0]?.key ?? null
  const focus = (key: string | null | undefined) => {
    if (!key) return
    onSelect(key)
    requestAnimationFrame(() => tree.current?.querySelector<HTMLElement>(`[data-times-row="${CSS.escape(key)}"]`)?.focus())
  }
  const onKey = (e: ReactKeyboardEvent<HTMLDivElement>, row: Row) => {
    if (e.altKey || e.metaKey || e.ctrlKey) return
    const at = rows.findIndex((r) => r.key === row.key)
    const isBot = row.item.kind === 'bot'
    const handled = (() => {
      switch (e.key) {
        case 'ArrowDown': focus(rows[at + 1]?.key); return true
        case 'ArrowUp': focus(rows[at - 1]?.key); return true
        case 'ArrowRight':
          if (row.expanded === false) onToggle(row.key)
          else if (row.expanded) focus(rows[at + 1]?.key)
          return true
        case 'ArrowLeft':
          if (row.expanded && row.item.kind !== 'owner') onToggle(row.key)
          else focus(row.parent)
          return true
        case 'Home': focus(rows[0]?.key); return true
        case 'End': focus(rows.at(-1)?.key); return true
        case 'Enter': case ' ': onSelect(row.key); return true
        case 'r': case 'R': if (isBot && !e.shiftKey) { onSelect(row.key); onAction('reports', row.key, e.currentTarget); return true } return false
        case 'm': case 'M': if (isBot && !e.shiftKey) { onSelect(row.key); onAction('move', row.key, e.currentTarget); return true } return false
        case 'F10': if (e.shiftKey) { onSelect(row.key); onAction('menu', row.key, e.currentTarget); return true } return false
        case 'ContextMenu': onSelect(row.key); onAction('menu', row.key, e.currentTarget); return true
        default: return false
      }
    })()
    if (handled) { e.preventDefault(); e.stopPropagation() }
  }

  const x = useTimesTextos()
  return (
    <div ref={tree} className="times-list" role="tree" aria-label={x.tx(frase('times.quadro.lista.rotulo'))}>
      {rows.map((row) => {
        const { item } = row
        const label = rowLabel(item, graph, bots, primaryBotId, x)
        return (
          <div
            key={row.key}
            role="treeitem"
            aria-level={row.level}
            aria-expanded={row.expanded ?? undefined}
            aria-selected={selected === row.key}
            aria-label={label.aria}
            aria-description={item.kind === 'bot' ? x.tx(frase('times.quadro.lista.teclas')) : undefined}
            tabIndex={stop === row.key ? 0 : -1}
            data-times-row={row.key}
            className={`times-row${selected === row.key ? ' is-selected' : ''}`}
            style={{ paddingInlineStart: 8 + (row.level - 1) * 18 }}
            onClick={() => onSelect(row.key)}
            onKeyDown={(e) => onKey(e, row)}
          >
            {row.expanded !== null && item.kind !== 'owner' ? (
              <button type="button" className="times-row-toggle" tabIndex={-1} aria-hidden onClick={(e) => { e.stopPropagation(); onToggle(row.key) }}>
                <Icon name={row.expanded ? 'chevron.down' : 'chevron.right'} size={10} weight="semibold" />
              </button>
            ) : <span className="times-row-toggle" aria-hidden />}
            {item.kind === 'bot' ? <TimesFace bot={bots.get(item.botId) ?? null} size={22} /> : item.kind === 'owner' ? <OwnerFace size={22} /> : <span className="times-row-icon" aria-hidden><Icon name={item.kind === 'area' ? 'person.2' : 'list.bullet'} size={12} /></span>}
            <span className="times-row-name" dir="auto">{label.name}</span>
            {label.tag ? <span className="times-tag">{label.tag}</span> : null}
            <span className="times-row-detail" dir="auto">{label.detail}</span>
            {item.kind === 'bot' || item.kind === 'area' ? (
              <button type="button" className="times-row-menu" tabIndex={-1} aria-hidden onClick={(e) => { e.stopPropagation(); onSelect(row.key); onAction('menu', row.key, e.currentTarget) }}>
                <Icon name="ellipsis" size={12} />
              </button>
            ) : null}
          </div>
        )
      })}
    </div>
  )
}

function rowLabel(item: TreeItem, graph: TimesGraph, bots: ReadonlyMap<string, TimesBot>, primaryBotId: string | null, x: ReturnType<typeof useTimesTextos>) {
  const { T, tx, ROLE_LABEL, nameOf, reportsLabel } = x
  if (item.kind === 'owner') return { name: T.owner, tag: null, detail: tx(frase('times.quadro.lista.recebe')), aria: T.owner }
  if (item.kind === 'loose') return { name: T.loose, tag: null, detail: tx(plural('times.quadro.lista.foraAreas', botsBelow(item))), aria: tx(frase('times.quadro.lista.semEquipeAria', { bots: plural('times.quadro.contagem.bots', botsBelow(item)) })) }
  if (item.kind === 'area') {
    const node = graph.nodes.find((n) => n.id === item.nodeId)
    const manager = node?.managerBotId ? nameOf(bots, node.managerBotId) : null
    const area = tx(frase('times.quadro.lista.area'))
    const nome = node?.name ?? area
    return { name: nome, tag: area, detail: tx(manager ? frase('times.quadro.lista.gerente', { nome: manager }) : frase('times.quadro.lista.semGerente')), aria: tx(manager ? frase('times.quadro.board.areaGerente', { nome, gerente: manager }) : frase('times.quadro.board.area', { nome })) }
  }
  const m = graph.members.find((y) => y.botId === item.botId)
  const primary = item.botId === primaryBotId
  const tag = primary ? tx(frase('times.quadro.board.principal')) : m?.role === 'manager' ? ROLE_LABEL.manager : null
  const reports = m ? reportsLabel(bots, m) : '—'
  const name = nameOf(bots, item.botId)
  return { name, tag, detail: tx(frase('times.quadro.lista.reportaA', { nome: reports })), aria: tx(frase(tag ? 'times.quadro.board.cartaoPapel' : 'times.quadro.board.cartao', { nome: name, papel: tag ?? '', alvo: reports })) }
}
