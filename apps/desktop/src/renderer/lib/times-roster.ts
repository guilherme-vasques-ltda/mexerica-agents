import type { TimesSnapshot, TimesNode } from '../../shared/fork-times.ts'
import { matchesWords, mergeOrder, searchWords } from './busca.ts'

/** Somente dados que o produtor da lista já permite mostrar; nunca expandir pelo DTO administrativo. */
export interface TimesRow { bot: { id: string; name: string; pinned: boolean; hidden?: boolean } }
export interface TimesPresentation {
  order: string[]
  /** Barra02: equipes recolhidas. Barra04 inverteu o padrão (recolhido); o valor antigo fica guardado e é ignorado. */
  folded: string[]
  /** Barra04 (dono, 05/11/2026): as equipes que a pessoa abriu neste aparelho. Ausente = todas recolhidas. */
  open: string[]
  members: Record<string, string[]>
  filter: string | null
}
export const emptyTimesPresentation = (): TimesPresentation => ({ order: [], folded: [], open: [], members: {}, filter: null })
/** Os bots sem equipe (Agentes): chave da ordem local e do filtro "Agentes". */
export const NO_TEAM = '$no-team'
export const teamKey = (id: string | null) => id === null ? NO_TEAM : `team:${id}`
/** Barra04: o cargo do chefe vem do nível da equipe (sem campo no Hub). */
export const CHIEF_TITLES = ['Gerente', 'Coordenação', 'Responsável'] as const
export const chiefTitle = (level: number) => CHIEF_TITLES[Math.min(Math.max(level, 1), 3) - 1]!
export interface TimesGroup<T> {
  id: string
  parentId: string | null
  name: string
  /** Caminho das equipes acima ("Marketing › Social"), vazio no topo. */
  path: string
  /** 1 no topo, 2 logo abaixo, 3 e além (a cor da etiqueta e o cargo param no 3). */
  level: number
  chiefId: string | null
  /**
   * Chefe gravado que a barra pode mostrar, mas é membro de outra equipe (ou de Agentes): a linha de referência no
   * topo desta equipe. Não é membro: fica fora de `items`, `rows`, `members` e `below` (contagem, ordem, arraste, busca).
   */
  chiefRef: { item: T; home: string } | null
  /** Chefe gravado que a barra não pode mostrar (oculto, a principal, fora da lista): "Confira o chefe no quadro". */
  checkChief: boolean
  /** Aberta: guardada pela pessoa ou aberta só durante a busca (`temporary`). */
  open: boolean
  temporary: boolean
  /** Todos os membros visíveis autorizados (chefe primeiro); rows é exatamente o desenhado desta equipe. */
  items: T[]
  rows: T[]
  /**
   * Revisão #355: a linha da equipe está na tela (o topo, ou a equipe-mãe aberta). As de baixo de uma recolhida
   * também vêm, com `shown: false`, porque o corpo recolhido continua montado e escondido.
   */
  shown: boolean
  /** O corpo da equipe, aberto ou não: os membros sem o chefe (na busca, só os que batem). */
  members: T[]
  /** Quem a linha recolhida representa além do chefe: membros e equipes abaixo, desenhados ou não. */
  below: T[]
}
/** `groups`: as equipes, em ordem de árvore (pai antes dos filhos; `shown` diz quais linhas estão na tela); `simple`: Agentes. */
export interface TimesProjection<T> { groups: TimesGroup<T>[]; rows: T[]; simple: T[]; filtering: boolean; parents: Record<string, string | null> }

export function parseTimesPresentation(value: unknown): TimesPresentation {
  const strings = (v: unknown): v is string[] => Array.isArray(v) && v.every((x) => typeof x === 'string')
  if (!value || typeof value !== 'object' || Array.isArray(value)) return emptyTimesPresentation()
  const v = value as Record<string, unknown>
  if (!strings(v.order) || !strings(v.folded) || !v.members || typeof v.members !== 'object' || Array.isArray(v.members)
    || !(v.filter === null || typeof v.filter === 'string') || !Object.values(v.members).every(strings) || !(v.open === undefined || strings(v.open))) return emptyTimesPresentation()
  return { order: [...v.order], folded: [...v.folded], open: v.open ? [...v.open as string[]] : [], members: Object.fromEntries(Object.entries(v.members).map(([k, ids]) => [k, [...ids as string[]]])), filter: v.filter }
}

/** Abre ou fecha uma equipe na apresentação guardada. */
export function setTimesOpen(pref: TimesPresentation, id: string, open: boolean): TimesPresentation {
  const rest = pref.open.filter((x) => x !== id)
  return { ...pref, open: open ? [...rest, id] : rest }
}

function localOrder<T extends TimesRow>(items: T[], saved: string[], chiefId: string | null): T[] {
  const ids = items.map((i) => i.bot.id)
  const order = mergeOrder(ids, [...new Set(saved)].filter((id) => ids.includes(id)))
  const rank = new Map(order.map((id, index) => [id, index]))
  return [...items].sort((a, b) => Number(b.bot.id === chiefId) - Number(a.bot.id === chiefId)
    || Number(b.bot.pinned) - Number(a.bot.pinned) || rank.get(a.bot.id)! - rank.get(b.bot.id)!)
}

/**
 * Fonte única para sidebar/busca/setas. Não conhece Store, armazenamento, RPC ou seções legadas.
 * Barra04: cada equipe é uma linha (o chefe); aberta, mostra membros e subequipes. Recolhida é o padrão. A busca
 * mostra só quem bate, com as linhas das equipes acima para contexto, e abre as recolhidas enquanto durar.
 */
export function projectTimes<T extends TimesRow>(items: T[], snapshot: TimesSnapshot | null, pref: TimesPresentation, query = '', primaryId = snapshot?.primaryBotId ?? null): TimesProjection<T> {
  const words = searchWords(query)
  const visible = items.filter((i) => !i.bot.hidden && i.bot.id !== primaryId)
  if (!snapshot) {
    const simple = visible.filter((i) => matchesWords(words, [i.bot.name]))
    return { groups: [], simple, rows: simple, filtering: words.length > 0, parents: {} }
  }
  const nodes = new Map(snapshot.nodes.map((n) => [n.id, n]))
  const members = new Map(snapshot.members.map((m) => [m.botId, m]))
  const assigned = (item: T) => {
    const member = members.get(item.bot.id)
    if (!member || (member.teamId !== null && !nodes.has(member.teamId))) return null
    return member.teamId === null ? NO_TEAM : teamKey(member.teamId)
  }
  const ordered: TimesNode[] = []
  const seen = new Set<string>()
  const walk = (parentId: string | null) => {
    const siblings = snapshot.nodes.filter((n) => n.parentId === parentId)
      .sort((a, b) => a.position - b.position || a.name.localeCompare(b.name, 'pt-BR') || a.id.localeCompare(b.id))
    const keys = siblings.map((n) => teamKey(n.id))
    const ids = mergeOrder(keys, [...new Set(pref.order)].filter((id) => keys.includes(id)))
    siblings.sort((a, b) => ids.indexOf(teamKey(a.id)) - ids.indexOf(teamKey(b.id)))
    for (const n of siblings) if (!seen.has(n.id)) { seen.add(n.id); ordered.push(n); walk(n.id) }
  }
  walk(null)
  // Parser do contrato não é validador de grafo: apresentação também termina com resposta inválida cíclica.
  // Nó fora da árvore (pai ausente ou ciclo) vira equipe de topo, para nunca sumir nem prender a recursão.
  const orphans = snapshot.nodes.filter((n) => !seen.has(n.id))
  for (const n of orphans) { seen.add(n.id); ordered.push(n) }
  const rooted = new Set(orphans.map((n) => n.id))
  const parentOf = (n: TimesNode) => (rooted.has(n.id) || !n.parentId || !nodes.has(n.parentId) ? null : n.parentId)
  const pathOf = (n: TimesNode) => {
    const names: string[] = [], visited = new Set([n.id])
    let parent = parentOf(n) && nodes.get(parentOf(n)!)
    while (parent && !visited.has(parent.id)) { visited.add(parent.id); names.unshift(parent.name); parent = parentOf(parent) && nodes.get(parentOf(parent)!) }
    return names
  }
  const filter = pref.filter && (pref.filter === NO_TEAM || ordered.some((n) => parentOf(n) === null && teamKey(n.id) === pref.filter)) ? pref.filter : null
  const filtering = words.length > 0 || filter !== null
  const matches = (i: T) => matchesWords(words, [i.bot.name])
  // Cada equipe com os seus membros, já na ordem local, antes de decidir o que se desenha.
  const built = ordered.map((node) => {
    const id = teamKey(node.id)
    const all = visible.filter((i) => assigned(i) === id)
    const manager = node.managerBotId
    // Chefe membro desta equipe é a linha dela, mesmo chefiando outras; chefe de outra equipe vira a referência.
    const chiefId = manager && all.some((i) => i.bot.id === manager) ? manager : null
    const away = manager && !chiefId ? visible.find((i) => i.bot.id === manager) : undefined
    const home = away && assigned(away)
    const chiefRef = away ? { item: away, home: home && home !== NO_TEAM ? nodes.get(home.slice('team:'.length))!.name : 'Agentes' } : null
    return { node, id, chiefId, chiefRef, checkChief: !!manager && !chiefId && !chiefRef, sorted: localOrder(all, pref.members[id] ?? [], chiefId) }
  })
  const kids = (nodeId: string) => built.filter((b) => parentOf(b.node) === nodeId)
  const belowOf = (b: (typeof built)[number]): T[] => [...b.sorted.filter((i) => i.bot.id !== b.chiefId), ...kids(b.node.id).flatMap((k) => [...k.sorted.filter((i) => i.bot.id === k.chiefId), ...belowOf(k)])]
  const hit = (b: (typeof built)[number]): boolean => b.sorted.some(matches) || kids(b.node.id).some(hit)
  const groups: TimesGroup<T>[] = []
  const draw = (b: (typeof built)[number], level: number, shown: boolean) => {
    if (words.length && !hit(b)) return
    const saved = pref.open.includes(b.id)
    const members = b.sorted.filter((i) => i.bot.id !== b.chiefId)
    const temporary = words.length > 0 && !saved && (members.some(matches) || kids(b.node.id).some(hit))
    const open = saved || temporary
    const chief = b.sorted.find((i) => i.bot.id === b.chiefId)
    const body = members.filter((i) => !words.length || matches(i))
    groups.push({ id: b.id, parentId: parentOf(b.node) ? teamKey(parentOf(b.node)) : null, name: b.node.name, path: pathOf(b.node).join(' › '), level,
      chiefId: b.chiefId, chiefRef: b.chiefRef, checkChief: b.checkChief, open, temporary, items: b.sorted, below: belowOf(b), shown, members: body,
      rows: shown ? [...(chief ? [chief] : []), ...(open ? body : [])] : [] })
    for (const k of kids(b.node.id)) draw(k, level + 1, shown && open)
  }
  for (const b of built) if (parentOf(b.node) === null && (!filter || filter === b.id)) draw(b, 1, true)
  const agents = localOrder(visible.filter((i) => assigned(i) === null || assigned(i) === NO_TEAM), pref.members[NO_TEAM] ?? [], null)
  const simple = filter && filter !== NO_TEAM ? [] : agents.filter(matches)
  return { groups: filter === NO_TEAM ? [] : groups, simple, rows: [...(filter === NO_TEAM ? [] : groups.flatMap((g) => g.rows)), ...simple], filtering, parents: Object.fromEntries(snapshot.members.map((m) => [m.botId, m.parentBotId])) }
}

/**
 * As equipes a abrir (e guardar abertas) para mostrar um bot que está escondido: as acima dele e a dele, menos a que
 * ele chefia (o chefe já é a linha da equipe). Vale para a busca e para quem chega de fora (uma notificação).
 */
export function teamsToReveal(snapshot: TimesSnapshot | null, botId: string): string[] {
  if (!snapshot) return []
  const nodes = new Map(snapshot.nodes.map((n) => [n.id, n]))
  const member = snapshot.members.find((m) => m.botId === botId)
  const out: string[] = [], visited = new Set<string>()
  for (let n = member?.teamId ? nodes.get(member.teamId) : undefined; n && !visited.has(n.id); n = n.parentId ? nodes.get(n.parentId) : undefined) {
    visited.add(n.id)
    if (n.id !== member!.teamId || n.managerBotId !== botId) out.push(teamKey(n.id))
  }
  return out
}

/** Setas/menu procuram um irmão elegível; a linha vizinha pode ser um filho de outro pai. */
export function nextTimesSibling<T extends TimesRow>(projection: TimesProjection<T> | undefined, id: string, offset: number): T | null {
  if (!projection || (offset !== -1 && offset !== 1)) return null
  const group = projection.groups.find((g) => g.rows.some((i) => i.bot.id === id))
  if (!group || id === group.chiefId) return null
  const index = group.rows.findIndex((i) => i.bot.id === id)
  const item = group.rows[index]!
  for (let at = index + offset; at >= 0 && at < group.rows.length; at += offset) {
    const target = group.rows[at]!
    if (target.bot.id !== group.chiefId && target.bot.pinned === item.bot.pinned
      && projection.parents[target.bot.id] === projection.parents[id]) return target
  }
  return null
}

/** Só troca irmãos do mesmo Time/pin; Chefe, Sem time e hierarquia nunca são alvos de associação. */
export function moveTimesMember<T extends TimesRow>(projection: TimesProjection<T>, pref: TimesPresentation, id: string, targetId: string): TimesPresentation {
  const group = projection.groups.find((g) => g.rows.some((i) => i.bot.id === id))
  const item = group?.rows.find((i) => i.bot.id === id), target = group?.rows.find((i) => i.bot.id === targetId)
  if (!group || !item || !target || id === group.chiefId || targetId === group.chiefId || item.bot.pinned !== target.bot.pinned || projection.parents[id] !== projection.parents[targetId]) return pref
  const ids = group.rows.map((i) => i.bot.id), from = ids.indexOf(id), to = ids.indexOf(targetId)
  ids.splice(from, 1); ids.splice(to, 0, id)
  const merged = mergeOrder(group.items.map((i) => i.bot.id), ids)
  return { ...pref, members: { ...pref.members, [group.id]: merged } }
}

export function moveTimesGroup<T>(projection: TimesProjection<T>, pref: TimesPresentation, id: string, targetId: string): TimesPresentation {
  const a = projection.groups.find((g) => g.id === id), b = projection.groups.find((g) => g.id === targetId)
  if (!a || !b || a.id === NO_TEAM || b.id === NO_TEAM || a.parentId !== b.parentId) return pref
  const ids = projection.groups.filter((g) => g.id !== NO_TEAM && g.parentId === a.parentId).map((g) => g.id)
  const from = ids.indexOf(id), to = ids.indexOf(targetId)
  ids.splice(from, 1); ids.splice(to, 0, id)
  // Manter slots de subáreas e IDs temporariamente ausentes.
  return { ...pref, order: mergeOrder([...new Set([...pref.order, ...projection.groups.map((g) => g.id)])], ids) }
}
