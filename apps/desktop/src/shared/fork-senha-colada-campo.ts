// MX-214: o que o campo faz com os trechos de senha de uma colagem. Puro: sem DOM, sem rede, sem log. A colagem
// suspeita é posta em quarentena na memória da caixa: TODOS os trechos que parecem segredo saem do texto antes de
// qualquer gravação (rascunho persistido, metadado de envio, "colado"), e só o resto entra no campo. A pendência vale
// para UM editor (o rascunho ou uma mensagem da fila em edição); trocar ou cancelar a edição a descarta.
import { acharSegredosColados } from './fork-senha-colada.ts'

/** Um trecho escondido e onde ele estava no texto do campo já sem nenhum trecho (índice UTF-16). Nunca registre. */
export interface TrechoEscondido {
  valor: string
  posicao: number
}

/** Quem está sendo editado: o rascunho da caixa ou uma mensagem da fila (pelo id). */
export type EditorId = 'rascunho' | `fila:${string}`
export const editorDe = (filaEntryId?: string | null): EditorId => (filaEntryId ? `fila:${filaEntryId}` : 'rascunho')

/** A decisão pendente: os trechos de uma colagem e o editor a que pertencem. Vive só em memória transitória. */
export interface Pendencia {
  editor: EditorId
  pedacos: TrechoEscondido[]
}

/** Com a pendência aberta nada é enviado: nem Enter, nem Enviar, nem a fila (atalho e "Mandar agora"). */
export const bloqueiaEnvio = (pendencia: Pendencia | null): boolean => pendencia !== null

/**
 * Congela uma escrita no campo enquanto `congelado()` for verdadeiro: com o aviso aberto NENHUM caminho altera o
 * rascunho (comando, menção, ditado, rotina, inserção programática). Só "Não é senha" escreve, depois de descongelar.
 */
export function guardarEscrita<A extends unknown[]>(congelado: () => boolean, escrever: (...args: A) => void): (...args: A) => void {
  return (...args) => { if (!congelado()) escrever(...args) }
}

/** O menu de comandos e as sugestões de menção somem com a pendência aberta (nada a escolher, nada a alterar). */
export const itensLivres = <T>(pendencia: Pendencia | null, itens: readonly T[]): T[] => (pendencia ? [] : [...itens])

/** A pendência só vale para o editor a que pertence; em outro, ela deve ser descartada (nunca restaurada lá). */
export const pendenciaVale = (pendencia: Pendencia | null, editor: EditorId): boolean => pendencia !== null && pendencia.editor === editor

export interface ColagemTratada {
  /** O campo já sem os trechos: é isto que vai ao rascunho. */
  texto: string
  /** Onde o cursor fica depois da colagem. */
  cursor: number
  /** O que foi escrito no campo (o colado sem os trechos), para marcar como colado. */
  inserido: string
  pedacos: TrechoEscondido[]
}

/** Aplica a colagem na seleção; se o colado tem trechos que parecem senha, esconde TODOS e insere só o resto. */
export function tratarColagem(campo: string, inicio: number, fim: number, colado: string): ColagemTratada | null {
  const trechos = acharSegredosColados(colado)
  if (!trechos.length) return null
  const de = Math.max(0, Math.min(inicio, fim, campo.length))
  const ate = Math.min(campo.length, Math.max(inicio, fim))
  let inserido = ''
  let cur = 0
  const pedacos: TrechoEscondido[] = []
  for (const t of trechos) {
    inserido += colado.slice(cur, t.inicio)
    pedacos.push({ valor: colado.slice(t.inicio, t.fim), posicao: de + inserido.length })
    cur = t.fim
  }
  inserido += colado.slice(cur)
  return { texto: campo.slice(0, de) + inserido + campo.slice(ate), cursor: pedacos[0]!.posicao, inserido, pedacos }
}

/** "Não é senha": os trechos voltam onde estavam (no fim, se o texto encolheu); `inicio`/`fim` cobrem o conjunto. */
export function devolverTrechos(campo: string, pedacos: readonly TrechoEscondido[]): { texto: string; inicio: number; fim: number } {
  let texto = ''
  let cur = 0
  let inicio = -1
  for (const p of pedacos) {
    const pos = Math.min(Math.max(cur, p.posicao), campo.length)
    texto += campo.slice(cur, pos)
    if (inicio < 0) inicio = texto.length
    texto += p.valor
    cur = pos
  }
  const fim = texto.length
  return { texto: texto + campo.slice(cur), inicio: Math.max(0, inicio), fim }
}

/**
 * "Não é senha" no editor de agora: devolve os trechos só ao editor a que a pendência pertence. Em outro (a edição da
 * fila foi cancelada ou trocada), o trecho é descartado e nada é devolvido a lugar nenhum.
 */
export function resolverNaoESenha(pendencia: Pendencia, editorAgora: EditorId, textoAgora: string):
  { acao: 'devolver'; texto: string; fim: number; valores: string[] } | { acao: 'descartar' } {
  if (!pendenciaVale(pendencia, editorAgora)) return { acao: 'descartar' }
  const volta = devolverTrechos(textoAgora, pendencia.pedacos)
  return { acao: 'devolver', texto: volta.texto, fim: volta.fim, valores: pendencia.pedacos.map((p) => p.valor) }
}
