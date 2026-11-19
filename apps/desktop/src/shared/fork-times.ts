import { validMexeriquinha, type MexeriquinhaConfig } from './fork-mexeriquinha-config.ts'
import { ErroFrase, frase, type Frase } from '../renderer/lib/fork-i18n/frase.ts'
// Times, primeira fatia (times-03; docs/reference/redesenho/components/TimesView): o contrato administrativo do Hub
// (`donnaTimes`, `donnaTimesPreview`, `donnaTimesSave`, host/src/fork/times/) e as mudanças que a tela monta
// sobre o grafo. RPC própria, fora do espelho: nada aqui entra no bot, na entrada ou no hello.
//
// As regras abaixo só orientam a tela (desativar uma opção, explicar o motivo). Quem decide é o Hub: toda mudança
// vai como prévia e, depois, como gravação com revisão e catálogo. Nada aqui concede poder a um bot.

export type TimesReportsTo = { kind: 'owner' } | { kind: 'bot'; botId: string }
export type TimesRole = 'manager' | 'executor'
export type TimesContact = 'reports' | 'urgent_only' | 'on_request'

export interface TimesNode { id: string; name: string; parentId: string | null; managerBotId: string | null; position: number }
export interface TimesMember {
  botId: string
  teamId: string | null
  parentBotId: string | null
  role: TimesRole
  reportsTo: TimesReportsTo
  ownerContact: TimesContact
}
export interface TimesRoute { fromBotId: string; toBotId: string; state: 'proposed' }
export interface TimesGraph { nodes: TimesNode[]; members: TimesMember[]; routes: TimesRoute[] }

/** DTO mínimo para rosto e nome: sem prompt, memória, conta, pasta, sessão ou estado de trabalho. */
export interface TimesBot { id: string; name: string; spaceId: string; title: string; avatarColor: string; avatarShape: string; avatarConfig?: unknown; mexeriquinha?: MexeriquinhaConfig; hidden: boolean }

export interface TimesSnapshot extends TimesGraph {
  version: 1
  spaceId: string
  revision: number
  catalogVersion: string
  primaryBotId: string | null
  bots: TimesBot[]
}

/** O que a prévia e a gravação recebem. `spaceId` só identifica o contexto; não concede nada. */
export interface TimesChange extends TimesGraph { spaceId?: string; expectedRevision: number; catalogVersion: string }
export interface TimesSaveAnswer { snapshot: TimesSnapshot; receipt: { clientRequestId: string; revision: number } }

/** Limites do Hub (host/src/fork/times/validation.rs). */
export const TIMES_LIMITS = { nodes: 128, members: 512, routes: 2048, depth: 8, name: 80, position: 1_000_000 } as const

// MARK: leitura da resposta

const isText = (v: unknown): v is string => typeof v === 'string'
const isId = (v: unknown): v is string => isText(v) && v.length > 0 && v.length <= 128
const isNullableId = (v: unknown): v is string | null => v === null || isId(v)
const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

function readReports(v: unknown): TimesReportsTo | null {
  if (!isObject(v)) return null
  if (v.kind === 'owner') return { kind: 'owner' }
  if (v.kind === 'bot' && isId(v.botId)) return { kind: 'bot', botId: v.botId }
  return null
}

function readGraph(v: Record<string, unknown>): TimesGraph | null {
  if (!Array.isArray(v.nodes) || !Array.isArray(v.members) || !Array.isArray(v.routes)) return null
  const nodes: TimesNode[] = []
  for (const n of v.nodes) {
    if (!isObject(n) || !isId(n.id) || !isText(n.name) || !isNullableId(n.parentId) || !isNullableId(n.managerBotId) || typeof n.position !== 'number') return null
    nodes.push({ id: n.id, name: n.name, parentId: n.parentId, managerBotId: n.managerBotId, position: n.position })
  }
  const members: TimesMember[] = []
  for (const m of v.members) {
    if (!isObject(m) || !isId(m.botId) || !isNullableId(m.teamId) || !isNullableId(m.parentBotId)) return null
    const reportsTo = readReports(m.reportsTo)
    if (!reportsTo || (m.role !== 'manager' && m.role !== 'executor') || (m.ownerContact !== 'reports' && m.ownerContact !== 'urgent_only' && m.ownerContact !== 'on_request')) return null
    members.push({ botId: m.botId, teamId: m.teamId, parentBotId: m.parentBotId, role: m.role, reportsTo, ownerContact: m.ownerContact })
  }
  const routes: TimesRoute[] = []
  for (const r of v.routes) {
    if (!isObject(r) || !isId(r.fromBotId) || !isId(r.toBotId) || r.state !== 'proposed') return null
    routes.push({ fromBotId: r.fromBotId, toBotId: r.toBotId, state: 'proposed' })
  }
  return { nodes, members, routes }
}

/**
 * O snapshot do Hub, conferido campo a campo. `spaceId` é o contexto pedido (null quando o Hub escolheu o Pessoal):
 * outro space na resposta é recusado, como em `SpaceClient`. Só os campos do DTO mínimo seguem para a tela.
 */
export function parseTimesSnapshot(value: unknown, spaceId: string | null): TimesSnapshot {
  if (!isObject(value) || value.version !== 1 || !isId(value.spaceId) || typeof value.revision !== 'number' || !isText(value.catalogVersion) || !isNullableId(value.primaryBotId) || !Array.isArray(value.bots)) {
    throw new Error('times_invalid_answer')
  }
  if (spaceId !== null && value.spaceId !== spaceId) throw new Error('cross_space_response')
  const graph = readGraph(value)
  if (!graph) throw new Error('times_invalid_answer')
  const bots: TimesBot[] = []
  for (const b of value.bots) {
    if (!isObject(b) || !isId(b.id) || !isText(b.name) || b.spaceId !== value.spaceId || typeof b.hidden !== 'boolean') throw new Error('times_invalid_answer')
    bots.push({ id: b.id, name: b.name, spaceId: b.spaceId, title: isText(b.title) ? b.title : '', avatarColor: isText(b.avatarColor) ? b.avatarColor : '', avatarShape: isText(b.avatarShape) ? b.avatarShape : '', ...(b.avatarConfig === undefined ? {} : { avatarConfig: b.avatarConfig }), ...(validMexeriquinha(b.mexeriquinha) ? {mexeriquinha:b.mexeriquinha} : {}), hidden: b.hidden })
  }
  return { version: 1, spaceId: value.spaceId, revision: value.revision, catalogVersion: value.catalogVersion, primaryBotId: value.primaryBotId, bots, ...graph }
}

export function graphOf(g: TimesGraph): TimesGraph {
  return {
    nodes: g.nodes.map((n) => ({ ...n })),
    members: g.members.map((m) => ({ ...m, reportsTo: m.reportsTo.kind === 'bot' ? { kind: 'bot', botId: m.reportsTo.botId } : { kind: 'owner' } })),
    routes: g.routes.map((r) => ({ ...r })),
  }
}

/** Mesmo conteúdo, sem depender da ordem (o Hub devolve o grafo ordenado). */
export function sameGraph(a: TimesGraph, b: TimesGraph): boolean {
  const key = (g: TimesGraph) => JSON.stringify([
    [...g.nodes].sort((x, y) => x.id.localeCompare(y.id)).map((n) => [n.id, n.name, n.parentId, n.managerBotId, n.position]),
    [...g.members].sort((x, y) => x.botId.localeCompare(y.botId)).map((m) => [m.botId, m.teamId, m.parentBotId, m.role, m.reportsTo.kind === 'bot' ? m.reportsTo.botId : null, m.ownerContact]),
    g.routes.map((r) => `${r.fromBotId}>${r.toBotId}`).sort(),
  ])
  return key(a) === key(b)
}

// MARK: consultas

export const memberOf = (g: TimesGraph, botId: string) => g.members.find((m) => m.botId === botId) ?? null
export const nodeOf = (g: TimesGraph, id: string) => g.nodes.find((n) => n.id === id) ?? null

/** Os bots abaixo de `botId` na estrutura (subbots, em qualquer nível). */
export function subbotsOf(g: TimesGraph, botId: string): Set<string> {
  const out = new Set<string>()
  const walk = (id: string) => {
    for (const m of g.members) if (m.parentBotId === id && !out.has(m.botId) && m.botId !== botId) { out.add(m.botId); walk(m.botId) }
  }
  walk(botId)
  return out
}

/** As áreas dentro de `nodeId` (subáreas, em qualquer nível). */
export function subareasOf(g: TimesGraph, nodeId: string): Set<string> {
  const out = new Set<string>()
  const walk = (id: string) => {
    for (const n of g.nodes) if (n.parentId === id && !out.has(n.id) && n.id !== nodeId) { out.add(n.id); walk(n.id) }
  }
  walk(nodeId)
  return out
}

/** Quem depende de um gerente: reportes e áreas que ele gerencia. */
export function dependentsOf(g: TimesGraph, botId: string) {
  return {
    reporters: g.members.filter((m) => m.reportsTo.kind === 'bot' && m.reportsTo.botId === botId).map((m) => m.botId),
    areas: g.nodes.filter((n) => n.managerBotId === botId).map((n) => n.id),
  }
}

/**
 * Leva um rascunho para o elenco atual do Hub antes de conferir de novo. Um bot que entrou no space chega com a
 * associação que o próprio Hub projetou (a do snapshot); as edições do rascunho ficam. Bot que saiu ou principal
 * trocada não são realocados em silêncio: voltam como motivo, para a pessoa ver a versão atual.
 */
export function reconcileRoster(draft: TimesGraph, snapshot: TimesSnapshot): { graph: TimesGraph } | { problem: Frase } {
  const current = new Map(snapshot.members.map((m) => [m.botId, m]))
  if (draft.members.some((m) => !current.has(m.botId))) {
    return { problem: frase('times.regras.saiuDoSpace') }
  }
  const primary = snapshot.primaryBotId ? memberOf(draft, snapshot.primaryBotId) : null
  const projected = snapshot.primaryBotId ? current.get(snapshot.primaryBotId) : undefined
  if (primary && projected && !sameMember(primary, projected)) {
    return { problem: frase('times.regras.principalMudou') }
  }
  const next = graphOf(draft)
  const known = new Set(next.members.map((m) => m.botId))
  for (const m of snapshot.members) if (!known.has(m.botId)) next.members.push(graphOf({ nodes: [], members: [m], routes: [] }).members[0]!)
  return { graph: next }
}

function sameMember(a: TimesMember, b: TimesMember) {
  return a.teamId === b.teamId && a.parentBotId === b.parentBotId && a.role === b.role && a.ownerContact === b.ownerContact
    && a.reportsTo.kind === b.reportsTo.kind && (a.reportsTo.kind === 'owner' || (b.reportsTo.kind === 'bot' && a.reportsTo.botId === b.reportsTo.botId))
}

// MARK: conferência local (só orienta a tela; o Hub confere de novo)

/**
 * O primeiro motivo pelo qual o Hub recusaria `g`, como frase do catálogo (D454, fatia 10b), ou null. Espelha as regras de estado final de
 * `validation.rs`: gerente exigido, Reporta a Você com Relatórios, principal protegida e os ciclos de área,
 * subbot, reporte e o composto (estrutura mais reporte), com até oito níveis.
 */
export function localRefusal(g: TimesGraph, primaryBotId: string | null): Frase | null {
  const members = new Map(g.members.map((m) => [m.botId, m]))
  const nodes = new Map(g.nodes.map((n) => [n.id, n]))
  for (const n of g.nodes) {
    const name = n.name
    if (!name || name !== name.trim() || [...name].length > TIMES_LIMITS.name) return frase('times.regras.nomeArea')
    if (n.parentId && !nodes.has(n.parentId)) return frase('times.regras.areaDeCima')
    if (n.managerBotId && members.get(n.managerBotId)?.role !== 'manager') return frase('times.regras.gerenteArea')
  }
  for (const m of g.members) {
    if (m.teamId && !nodes.has(m.teamId)) return frase('times.regras.areaSumiu')
    if (m.parentBotId && !members.has(m.parentBotId)) return frase('times.regras.chefeSumiu')
    if (m.reportsTo.kind === 'owner' && m.ownerContact !== 'reports') return frase('times.regras.reportaVoce')
    if (m.reportsTo.kind === 'bot' && members.get(m.reportsTo.botId)?.role !== 'manager') return frase('times.regras.gerenteReporte')
    if (m.botId === primaryBotId && (m.teamId || m.parentBotId || m.role !== 'manager' || m.reportsTo.kind !== 'owner' || m.ownerContact !== 'reports')) {
      return frase('times.regras.principalTopo')
    }
  }
  const area = (id: string) => `a:${id}`
  const bot = (id: string) => `b:${id}`
  const areas = new Map<string, string[]>()
  const structure = new Map<string, string[]>()
  const reports = new Map<string, string[]>()
  for (const n of g.nodes) {
    const edges = n.parentId ? [area(n.parentId)] : []
    areas.set(area(n.id), edges)
    structure.set(area(n.id), [...edges])
  }
  for (const m of g.members) {
    structure.set(bot(m.botId), [...(m.teamId ? [area(m.teamId)] : []), ...(m.parentBotId ? [bot(m.parentBotId)] : [])])
    reports.set(bot(m.botId), m.reportsTo.kind === 'bot' ? [bot(m.reportsTo.botId)] : [])
  }
  const compound = new Map([...structure].map(([k, v]) => [k, [...v, ...(reports.get(k) ?? [])]]))
  const checks: [Map<string, string[]>, Frase][] = [
    [areas, frase('times.regras.cicloArea')],
    [structure, frase('times.regras.cicloEstrutura')],
    [reports, frase('times.regras.cicloReporte')],
    [compound, frase('times.regras.cicloComposto')],
  ]
  for (const [edges, message] of checks) {
    const problem = cycleOrDepth(edges)
    if (problem === 'cycle') return message
    if (problem === 'depth') return frase('times.regras.profundidade')
  }
  return null
}

function cycleOrDepth(edges: Map<string, string[]>): 'cycle' | 'depth' | null {
  const heights = new Map<string, number>()
  const visit = (id: string, path: Set<string>): number | 'cycle' | 'depth' => {
    if (path.has(id)) return 'cycle'
    const known = heights.get(id)
    if (known !== undefined) return known
    if (path.size >= TIMES_LIMITS.depth) return 'depth'
    path.add(id)
    let height = 1
    for (const parent of edges.get(id) ?? []) {
      const h = visit(parent, path)
      if (typeof h !== 'number') return h
      height = Math.max(height, 1 + h)
    }
    path.delete(id)
    if (height > TIMES_LIMITS.depth) return 'depth'
    heights.set(id, height)
    return height
  }
  for (const id of edges.keys()) {
    const h = visit(id, new Set())
    if (typeof h !== 'number') return h
  }
  return null
}

// MARK: mudanças (sempre um grafo novo; o anterior não muda)

function withMember(g: TimesGraph, botId: string, change: (m: TimesMember) => TimesMember): TimesGraph {
  const next = graphOf(g)
  const at = next.members.findIndex((m) => m.botId === botId)
  if (at < 0) throw new ErroFrase(frase('times.regras.botSumiu'))
  next.members[at] = change(next.members[at]!)
  return next
}

/** Reporta a: o destinatário explícito. Reporta a Você leva o contato para Relatórios, como o Hub exige. */
export function setReportsTo(g: TimesGraph, botId: string, to: TimesReportsTo): TimesGraph {
  return withMember(g, botId, (m) => ({ ...m, reportsTo: to.kind === 'owner' ? { kind: 'owner' } : { kind: 'bot', botId: to.botId }, ownerContact: to.kind === 'owner' ? 'reports' : m.ownerContact }))
}

export function setRole(g: TimesGraph, botId: string, role: TimesRole): TimesGraph {
  return withMember(g, botId, (m) => ({ ...m, role }))
}

export function setContact(g: TimesGraph, botId: string, ownerContact: TimesContact): TimesGraph {
  return withMember(g, botId, (m) => ({ ...m, ownerContact }))
}

/** Para onde Mover leva um bot: o topo de uma área, abaixo de um bot ou Sem time (as duas nulas). */
export interface MoveTarget { teamId: string | null; parentBotId: string | null }

/**
 * Mover: muda a estrutura. A subárvore vai junto (os subbots seguem o bot e passam para a área nova);
 * Reporta a só muda quando `alsoReportTo` é passado (a tela oferece o nível de cima, marcado, e a pessoa confirma).
 */
export function moveBot(g: TimesGraph, botId: string, target: MoveTarget, alsoReportTo: string | null = null): TimesGraph {
  const parentTeam = target.parentBotId ? memberOf(g, target.parentBotId)?.teamId ?? null : target.teamId
  let next = withMember(g, botId, (m) => ({ ...m, teamId: parentTeam, parentBotId: target.parentBotId }))
  const below = subbotsOf(next, botId)
  next = { ...next, members: next.members.map((m) => (below.has(m.botId) ? { ...m, teamId: parentTeam } : m)) }
  return alsoReportTo ? setReportsTo(next, botId, { kind: 'bot', botId: alsoReportTo }) : next
}

/**
 * O nível de cima de um destino: o bot de cima, ou o gerente da área. É o padrão oferecido para o Reporta a ao
 * organizar; a pessoa confirma (ou desmarca) e só vale depois de Salvar.
 */
export function upstreamManager(g: TimesGraph, target: MoveTarget): string | null {
  return target.parentBotId ?? (target.teamId ? nodeOf(singleLeader(g), target.teamId)?.managerBotId ?? null : null)
}

/**
 * D340, espelho do Hub (`host/src/fork/times/lider.rs`; a fonte da regra é o Hub): área com exatamente 1 membro
 * direto e sem chefe informado → esse membro vira o chefe e Gerente (nunca a principal). Se o chefe era membro
 * (em `before`) e saiu, deixando a área vazia, a área fica sem chefe; o papel dele não muda.
 */
export function singleLeader(g: TimesGraph, before: TimesGraph = g, primaryBotId: string | null = null): TimesGraph {
  const promote = new Set<string>()
  const nodes = g.nodes.map((n) => {
    const direct = g.members.filter((m) => m.teamId === n.id)
    if (!n.managerBotId && direct.length === 1 && direct[0]!.botId !== primaryBotId) {
      promote.add(direct[0]!.botId)
      return { ...n, managerBotId: direct[0]!.botId }
    }
    if (n.managerBotId && !direct.length && before.members.some((m) => m.botId === n.managerBotId && m.teamId === n.id)) return { ...n, managerBotId: null }
    return n
  })
  return { ...g, nodes, members: g.members.map((m) => (promote.has(m.botId) ? { ...m, role: 'manager' } : m)) }
}

/**
 * Por que `botId` não pode passar a reportar a `to` em `g` (o grafo já com a mudança de estrutura), ou null.
 * Próprio bot, não gerente e círculo ficam de fora; a principal continua reportando a Você.
 */
export function reportOfferRefusal(g: TimesGraph, botId: string, to: string, primaryBotId: string | null, toName?: string): Frase | null {
  if (to === botId) return frase('times.regras.proprioBot')
  if (memberOf(g, to)?.role !== 'manager') return toName ? frase('times.regras.naoGerente', { nome: toName }) : frase('times.regras.naoGerenteSemNome')
  return localRefusal(setReportsTo(g, botId, { kind: 'bot', botId: to }), primaryBotId)
}

/** Uma área nova (ou subárea), com ID UUID emitido aqui e conferido pelo Hub. O gerente, se houver, vira Gerente. */
export function addArea(g: TimesGraph, area: { id: string; name: string; parentId: string | null; managerBotId: string | null }): TimesGraph {
  let next = graphOf(g)
  const position = next.nodes.reduce((max, n) => Math.max(max, n.position + 1), 0)
  next.nodes.push({ id: area.id, name: area.name.trim(), parentId: area.parentId, managerBotId: area.managerBotId, position: Math.min(position, TIMES_LIMITS.position) })
  if (area.managerBotId) next = setRole(next, area.managerBotId, 'manager')
  return next
}

export function updateArea(g: TimesGraph, id: string, patch: { name?: string; parentId?: string | null; managerBotId?: string | null }): TimesGraph {
  let next = graphOf(g)
  const at = next.nodes.findIndex((n) => n.id === id)
  if (at < 0) throw new ErroFrase(frase('times.regras.areaNaoExiste'))
  const node = next.nodes[at]!
  next.nodes[at] = { ...node, ...(patch.name !== undefined ? { name: patch.name.trim() } : {}), ...(patch.parentId !== undefined ? { parentId: patch.parentId } : {}), ...(patch.managerBotId !== undefined ? { managerBotId: patch.managerBotId } : {}) }
  if (patch.managerBotId) next = setRole(next, patch.managerBotId, 'manager')
  return next
}

/** O que impede remover a área: bots e subáreas dentro dela. Não há realocação silenciosa. */
export function areaContents(g: TimesGraph, id: string) {
  return { bots: g.members.filter((m) => m.teamId === id).map((m) => m.botId), areas: g.nodes.filter((n) => n.parentId === id).map((n) => n.id) }
}

export function removeArea(g: TimesGraph, id: string): TimesGraph {
  const inside = areaContents(g, id)
  if (inside.bots.length || inside.areas.length) throw new ErroFrase(frase('times.regras.esvaziarArea'))
  const next = graphOf(g)
  return { ...next, nodes: next.nodes.filter((n) => n.id !== id) }
}

export function addRoute(g: TimesGraph, fromBotId: string, toBotId: string): TimesGraph {
  const next = graphOf(g)
  if (fromBotId === toBotId || next.routes.some((r) => r.fromBotId === fromBotId && r.toBotId === toBotId)) return next
  next.routes.push({ fromBotId, toBotId, state: 'proposed' })
  return next
}

export function removeRoute(g: TimesGraph, fromBotId: string, toBotId: string): TimesGraph {
  const next = graphOf(g)
  return { ...next, routes: next.routes.filter((r) => !(r.fromBotId === fromBotId && r.toBotId === toBotId)) }
}

// MARK: erros

/** Como uma chamada de Times terminou. `unknown` é sem resposta: não se sabe se o Hub gravou. */
export type TimesFailure = 'unknown' | 'conflict' | 'catalog' | 'owner' | 'old_hub' | 'moved' | 'refused'

export function timesFailure(e: unknown): TimesFailure {
  const status = isObject(e) || e instanceof Error ? (e as { status?: unknown }).status : undefined
  if (status === 0) return 'unknown'
  // O transporte pode recusar antes da RPC, sem o código not_allowed do editor.
  if (status === 401 || status === 403) return 'owner'
  const message = e instanceof Error ? e.message : String(e)
  if (message.includes('times_conflict')) return 'conflict'
  if (message.includes('catalog_conflict') || message.includes('bot_catalog_conflict') || message.includes('primary_protected')) return 'catalog'
  if (message.includes('not_allowed')) return 'owner'
  if (message.includes('unknown method') || status === 404) return 'old_hub'
  if (message.includes('cross_space_response') || message.includes('stale_times_context')) return 'moved'
  return 'refused'
}

/** Os códigos do Hub em palavras; o código em si nunca chega à tela. */
const REFUSALS: [readonly string[], Frase][] = [
  [['area_cycle'], frase('times.regras.cicloArea')],
  [['subbot_cycle'], frase('times.regras.cicloEstrutura')],
  [['report_cycle'], frase('times.regras.cicloReporte')],
  [['compound_cycle'], frase('times.regras.cicloComposto')],
  [['times_depth'], frase('times.regras.profundidade')],
  [['manager_unavailable'], frase('times.regras.escolhaGerente')],
  [['invalid_contact'], frase('times.regras.reportaVoce')],
  [['team_not_found'], frase('times.regras.esvaziarArea')],
  [['bot_not_found'], frase('times.regras.moverSubbots')],
  [['invalid_route'], frase('times.regras.rotaInvalida')],
  [['times_limit'], frase('times.regras.limite')],
  [['space_not_found'], frase('times.regras.spaceSumiu')],
  [['catalog_unavailable'], frase('times.regras.dadosDanificados')],
  [['request_conflict'], frase('times.regras.gravacaoUsada')],
  [['invalid_field'], frase('times.regras.camposInvalidos')],
]

/**
 * O texto de uma falha de Times. `op`: leitura do quadro ou conferência/gravação de uma mudança; uma recusa
 * sem código conhecido na leitura não fala de "mudança" (auditoria D249, R4).
 */
export function timesErrorText(e: unknown, op: 'read' | 'save' = 'save'): Frase {
  switch (timesFailure(e)) {
    case 'unknown': return frase('times.regras.semResposta')
    case 'conflict': return frase('times.regras.outroAparelho')
    case 'catalog': return frase('times.regras.elencoMudou')
    case 'owner': return frase('times.regras.soDono')
    case 'old_hub': return frase('marca.times.hubAntigo')
    case 'moved': return frase('times.regras.conexaoMudou')
    case 'refused': {
      const message = e instanceof Error ? e.message : String(e)
      return REFUSALS.find(([codes]) => codes.some((code) => message.includes(code)))?.[1] ?? (op === 'read' ? frase('times.regras.naoLeu') : frase('times.regras.naoAceita'))
    }
  }
}
