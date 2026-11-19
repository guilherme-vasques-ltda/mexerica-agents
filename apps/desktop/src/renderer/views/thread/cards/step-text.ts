// MX-141: o texto do passo em linguagem simples. O motor manda o título em inglês ("Read src/pace.js"); a pessoa lê
// "Lendo src/pace.js". Puro e sem regra de negócio: só troca o verbo do INÍCIO do título, e o resto (arquivo, comando)
// segue como veio. Nada é inferido: título que não se reconhece fica como está (os do próprio Hub já vêm no idioma).
import type { Tradutor } from '../../../lib/fork-i18n/estado'
import { toolLabel } from './tool-steps.ts'

const ACOES = ['ler', 'alterar', 'escrever', 'criar', 'apagar', 'mover', 'rodar', 'procurar', 'listar', 'web', 'pensar', 'tarefas', 'trabalhar'] as const
export type Acao = (typeof ACOES)[number]

/** Os verbos que os motores (Claude Code, Codex, Gemini via ACP) mandam no começo do título. Palavra inteira, sem maiúsculas. */
const POR_VERBO: Readonly<Record<string, Acao>> = {
  read: 'ler', view: 'ler',
  edit: 'alterar', multiedit: 'alterar', patch: 'alterar', apply_patch: 'alterar', replace: 'alterar',
  write: 'escrever', create: 'criar',
  delete: 'apagar', remove: 'apagar',
  move: 'mover', rename: 'mover',
  run: 'rodar', bash: 'rodar', exec: 'rodar', execute: 'rodar', shell: 'rodar',
  search: 'procurar', grep: 'procurar', glob: 'procurar', find: 'procurar',
  list: 'listar', ls: 'listar',
  fetch: 'web', webfetch: 'web', websearch: 'web',
  think: 'pensar', thinking: 'pensar',
  todo: 'tarefas', todos: 'tarefas', todowrite: 'tarefas',
}

/** Verbo de duas palavras, antes da regra da primeira palavra: "Web Search", "Web Fetch", "Update todos". */
const DUAS_PALAVRAS: readonly (readonly [RegExp, Acao])[] = [
  [/^web\s+(?:search|fetch)(?=\s|$)/i, 'web'],
  [/^update\s+(?:the\s+)?todos?(?:\s+list)?(?=\s|$)/i, 'tarefas'],
]

/** Quando o verbo não vem no título, o tipo do passo (`kind` do ACP) diz o que se faz. */
const POR_TIPO: Readonly<Record<string, Acao>> = {
  execute: 'rodar', read: 'ler', edit: 'alterar', delete: 'apagar', move: 'mover', search: 'procurar', fetch: 'web', think: 'pensar',
}

/** Títulos que o Hub/motor põem quando não há nada a dizer: valem como "sem título". */
const SEM_TITULO = new Set(['tool', 'terminal'])

/** "Read File" (o Claude Code, sem caminho no título): só na leitura a palavra não é objeto; nas outras ações fica. */
const SO_PALAVRA_GENERICA = /^files?$/i

const PRIMEIRA_PALAVRA = /^([A-Za-z_]+)(?=\s|$)/
const ENTRE_CRASES = /^`([^`]+)`$/

export interface PassoTexto {
  title: string
  kind: string
}

const semCrases = (objeto: string) => ENTRE_CRASES.exec(objeto)?.[1] ?? objeto

/** Comando cru no título de um passo `execute`: primeira palavra em minúsculas ASCII ("npm test", "./run.sh"). Uma frase já
 *  no idioma ("Rodando npm test", "Executando testes", 运行…) começa diferente e fica como está. */
const COMANDO_CRU = /^[a-z0-9._~/-]+(?=\s|$)/

/** Algo que a pessoa reconhece como objeto, não uma frase pronta: o título inteiro entre crases, um comando cru de
 *  `execute`, ou uma palavra só (caminho, arquivo ou, com tipo conhecido, o alvo do passo). */
function pareceObjeto(titulo: string, kind: string, tipoEspecifico: boolean) {
  if (ENTRE_CRASES.test(titulo)) return true
  if (kind === 'execute') return COMANDO_CRU.test(titulo)
  if (/\s/.test(titulo)) return false
  return tipoEspecifico || /[/\\]/.test(titulo) || /\.\w{1,8}$/.test(titulo)
}

/** Só as chaves próprias do mapa: "constructor" ou "__proto__" no título não viram ação herdada. */
const proprio = <T,>(mapa: Readonly<Record<string, T>>, chave: string): T | undefined => (Object.hasOwn(mapa, chave) ? mapa[chave] : undefined)

/** O título já começa com um verbo de ação no idioma do app ("Lendo", "rodando npm test", "正在读取…"): é frase
 *  pronta e fica como veio. Em verbo que termina em letra latina, exige fim de palavra ("Lendorama" não conta). */
function jaNoIdioma(t: Tradutor, titulo: string) {
  const minusculo = titulo.toLocaleLowerCase()
  return ACOES.some((acao) => {
    const verbo = t(`conversa.cartoes.passos.acao.${acao}`).toLocaleLowerCase()
    if (!verbo || !minusculo.startsWith(verbo)) return false
    const depois = minusculo.charAt(verbo.length)
    return !/[a-z\u00c0-\u024f]$/.test(verbo) || depois === '' || !/[\p{L}\p{N}]/u.test(depois)
  })
}

function montar(t: Tradutor, acao: Acao, objeto: string) {
  const verbo = t(`conversa.cartoes.passos.acao.${acao}`)
  return objeto ? t('conversa.cartoes.passos.acao.comObjeto', { acao: verbo, objeto: semCrases(objeto) }) : verbo
}

/**
 * O texto exibido do passo. Verbo conhecido no início do título: o verbo no idioma + o resto como veio. Só comando
 * entre crases ou caminho: o verbo do tipo do passo. Sem título: o nome do tipo (`toolLabel`). Título que não se
 * reconhece fica como está.
 */
export function stepText(t: Tradutor, step: PassoTexto): string {
  const titulo = step.title.trim()
  if (!titulo || SEM_TITULO.has(titulo.toLowerCase())) return toolLabel(t, step.kind)
  if (jaNoIdioma(t, titulo)) return titulo

  for (const [padrao, acao] of DUAS_PALAVRAS) {
    const achou = padrao.exec(titulo)
    if (achou) return montar(t, acao, titulo.slice(achou[0].length).trim())
  }
  const palavra = PRIMEIRA_PALAVRA.exec(titulo)?.[1]
  const acao = palavra ? proprio(POR_VERBO, palavra.toLowerCase()) : undefined
  if (palavra && acao) {
    const resto = titulo.slice(palavra.length).trim()
    return montar(t, acao, acao === 'ler' && SO_PALAVRA_GENERICA.test(resto) ? '' : resto)
  }

  const doTipo = proprio(POR_TIPO, step.kind)
  if (pareceObjeto(titulo, step.kind, doTipo !== undefined)) return montar(t, doTipo ?? 'trabalhar', titulo)
  return titulo
}
