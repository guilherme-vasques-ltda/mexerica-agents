// Regras puras do quadro Lado a lado: sem React, sem rede, sem disco. A loja em
// renderer/store/fork-lado-a-lado.ts guarda o mapa e chama estas funções; as views só desenham.
// O Hub não conhece este quadro: refs, escopos e sessões chegam prontos e são tratados como
// entrada não confiável (fail-closed: dado ausente ou malformado nunca vira painel).

/** Máximo de painéis abertos ao mesmo tempo num escopo. */
export const MAX_PAINEIS = 3

/** Largura mínima de um painel, em px, para o encaixe por largura disponível. */
export const MIN_LARGURA_PAINEL = 360

/** Chave do mapa gravado em disco; `v1` quebra compatibilidade com formato anterior. */
export const CHAVE_ARMAZENAMENTO_LADO_A_LADO = 'fork.ladoALado.v1'

/** Uma conversa que pode virar painel: computador, bot e, quando houver, o espaço e o hub. */
export interface LadoALadoRef { computerId: string; botId: string; spaceId?: string; hubId?: string }

/** O escopo corrente: painéis de um espaço pertencem só a ele; sem espaço, é o quadro pessoal. */
export interface LadoALadoEscopo { computerId: string; hostId?: string; spaceId: string }

/** Painel aberto: a conversa (`ref`) e a sessão em curso (`null` quando o painel é só da conversa). */
export interface LadoALadoPainel { id: string; ref: LadoALadoRef; sessionId: string | null }

/** Estado do quadro num escopo. */
export interface LadoALadoEscopoEstado { ativo: boolean; focoId: string | null; paineis: LadoALadoPainel[] }

/** Estado por escopo, lido e gravado pela loja. */
export type LadoALadoMapa = Record<string, LadoALadoEscopoEstado>

/** Resultado de tentar abrir uma conversa no quadro. */
export type ResultadoAbrirPainel = 'adicionado' | 'focado' | 'limite' | 'escopo_invalido'

/** Quantos painéis cabem na largura disponível e como. */
export interface EncaixeLargura { cabem: number; estreita: boolean; modo: 'todos' | 'parcial' | 'unico' }

/** Conversa que existe e pode ficar num painel; `sessionIds`, quando definido, é o que resta dela. */
export interface ConversaDisponivel { ref: LadoALadoRef; sessionIds?: ReadonlySet<string> }

/** Chave do escopo no mapa: `pessoal` sem espaço, `space:<hub|computador>:<espaço>` com ele. */
export function escopoDoSpace(escopo: LadoALadoEscopo | null): string {
  if (!escopo) return 'pessoal'
  return `space:${escopo.hostId || escopo.computerId}:${escopo.spaceId}`
}

/** Quadro novo: fechado, sem foco, sem painéis. */
export function estadoVazio(): LadoALadoEscopoEstado {
  return { ativo: false, focoId: null, paineis: [] }
}

/** Mesma conversa: computador, bot, espaço e hub iguais (`spaceId`/`hubId` ausentes valem `null`). */
export function mesmaRefLadoALado(a: LadoALadoRef | null, b: LadoALadoRef | null): boolean {
  if (!a || !b) return false
  return a.computerId === b.computerId && a.botId === b.botId
    && (a.spaceId ?? null) === (b.spaceId ?? null) && (a.hubId ?? null) === (b.hubId ?? null)
}

/** Se a ref pertence ao escopo: sem escopo, só conversas sem espaço; com ele, espaço e computador
 *  iguais, e o hub conferido quando ambos o declaram. */
export function pertenceAoEscopo(ref: LadoALadoRef, escopo: LadoALadoEscopo | null): boolean {
  if (!escopo) return !ref.spaceId
  if (ref.spaceId !== escopo.spaceId || ref.computerId !== escopo.computerId) return false
  return !ref.hubId || !escopo.hostId || ref.hubId === escopo.hostId
}

/** Id de painel: do gerador entregue, ou um uuid com contador de reserva. */
let sequenciaPainel = 0
export function criarPainelId(gerador?: () => string): string {
  if (gerador) return gerador()
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  return `painel-${Date.now().toString(36)}-${(++sequenciaPainel).toString(36)}`
}

/** Abre uma conversa no quadro. Sem espaço válido recusa; igual à aberta foca; cheio não abre;
 *  senão acrescenta no fim, foca e (por padrão) ativa. Duas sessões do mesmo bot são dois painéis. */
export function abrirNoQuadro(
  estado: LadoALadoEscopoEstado,
  ref: LadoALadoRef,
  escopo: LadoALadoEscopo | null,
  opcoes?: { sessionId?: string | null; ativar?: boolean; criarId?: () => string; limite?: number },
): { estado: LadoALadoEscopoEstado; resultado: ResultadoAbrirPainel; painelId: string | null } {
  if (!ref.computerId || !ref.botId || !pertenceAoEscopo(ref, escopo)) {
    return { estado, resultado: 'escopo_invalido', painelId: null }
  }
  const sessionId = opcoes?.sessionId ?? null
  const existente = estado.paineis.find(p => mesmaRefLadoALado(p.ref, ref) && p.sessionId === sessionId)
  if (existente) {
    const ativo = opcoes?.ativar === false ? estado.ativo : true
    if (estado.focoId === existente.id && estado.ativo === ativo) return { estado, resultado: 'focado', painelId: existente.id }
    return { estado: { ...estado, focoId: existente.id, ativo }, resultado: 'focado', painelId: existente.id }
  }
  const limite = opcoes?.limite ?? MAX_PAINEIS
  if (estado.paineis.length >= limite) return { estado, resultado: 'limite', painelId: null }
  const id = criarPainelId(opcoes?.criarId)
  return {
    estado: { ativo: opcoes?.ativar ?? true, focoId: id, paineis: [...estado.paineis, { id, ref: { ...ref }, sessionId }] },
    resultado: 'adicionado',
    painelId: id,
  }
}

/** Remove o painel; se era o foco, o foco vai ao vizinho anterior (ou ao próximo). Sem painéis,
 *  o quadro fecha. */
export function removerPainel(estado: LadoALadoEscopoEstado, panelId: string): LadoALadoEscopoEstado {
  const idx = estado.paineis.findIndex(p => p.id === panelId)
  if (idx === -1) return estado
  const paineis = estado.paineis.filter(p => p.id !== panelId)
  if (paineis.length === 0) return { ativo: false, focoId: null, paineis }
  const focoId = paineis.some(p => p.id === estado.focoId) ? estado.focoId : paineis[Math.max(0, idx - 1)].id
  return { ...estado, focoId, paineis }
}

/** Troca a conversa do painel por outra do mesmo escopo e foca nele; fora do escopo, nada muda. */
export function trocarPainel(
  estado: LadoALadoEscopoEstado,
  panelId: string,
  ref: LadoALadoRef,
  escopo: LadoALadoEscopo | null,
  sessionId?: string | null,
): LadoALadoEscopoEstado {
  if (!pertenceAoEscopo(ref, escopo)) return estado
  if (!estado.paineis.some(p => p.id === panelId)) return estado
  const paineis = estado.paineis.map(p => p.id === panelId ? { ...p, ref: { ...ref }, sessionId: sessionId ?? null } : p)
  return { ...estado, focoId: panelId, paineis }
}

/** Atualiza a sessão só deste painel; painéis gêmeos do mesmo bot não são tocados. */
export function atualizarSessaoPainel(estado: LadoALadoEscopoEstado, panelId: string, sessionId: string | null): LadoALadoEscopoEstado {
  if (!estado.paineis.some(p => p.id === panelId && p.sessionId !== sessionId)) return estado
  const paineis = estado.paineis.map(p => p.id === panelId ? { ...p, sessionId } : p)
  return { ...estado, paineis }
}

/** Move o painel uma posição (limites não passam por cima), preservando o foco. */
export function moverPainel(estado: LadoALadoEscopoEstado, panelId: string, delta: -1 | 1): LadoALadoEscopoEstado {
  const idx = estado.paineis.findIndex(p => p.id === panelId)
  const alvo = idx + delta
  if (idx === -1 || alvo < 0 || alvo >= estado.paineis.length) return estado
  const paineis = [...estado.paineis]
  const [painel] = paineis.splice(idx, 1)
  paineis.splice(alvo, 0, painel)
  return { ...estado, paineis }
}

/** Foca o painel; inexistente ou já focado, nada muda. */
export function focarPainel(estado: LadoALadoEscopoEstado, panelId: string): LadoALadoEscopoEstado {
  if (estado.focoId === panelId || !estado.paineis.some(p => p.id === panelId)) return estado
  return { ...estado, focoId: panelId }
}

/** Cicla o foco entre os painéis, circular, na direção do delta. */
export function ciclarFocoPainel(estado: LadoALadoEscopoEstado, delta: -1 | 1): LadoALadoEscopoEstado {
  const n = estado.paineis.length
  if (n === 0) return estado
  const idx = Math.max(0, estado.paineis.findIndex(p => p.id === estado.focoId))
  const focoId = estado.paineis[(idx + delta + n) % n].id
  if (focoId === estado.focoId) return estado
  return { ...estado, focoId }
}

/** Liga ou desliga o quadro sem tocar nos painéis. */
export function definirAtivo(estado: LadoALadoEscopoEstado, ativo: boolean): LadoALadoEscopoEstado {
  if (estado.ativo === ativo) return estado
  return { ...estado, ativo }
}

/** Ajusta o quadro ao que existe: tira painel de outro escopo; do computador pronto, tira painel
 *  de conversa que sumiu e limpa sessão que não existe mais. Computador fora não prova nada: fica.
 *  Sem mudança, devolve a mesma referência (sem render extra). */
export function reconciliarEscopo(
  estado: LadoALadoEscopoEstado,
  escopo: LadoALadoEscopo | null,
  disponiveis: readonly ConversaDisponivel[],
  computadoresProntos: ReadonlySet<string>,
): { estado: LadoALadoEscopoEstado; removidos: number } {
  let removidos = 0
  let mudou = false
  const mantidos: LadoALadoPainel[] = []
  for (const painel of estado.paineis) {
    if (!pertenceAoEscopo(painel.ref, escopo)) { mudou = true; continue }
    if (!computadoresProntos.has(painel.ref.computerId)) { mantidos.push(painel); continue }
    const disponivel = disponiveis.find(d => mesmaRefLadoALado(d.ref, painel.ref))
    if (!disponivel) { removidos++; mudou = true; continue }
    if (disponivel.sessionIds && painel.sessionId !== null && !disponivel.sessionIds.has(painel.sessionId)) {
      mantidos.push({ ...painel, sessionId: null })
      mudou = true
      continue
    }
    mantidos.push(painel)
  }
  if (!mudou) return { estado, removidos: 0 }
  if (mantidos.length === 0) return { estado: { ativo: false, focoId: null, paineis: [] }, removidos }
  const focoId = mantidos.some(p => p.id === estado.focoId) ? estado.focoId : mantidos[0].id
  return { estado: { ...estado, focoId, paineis: mantidos }, removidos }
}

/** Quantos painéis cabem na largura: todos, parte ou um só (nunca zero). */
export function calcularEncaixeLargura(larguraDisponivel: number, totalPaineis: number, minLargura: number = MIN_LARGURA_PAINEL): EncaixeLargura {
  const cabem = Math.max(0, Math.floor(larguraDisponivel / minLargura))
  if (totalPaineis <= 1) return { cabem: Math.max(1, cabem), estreita: false, modo: 'todos' }
  if (cabem >= totalPaineis) return { cabem, estreita: false, modo: 'todos' }
  if (cabem >= 2) return { cabem, estreita: true, modo: 'parcial' }
  return { cabem: 1, estreita: true, modo: 'unico' }
}

/** Janela contígua de até `cabem` painéis contendo o foco (ou o primeiro), na ordem original. */
export function paineisVisiveis(paineis: readonly LadoALadoPainel[], focoId: string | null, cabem: number): LadoALadoPainel[] {
  if (paineis.length <= cabem) return [...paineis]
  const janela = Math.max(1, cabem)
  const idx = paineis.findIndex(p => p.id === focoId)
  const foco = idx === -1 ? 0 : idx
  const inicio = Math.min(Math.max(foco - janela + 1, 0), paineis.length - janela)
  return paineis.slice(inicio, inicio + janela)
}

/** Lê o mapa gravado como dado não confiável: entrada inválida é ignorada, painel inválido cai,
 *  passa de `MAX_PAINEIS` não entra, foco inválido vira o primeiro painel e quadro vazio fica fechado. */
export function parseLadoALadoMapa(raw: unknown): LadoALadoMapa {
  const mapa: LadoALadoMapa = {}
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return mapa
  for (const chave of Object.keys(raw as Record<string, unknown>)) {
    if (!chave) continue
    const estado = parseEstado((raw as Record<string, unknown>)[chave])
    if (estado) mapa[chave] = estado
  }
  return mapa
}

function parseRef(valor: unknown): LadoALadoRef | null {
  if (typeof valor !== 'object' || valor === null) return null
  const v = valor as Record<string, unknown>
  if (typeof v.computerId !== 'string' || !v.computerId || typeof v.botId !== 'string' || !v.botId) return null
  const ref: LadoALadoRef = { computerId: v.computerId, botId: v.botId }
  if (typeof v.spaceId === 'string' && v.spaceId) ref.spaceId = v.spaceId
  if (typeof v.hubId === 'string' && v.hubId) ref.hubId = v.hubId
  return ref
}

function parsePainel(valor: unknown): LadoALadoPainel | null {
  if (typeof valor !== 'object' || valor === null) return null
  const v = valor as Record<string, unknown>
  if (typeof v.id !== 'string' || !v.id) return null
  const ref = parseRef(v.ref)
  if (!ref) return null
  const sessionId = v.sessionId ?? null
  if (sessionId !== null && typeof sessionId !== 'string') return null
  return { id: v.id, ref, sessionId }
}

function parseEstado(valor: unknown): LadoALadoEscopoEstado | null {
  if (typeof valor !== 'object' || valor === null) return null
  const v = valor as Record<string, unknown>
  if (!Array.isArray(v.paineis)) return null
  const paineis: LadoALadoPainel[] = []
  for (const item of v.paineis) {
    const painel = parsePainel(item)
    if (painel) paineis.push(painel)
    if (paineis.length >= MAX_PAINEIS) break
  }
  if (paineis.length === 0) return { ativo: false, focoId: null, paineis: [] }
  const focoId = typeof v.focoId === 'string' && paineis.some(p => p.id === v.focoId) ? v.focoId : paineis[0].id
  return { ativo: v.ativo === true, focoId, paineis }
}
