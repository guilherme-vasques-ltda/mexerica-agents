import type { TimesBot, TimesGraph, TimesMember } from '../../../../shared/fork-times.ts'

// A estrutura de Times em árvore, a mesma para o Quadro e a Lista: Você no topo; a principal e as áreas de primeiro
// nível abaixo; numa área, o gerente no topo e o resto abaixo dele; subbots abaixo do bot de cima; Sem time no fim.
// É só desenho: as linhas de Reporta a e as rotas são outra coisa (board-layout.ts).

export type TreeItem =
  | { kind: 'owner'; id: 'owner'; children: TreeItem[] }
  | { kind: 'bot'; id: string; botId: string; children: TreeItem[] }
  | { kind: 'area'; id: string; nodeId: string; children: TreeItem[] }
  | { kind: 'loose'; id: 'loose'; children: TreeItem[] }

export const AREA = 'area:'
export const itemKey = (item: TreeItem) => (item.kind === 'area' ? AREA + item.nodeId : item.id)

const collator = new Intl.Collator('pt-BR', { sensitivity: 'base', numeric: true })

/** A árvore do grafo. Bots sem membro (não deveria acontecer) vão para Sem time; ciclos nunca repetem um item. */
export function structureTree(graph: TimesGraph, bots: readonly TimesBot[], primaryBotId: string | null): TreeItem {
  const names = new Map(bots.map((b) => [b.id, b.name]))
  const byName = (a: TimesMember, b: TimesMember) => collator.compare(names.get(a.botId) ?? a.botId, names.get(b.botId) ?? b.botId) || a.botId.localeCompare(b.botId)
  const members = new Map(graph.members.map((m) => [m.botId, m]))
  const nodes = new Map(graph.nodes.map((n) => [n.id, n]))
  const placed = new Set<string>()
  const areasDone = new Set<string>()

  const subbots = (botId: string) => graph.members.filter((m) => m.parentBotId === botId && members.has(botId)).sort(byName)
  const botItem = (m: TimesMember): TreeItem | null => {
    if (placed.has(m.botId)) return null
    placed.add(m.botId)
    return { kind: 'bot', id: m.botId, botId: m.botId, children: subbots(m.botId).map(botItem).filter((i): i is TreeItem => !!i) }
  }
  const areaItem = (nodeId: string): TreeItem | null => {
    if (areasDone.has(nodeId)) return null
    areasDone.add(nodeId)
    const node = nodes.get(nodeId)!
    const top = graph.members.filter((m) => m.teamId === nodeId && (!m.parentBotId || !members.has(m.parentBotId))).sort(byName)
    const subareas = graph.nodes.filter((n) => n.parentId === nodeId).sort((a, b) => a.position - b.position || collator.compare(a.name, b.name))
    const manager = node.managerBotId ? top.find((m) => m.botId === node.managerBotId) ?? null : null
    const rest = [
      ...top.filter((m) => m !== manager).map(botItem),
      ...subareas.map((n) => areaItem(n.id)),
    ].filter((i): i is TreeItem => !!i)
    if (manager) {
      const lead = botItem(manager)
      if (lead) {
        lead.children.push(...rest)
        return { kind: 'area', id: AREA + nodeId, nodeId, children: [lead] }
      }
    }
    return { kind: 'area', id: AREA + nodeId, nodeId, children: rest }
  }

  const root: TreeItem = { kind: 'owner', id: 'owner', children: [] }
  const primary = primaryBotId ? members.get(primaryBotId) : undefined
  if (primary && !primary.teamId && !primary.parentBotId) {
    const item = botItem(primary)
    if (item) root.children.push(item)
  }
  for (const n of graph.nodes.filter((n) => !n.parentId || !nodes.has(n.parentId)).sort((a, b) => a.position - b.position || collator.compare(a.name, b.name))) {
    const item = areaItem(n.id)
    if (item) root.children.push(item)
  }
  // Áreas presas num ciclo (o Hub não aceita, mas a tela não some com elas).
  for (const n of graph.nodes) {
    const item = areaItem(n.id)
    if (item) root.children.push(item)
  }
  const loose: TreeItem = { kind: 'loose', id: 'loose', children: [] }
  for (const m of [...graph.members].sort(byName)) {
    if (placed.has(m.botId) || (m.parentBotId && members.has(m.parentBotId) && !isStuck(m, members))) continue
    if (m.teamId && nodes.has(m.teamId)) continue
    const item = botItem(m)
    if (item) loose.children.push(item)
  }
  // Quem ficou de fora (bot de cima em ciclo, área sem lugar) também aparece.
  for (const m of [...graph.members].sort(byName)) {
    const item = botItem(m)
    if (item) loose.children.push(item)
  }
  for (const b of bots) {
    if (members.has(b.id) || placed.has(b.id)) continue
    placed.add(b.id)
    loose.children.push({ kind: 'bot', id: b.id, botId: b.id, children: [] })
  }
  if (loose.children.length) root.children.push(loose)
  return root
}

/** Um bot cuja cadeia de bots de cima volta para ele mesmo. */
function isStuck(m: TimesMember, members: Map<string, TimesMember>) {
  const seen = new Set<string>([m.botId])
  let at = m.parentBotId
  while (at) {
    if (seen.has(at)) return true
    seen.add(at)
    at = members.get(at)?.parentBotId ?? null
  }
  return false
}

/** Pai de cada item e a ordem de leitura (de cima para baixo, da esquerda para a direita). */
export function treeIndex(root: TreeItem) {
  const parent = new Map<string, string | null>()
  const order: TreeItem[] = []
  const byKey = new Map<string, TreeItem>()
  const walk = (item: TreeItem, up: string | null) => {
    const key = itemKey(item)
    parent.set(key, up)
    byKey.set(key, item)
    order.push(item)
    for (const child of item.children) walk(child, key)
  }
  walk(root, null)
  return { parent, order, byKey }
}

/** As chaves dos itens acima de `key`, do mais próximo ao topo. */
export function ancestorsOf(index: ReturnType<typeof treeIndex>, key: string): string[] {
  const out: string[] = []
  let at = index.parent.get(key) ?? null
  while (at) {
    out.push(at)
    at = index.parent.get(at) ?? null
  }
  return out
}

/** Quantos bots há abaixo de um item (para o “+N” de um item recolhido). */
export function botsBelow(item: TreeItem): number {
  return item.children.reduce((sum, c) => sum + (c.kind === 'bot' ? 1 : 0) + botsBelow(c), 0)
}
