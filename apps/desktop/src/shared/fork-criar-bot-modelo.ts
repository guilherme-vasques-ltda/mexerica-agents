// DON-865, fatia 4 (D373): o passo Motor do criar bot usa o seletor único da Conversa (C9) no modo `padraoDoBot`.
// O seletor só devolve a escolha; estas regras puras a aplicam ao rascunho, que o `createBot` grava. Sem DOM nem
// React. A forma da escolha é a do `engine` da D373: `fastMode` booleano, nunca `modelOptions.fast`.
import { escolherMotor, type Destino } from './fork-criar-bot-destino.ts'
import type { EngineAccountModel } from './fork-engine-accounts.ts'

/** A escolha que o seletor devolve (`Escolha` em `seletor-modelo/regras.ts`). */
export interface EscolhaDoSeletor {
  accountId?: string | null
  backend: string
  model: string | null
  modelOptions: Record<string, string>
  fastMode: boolean
}

type Rascunho = { backend: string; model?: string | null; modelOptions?: Record<string, string>; fastMode?: boolean }

/** O valor que o seletor mostra: o motor ativo, a conta escolhida e o modelo do rascunho (`null` = padrão). */
export function valorDoSeletor(draft: Rascunho, backend: string, conta: string | null): EscolhaDoSeletor {
  return { accountId: conta, backend, model: draft.model ?? null, modelOptions: draft.modelOptions ?? {}, fastMode: draft.fastMode === true }
}

/**
 * Aplica a escolha do seletor ao rascunho. Mesmo motor: troca modelo, opções e velocidade e mantém a conta. Outro
 * motor (o modelo veio de outra aba do seletor): a mesma troca do passo, que limpa a conta, e depois o modelo escolhido.
 */
export function aplicarEscolha<D extends Rascunho>(draft: D, destino: Destino, atual: string, escolha: EscolhaDoSeletor): { draft: D; destino: Destino; motorTrocado: boolean } {
  const troca = escolherMotor(draft, destino, atual, escolha.backend)
  const base = troca ?? { draft, destino }
  return {
    draft: { ...base.draft, model: escolha.model, modelOptions: { ...escolha.modelOptions }, fastMode: escolha.fastMode },
    destino: base.destino,
    motorTrocado: troca !== null,
  }
}

/** Um modelo como o seletor o lista (`ModeloCatalogo`). */
export interface ModeloDoSeletor {
  id: string
  name: string
  contextTokens?: number | null
  options?: { id: string; name: string; values: { id: string; name: string }[]; default?: string | null }[]
}

/**
 * Com conta escolhida, o motor dela lista só os modelos que a conta comprovou (o Hub recusa outro). Nome e tamanho de
 * contexto vêm do catálogo real do motor quando ele conhece o mesmo id; sem isso, o id e nenhum número.
 */
export function modelosDaConta(conta: readonly EngineAccountModel[], catalogo: readonly ModeloDoSeletor[] | null): ModeloDoSeletor[] {
  return conta.map((m) => {
    const real = catalogo?.find((c) => c.id === m.id)
    return {
      id: m.id,
      name: real?.name ?? m.id,
      contextTokens: real?.contextTokens ?? null,
      options: m.options.map((o) => ({ id: o.id, name: real?.options?.find((r) => r.id === o.id)?.name ?? o.id, default: o.default, values: o.values.map((v) => ({ id: v, name: real?.options?.find((r) => r.id === o.id)?.values.find((x) => x.id === v)?.name ?? v })) })),
    }
  })
}
