// D371 (T1): um campo de regras de delegação (geral ou de uma área), sem DOM nem React. Lê e grava só pela RPC do
// dono (`donnaDelegationRules` / `donnaDelegationRulesSave`); a versão esperada protege de sobrescrever outra
// mudança, e um envio sem resposta guarda a mesma chave: Tentar de novo reenvia o mesmo pedido.
import { Observable } from '../../../lib/observable.ts'

export const LIMITE = 4_000

/** As chaves (sem parâmetros) que o campo pode mostrar embaixo; a view traduz com `t`. */
export type ChaveEstado =
  | 'delegacao.estado.lendo' | 'delegacao.estado.salvando' | 'delegacao.estado.conflito' | 'delegacao.estado.naoSalvo'
  | 'delegacao.estado.salvo' | 'delegacao.estado.vazio' | 'delegacao.falha.envio' | 'delegacao.falha.leitura'
  | 'delegacao.falha.limite' | 'delegacao.falha.invalido' | 'delegacao.falha.equipeSumiu' | 'delegacao.falha.soDono'
  | 'delegacao.falha.hubAntigo'

export interface RegrasLink { call(method: string, body?: unknown, timeoutMs?: number): Promise<unknown> }

export interface RegrasSnapshot { version: number; general: string; teams: Record<string, string> }

export function lerRegras(answer: unknown): RegrasSnapshot {
  const a = answer as { version?: unknown; general?: unknown; teams?: unknown } | null
  if (!a || typeof a.version !== 'number' || typeof a.general !== 'string' || typeof a.teams !== 'object' || !a.teams) throw new Error('delegation_invalid_answer')
  const teams: Record<string, string> = {}
  for (const [id, body] of Object.entries(a.teams as Record<string, unknown>)) if (typeof body === 'string') teams[id] = body
  return { version: a.version, general: a.general, teams }
}

/** Mesma limpeza do Hub: pontas aparadas, quebra de linha \n. */
export const limpar = (texto: string) => texto.replace(/\r\n/g, '\n').trim()
export const contar = (texto: string) => [...limpar(texto)].length

/** A chave do texto da falha, pelo status e pelo código do Hub; a view traduz. */
export function chaveDaFalha(e: unknown): ChaveEstado {
  const status = typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
  const message = e instanceof Error ? e.message : String(e)
  if (status === 404 || /unknown method/i.test(message)) return 'delegacao.falha.hubAntigo'
  if (status === 401 || status === 403 || message.includes('not_allowed')) return 'delegacao.falha.soDono'
  if (message.includes('delegation_limit')) return 'delegacao.falha.limite'
  if (message.includes('delegation_invalid')) return 'delegacao.falha.invalido'
  if (message.includes('delegation_team')) return 'delegacao.falha.equipeSumiu'
  return 'delegacao.falha.envio'
}

export type Fase = 'lendo' | 'pronto' | 'salvando' | 'conflito' | 'falha'

export class CampoRegras extends Observable {
  fase: Fase = 'lendo'
  /** O texto gravado no Hub e a versão em que foi lido. */
  salvo = ''
  versao = 0
  texto = ''
  falha: ChaveEstado | null = null
  #pedido: { texto: string; body: Record<string, unknown> } | null = null
  #leitura = 0

  readonly link: RegrasLink
  readonly teamId: string | null
  readonly novoId: () => string

  constructor(link: RegrasLink, teamId: string | null, novoId: () => string = () => crypto.randomUUID()) {
    super()
    this.link = link
    this.teamId = teamId
    this.novoId = novoId
  }

  get sujo() { return limpar(this.texto) !== this.salvo }
  get excede() { return contar(this.texto) > LIMITE }

  async ler(manterTexto = false) {
    const minha = ++this.#leitura
    try {
      const r = lerRegras(await this.link.call('donnaDelegationRules', {}))
      if (minha !== this.#leitura) return
      this.salvo = this.teamId ? r.teams[this.teamId] ?? '' : r.general
      this.versao = r.version
      if (!manterTexto) this.texto = this.salvo
      this.fase = 'pronto'
      this.falha = null
    } catch (e) {
      if (minha !== this.#leitura) return
      this.fase = 'falha'
      const chave = chaveDaFalha(e)
      this.falha = chave === 'delegacao.falha.envio' ? 'delegacao.falha.leitura' : chave
    }
    this.changed()
  }

  editar(texto: string) {
    this.texto = texto
    this.changed()
  }

  async salvar() {
    if (this.excede || this.fase === 'salvando') return
    const texto = limpar(this.texto)
    // Mesmo texto do pedido sem resposta: a mesma chave, para o Hub devolver o recibo em vez de gravar de novo.
    if (this.#pedido?.texto !== texto) {
      this.#pedido = { texto, body: { teamId: this.teamId, text: texto, expectedVersion: this.versao, clientRequestId: this.novoId() } }
    }
    this.fase = 'salvando'
    this.falha = null
    this.changed()
    try {
      const answer = await this.link.call('donnaDelegationRulesSave', this.#pedido.body, 15_000) as { snapshot?: unknown }
      const r = lerRegras(answer?.snapshot)
      this.#pedido = null
      this.salvo = this.teamId ? r.teams[this.teamId] ?? '' : r.general
      this.versao = r.version
      if (limpar(this.texto) === texto) this.texto = this.salvo
      this.fase = 'pronto'
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      if (message.includes('delegation_conflict')) {
        this.#pedido = null
        this.fase = 'conflito'
      } else {
        // Recusa do Hub: o pedido não serve mais; falha sem resposta: guarda para reenviar igual.
        if (/delegation_|not_allowed|request_conflict|invalid_field/.test(message)) this.#pedido = null
        this.fase = 'falha'
        this.falha = chaveDaFalha(e)
      }
    }
    this.changed()
  }

  get podeRepetir() { return this.fase === 'conflito' || this.fase === 'falha' }

  /** Conflito: relê a versão nova, mantém o texto da pessoa e grava. Envio sem resposta: o mesmo pedido. Leitura: relê. */
  async tentarDeNovo() {
    if (this.fase === 'conflito') {
      await this.ler(true)
      if ((this.fase as Fase) === 'pronto') await this.salvar()
    } else if (this.#pedido) {
      await this.salvar()
    } else {
      await this.ler(this.sujo)
    }
  }

  /** A chave do que a linha de baixo diz; a view traduz. */
  get estado(): ChaveEstado {
    if (this.fase === 'lendo') return 'delegacao.estado.lendo'
    if (this.fase === 'salvando') return 'delegacao.estado.salvando'
    if (this.fase === 'conflito') return 'delegacao.estado.conflito'
    if (this.fase === 'falha') return this.falha ?? 'delegacao.falha.envio'
    if (this.excede) return 'delegacao.falha.limite'
    if (this.sujo) return 'delegacao.estado.naoSalvo'
    return this.salvo ? 'delegacao.estado.salvo' : 'delegacao.estado.vazio'
  }
}
