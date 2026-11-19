// D133: a função do bot. As 30 funções da biblioteca são exemplos; o dono renomeia ou cria a sua.
import { ROLES, avatarDoBot, configFromRole, configForRole, type BotAvatarConfig } from './fork-avatar-config.ts'
import { ptBR } from '../renderer/lib/fork-i18n/pt-BR.ts'

/** O mesmo limite do Hub (`host/src/fork/funcao.rs`). */
export const FUNCAO_LIMITE = 40

export interface Funcao {
  /** `exemplo:<id da biblioteca>` ou `propria:<nome>`. */
  id: string
  /** O rótulo exibido na tela. */
  nome: string
  /** O valor gravado no `title` do Hub: o texto original da biblioteca, nunca a tradução da tela. */
  valor: string
  /** Nome que o exemplo sugere para o bot; vazio nas funções do dono. */
  nomeDoBot: string
  /** Exemplos: o preset antigo da biblioteca; próprias: o avatar do bot de origem, v2 preservado (fase 2a). */
  config: BotAvatarConfig
  propria: boolean
}

const dobrado = (s: string) => s.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase().replace(/\s+/g, ' ').trim()
// Mesmo conjunto que o Hub recusa: controles, direção de texto e caracteres de largura zero.
const INVISIVEL = /[\u0000-\u001f\u007f-\u009f\u00ad\u061c\u180e\u200b-\u200f\u202a-\u202e\u2060-\u2069\ufeff\ufff9-\ufffb]/

/** Como o Hub grava: espaços repetidos viram um só. */
export const limparFuncao = (texto: string) => texto.split(/\s+/).filter(Boolean).join(' ')

export function erroDaFuncao(texto: string): string | null {
  const limpo = limparFuncao(texto)
  if (INVISIVEL.test(limpo)) return 'A função tem um caractere invisível. Apague-o e tente de novo.'
  if ([...limpo].length > FUNCAO_LIMITE) return `A função pode ter até ${FUNCAO_LIMITE} caracteres.`
  return null
}

/**
 * Rótulo exibido das funções da biblioteca de avatares (`core/data.ts`, herdado, com nomes em inglês e em formas
 * de pessoa): as chaves `botRoles.<id>.nome` dos catálogos (DON-903). Aqui fica o pt-BR, fonte e fallback, para as
 * regras puras (exemplos conhecidos, Suas funções); a tela mostra o rótulo do idioma escolhido por `useT()`.
 * IDs, dados e instruções dos bots não mudam: a função vai ao Hub com o valor original (`Funcao.valor`).
 */
const rotuloPtBR = (id: string): string | null => (ptBR as Record<string, string>)[`botRoles.${id}.nome`] ?? null

export const FUNCOES_EXEMPLO: readonly Funcao[] = ROLES.map((r) => ({
  id: `exemplo:${r.id}`, nome: rotuloPtBR(r.id) ?? r.role, valor: r.role, nomeDoBot: r.name, config: configFromRole(r), propria: false,
}))

// A grafia antiga em inglês também é exemplo: bots antigos com "Researcher" não viram "Suas funções".
const exemplos = new Set([...FUNCOES_EXEMPLO.map((f) => dobrado(f.nome)), ...ROLES.map((r) => dobrado(r.role))])

/**
 * Funções que o dono já criou: as dos bots atuais que não são um dos exemplos, com a aparência
 * do bot mais recente. Sem lista própria no Hub, a lista acompanha os bots que existem.
 */
export function funcoesProprias(bots: readonly { kind: string; name: string; description: string; title?: string; avatarConfig?: unknown; mexeriquinha?: unknown; createdAt: number }[]): Funcao[] {
  const vistas = new Map<string, Funcao>()
  for (const bot of [...bots].sort((a, b) => b.createdAt - a.createdAt)) {
    const nome = limparFuncao(bot.title ?? '')
    const chave = dobrado(nome)
    if (bot.kind === 'group' || !nome || exemplos.has(chave) || vistas.has(chave) || erroDaFuncao(nome)) continue
    vistas.set(chave, { id: `propria:${chave}`, nome, valor: nome, nomeDoBot: '', config: avatarDoBot(bot) ?? configForRole(bot.name, bot.description), propria: true })
  }
  return [...vistas.values()].sort((a, b) => a.nome.localeCompare(b.nome, 'pt-BR'))
}

const porValor = new Map(FUNCOES_EXEMPLO.map((f) => [dobrado(f.valor), f]))
/** O ID da biblioteca de um `title` gravado (ou do seu rótulo pt-BR), para a tela traduzir; null fora dos exemplos. */
export function papelDoTitulo(title: string): string | null {
  const chave = dobrado(title)
  const f = porValor.get(chave) ?? FUNCOES_EXEMPLO.find((x) => dobrado(x.nome) === chave)
  return f ? f.id.replace(/^exemplo:/, '') : null
}
/** O rótulo pt-BR de um `title` gravado: exemplo da biblioteca vira o rótulo; o resto fica como está. */
export function exibirFuncao(title: string): string {
  return porValor.get(dobrado(title))?.nome ?? title
}
