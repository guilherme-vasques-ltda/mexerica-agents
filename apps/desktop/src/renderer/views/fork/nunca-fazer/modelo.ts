// MX-111 (fase 1, só orientação): a lista "Nunca fazer" de um Space ou de um bot, sem DOM nem React.
// Lê e grava pelas RPCs do Hub (#700): `nuncaFazer`, `nuncaFazerAdicionar`, `nuncaFazerEditar`,
// `nuncaFazerRemover` e `nuncaFazerReordenar`, sempre com a revisão lida. O texto digitado fica na tela:
// o modelo só diz se gravou (true) ou não (false), e quem desenha limpa o campo só no true.
import { Observable } from '../../../lib/observable.ts'

/** Os mesmos limites do Hub (`host/src/fork/nunca_fazer.rs`). */
export const LIMITE_ITENS = 20
export const LIMITE_TEXTO = 200

export type Origem = 'space' | 'bot'
export interface ItemNuncaFazer { id: string; texto: string; ordem: number; origem: Origem }
export interface ListaNuncaFazer { space: ItemNuncaFazer[]; bot: ItemNuncaFazer[]; revision: number; spaceRevision: number }

/** O que a tela mostra quando algo não deu certo; cada uma vira a chave `nuncaFazer.falha.<falha>`. */
export type Falha = 'hubAntigo' | 'soDono' | 'semConexao' | 'conflito' | 'limite' | 'invalido' | 'itemSumiu' | 'spaceSumiu' | 'botSumiu' | 'leitura' | 'envio'

export interface Link { call<T>(method: string, body?: unknown, timeoutMs?: number): Promise<T> }

/** Pontas aparadas e contagem por caractere Unicode, como o Hub. */
export const limpar = (texto: string) => texto.trim()
export const contar = (texto: string) => [...limpar(texto)].length
export const textoValido = (texto: string) => { const n = contar(texto); return n > 0 && n <= LIMITE_TEXTO }

const inteiro = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0

function lerItens(v: unknown, origem: Origem): ItemNuncaFazer[] {
  if (!Array.isArray(v)) throw new Error('nunca_fazer_resposta_invalida')
  return v.map((raw) => {
    const i = raw as Record<string, unknown> | null
    if (!i || typeof i.id !== 'string' || !i.id || typeof i.texto !== 'string' || !inteiro(i.ordem) || i.origem !== origem) throw new Error('nunca_fazer_resposta_invalida')
    return { id: i.id, texto: i.texto, ordem: i.ordem, origem }
  })
}

/** A resposta de qualquer RPC da lista; fora do contrato, nada é presumido. */
export function lerLista(value: unknown): ListaNuncaFazer {
  const v = value as Record<string, unknown> | null
  if (!v || typeof v !== 'object' || !inteiro(v.revision) || !inteiro(v.spaceRevision)) throw new Error('nunca_fazer_resposta_invalida')
  return { space: lerItens(v.space, 'space'), bot: lerItens(v.bot, 'bot'), revision: v.revision, spaceRevision: v.spaceRevision }
}

/** A falha pelo status e pelo código do Hub (`api::error_status`). */
export function falhaDe(e: unknown): Falha {
  const status = typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
  const message = e instanceof Error ? e.message : typeof e === 'object' && e && 'message' in e ? String((e as { message: unknown }).message) : String(e)
  if (status === 404 || /unknown method/i.test(message)) return 'hubAntigo'
  if (status === 409 || message.includes('nunca_fazer_revision_conflict')) return 'conflito'
  if (status === 401 || status === 403) return 'soDono'
  if (status === 0) return 'semConexao'
  if (message.includes('limit_exceeded')) return 'limite'
  if (message.includes('item_not_found')) return 'itemSumiu'
  if (message.includes('space_not_found')) return 'spaceSumiu'
  if (message.includes('bot_not_found')) return 'botSumiu'
  if (message.includes('invalid_field')) return 'invalido'
  return 'envio'
}

/** Falhas que escondem a lista: sem as RPCs, sem acesso ou sem o dono da lista. */
/** Falha de uma tela sem modelo (ainda sem Space escolhido). "Hub antigo" só vem do 404 do próprio Hub (NuncaFazerStore.falha);
 * sem modelo o Space só não carregou, então nunca é "hubAntigo": offline vira "semConexao" e online fica lendo (null). */
export const falhaSemModelo = (online: boolean): Falha | null => online ? null : 'semConexao'

export const falhaFatal = (f: Falha | null) => f === 'hubAntigo' || f === 'soDono' || f === 'spaceSumiu' || f === 'botSumiu'

/** Falhas de escrita depois das quais a lista mostrada pode estar velha. Status 0 também: a gravação pode ter
 * chegado ao Hub antes de a conexão cair, então o resultado é incerto e a revisão lida pode estar vencida. */
const RELER = new Set<Falha>(['conflito', 'itemSumiu', 'envio', 'semConexao'])

/** TEC706-01: rascunhos de edição cujo item não está mais na lista (removido em outro lugar). Durante uma
 * leitura nada é órfão: a lista ainda pode trazer o item de volta. */
export function rascunhosOrfaos(rascunhos: Readonly<Record<string, string>>, itens: readonly ItemNuncaFazer[], lendo: boolean): [string, string][] {
  if (lendo) return []
  return Object.entries(rascunhos).filter(([id]) => !itens.some((i) => i.id === id))
}

/** O Space que a página de Ajustes edita: sempre o ativo no seletor do topo, no computador e no Hub desse Space
 * (regra do dono 15/11: cada Space tem as suas configurações; nunca o de outro). Sem Space ativo, o Pessoal aberto.
 * Ativo que não bate com o computador/Hub deste store: `null` (aguarda a resolução; nunca troca por Pessoal). */
export interface SpaceAtivo { computerId: string; hostId: string | null; spaceId: string }
export function spaceDaPagina(ativo: SpaceAtivo | null, computerId: string, hostId: string | null | undefined, pessoal: string | null | undefined): string | null {
  if (!ativo) return pessoal ?? null
  return ativo.computerId === computerId && ativo.hostId === (hostId ?? null) ? ativo.spaceId : null
}

/** O computador da página: o do Space ativo; sem Space ativo (Pessoal aberto), o escolhido na página. */
export const computadorDaPagina = (ativo: SpaceAtivo | null, escolhido: string) => ativo ? ativo.computerId : escolhido

/** `hub`: o computador do Hub (ids de Space e bot só são únicos dentro dele). */
export interface Escopo { spaceId: string; botId: string | null; hub?: string }

/** UI706-01: os modelos montados agora. O Hub (#700) não emite evento de mudança da lista; quem grava avisa os
 * outros modelos do mesmo Hub que mostram a lista mudada (o editor do bot fica montado sob Ajustes). */
const vivos = new Set<NuncaFazerStore>()

/** A lista de `origem` mudou: relê os modelos que a mostram. Space → todos do Space (bot e Ajustes); bot → os do bot. */
function avisar(origem: NuncaFazerStore) {
  const { hub, spaceId, botId } = origem.escopo
  for (const outro of vivos) {
    if (outro === origem || outro.escopo.hub !== hub || outro.escopo.spaceId !== spaceId) continue
    if (botId === null || outro.escopo.botId === botId) outro.invalidar()
  }
}

/** Uma lista (a do Space, ou a só do bot com a do Space junto para leitura). */
export class NuncaFazerStore extends Observable {
  lista: ListaNuncaFazer | null = null
  lendo = false
  ocupado = false
  falha: Falha | null = null
  /** TEC706-02: a lista à vista pode estar velha (escrita incerta ou releitura que falhou). Nada se grava com
   * a revisão dela até uma leitura dar certo; a tela trava e oferece tentar de novo. */
  desatualizada = false
  #pedido = 0
  #reler = false
  readonly escopo: Escopo
  readonly #link: () => Link | null

  /** `link` devolve o cliente conectado agora, ou null sem conexão: aí nada é lido nem gravado. */
  constructor(link: () => Link | null, escopo: Escopo) {
    super()
    this.#link = link
    this.escopo = escopo
  }

  get online() { return this.#link() !== null }
  /** Os itens que esta tela edita: os do bot, ou os do Space na página de Ajustes. */
  get editaveis() { return this.lista ? (this.escopo.botId ? this.lista.bot : this.lista.space) : [] }
  get cheia() { return this.editaveis.length >= LIMITE_ITENS }
  /** A escrita fica travada: sem conexão, ocupado, lendo ou com a lista possivelmente velha. */
  get travado() { return !this.online || this.ocupado || this.lendo || this.desatualizada }

  #corpo(extra: Record<string, unknown> = {}) {
    return { spaceId: this.escopo.spaceId, ...(this.escopo.botId ? { botId: this.escopo.botId } : {}), ...extra }
  }

  /** `silencioso`: releitura de fundo (invalidar) — não trava campos nem tira o foco de quem digita. Uma escrita
   * que comece no meio passa na frente (o `#pedido` descarta esta resposta) e o CAS do Hub segura a revisão. */
  async carregar(silencioso = false) {
    const link = this.#link()
    const pedido = ++this.#pedido
    if (!link) { this.falha = 'semConexao'; this.changed(); return }
    if (!silencioso) {
      this.lendo = true
      this.changed()
    }
    try {
      const lista = lerLista(await link.call('nuncaFazer', this.#corpo()))
      if (pedido !== this.#pedido) return
      this.lista = lista
      this.falha = null
      this.desatualizada = false
    } catch (e) {
      if (pedido !== this.#pedido) return
      const falha = falhaDe(e)
      this.falha = falha === 'envio' ? 'leitura' : falha
      if (falhaFatal(falha)) this.lista = null
    } finally {
      if (pedido === this.#pedido) {
        this.lendo = false
        this.changed()
        // Pediram releitura durante esta leitura: a resposta pode ser de antes da mudança.
        if (this.#reler) { this.#reler = false; void this.carregar(true) }
      }
    }
  }

  /** Uma escrita com a revisão lida. Conflito relê a lista e mantém a falha à vista; o texto fica com a tela. */
  async #gravar(metodo: string, extra: Record<string, unknown>): Promise<boolean> {
    const link = this.#link()
    if (!link) { this.falha = 'semConexao'; this.changed(); return false }
    if (!this.lista || this.ocupado || this.lendo || this.desatualizada) return false
    const pedido = ++this.#pedido
    this.ocupado = true
    this.falha = null
    this.changed()
    try {
      const lista = lerLista(await link.call(metodo, this.#corpo({ ...extra, expectedRevision: this.lista.revision })))
      if (pedido !== this.#pedido) return false
      this.lista = lista
      avisar(this)
      return true
    } catch (e) {
      const falha = falhaDe(e)
      if (pedido !== this.#pedido) return false
      this.ocupado = false
      // Conflito, item sumido ou resultado incerto: relê a lista do Hub em vez de presumir. Até a releitura
      // dar certo, a revisão velha não serve para gravar (a leitura bem-sucedida limpa a marca).
      if (RELER.has(falha)) {
        this.desatualizada = true
        await this.carregar()
      }
      // Releitura falhou: fica a falha dela (a lista segue travada). Deu certo: volta a da escrita, e "sem
      // conexão" vira "não deu para salvar", porque a lista já foi relida e destravou.
      if (this.desatualizada) this.falha ??= falha
      else if (!falhaFatal(this.falha)) this.falha = falha === 'semConexao' ? 'envio' : falha
      return false
    } finally {
      this.ocupado = false
      this.changed()
      if (this.#reler) { this.#reler = false; void this.carregar(true) }
    }
  }

  adicionar(texto: string) {
    if (!textoValido(texto) || this.cheia) return Promise.resolve(false)
    return this.#gravar('nuncaFazerAdicionar', { texto: limpar(texto) })
  }

  editar(id: string, texto: string) {
    if (!textoValido(texto)) return Promise.resolve(false)
    return this.#gravar('nuncaFazerEditar', { id, texto: limpar(texto) })
  }

  remover(id: string) { return this.#gravar('nuncaFazerRemover', { id }) }

  /** Sobe (-1) ou desce (+1) um item; manda a lista inteira na ordem nova. */
  mover(id: string, passo: -1 | 1) {
    const ids = this.editaveis.map((i) => i.id)
    const de = ids.indexOf(id)
    const para = de + passo
    if (de < 0 || para < 0 || para >= ids.length) return Promise.resolve(false)
    ;[ids[de], ids[para]] = [ids[para]!, ids[de]!]
    return this.#gravar('nuncaFazerReordenar', { ids })
  }

  /** Liga este modelo ao aviso entre modelos; devolve o desligar (para o efeito da tela). */
  ativar() { vivos.add(this); return () => { vivos.delete(this) } }

  /** A lista pode ter mudado fora daqui (outro modelo, outra janela ou dispositivo): relê. Com uma escrita em
   * andamento, relê depois dela, sem cortar o resultado; sem conexão, quem relê é a volta da conexão. */
  invalidar() {
    if (!this.online) return
    if (this.ocupado || this.lendo) { this.#reler = true; return }
    void this.carregar(true)
  }

  limparFalha() { if (this.falha) { this.falha = null; this.changed() } }
}
