// Instruções do dono para todos os bots e para um Space: um campo de texto por escopo, sem DOM nem React. Lê e grava só
// pela RPC do dono (`instrucaoGeral` / `instrucaoGeralSalvar`); a revisão lida protege de sobrescrever outra mudança.
import { Observable } from '../../../lib/observable.ts'

/** O mesmo limite do Hub (`host/src/fork/instrucao_geral/mod.rs`). */
export const LIMITE = 2_000

export type Escopo = { tipo: 'geral' } | { tipo: 'space'; spaceId: string }

/** As chaves (sem parâmetros) que o campo pode mostrar embaixo; a view traduz com `t`. */
export type ChaveEstado =
  | 'instrucoes.estado.lendo' | 'instrucoes.estado.salvando' | 'instrucoes.estado.conflito' | 'instrucoes.estado.naoSalvo'
  | 'instrucoes.estado.salvo' | 'instrucoes.estado.vazio' | 'instrucoes.falha.envio' | 'instrucoes.falha.leitura'
  | 'instrucoes.falha.limite' | 'instrucoes.falha.invalido' | 'instrucoes.falha.spaceSumiu' | 'instrucoes.falha.soDono'
  | 'instrucoes.falha.hubAntigo'

export interface InstrucoesLink { call(method: string, body?: unknown, timeoutMs?: number): Promise<unknown> }
/** O link vivo do Hub a cada chamada: uma reconexão troca o link sem trocar o campo nem descartar o que foi digitado. */
export type FonteLink = () => InstrucoesLink | null

export interface Campo { texto: string; revision: number }
export interface Estado { geral: Campo; space: Campo }

const inteiro = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0

function lerCampo(v: unknown): Campo {
  const c = v as { texto?: unknown; revision?: unknown } | null
  if (!c || typeof c.texto !== 'string' || !inteiro(c.revision)) throw new Error('instrucao_geral_resposta_invalida')
  return { texto: c.texto, revision: c.revision }
}

/** A resposta de qualquer RPC das instruções; fora do contrato, nada é presumido. */
export function lerEstado(answer: unknown): Estado {
  const a = answer as { geral?: unknown; space?: unknown } | null
  if (!a || typeof a !== 'object') throw new Error('instrucao_geral_resposta_invalida')
  return { geral: lerCampo(a.geral), space: lerCampo(a.space) }
}

/** Mesma limpeza do Hub: pontas aparadas, quebra de linha \n. */
export const limpar = (texto: string) => texto.replace(/\r\n/g, '\n').trim()
export const contar = (texto: string) => [...limpar(texto)].length
/** Caractere de controle ou invisível (menos quebra de linha e tabulação), que o Hub recusa: o campo avisa antes de enviar. */
// eslint-disable-next-line no-control-regex
const INVISIVEL = /[\u0000-\u0008\u000B-\u001F\u007F-\u009F\u00AD\u061C\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2069\uFEFF\uFFF9-\uFFFB]/
export const invisivel = (texto: string) => INVISIVEL.test(limpar(texto))

/** A chave do texto da falha, pelo status e pelo código do Hub; a view traduz. */
export function chaveDaFalha(e: unknown): ChaveEstado {
  const status = typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
  const message = e instanceof Error ? e.message : String(e)
  if (status === 404 || /unknown method/i.test(message)) return 'instrucoes.falha.hubAntigo'
  if (status === 401 || status === 403) return 'instrucoes.falha.soDono'
  if (message.includes('limit_exceeded')) return 'instrucoes.falha.limite'
  if (message.includes('space_not_found')) return 'instrucoes.falha.spaceSumiu'
  if (message.includes('invalid_field')) return 'instrucoes.falha.invalido'
  return 'instrucoes.falha.envio'
}

const conflito = (e: unknown) => {
  const status = typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
  return status === 409 || (e instanceof Error ? e.message : String(e)).includes('instrucao_geral_revision_conflict')
}

export type Fase = 'lendo' | 'pronto' | 'salvando' | 'conflito' | 'falha'

export class CampoInstrucao extends Observable {
  fase: Fase = 'lendo'
  /** O texto gravado no Hub e a revisão em que foi lido. */
  salvo = ''
  revisao = 0
  texto = ''
  falha: ChaveEstado | null = null
  #leitura = 0

  /** A falha vigente veio de uma leitura (nada foi lido): Tentar de novo só relê, nunca grava. */
  #falhouNaLeitura = false
  /** Já houve uma leitura bem-sucedida: a revisão guardada é a que a pessoa viu ao começar a editar. */
  #lido = false

  readonly fonte: FonteLink
  readonly escopo: Escopo

  constructor(fonte: FonteLink, escopo: Escopo) {
    super()
    this.fonte = fonte
    this.escopo = escopo
  }

  #link(): InstrucoesLink {
    const link = this.fonte()
    if (!link) throw Object.assign(new Error('sem_conexao'), { status: 0 })
    return link
  }

  get sujo() { return limpar(this.texto) !== this.salvo }
  get excede() { return contar(this.texto) > LIMITE }
  get invalido() { return invisivel(this.texto) }

  #corpoDaLeitura() { return this.escopo.tipo === 'space' ? { spaceId: this.escopo.spaceId } : {} }

  #aplicar(estado: Estado) {
    const campo = this.escopo.tipo === 'space' ? estado.space : estado.geral
    this.salvo = campo.texto
    this.revisao = campo.revision
  }

  async ler(manterTexto = false) {
    const minha = ++this.#leitura
    try {
      const estado = lerEstado(await this.#link().call('instrucaoGeral', this.#corpoDaLeitura()))
      if (minha !== this.#leitura) return
      const campo = this.escopo.tipo === 'space' ? estado.space : estado.geral
      // Rascunho de uma revisão antiga e o Hub já está em outra: avisa em vez de adotar a nova em silêncio e gravar por cima.
      const mudouPorFora = manterTexto && this.#lido && this.sujo && campo.revision !== this.revisao && this.fase !== 'conflito' && this.fase !== 'salvando'
      this.#aplicar(estado)
      this.#lido = true
      if (mudouPorFora) {
        this.fase = 'conflito'
        this.changed()
        return
      }
      if (!manterTexto) this.texto = this.salvo
      this.fase = 'pronto'
      this.falha = null
      this.#falhouNaLeitura = false
    } catch (e) {
      if (minha !== this.#leitura) return
      this.#falhouNaLeitura = true
      this.fase = 'falha'
      const chave = chaveDaFalha(e)
      this.falha = chave === 'instrucoes.falha.envio' ? 'instrucoes.falha.leitura' : chave
    }
    this.changed()
  }

  editar(texto: string) {
    this.texto = texto
    this.changed()
  }

  async salvar() {
    if (this.excede || this.invalido || this.fase === 'salvando') return
    const texto = limpar(this.texto)
    this.fase = 'salvando'
    this.falha = null
    this.changed()
    try {
      const corpo = { escopo: this.escopo.tipo, ...(this.escopo.tipo === 'space' ? { spaceId: this.escopo.spaceId } : {}), texto, expectedRevision: this.revisao }
      this.#aplicar(lerEstado(await this.#link().call('instrucaoGeralSalvar', corpo, 15_000)))
      if (limpar(this.texto) === texto) this.texto = this.salvo
      this.fase = 'pronto'
    } catch (e) {
      if (conflito(e)) {
        this.fase = 'conflito'
      } else {
        this.#falhouNaLeitura = false
        this.fase = 'falha'
        this.falha = chaveDaFalha(e)
      }
    }
    this.changed()
  }

  get podeRepetir() { return this.fase === 'conflito' || this.fase === 'falha' }

  /** Conflito: relê a revisão nova, mantém o texto da pessoa e grava. Falha: tenta gravar de novo (ou relê, se nada foi lido). */
  async tentarDeNovo() {
    if (this.fase === 'conflito' || (this.fase === 'falha' && !this.#falhouNaLeitura)) {
      await this.ler(true)
      if ((this.fase as Fase) === 'pronto') await this.salvar()
    } else {
      await this.ler(this.sujo)
    }
  }

  /** A chave do que a linha de baixo diz; a view traduz. */
  get estado(): ChaveEstado {
    if (this.fase === 'lendo') return 'instrucoes.estado.lendo'
    if (this.fase === 'salvando') return 'instrucoes.estado.salvando'
    if (this.fase === 'conflito') return 'instrucoes.estado.conflito'
    if (this.fase === 'falha') return this.falha ?? 'instrucoes.falha.envio'
    if (this.excede) return 'instrucoes.falha.limite'
    if (this.invalido) return 'instrucoes.falha.invalido'
    if (this.sujo) return 'instrucoes.estado.naoSalvo'
    return this.salvo ? 'instrucoes.estado.salvo' : 'instrucoes.estado.vazio'
  }
}
