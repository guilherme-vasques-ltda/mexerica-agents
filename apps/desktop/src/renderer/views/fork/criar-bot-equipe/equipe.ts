// DON-865, fatia 3 (D365, pergunta 3): equipe e chefe na criação, em dois passos, como o modelo Equipe Marketing.
// 1) createBot; 2) a mesma escrita no quadro de Equipes (TimesSession: reler, conferir no Hub e gravar com revisão).
// Se o passo 2 falhar, o bot continua criado e a tela diz o motivo pelo código; nada é desfeito. Quem decide é o Hub
// (inclusive `primary_protected`, D366/D368); aqui só se monta a intenção.
import { moveBot, type TimesGraph, type TimesSnapshot } from '../../../../shared/fork-times.ts'

export interface EscolhaEquipe {
  areaId: string
  /** Para quem ele reporta dentro da equipe; null = mantém o padrão (Você). */
  chefeId: string | null
}

export interface OpcaoChefe { id: string; nome: string }
export interface OpcaoEquipe { id: string; nome: string; chefes: OpcaoChefe[]; chefePadrao: string | null }

/**
 * As equipes que o quadro do Hub já tem, com o caminho (Marketing › Conteúdo), e os chefes possíveis de cada uma:
 * gerentes que estão nela, o chefe da área primeiro. A principal (Donna) nunca entra (ela fica no topo, D366).
 */
export function opcoesDeEquipe(snapshot: Pick<TimesSnapshot, 'nodes' | 'members' | 'bots' | 'primaryBotId'>): OpcaoEquipe[] {
  const nodes = new Map(snapshot.nodes.map((n) => [n.id, n]))
  const nome = (id: string) => snapshot.bots.find((b) => b.id === id)?.name ?? ''
  const caminho = (id: string): string[] => {
    const partes: string[] = []
    for (let n = nodes.get(id); n && partes.length < 9; n = n.parentId ? nodes.get(n.parentId) : undefined) partes.unshift(n.name)
    return partes
  }
  return [...snapshot.nodes]
    .sort((a, b) => caminho(a.id).join('\u0000').localeCompare(caminho(b.id).join('\u0000'), 'pt-BR') || a.position - b.position)
    .map((n) => {
      const gerentes = snapshot.members
        .filter((m) => m.teamId === n.id && m.role === 'manager' && m.botId !== snapshot.primaryBotId && nome(m.botId))
        .map((m) => m.botId)
      const ids = [...new Set([...(n.managerBotId && n.managerBotId !== snapshot.primaryBotId && nome(n.managerBotId) ? [n.managerBotId] : []), ...gerentes])]
      const chefes = ids.map((id) => ({ id, nome: nome(id) }))
      return { id: n.id, nome: caminho(n.id).join(' › '), chefes, chefePadrao: chefes[0]?.id ?? null }
    })
}

/** A mudança no quadro: o bot novo entra no topo da equipe e, com chefe, passa a reportar a ele. Só isso muda. */
export function mudancaDaEquipe(botId: string, escolha: EscolhaEquipe, primaryBotId: string | null) {
  return (g: TimesGraph): TimesGraph => {
    // D366/D368: a principal nunca é movida nem fica abaixo de alguém; o Hub também recusa (primary_protected).
    if (botId === primaryBotId) throw new Error('primary_protected')
    if (!g.nodes.some((n) => n.id === escolha.areaId)) throw new Error('team_not_found')
    return moveBot(g, botId, { teamId: escolha.areaId, parentBotId: null }, escolha.chefeId)
  }
}

/** O que a criação usa da TimesSession (o mesmo caminho do modelo Marketing). */
export interface SessaoEquipe {
  readonly phase: { kind: string; failure?: string }
  readonly check: { kind: string }
  readonly save: { kind: string }
  sync(): Promise<TimesSnapshot | null>
  propose(label: string, change: (g: TimesGraph) => TimesGraph): Promise<boolean>
  commit(): Promise<boolean>
  /** Gravação sem resposta: reenvia a mesma intenção, com a mesma chave (o Hub devolve o mesmo recibo se já gravou). */
  retrySave(): Promise<boolean>
  /** Descarta um rascunho anterior (recusado ou em conflito) e relê. */
  discard(): void
}

/** Por que o passo 2 não gravou; a tela escolhe o texto pelo código. */
export const FALHAS_DA_EQUIPE = ['sem_leitura', 'sem_dono', 'hub_antigo', 'bot_fora_do_quadro', 'equipe_sumiu', 'chefe_mudou', 'recusada', 'sem_resposta', 'conflito'] as const
export type FalhaDaEquipe = (typeof FALHAS_DA_EQUIPE)[number]
export type ResultadoEquipe = { ok: true } | { ok: false; codigo: FalhaDaEquipe }

/**
 * Passo 2: relê o quadro, confere a escolha na base atual, pede a conferência do Hub e grava. Pode ser repetido
 * (Tentar de novo) sem recriar o bot: só mexe no quadro.
 */
export async function gravarEquipe(sessao: SessaoEquipe, botId: string, escolha: EscolhaEquipe): Promise<ResultadoEquipe> {
  if (sessao.save.kind === 'unknown') {
    if (await sessao.retrySave()) return { ok: true }
    return falhaDaGravacao(sessao)
  }
  sessao.discard()
  const atual = await sessao.sync().catch(() => null)
  if (!atual) {
    const falha = sessao.phase.kind === 'error' ? sessao.phase.failure : null
    return { ok: false, codigo: falha === 'owner' ? 'sem_dono' : falha === 'old_hub' ? 'hub_antigo' : 'sem_leitura' }
  }
  if (!atual.members.some((m) => m.botId === botId)) return { ok: false, codigo: 'bot_fora_do_quadro' }
  const equipe = opcoesDeEquipe(atual).find((e) => e.id === escolha.areaId)
  if (!equipe) return { ok: false, codigo: 'equipe_sumiu' }
  if (escolha.chefeId && !equipe.chefes.some((c) => c.id === escolha.chefeId)) return { ok: false, codigo: 'chefe_mudou' }
  // textos-ok: rótulo interno da conferência da TimesSession; a criação nunca o exibe (mostra o código da falha).
  if (!await sessao.propose('criar-bot', mudancaDaEquipe(botId, escolha, atual.primaryBotId))) {
    return { ok: false, codigo: sessao.phase.kind === 'error' && sessao.phase.failure === 'owner' ? 'sem_dono' : 'recusada' }
  }
  if (await sessao.commit()) return { ok: true }
  return falhaDaGravacao(sessao)
}

function falhaDaGravacao(sessao: SessaoEquipe): ResultadoEquipe {
  const save = sessao.save.kind
  if (sessao.phase.kind === 'error' && sessao.phase.failure === 'owner') return { ok: false, codigo: 'sem_dono' }
  return { ok: false, codigo: save === 'unknown' ? 'sem_resposta' : save === 'conflict' ? 'conflito' : 'recusada' }
}

/**
 * Depois de criar, com equipe: o pedido original (bot, equipe, chefe e o lugar da sessão) fica guardado. "Tentar de
 * novo" usa só ele, nunca o que estiver na tela; e a tela trava equipe, chefe e Space enquanto houver pós-criação.
 */
export interface PosCriacao {
  id: string
  nome: string
  /** O Space do bot criado (null = Pessoal); é também o lugar da sessão de Equipes usada no passo 2. */
  spaceId: string | null
  equipe: { escolha: EscolhaEquipe; nome: string }
  fase: 'gravando' | 'falhou'
  codigo?: FalhaDaEquipe
}

/** Com pós-criação em andamento ou falha, nada do destino (equipe, chefe, Space) pode mudar. */
export const destinoTravado = (pos: PosCriacao | null) => pos !== null

/** Nova tentativa do passo 2 com o pedido guardado; a sessão precisa ser a do mesmo lugar. */
export async function tentarDeNovo(sessao: SessaoEquipe, pos: PosCriacao, lugarDaSessao: string | null): Promise<ResultadoEquipe> {
  if (lugarDaSessao !== pos.spaceId) return { ok: false, codigo: 'sem_leitura' }
  return gravarEquipe(sessao, pos.id, pos.equipe.escolha)
}
