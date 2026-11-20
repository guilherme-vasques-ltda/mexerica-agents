// DON-865, fatia 2: conta de IA e Space na criação. Regras puras, sem DOM nem React. A tela só envia a
// intenção; quem valida a posse (engine.accounts, SAAS-04), a conta e o Space é o Hub.
import type { ManagedAccount } from './fork-engine-accounts.ts'
import type { Space, SpacesSnapshot } from './fork-spaces.ts'

/** O que a pessoa escolheu além do que a fatia 1 já mandava. `null` = padrão (conta do Hub, Pessoal). */
export interface Destino {
  conta: string | null
  spaceId: string | null
}

/**
 * O corpo do `createBot`: o da fatia 1 mais, só quando escolhidos, `forkEditor.engineAccountId` e `spaceId`.
 * Pessoal não manda `spaceId` (é o padrão do Hub), e conta padrão não manda `forkEditor` (sem pedir autoridade).
 */
export function corpoDaCriacao<T extends object>(base: T, destino: Destino, personalId: string | null): T & { forkEditor?: { engineAccountId: string }; spaceId?: string } {
  const corpo: T & { forkEditor?: { engineAccountId: string }; spaceId?: string } = { ...base }
  if (destino.conta) corpo.forkEditor = { engineAccountId: destino.conta }
  if (destino.spaceId && destino.spaceId !== personalId) corpo.spaceId = destino.spaceId
  return corpo
}

export const noSpace = (destino: Destino, personalId: string | null) => !!destino.spaceId && destino.spaceId !== personalId

/** Os motores com contas de IA do Hub (D164). */
export const motorComConta = (backend: string): backend is 'codex' | 'claude' | 'grok' => backend === 'codex' || backend === 'claude' || backend === 'grok'

/** Se a tela mostra a escolha de conta: só com a capacidade `engine.accounts` respondida pelo Hub. */
export type EscolhaDeConta = 'escolher' | 'sem-posse' | 'motor-sem-conta'
export function escolhaDeConta(podeGerenciar: boolean, backend: string): EscolhaDeConta {
  if (!motorComConta(backend)) return 'motor-sem-conta'
  return podeGerenciar ? 'escolher' : 'sem-posse'
}

/** As contas gerenciadas do motor, na ordem do Hub; só as conectadas podem ser escolhidas. */
export function contasDoMotor(contas: readonly ManagedAccount[], backend: string): { conta: ManagedAccount; escolhivel: boolean }[] {
  return contas.filter((c) => c.origin === 'managed' && c.backend === backend).map((conta) => ({ conta, escolhivel: conta.state === 'connected' }))
}

/** Só um padrão legado conectado é inequívoco. Nunca usa uma gerenciada ou outro motor como fallback. */
export function contaPadraoDoHub(contas: readonly ManagedAccount[], backend: string): ManagedAccount | null {
  const legacy = contas.filter((c) => c.origin === 'legacy' && c.backend === backend && c.state === 'connected')
  return legacy.length === 1 ? legacy[0]! : null
}

/** Pessoal primeiro, depois os Spaces na ordem da faixa. Sem o contrato de Spaces, só o Pessoal. */
export function spacesDaCriacao(snapshot: SpacesSnapshot | null | undefined): Space[] {
  if (!snapshot) return []
  const pessoal = snapshot.spaces.filter((s) => s.id === snapshot.personalSpaceId)
  const outros = snapshot.spaces.filter((s) => s.id !== snapshot.personalSpaceId && s.kind === 'space').sort((a, b) => a.position - b.position)
  return [...pessoal, ...outros]
}

/** Recusas conhecidas do Hub na criação; o resto vira `generico`. A tela decide pelo código, nunca ecoa a mensagem. */
export const ERROS_DA_CRIACAO = [
  'remote_setup_unavailable', 'binding_unavailable', 'setup_authority_lost', 'account_incompatible', 'account_unavailable',
  'account_not_found', 'account_invalid', 'model_unavailable', 'model_option_unavailable', 'space_not_found',
  'space_backend_selection_required', 'cross_space_reference', 'offline',
] as const
export type ErroDaCriacao = (typeof ERROS_DA_CRIACAO)[number] | 'generico'

const CODIGO = /^(?:Error: )?([a-z][a-z0-9_]{2,40})(?::|$)/
export function erroDaCriacao(erro: unknown): ErroDaCriacao {
  if (typeof erro === 'object' && erro !== null && 'status' in erro && erro.status === 0) return 'offline'
  const texto = typeof erro === 'object' && erro !== null && 'message' in erro ? String(erro.message) : String(erro ?? '')
  const codigo = CODIGO.exec(texto.trim())?.[1]
  return (ERROS_DA_CRIACAO as readonly string[]).includes(codigo ?? '') ? codigo as ErroDaCriacao : 'generico'
}

/**
 * Escolher motor: clicar no motor já ativo não muda nada (`null`). Trocar de verdade limpa juntos a conta,
 * o modelo e as opções, porque o modelo e as opções da conta anterior não valem no outro motor nem no
 * padrão do Hub.
 */
export function escolherMotor<D extends { backend: string; model?: string | null; modelOptions?: Record<string, string> }>(
  draft: D, destino: Destino, atual: string, novo: string,
): { draft: D; destino: Destino } | null {
  if (novo === atual) return null
  return { draft: { ...draft, backend: novo, model: null, modelOptions: {} }, destino: { ...destino, conta: null } }
}

/** Falha ao ler os padrões da criação (autonomia geral, motores): código estável; a tela traduz, sem eco do Hub. */
export type FalhaDosPadroes = 'motores' | 'offline' | 'padroes'
export function falhaDosPadroes(erro: unknown): Exclude<FalhaDosPadroes, 'motores'> {
  return typeof erro === 'object' && erro !== null && 'status' in erro && erro.status === 0 ? 'offline' : 'padroes'
}
