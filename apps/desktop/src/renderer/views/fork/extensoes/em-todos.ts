// MX-264 (opção A do dono): "Ligar em todos os bots" nas Extensões. Modelo sem DOM nem React: lê a prévia do Hub, dá
// UM OK, aplica (o Hub faz tudo-ou-nada) e, se o Hub disser que a lista mudou, refaz a prévia sozinho. O token da
// prévia (`versao`) é opaco: volta ao Hub exatamente como veio. A tela só mostra o que o Hub disse.
// Contrato: docs/features/marketplace.md (extensaoEmTodosPrevia, extensaoEmTodosAplicar, extensaoNovosBotsDefinir).
import { Observable } from '../../../lib/observable.ts'

export type TipoExtensao = 'skill' | 'conector'

/** Os códigos de erro do contrato do Hub (#764). */
export const CODIGO_CONFLITO = 'extensoes_em_todos_conflito'
export const CODIGO_INDISPONIVEL = 'extensoes_em_todos_indisponivel'
export const CODIGO_NOVOS_BOTS_RECUSADO = 'extensoes_novos_bots_recusado'

/** Motivos que o Hub conhece, na ordem em que a tela os lista; qualquer outro (Hub mais novo) cai em `outro`. */
export const MOTIVOS = ['ja_tem', 'oculto', 'grupo', 'apagado', 'fora_do_pessoal', 'principal_de_space'] as const
export type Motivo = (typeof MOTIVOS)[number] | 'outro'

export interface Recebedor { botId: string; nome: string }
export interface Excluido { botId: string; motivo: string }
export interface Previa {
  tipo: TipoExtensao
  id: string
  /** Opaco: só volta ao Hub no aplicar. */
  versao: string
  recebedores: Recebedor[]
  excluidos: Excluido[]
}

/** Quem fica de fora, agrupado pelo motivo, com o nome que o app conhece (o Hub só manda o id). */
export interface GrupoExcluidos { motivo: Motivo; nomes: string[]; semNome: number }

export interface Cliente {
  call<T>(method: string, body?: unknown, timeoutMs?: number): Promise<T>
}

/** O que está em jogo: a extensão e o nome que a tela mostra (dado, não traduzido). */
export interface Alvo { tipo: TipoExtensao; id: string; nome: string }

export type ErroEmTodos = 'permissao' | 'semFuncao' | 'indisponivel' | 'incerto' | 'generico'

export type Estado =
  | { fase: 'fechado' }
  | { fase: 'lendo'; mudou: boolean }
  | { fase: 'previa'; previa: Previa; mudou: boolean }
  | { fase: 'aplicando'; previa: Previa }
  | { fase: 'feito'; quantos: number }
  | { fase: 'erro'; erro: ErroEmTodos }

const objeto = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const texto = (v: unknown): v is string => typeof v === 'string' && v.length > 0

/** A prévia como o app a entende, ou `null` (resposta de Hub que o app não sabe ler: nada é mostrado nem aplicado). */
export function lerPrevia(raw: unknown, alvo: Alvo): Previa | null {
  if (!objeto(raw) || raw.tipo !== alvo.tipo || raw.id !== alvo.id || !texto(raw.versao)) return null
  if (!Array.isArray(raw.recebedores) || !Array.isArray(raw.excluidos)) return null
  const recebedores: Recebedor[] = []
  for (const r of raw.recebedores) {
    if (!objeto(r) || !texto(r.botId) || typeof r.nome !== 'string') return null
    recebedores.push({ botId: r.botId, nome: r.nome })
  }
  const excluidos: Excluido[] = []
  for (const e of raw.excluidos) {
    if (!objeto(e) || !texto(e.botId) || typeof e.motivo !== 'string') return null
    excluidos.push({ botId: e.botId, motivo: e.motivo })
  }
  return { tipo: alvo.tipo, id: alvo.id, versao: raw.versao, recebedores, excluidos }
}

const motivoConhecido = (m: string): m is (typeof MOTIVOS)[number] => (MOTIVOS as readonly string[]).includes(m)

/** Agrupa os de fora por motivo (ordem fixa; motivo novo por último). `nomeDe` devolve `null` se o app não conhece o bot. */
export function agruparExcluidos(excluidos: readonly Excluido[], nomeDe: (botId: string) => string | null): GrupoExcluidos[] {
  const grupos = new Map<Motivo, GrupoExcluidos>()
  for (const e of excluidos) {
    const motivo: Motivo = motivoConhecido(e.motivo) ? e.motivo : 'outro'
    const g = grupos.get(motivo) ?? { motivo, nomes: [], semNome: 0 }
    const nome = nomeDe(e.botId)
    if (nome) g.nomes.push(nome)
    else g.semNome += 1
    grupos.set(motivo, g)
  }
  return [...MOTIVOS, 'outro' as const].flatMap((m) => grupos.get(m) ?? [])
}

/** O status e o texto de um erro do Hub (`HostError`), sem depender da classe. */
function dadosDoErro(e: unknown): { status: number | null; mensagem: string } {
  if (typeof e !== 'object' || e === null) return { status: null, mensagem: String(e) }
  const { status, message } = e as { status?: unknown; message?: unknown }
  return { status: typeof status === 'number' ? status : null, mensagem: typeof message === 'string' ? message : '' }
}

/** Uma falha do Hub vira um caso da tela. `conflito` não é erro: a tela refaz a prévia. */
export function classificarFalha(e: unknown): 'conflito' | ErroEmTodos {
  const { status, mensagem } = dadosDoErro(e)
  if (mensagem.includes(CODIGO_CONFLITO)) return 'conflito'
  if (mensagem.includes(CODIGO_INDISPONIVEL)) return 'indisponivel'
  if (status === 401 || status === 403 || /not_allowed|forbidden|unauthorized/.test(mensagem)) return 'permissao'
  if (status === 404 || /method_not_found|unknown_method/.test(mensagem)) return 'semFuncao'
  return 'generico'
}

/** Sem resposta clara (link caiu, tempo esgotado, erro sem status): a aplicação pode ter acontecido no Hub. */
export function semRespostaClara(e: unknown): boolean {
  const { status } = dadosDoErro(e)
  return status === null || status === 0
}

/** O interruptor "Novos bots já vêm com esta skill" existe só para skills (D91: conectores e contas nascem desligados). */
export const temInterruptorNovosBots = (tipo: TipoExtensao): boolean => tipo === 'skill'

/** Liga ou desliga o padrão dos bots novos; devolve o valor que o Hub gravou. Para conector nem pergunta ao Hub. */
export async function definirNovosBots(cliente: Cliente, alvo: Pick<Alvo, 'tipo' | 'id'>, ligado: boolean): Promise<boolean> {
  if (!temInterruptorNovosBots(alvo.tipo)) throw new Error(CODIGO_NOVOS_BOTS_RECUSADO)
  const r = await cliente.call<unknown>('extensaoNovosBotsDefinir', { protocol: 1, tipo: 'skill', id: alvo.id, ligado }, 30_000)
  if (!objeto(r) || r.id !== alvo.id || typeof r.novosBots !== 'boolean') throw new Error('extensoes_novos_bots_resposta_invalida')
  return r.novosBots
}

/** O padrão dos bots novos como o Hub o tem agora (lista de skills), ou `null` se não deu para ler ou a skill não está lá. */
export async function lerNovosBots(cliente: Cliente, id: string): Promise<boolean | null> {
  try {
    const r = await cliente.call<unknown>('skills', { skillProtocol: 2 }, 30_000)
    if (!objeto(r) || r.skillProtocol !== 2 || !Array.isArray(r.items)) return null
    const item = r.items.find((i): i is Record<string, unknown> => objeto(i) && i.id === id)
    return item ? item.novosBots === true : null
  } catch {
    return null
  }
}

/**
 * O resultado de mexer no interruptor. `gravado`: o Hub respondeu. `conferido`: a resposta se perdeu, mas o Hub, lido de
 * novo, disse o valor que vale. `incerto`: a resposta se perdeu e a releitura também; o valor mostrado não pode ser dado
 * como confirmado (o Hub pode ter gravado).
 */
export type ResultadoNovosBots =
  | { tipo: 'gravado'; ligado: boolean }
  | { tipo: 'conferido'; ligado: boolean }
  | { tipo: 'incerto' }
  | { tipo: 'erro'; erro: 'permissao' | 'semFuncao' | 'generico' }

export async function alterarNovosBots(cliente: Cliente, alvo: Pick<Alvo, 'tipo' | 'id'>, ligado: boolean): Promise<ResultadoNovosBots> {
  if (!temInterruptorNovosBots(alvo.tipo)) return { tipo: 'erro', erro: 'generico' }
  try {
    return { tipo: 'gravado', ligado: await definirNovosBots(cliente, alvo, ligado) }
  } catch (e) {
    if (!semRespostaClara(e)) return { tipo: 'erro', erro: falhaDoInterruptor(e) }
    const lido = await lerNovosBots(cliente, alvo.id)
    return lido === null ? { tipo: 'incerto' } : { tipo: 'conferido', ligado: lido }
  }
}

/**
 * O que a tela mostra do interruptor. `ligado` é o valor exibido; `incerto`: a resposta se perdeu e a releitura falhou
 * (o valor não está confirmado); `conferido`: o valor veio de uma releitura do Hub; `aguardaLista`: o Hub já disse o valor
 * e a lista de skills (da página) ainda pode estar velha — enquanto não trouxer o MESMO valor, uma lista diferente é
 * considerada velha e não sobrescreve o que o Hub disse por último.
 */
export interface InterruptorNovosBots { ligado: boolean; incerto: boolean; conferido: boolean; aguardaLista: boolean }

export type EventoInterruptor =
  /** A lista de skills da página trouxe este valor (a prop mudou). */
  | { tipo: 'lista'; valor: boolean }
  /** O resultado de mexer no interruptor; `lista` é o valor que a lista tem agora. */
  | { tipo: 'resultado'; resultado: ResultadoNovosBots; lista: boolean }
  /** O resultado de "Conferir de novo" (`null`: a leitura falhou ou a skill não está lá). */
  | { tipo: 'conferiu'; lido: boolean | null; lista: boolean }

export const interruptorInicial = (lista: boolean): InterruptorNovosBots => ({ ligado: lista, incerto: false, conferido: false, aguardaLista: false })

/** Transição pura do interruptor (testada sem DOM). Erro claro não muda o valor: a tela só mostra a frase. */
export function proximoInterruptor(e: InterruptorNovosBots, ev: EventoInterruptor): InterruptorNovosBots {
  switch (ev.tipo) {
    case 'lista':
      if (e.incerto) return e
      if (e.aguardaLista) return ev.valor === e.ligado ? { ...e, aguardaLista: false } : e
      return { ...e, ligado: ev.valor }
    case 'resultado': {
      const r = ev.resultado
      if (r.tipo === 'erro') return e
      if (r.tipo === 'incerto') return { ...e, incerto: true, conferido: false }
      return { ligado: r.ligado, incerto: false, conferido: r.tipo === 'conferido', aguardaLista: r.ligado !== ev.lista }
    }
    case 'conferiu':
      return ev.lido === null ? e : { ligado: ev.lido, incerto: false, conferido: true, aguardaLista: ev.lido !== ev.lista }
  }
}

/** Classifica uma falha do interruptor (só permissão, Hub sem a função ou genérica). */
export function falhaDoInterruptor(e: unknown): 'permissao' | 'semFuncao' | 'generico' {
  const c = classificarFalha(e)
  return c === 'permissao' || c === 'semFuncao' ? c : 'generico'
}

/**
 * A sessão de UM "Ligar em todos os bots": fechada → lendo → prévia → aplicando → feito. `valida` diz se o Hub, o link
 * e a geração de onde o gesto partiu ainda são os mesmos; resposta de outra origem é descartada e o bloco fecha.
 */
export class SessaoEmTodos extends Observable {
  estado: Estado = { fase: 'fechado' }
  /** Cada abertura e cada refazer tem um número; resposta de número velho é ignorada. */
  #geracao = 0

  readonly alvo: Alvo
  private readonly cliente: Cliente
  private readonly valida: () => boolean

  constructor(cliente: Cliente, alvo: Alvo, valida: () => boolean = () => true) {
    super()
    this.cliente = cliente
    this.alvo = alvo
    this.valida = valida
  }

  private mudar(estado: Estado) {
    this.estado = estado
    this.changed()
  }

  /** Abre a prévia no mesmo lugar. Aberta ou lendo, não faz nada de novo. */
  abrir() {
    if (this.estado.fase !== 'fechado' && this.estado.fase !== 'erro' && this.estado.fase !== 'feito') return
    void this.lerPrevia(false)
  }

  /** Fecha e descarta qualquer resposta que ainda venha. Não desfaz uma aplicação que já foi ao Hub. */
  fechar() {
    this.#geracao += 1
    if (this.estado.fase !== 'fechado') this.mudar({ fase: 'fechado' })
  }

  /** Chamada a cada mudança do Hub/link: com a origem trocada, a prévia vira velha e o bloco fecha. */
  reconferir() {
    if (this.estado.fase !== 'fechado' && this.estado.fase !== 'aplicando' && !this.valida()) this.fechar()
  }

  /** O OK único. Só vale com uma prévia na tela que tenha quem receber. */
  async confirmar() {
    const atual = this.estado
    if (atual.fase !== 'previa' || atual.previa.recebedores.length === 0) return
    // A origem (Hub, link e geração) é conferida ANTES de qualquer escrita: com ela mudada, o bloco fecha e nada sai.
    if (!this.valida()) return this.fechar()
    const { previa } = atual
    const numero = ++this.#geracao
    this.mudar({ fase: 'aplicando', previa })
    let resposta: unknown
    try {
      resposta = await this.cliente.call<unknown>('extensaoEmTodosAplicar', {
        protocol: 1, tipo: previa.tipo, id: previa.id, botIds: previa.recebedores.map((r) => r.botId), versao: previa.versao,
      }, 60_000)
    } catch (e) {
      if (numero !== this.#geracao) return
      if (!this.valida()) return this.fechar()
      const caso = classificarFalha(e)
      // O Hub não gravou nada: refaz a prévia sozinho, sem reenviar a lista velha.
      if (caso === 'conflito') return void this.lerPrevia(true)
      return this.mudar({ fase: 'erro', erro: caso === 'generico' && semRespostaClara(e) ? 'incerto' : caso })
    }
    if (numero !== this.#geracao) return
    if (!this.valida()) return this.fechar()
    const aplicados = objeto(resposta) && Array.isArray(resposta.aplicados) && resposta.aplicados.every(texto) ? resposta.aplicados.length : null
    // Resposta que o app não sabe ler depois de enviar: pode ter aplicado; nunca repete sozinho.
    this.mudar(aplicados === null ? { fase: 'erro', erro: 'incerto' } : { fase: 'feito', quantos: aplicados })
  }

  private async lerPrevia(mudou: boolean) {
    const numero = ++this.#geracao
    this.mudar({ fase: 'lendo', mudou })
    let raw: unknown
    try {
      raw = await this.cliente.call<unknown>('extensaoEmTodosPrevia', { protocol: 1, tipo: this.alvo.tipo, id: this.alvo.id }, 30_000)
    } catch (e) {
      if (numero !== this.#geracao) return
      if (!this.valida()) return this.fechar()
      const caso = classificarFalha(e)
      return this.mudar({ fase: 'erro', erro: caso === 'conflito' ? 'generico' : caso })
    }
    if (numero !== this.#geracao) return
    if (!this.valida()) return this.fechar()
    const previa = lerPrevia(raw, this.alvo)
    this.mudar(previa ? { fase: 'previa', previa, mudou } : { fase: 'erro', erro: 'generico' })
  }
}
