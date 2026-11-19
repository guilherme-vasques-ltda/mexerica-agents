// Modelo Equipe Marketing (D353; Linear: DON-835): o Hype como chefe e Feed, Prosa, Rank, Canal, Traço e Pixel como
// membros. Só compõe caminhos que já existem: bots que faltam nascem pelo createBot normal e a equipe vai como uma
// mudança comum de Times (prévia, depois Salvar). Nenhum campo novo, nenhum poder novo: o chefe só vira Gerente da
// área, como em qualquer Novo time. Regras de conduta adaptadas do bloco COMMON da Anomalia (Apache-2.0) para
// PT-BR; procedência em THIRD_PARTY.md.
import { addArea, memberOf, moveBot, type TimesBot, type TimesGraph } from '../../../../shared/fork-times.ts'
import { chooseNewBotPermission } from '../../../../shared/bot-autonomy.ts'
import { NEW_BOT_BACKEND, fillNewBotDefaults } from '../../../../shared/new-bot-defaults.ts'
import type { Backend, BotDraft } from '../../../../shared/models.ts'
import { ROLES, configFromRole } from '../../../lib/donnabot-avatars/src/core/data.ts'
import type { RolePreset } from '../../../lib/donnabot-avatars/src/core/types.ts'
import { cru, ErroFrase, frase, plural, type Frase } from '../../../lib/fork-i18n/frase.ts'

export const MARKETING = { area: 'Marketing', chefe: 'marketing', membros: ['social', 'escritor', 'seo', 'youtuber', 'designer', 'metaads'] } as const

/** As regras comuns da equipe, só nas instruções dos bots que o modelo cria (bots existentes não mudam). */
export const REGRAS_MARKETING = [
  'Fale com o dono só pela resposta final do pedido.',
  'Não invente número, métrica ou resultado; sem o dado, diga que não tem.',
  'Diga "pronto" só quando o trabalho foi entregue de verdade.',
  'Não descreva arquivo que você não abriu nesta conversa.',
  'Se algo falhar, tente outro jeito uma vez; se falhar de novo, diga o erro.',
] as const

const preset = (id: string): RolePreset => {
  const r = ROLES.find((x) => x.id === id)
  if (!r) throw new Error(`Função ${id} sumiu da biblioteca.`)
  return r
}
const folded = (s: string) => s.normalize('NFC').trim().toLowerCase()

/**
 * Uma função do modelo: quem pode ocupá-la (bots de Sem time com essa função, sem subbots) e quem tem a função mas
 * não pode entrar: está em outra área ou tem subbots (que iriam junto sem aparecer na confirmação; R4).
 */
export interface Vaga {
  role: RolePreset
  chefe: boolean
  candidatos: TimesBot[]
  fora: { bot: TimesBot; teamId: string | null; subbots: boolean }[]
}

export interface PlanoMarketing { vagas: Vaga[]; bloqueio: Frase | null }

/**
 * Lê o quadro sem mudar nada: para cada função, os bots de Sem time com essa função (a principal e os escondidos
 * ficam de fora). Uma área de primeiro nível chamada Marketing bloqueia: o modelo nunca a sobrescreve.
 */
export function planoMarketing(graph: TimesGraph, bots: readonly TimesBot[], primaryBotId: string | null, pendente = false): PlanoMarketing {
  const vagas = [MARKETING.chefe, ...MARKETING.membros].map((id, i): Vaga => {
    const role = preset(id)
    const mesmos = bots.filter((b) => !b.hidden && b.id !== primaryBotId && folded(b.title) === folded(role.role) && memberOf(graph, b.id))
    const comSubbots = (b: TimesBot) => graph.members.some((m) => m.parentBotId === b.id)
    const solto = (b: TimesBot) => { const m = memberOf(graph, b.id)!; return !m.teamId && !m.parentBotId && !comSubbots(b) }
    return {
      role, chefe: i === 0,
      candidatos: mesmos.filter(solto),
      fora: mesmos.filter((b) => !solto(b)).map((b) => ({ bot: b, teamId: memberOf(graph, b.id)!.teamId, subbots: comSubbots(b) })),
    }
  })
  const existe = graph.nodes.some((n) => !n.parentId && folded(n.name) === folded(MARKETING.area))
  const bloqueio = existe ? frase('times.quadro.mkt.jaExiste')
    : pendente ? frase('times.quadro.mkt.pendente') : null
  return { vagas, bloqueio }
}

/** O que a janela lista: o bot que fica com a função (`existe`) ou que nasce ao confirmar (`criar`). */
export type Linha = { role: RolePreset; chefe: boolean; estado: 'existe'; botId: string } | { role: RolePreset; chefe: boolean; estado: 'criar' }

/** `escolhas`: o bot escolhido quando a função tem mais de um candidato; sem escolha, o primeiro. */
export function linhasMarketing(plano: PlanoMarketing, escolhas: Readonly<Record<string, string>> = {}): Linha[] {
  return plano.vagas.map((v) => {
    const escolhido = v.candidatos.find((b) => b.id === escolhas[v.role.id]) ?? v.candidatos[0]
    return escolhido ? { role: v.role, chefe: v.chefe, estado: 'existe', botId: escolhido.id } : { role: v.role, chefe: v.chefe, estado: 'criar' }
  })
}

/** O bot novo pelo caminho normal do criador: função e rosto do preset, motor padrão, Pedir, nada ligado (D91). */
export function rascunhoMarketing(role: RolePreset, backends: Backend[]): BotDraft {
  const draft: BotDraft = {
    name: role.name, title: role.role, avatarConfig: configFromRole(role), avatarColor: 'blue', avatarShape: 'blob',
    description: `${role.desc}\n\nRegras da Equipe Marketing:\n${REGRAS_MARKETING.map((r) => `- ${r}`).join('\n')}`,
    backend: NEW_BOT_BACKEND, cwd: '', permission: 'ask', connectors: [], logins: [],
  }
  return fillNewBotDefaults(chooseNewBotPermission(draft, 'ask'), backends)
}

/** A equipe sobre o quadro: área Marketing com o chefe; os membros entram nela e reportam a ele. Só isso muda. */
export function equipeMarketing(graph: TimesGraph, areaId: string, chefeId: string, membros: readonly string[]): TimesGraph {
  let g = addArea(graph, { id: areaId, name: MARKETING.area, parentId: null, managerBotId: chefeId })
  g = moveBot(g, chefeId, { teamId: areaId, parentBotId: null })
  for (const id of membros) g = moveBot(g, id, { teamId: areaId, parentBotId: null }, chefeId)
  return g
}

export interface Dependencias {
  criar(draft: BotDraft): Promise<{ id: string; name: string }>
  /** Relê o quadro do Hub e devolve o estado atual (null sem leitura). */
  sincronizar(): Promise<{ nodes: TimesGraph['nodes']; members: TimesGraph['members']; routes: TimesGraph['routes']; bots: TimesBot[]; primaryBotId: string | null } | null>
  propor(label: Frase, change: (g: TimesGraph) => TimesGraph): Promise<boolean>
  /** A lista atual de motores do Hub (o motor padrão do bot novo sai dela), lida só se houver bot a criar. */
  motores(): Promise<Backend[]>
  novoId(): string
}

/** D454 (fatia 9c) e D507: o erro é `Frase`, traduzida na tela. */
export type Resultado = { ok: true } | { ok: false; criados: string[]; erro: Frase }

const doErro = (e: unknown): Frase => (e instanceof ErroFrase ? e.frase : cru(e instanceof Error ? e.message : String(e)))

/**
 * A execução do modelo, uma por sessão de Times (R1): fechar e reabrir a janela mostra o andamento, e não começa
 * outra. `criados` guarda, por função, os bots que o modelo já criou (R2): uma nova tentativa nunca os recria.
 */
export class ExecucaoMarketing {
  rodando = false
  falha: { criados: string[]; erro: Frase } | null = null
  readonly criados = new Map<string, string>()
  private readonly onChange: () => void
  constructor(onChange: () => void = () => {}) { this.onChange = onChange }

  /** As escolhas iniciais: os bots que o modelo já criou nesta sessão ocupam a função deles. */
  get escolhas(): Record<string, string> { return Object.fromEntries(this.criados) }

  async executar(linhas: readonly Linha[], deps: Dependencias): Promise<Resultado> {
    if (this.rodando) return { ok: false, criados: [], erro: frase('times.quadro.mkt.andamento') }
    this.rodando = true
    this.falha = null
    this.onChange()
    let r: Resultado
    try {
      r = await executarMarketing(linhas, deps, this.criados)
    } catch (e) {
      r = { ok: false, criados: [], erro: doErro(e) }
    }
    this.rodando = false
    this.falha = r.ok ? null : { criados: r.criados, erro: r.erro }
    if (r.ok) this.criados.clear()
    this.onChange()
    return r
  }
}

/**
 * Confirmar: cria só as funções `criar` que o modelo ainda não criou, uma de cada vez. Se uma criação falhar, para
 * ali: nada vai ao quadro e os já criados são ditos (ficam em Sem time; nada é apagado). Relê sempre o Hub e, na
 * base em que a mudança é aplicada, confere de novo cada escolha (R3): mudou algo, para sem tocar no quadro e diz o quê.
 */
export async function executarMarketing(linhas: readonly Linha[], deps: Dependencias, criadosAntes = new Map<string, string>()): Promise<Resultado> {
  const ids = new Map<string, string>()
  const criados: string[] = []
  let backends: Backend[] | null = null
  for (const l of linhas) {
    if (l.estado === 'existe') { ids.set(l.role.id, l.botId); continue }
    const ja = criadosAntes.get(l.role.id)
    if (ja) { ids.set(l.role.id, ja); continue }
    try {
      backends ??= await deps.motores()
      const bot = await deps.criar(rascunhoMarketing(l.role, backends))
      criadosAntes.set(l.role.id, bot.id)
      ids.set(l.role.id, bot.id)
      criados.push(bot.name || l.role.name)
    } catch (e) {
      return { ok: false, criados, erro: frase('times.quadro.mkt.criarFalhou', { nome: l.role.name, erro: doErro(e) }) }
    }
  }
  const atual = await deps.sincronizar().catch(() => null)
  if (!atual) return { ok: false, criados, erro: frase('times.quadro.mkt.relerFalhou') }
  const chefe = ids.get(MARKETING.chefe)!
  const membros = MARKETING.membros.map((id) => ids.get(id)!)
  const areaId = deps.novoId()
  // R3: confere as escolhas na base exata em que a mudança é aplicada (o rascunho do quadro, se houver). Mudou algo,
  // a mudança é recusada antes da prévia e o rascunho fica como estava.
  const change = (g: TimesGraph) => {
    const fresco = planoMarketing(g, atual.bots, atual.primaryBotId)
    if (fresco.bloqueio) throw new ErroFrase(fresco.bloqueio)
    const mudou = fresco.vagas.filter((v) => !v.candidatos.some((b) => b.id === ids.get(v.role.id)))
      .map((v) => atual.bots.find((b) => b.id === ids.get(v.role.id))?.name ?? v.role.name)
    if (mudou.length) throw new ErroFrase(plural('times.quadro.mkt.mudou', mudou.length, { nomes: mudou.join(', ') }))
    return equipeMarketing(g, areaId, chefe, membros)
  }
  try {
    if (await deps.propor(frase('times.quadro.mkt.equipe', { area: MARKETING.area }), change)) return { ok: true }
    return { ok: false, criados, erro: frase('times.quadro.mkt.naoAceitou') }
  } catch (e) {
    return { ok: false, criados, erro: doErro(e) }
  }
}
