import type { SkillClient } from '../../../../shared/fork-skills.ts'
import type { SkillReviewComparison, SkillReviewDocument, SkillReviewRequest } from '../../../../shared/fork-skill-review.ts'

/**
 * d2a / DON-855: comparar um rascunho com a skill escrita à mão instalada, pelo `skillDetails` com
 * `review` (contrato em `shared/fork-skill-review.ts`). Só leitura: `effect: 'none'`, nada é salvo,
 * aprovado ou reservado. Aqui ficam o pedido estrito, a conferência da resposta e o diff integral.
 */

/** Limite do Hub para o documento inteiro (bytes UTF-8). */
export const MAX_DOCUMENTO = 32 * 1024

/** Recusas do Hub e locais com motivo próprio na tela; o código nunca aparece cru. */
export type CodigoRevisao =
  | 'skill_review_invalid' | 'skill_review_manual_only' | 'skill_review_base_unknown'
  | 'skill_review_base_changed' | 'skill_review_scope_unavailable'
  | 'skill_installation_stale' | 'skill_installation_changed' | 'skill_installation_unavailable'
  /** O Hub não conhece `review` (recusa do formato antigo ou resposta sem `review`). */
  | 'hub_sem_revisao'
  /** A resposta não confere com o pedido: nada dela aparece, nunca vira sucesso nem lista vazia. */
  | 'resposta_invalida'

export class ErroRevisao extends Error {
  readonly code: CodigoRevisao
  constructor(code: CodigoRevisao) { super(code); this.code = code; this.name = 'ErroRevisao' }
}

const DO_HUB = new Set<string>([
  'skill_review_invalid', 'skill_review_manual_only', 'skill_review_base_unknown', 'skill_review_base_changed',
  'skill_review_scope_unavailable', 'skill_installation_stale', 'skill_installation_changed', 'skill_installation_unavailable',
])
/** Recusas de um Hub que não conhece a opção `review` de `skillDetails`. */
// textos-ok: mensagem exata de Hub antigo; protocolo comparado, nunca mostrado (D454: não se traduz).
const HUB_ANTIGO = new Set(['Escolha uma habilidade por id ou source', 'skill_installation_unsupported', 'method_not_found'])

const objeto = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const exato = (v: Record<string, unknown>, chaves: string[]) => Object.keys(v).length === chaves.length && chaves.every((k) => Object.hasOwn(v, k))
const digest = (v: unknown): v is string => typeof v === 'string' && /^[0-9a-f]{64}$/.test(v)
const encarnacao = (v: unknown): v is string => typeof v === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}:[1-9][0-9]*$/.test(v)

/** JSON compacto com chaves ordenadas em todos os níveis, como `registry::canonical` do Hub. */
export function canonico(v: unknown): string {
  if (Array.isArray(v)) return `[${v.map(canonico).join(',')}]`
  if (objeto(v)) return `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canonico(v[k])}`).join(',')}}`
  return JSON.stringify(v)
}

export const bytesDe = (texto: string) => new TextEncoder().encode(texto).length

async function sha256(texto: string) {
  const bytes = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(texto)))
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

/** O código de revisão de um erro, ou `null` (falha de transporte/conexão, com a mensagem dela). */
export function codigoDeRevisao(e: unknown): CodigoRevisao | null {
  if (e instanceof ErroRevisao) return e.code
  const m = e instanceof Error ? e.message : objeto(e) && typeof e.message === 'string' ? e.message : null
  return m !== null && DO_HUB.has(m) ? m as CodigoRevisao : null
}

async function documento(v: unknown): Promise<SkillReviewDocument> {
  if (!objeto(v) || !exato(v, ['revision', 'bodyDigest', 'markdown']) || !Number.isSafeInteger(v.revision) || (v.revision as number) < 1
    || !digest(v.bodyDigest) || typeof v.markdown !== 'string' || await sha256(v.markdown) !== v.bodyDigest) throw new ErroRevisao('resposta_invalida')
  return { revision: v.revision as number, bodyDigest: v.bodyDigest, markdown: v.markdown }
}

/**
 * Confere a resposta contra o pedido exato: mesma skill e encarnação, base íntegra, proposta com os
 * bytes enviados e a revisão candidata seguinte, bots completos. Qualquer divergência recusa tudo.
 */
export async function conferirResposta(raw: unknown, id: string, pedido: SkillReviewRequest): Promise<SkillReviewComparison> {
  if (objeto(raw) && raw.skillProtocol === 2 && !Object.hasOwn(raw, 'review') && Object.hasOwn(raw, 'markdown')) throw new ErroRevisao('hub_sem_revisao')
  if (!objeto(raw) || !exato(raw, ['skillProtocol', 'review']) || raw.skillProtocol !== 2) throw new ErroRevisao('resposta_invalida')
  const r = raw.review
  if (!objeto(r) || !exato(r, ['reviewProtocol', 'effect', 'id', 'installationId', 'base', 'proposal', 'affectedBots', 'comparisonDigest'])
    || r.reviewProtocol !== 1 || r.effect !== 'none' || r.id !== id || r.installationId !== pedido.expectedInstallationId
    || !digest(r.comparisonDigest) || !Array.isArray(r.affectedBots)) throw new ErroRevisao('resposta_invalida')
  const base = await documento(r.base)
  let proposta: SkillReviewDocument | null = null
  if (pedido.proposal) {
    proposta = await documento(r.proposal)
    if (base.bodyDigest !== pedido.proposal.expectedBodyDigest || proposta.markdown !== pedido.proposal.markdown
      || proposta.revision !== base.revision + 1) throw new ErroRevisao('resposta_invalida')
  } else if (r.proposal !== null) throw new ErroRevisao('resposta_invalida')
  const vistos = new Set<string>()
  const bots = r.affectedBots.map((b) => {
    if (!objeto(b) || !exato(b, ['id', 'name', 'selectionVersion']) || typeof b.id !== 'string' || !b.id || vistos.has(b.id)
      || typeof b.name !== 'string' || typeof b.selectionVersion !== 'string' || !/^[0-9a-f]{32}$/.test(b.selectionVersion)) throw new ErroRevisao('resposta_invalida')
    vistos.add(b.id)
    return { id: b.id, name: b.name, selectionVersion: b.selectionVersion }
  })
  const conteudo = { reviewProtocol: 1 as const, effect: 'none' as const, id, installationId: pedido.expectedInstallationId, base, proposal: proposta, affectedBots: bots }
  // O digest cobre base, proposta e alcance desta mesma resposta: qualquer troca recusa tudo.
  if (await sha256(canonico(conteudo)) !== r.comparisonDigest) throw new ErroRevisao('resposta_invalida')
  return { ...conteudo, comparisonDigest: r.comparisonDigest }
}

/** Monta o pedido estrito: sem `proposal` abre a base; com ela, os bytes integrais e o digest da base vista. */
export function pedidoDeRevisao(expectedInstallationId: string, proposta?: { expectedBodyDigest: string; markdown: string }): SkillReviewRequest {
  if (!encarnacao(expectedInstallationId) || (proposta && (!digest(proposta.expectedBodyDigest) || typeof proposta.markdown !== 'string'
    || bytesDe(proposta.markdown) > MAX_DOCUMENTO))) throw new ErroRevisao('skill_review_invalid')
  return proposta
    ? { protocol: 1, expectedInstallationId, proposal: { expectedBodyDigest: proposta.expectedBodyDigest, markdown: proposta.markdown } }
    : { protocol: 1, expectedInstallationId }
}

/** Uma consulta: só `skillDetails`, nunca install/update/remove. Recusa do Hub antigo vira `hub_sem_revisao`. */
export async function pedirRevisao(client: SkillClient, id: string, pedido: SkillReviewRequest): Promise<SkillReviewComparison> {
  let raw: unknown
  try { raw = await client.call<unknown>('skillDetails', { skillProtocol: 2, id, review: pedido }) }
  catch (e) {
    const m = e instanceof Error ? e.message : String(e)
    if (HUB_ANTIGO.has(m)) throw new ErroRevisao('hub_sem_revisao')
    throw e
  }
  return conferirResposta(raw, id, pedido)
}

/** Uma linha do diff: igual, removida (só na instalada) ou adicionada (só no rascunho). */
export type LinhaDiff = { tipo: 'igual' | 'removida' | 'adicionada'; antes: number | null; depois: number | null; texto: string }

/** Acima disso o meio diferente não passa pelo LCS: sai inteiro como removido e adicionado. */
const LIMITE_CELULAS = 250_000

/**
 * Diff por linha, integral: toda linha dos dois documentos aparece uma vez, na ordem, nada dobrado
 * nem cortado. Prefixo e sufixo iguais saem direto; o meio usa LCS só até `LIMITE_CELULAS` (memória
 * e tempo limitados para 32 KiB); acima disso, o meio inteiro vira remoção seguida de adição.
 * Não precisa ser mínimo; precisa ser completo.
 */
export function diffDeLinhas(antes: string, depois: string): LinhaDiff[] {
  const a = antes.split('\n'), b = depois.split('\n')
  let ini = 0
  while (ini < a.length && ini < b.length && a[ini] === b[ini]) ini++
  let fa = a.length, fb = b.length
  while (fa > ini && fb > ini && a[fa - 1] === b[fb - 1]) { fa--; fb-- }
  const out: LinhaDiff[] = []
  for (let i = 0; i < ini; i++) out.push({ tipo: 'igual', antes: i + 1, depois: i + 1, texto: a[i]! })
  const m = fa - ini, n = fb - ini
  if (m * n <= LIMITE_CELULAS) {
    // t[i][j] = LCS de a[ini+i..fa) e b[ini+j..fb), numa matriz linear.
    const w = n + 1, t = new Uint32Array((m + 1) * w)
    for (let i = m - 1; i >= 0; i--) for (let j = n - 1; j >= 0; j--) {
      t[i * w + j] = a[ini + i] === b[ini + j] ? t[(i + 1) * w + j + 1]! + 1 : Math.max(t[(i + 1) * w + j]!, t[i * w + j + 1]!)
    }
    let i = 0, j = 0
    while (i < m || j < n) {
      if (i < m && j < n && a[ini + i] === b[ini + j]) { out.push({ tipo: 'igual', antes: ini + i + 1, depois: ini + j + 1, texto: a[ini + i]! }); i++; j++ }
      else if (i < m && (j === n || t[(i + 1) * w + j]! >= t[i * w + j + 1]!)) { out.push({ tipo: 'removida', antes: ini + i + 1, depois: null, texto: a[ini + i]! }); i++ }
      else { out.push({ tipo: 'adicionada', antes: null, depois: ini + j + 1, texto: b[ini + j]! }); j++ }
    }
  } else {
    for (let i = ini; i < fa; i++) out.push({ tipo: 'removida', antes: i + 1, depois: null, texto: a[i]! })
    for (let j = ini; j < fb; j++) out.push({ tipo: 'adicionada', antes: null, depois: j + 1, texto: b[j]! })
  }
  for (let k = 0; k < a.length - fa; k++) out.push({ tipo: 'igual', antes: fa + k + 1, depois: fb + k + 1, texto: a[fa + k]! })
  return out
}

/**
 * D2A-UI1: a folha de detalhes recomeçou com um texto retido desta abertura. Só o mesmo Hub e a mesma
 * encarnação o adotam (e mesmo assim sem base nem pedido); outro Hub ou outra instalação, nunca.
 */
export function destinoDoRetido(retido: { hub: string; id: string; installationId: string } | null,
  instalada: { id: string; installationId: string } | null, hub: string | null): 'adota' | 'outroHub' | 'outraInstalacao' | null {
  if (!retido || !instalada || retido.id !== instalada.id) return null
  if (hub === null || retido.hub !== hub) return 'outroHub'
  return retido.installationId === instalada.installationId ? 'adota' : 'outraInstalacao'
}
