// Na Open, a conta e o modelo do Codex dos bots: o Hub (`donnaOpenModelSet`) só aceita conta Open conectada e modelo
// comprovado pelo `model/list` dela. Aqui fica só a lista que a tela mostra e o pedido; a regra é do Hub.
import type { EngineAccount } from '../../../../shared/fork-engine-accounts.ts'

export interface ChamadaHub {
  call<T>(method: string, body?: unknown, timeoutMs?: number): Promise<T>
}

export interface EscolhaOpen {
  accountId: string
  model: string
  /** Sem bot: vale para o Berga e os bots que ainda não escolheram. Com bot: troca só nele. */
  botId?: string
  /** Raciocínio e velocidade do modelo, só com bot. Sem o campo, o Hub mantém as escolhas do mesmo modelo. */
  options?: Record<string, string>
}

/** As únicas opções que o Hub leva ao Codex no turno. */
export const OPCOES_OPEN = ['reasoning_effort', 'service_tier'] as const

/** Opções que o `model/list` da conta comprovou para o modelo (raciocínio e velocidade), sem inventar nenhuma. */
export function opcoesDoModelo(conta: Pick<EngineAccount, 'models'> | null | undefined, model: string | null | undefined) {
  const entrada = conta?.models.find((m) => m.id === model)
  return (entrada?.options ?? []).filter((o) => (OPCOES_OPEN as readonly string[]).includes(o.id) && o.values.length > 0)
}

/** Valor mostrado: o escolhido no bot se a conta ainda o aceita; senão o padrão que o Codex informou (ou nenhum). */
export function valorDaOpcao(opcao: { id: string; values: string[]; default: string | null }, escolhidas: Record<string, string> | undefined): string | null {
  const escolhido = escolhidas?.[opcao.id]
  return escolhido && opcao.values.includes(escolhido) ? escolhido : opcao.default
}

/** Modelos que a conta comprovou, na ordem do Hub (o padrão do Codex vem primeiro), sem os ocultos pelo dono. */
export function modelosDaConta(conta: Pick<EngineAccount, 'models' | 'hiddenModelIds'> | null | undefined): string[] {
  if (!conta) return []
  const ocultos = new Set(conta.hiddenModelIds ?? [])
  return conta.models.map((m) => m.id).filter((id) => !ocultos.has(id))
}

/** O padrão do Codex: o Hub põe primeiro o modelo que o `model/list` marcou como padrão. */
export function padraoDaConta(conta: Pick<EngineAccount, 'models'> | null | undefined): string | null {
  return conta?.models[0]?.id ?? null
}

/** Contas da Open que podem receber escolha: Codex conectada e com catálogo comprovado. */
export function contasEscolhiveis<T extends Pick<EngineAccount, 'backend' | 'state' | 'models' | 'hiddenModelIds'>>(contas: readonly T[] | null | undefined): T[] {
  return (contas ?? []).filter((c) => c.backend === 'codex' && c.state === 'connected' && modelosDaConta(c).length > 0)
}

const ouvintes = new Set<() => void>()

/** Avisa quem mostra prontidão (onboarding, chip) que a conta ou o modelo mudou. */
export function avisarContaOpen() {
  for (const ouvinte of [...ouvintes]) ouvinte()
}

export function aoMudarContaOpen(ouvinte: () => void): () => void {
  ouvintes.add(ouvinte)
  return () => { ouvintes.delete(ouvinte) }
}

export async function aplicarModeloOpen(client: ChamadaHub, escolha: EscolhaOpen): Promise<string[]> {
  const body = escolha.botId ? escolha : { accountId: escolha.accountId, model: escolha.model }
  const resposta = await client.call<{ applied: string[] }>('donnaOpenModelSet', body)
  avisarContaOpen()
  return resposta.applied
}
