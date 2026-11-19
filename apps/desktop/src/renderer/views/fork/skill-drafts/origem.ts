import type { SkillClient } from '../../../../shared/fork-skills.ts'
import type { CatalogoStore } from '../catalogo-skills/catalogo-store.ts'
import type { RascunhoSkill } from './rascunho.ts'

export interface FonteRascunho {
  hello: { hostId: string } | null
  connectedClient: SkillClient | null
  principalEpoch: number
  spaceId?: string
  spaces: { personalSpaceId: string } | null
}
export function origemDoRender(fonte: FonteRascunho) {
  return { hub: fonte.hello?.hostId ?? null, cliente: fonte.connectedClient, geracao: fonte.principalEpoch }
}
type OrigemRender = ReturnType<typeof origemDoRender>
export function somentePessoal(fonte: FonteRascunho, spaceDaEntrada?: string) {
  // Ausência no mirror legado é Pessoal; Space explícito nunca cai nele por omissão.
  return [fonte.spaceId, spaceDaEntrada].every((id) => id === undefined || id === fonte.spaces?.personalSpaceId)
}
/** Token de contexto por render: instâncias e escopo, inclusive ida e volta no mesmo Hub. */
export type ContextoRascunho = Readonly<{ fonte: FonteRascunho; loja: CatalogoStore; spaceFonte: string | undefined; spaceEntrada: string | undefined }>
export function contextoDoRender(fonte: FonteRascunho, loja: CatalogoStore, spaceEntrada?: string, anterior?: ContextoRascunho | null): ContextoRascunho {
  return anterior?.fonte === fonte && anterior.loja === loja && anterior.spaceFonte === fonte.spaceId && anterior.spaceEntrada === spaceEntrada
    ? anterior : { fonte, loja, spaceFonte: fonte.spaceId, spaceEntrada }
}
/** Síncrona: fixa instâncias e origem antes de qualquer ready()/await. Só lê. */
export function capturarExame(contexto: ContextoRascunho, contextoAtual: ContextoRascunho | null, vista: OrigemRender, adiarCarga = false) {
  if (contexto !== contextoAtual) return null
  const { fonte, loja, spaceEntrada } = contexto
  const atual = origemDoRender(fonte)
  if (!vista.hub || atual.hub !== vista.hub || atual.cliente !== vista.cliente || atual.geracao !== vista.geracao
    || contexto.spaceFonte !== fonte.spaceId || !somentePessoal(fonte, spaceEntrada)) return null
  loja.sincronizar(vista.hub, { adiarCarga })
  const origem = loja.capturarIntencao()
  if (!loja.conexaoAtual(origem) || origem.hub !== vista.hub || origem.cliente !== vista.cliente || origem.geracao !== vista.geracao) return null
  return { contexto, fonte, loja, origem }
}

/** 119A/b1: a captura comum não depende de rascunho nem inventa campos para b2. */
export function prepararExame(contexto: ContextoRascunho, contextoAtual: ContextoRascunho | null, vista: OrigemRender, rascunho: RascunhoSkill) {
  const captura = capturarExame(contexto, contextoAtual, vista)
  return captura ? { ...captura, rascunho: { name: rascunho.name, description: rascunho.description, instructions: rascunho.instructions } } : null
}
