import type { Engine } from '../../../../shared/fork-engine-accounts.ts'
import { ENGINES } from './regras.ts'

// Conexões de IA → «Adicionar conexão»: o que a tela oferece. Só entra o que já funciona de verdade; item sem
// caminho pronto fica fora (sem botão morto). Planos vêm dos motores do Hub (`ENGINES`), chaves da lista do Hub.

export type GrupoDeConexao = 'planos' | 'chaves'

export interface ItemDeConexao {
  /** `plano:<motor>` ou `chave:<fornecedor>`. */
  id: string
  grupo: GrupoDeConexao
  nome: string
  /** Motor do plano; ausente nas chaves. */
  engine?: Engine
  /** Fornecedor da chave; ausente nos planos. */
  fornecedor?: string
}

export interface ContextoDaTela {
  /** A edição Open do app: só o Codex, sem chaves. */
  open: boolean
  /** O app fala com o Hub deste mesmo computador (cadastro de chave é só local). */
  isLoopback: boolean
  /** Os motores que o Hub declara (`hello.backends`). */
  backends: readonly { id: string }[]
}

/** A edição Open do Mac se declara pela ponte do app; sem a ponte, a edição é a completa. */
export const ehOpen = (ponte: { appScheme?: string } | null | undefined) => ponte?.appScheme === 'mexerica-open'

/** Os planos que a tela oferece: na Open, só o Codex. */
export const planosDaTela = (open: boolean, nome: (engine: Engine) => string): ItemDeConexao[] =>
  ENGINES.filter((engine) => !open || engine === 'codex').map((engine) => ({ id: `plano:${engine}`, grupo: 'planos', nome: nome(engine), engine }))

/** O grupo de chaves só existe com o motor Mexerica ligado, no Hub deste computador e fora da Open. */
export const chavesDisponiveis = (ctx: ContextoDaTela) => !ctx.open && ctx.isLoopback && ctx.backends.some((b) => b.id === 'mexerica')

export const chavesDaTela = (ctx: ContextoDaTela, fornecedores: readonly { id: string; nome: string }[]): ItemDeConexao[] =>
  chavesDisponiveis(ctx) ? fornecedores.map((f) => ({ id: `chave:${f.id}`, grupo: 'chaves', nome: f.nome, fornecedor: f.id })) : []

const normalizar = (texto: string) => texto.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase()

/** Busca sem acento nem caixa, por pedaços: «claude cod» acha «Claude Code». Busca vazia devolve tudo. */
export function buscar(itens: readonly ItemDeConexao[], consulta: string): ItemDeConexao[] {
  const pedacos = normalizar(consulta).split(/\s+/).filter(Boolean)
  if (pedacos.length === 0) return [...itens]
  return itens.filter((item) => {
    const nome = normalizar(item.nome)
    return pedacos.every((p) => nome.includes(p))
  })
}

/** Os grupos na ordem da tela (Planos, Chaves de API), sem os vazios. */
export function agrupar(itens: readonly ItemDeConexao[]): { grupo: GrupoDeConexao; itens: ItemDeConexao[] }[] {
  return (['planos', 'chaves'] as const)
    .map((grupo) => ({ grupo, itens: itens.filter((i) => i.grupo === grupo) }))
    .filter((g) => g.itens.length > 0)
}
