// DON-865, fatia 4 (D373, revisão Sol do #459): o modelo do editor do bot pelo seletor único da Conversa (C9), no modo
// `padraoDoBot`. O seletor só devolve a escolha; aqui ela vira as MESMAS gravações de antes: sem conta, as intenções
// `model`/`modelOptions`/`fastMode`; com conta, o patch versionado do editor (`modelPatch`/`modelOptions`), e a
// velocidade continua pela intenção. Trocar de motor não acontece por aqui (o seletor do editor só lista o motor salvo).
import type { BotEditorPatch } from '../../../../shared/fork-bot-editor.ts'
import type { EngineAccountModel } from '../../../../shared/fork-engine-accounts.ts'
import { modelosDaConta } from '../../../../shared/fork-criar-bot-modelo.ts'
import { separarPadrao, type Escolha, type Provedor } from '../seletor-modelo/regras.ts'
import { modelPatch } from './engine-account.ts'

export type Gravacao =
  | { via: 'intencao'; control: 'model'; patch: { model: string | null; modelOptions: Record<string, string> } }
  | { via: 'intencao'; control: 'modelOptions'; patch: { modelOptions: Record<string, string> } }
  | { via: 'intencao'; control: 'fastMode'; patch: { fastMode: boolean } }
  | { via: 'editor'; patch: BotEditorPatch }

const mesmas = (a: Record<string, string>, b: Record<string, string>) => {
  const ka = Object.keys(a)
  return ka.length === Object.keys(b).length && ka.every((k) => a[k] === b[k])
}

/** O que gravar para ir de `atual` a `nova`. Outro motor: nada (o motor muda pelo campo Motor, com a confirmação dele). */
export function gravacoesDaEscolha(atual: Escolha, nova: Escolha, comConta: boolean): Gravacao[] {
  if (nova.backend !== atual.backend) return []
  const out: Gravacao[] = []
  if ((nova.model ?? null) !== (atual.model ?? null)) {
    out.push(comConta ? { via: 'editor', patch: modelPatch(nova.model) } : { via: 'intencao', control: 'model', patch: { model: nova.model, modelOptions: {} } })
  } else if (!mesmas(nova.modelOptions, atual.modelOptions)) {
    out.push(comConta ? { via: 'editor', patch: { modelOptions: { ...nova.modelOptions } } } : { via: 'intencao', control: 'modelOptions', patch: { modelOptions: { ...nova.modelOptions } } })
  }
  if (nova.fastMode !== atual.fastMode) out.push({ via: 'intencao', control: 'fastMode', patch: { fastMode: nova.fastMode } })
  return out
}

/**
 * Os provedores do seletor no editor: só o motor salvo do bot. Com conta, os modelos que ela comprovou (o Hub recusa
 * outro), sem o "Default" do motor (`separarPadrao`), com nome e contexto do catálogo real quando houver.
 */
export function provedoresDoEditor(provedores: readonly Provedor[], backend: string, conta: { models: readonly EngineAccountModel[] } | null): Provedor[] {
  return provedores.filter((p) => p.id === backend).map((p) => (conta ? { ...p, ...separarPadrao(modelosDaConta(conta.models, p.models)) } : p))
}

/**
 * A `key` do seletor no editor (revisão Sol do #459, rodada 2): a aba interna do seletor nasce do motor e fica no
 * estado dele; trocar o motor no campo Motor remonta o seletor, para a aba não ficar num motor que saiu da lista.
 */
export const chaveDoSeletor = (backend: string) => `seletor-${backend}`
