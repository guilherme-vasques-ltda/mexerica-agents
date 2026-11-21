// DON-906 (C2): o medidor de contexto da conversa. Leitura própria e estreita do Hub (`donnaContext`), por conversa e
// sessão; nada no mirror, no hello nem no cache (MIRROR_SCHEMA intocado). Sem evento: a view relê a cada 2 s enquanto
// está montada, visível e online; resposta de outra conexão (Hub, reconexão, space) é descartada.
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { ROUTING_PERSONAL } from '../../../../shared/fork-roteamento.ts'
import { formatarData, formatarNumero } from '../../../lib/fork-i18n/index.ts'
import { Observable } from '../../../lib/observable.ts'

export const RPC_LER = 'donnaContext'
export const RPC_COMPRIMIR = 'donnaContextCompact'
export const INTERVALO_MS = 2000

export type ParteId = 'fixo' | 'memoria' | 'skills' | 'ferramentas' | 'conversa'
export const PARTES_ID: readonly ParteId[] = ['fixo', 'memoria', 'skills', 'ferramentas', 'conversa']
/**
 * O peso de uma parte do contexto. O motor informa só o total usado; `estimado` = a Mexerica contou o texto que entregou
 * (ou, na conversa, tirou as partes contadas do total) e estimou os tokens; `naoInformado` = não há número, e nenhum é inventado.
 */
export interface ParteContexto { id: ParteId; origem: 'estimado' | 'naoInformado'; tokens: number | null }
/** `null`: Hub sem a divisão por parte (ou resposta que o app não entende): o balão diz isso em vez de esconder. */
export interface PartesContexto { itens: ParteContexto[]; caracteresPorToken: number }

/** O `context` da resposta do Hub, já conferido. */
export interface LeituraContexto {
  used: number
  size: number
  updatedAt: number
  /** Perto de quantos tokens o motor comprime sozinho (regra do crate). */
  trigger: number
  lastCompaction: { at: number; byHost: boolean } | null
  canCompact: boolean
  /** Turno, espera ou compressão em andamento: Comprimir agora desligado. */
  busy: boolean
  /** Opaco: prende o clique ao mesmo motor/ACP que foi medido. */
  contextId: string
  /** DON-910 (C3): o Hub pausou a compressão automática desta conversa depois de falhas seguidas. */
  automaticPaused: boolean
  /** Falhas seguidas da compressão automática nesta conversa (zera com uma que dá certo). */
  automaticFailures: number
  /** O que ocupa o espaço, parte a parte. */
  partes: PartesContexto | null
}

const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) && v >= 0 ? v : null)

/** O pedido de leitura: `sessionId` null = a principal; uma string = exatamente aquela sessão extra. */
export interface Pedido { hubId: string; botId: string; sessionId: string | null; spaceId?: string | null }

/**
 * A resposta conferida. `scopeId`: opaco, um por Hub/bot/lane/ACP/motor (nova sessão ou troca de ACP = outro escopo).
 * `epoch`: época de compactação dentro do escopo (não o `principalEpoch` do transporte); só cresce no mesmo escopo.
 */
export interface Resposta { scopeId: string; epoch: number; leitura: LeituraContexto | null }

const corpo = (botId: string, sessionId: string | null, spaceId: string | null, extra: Record<string, unknown> = {}) => ({
  botId,
  ...(spaceId !== null ? { spaceId } : {}),
  ...(sessionId !== null ? { sessionId } : {}),
  ...extra,
})

/**
 * Confere a resposta de `donnaContext` contra o pedido: mesmo Hub (`hubId`), mesma conversa, mesma sessão (a
 * principal volta com `sessionId` = `botId`), `scopeId` não vazio e `epoch` válido. Fora disso, null: a resposta não vale e o medidor
 * some, sem cair em outra sessão. `context` ausente ou sem tamanho: resposta válida, leitura null (oculto).
 */
export function conferirContexto(raw: unknown, pedido: Pedido): Resposta | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  if (r.hubId !== pedido.hubId || r.botId !== pedido.botId || r.sessionId !== (pedido.sessionId ?? pedido.botId)) return null
  // Contrato de Space: a resposta carrega o Space resolvido; Hub antigo sem `spaceId` só vale para o Pessoal.
  if ((typeof r.spaceId === 'string' ? r.spaceId : ROUTING_PERSONAL) !== (pedido.spaceId ?? ROUTING_PERSONAL)) return null
  if (typeof r.scopeId !== 'string' || !r.scopeId) return null
  if (typeof r.epoch !== 'number' || !Number.isSafeInteger(r.epoch) || r.epoch < 0) return null
  return { scopeId: r.scopeId, epoch: r.epoch, leitura: conferirMedida(r.context) }
}

/** Confere `context.parts`: as cinco partes, uma vez cada, na ordem do contrato; qualquer desvio descarta tudo. */
export function conferirPartes(raw: unknown): PartesContexto | null {
  if (!raw || typeof raw !== 'object') return null
  const { items, charsPerToken } = raw as Record<string, unknown>
  const porToken = num(charsPerToken)
  if (!porToken || !Array.isArray(items) || items.length !== PARTES_ID.length) return null
  const itens: ParteContexto[] = []
  for (const [i, bruto] of items.entries()) {
    const item = bruto as Record<string, unknown> | null
    if (!item || item.id !== PARTES_ID[i]) return null
    if (item.origin === 'naoInformado') itens.push({ id: PARTES_ID[i]!, origem: 'naoInformado', tokens: null })
    else if (item.origin === 'estimado' && Number.isSafeInteger(item.tokens) && num(item.tokens) !== null) itens.push({ id: PARTES_ID[i]!, origem: 'estimado', tokens: item.tokens as number })
    else return null
  }
  return { itens, caracteresPorToken: porToken }
}

function conferirMedida(raw: unknown): LeituraContexto | null {
  if (!raw || typeof raw !== 'object') return null
  const c = raw as Record<string, unknown>
  const size = num(c.size)
  const used = num(c.used)
  const trigger = num(c.trigger)
  if (!size || used === null || trigger === null || typeof c.contextId !== 'string' || !c.contextId) return null
  const lc = c.lastCompaction as Record<string, unknown> | null | undefined
  const at = lc && typeof lc === 'object' ? num(lc.at) : null
  return {
    used,
    size,
    updatedAt: num(c.updatedAt) ?? 0,
    trigger,
    lastCompaction: at !== null ? { at, byHost: lc!.byHost === true } : null,
    canCompact: c.canCompact === true,
    busy: c.busy === true,
    contextId: c.contextId,
    automaticPaused: c.automaticPaused === true,
    automaticFailures: Number.isSafeInteger(c.automaticFailures) && (c.automaticFailures as number) >= 0 ? (c.automaticFailures as number) : 0,
    partes: conferirPartes(c.parts),
  }
}

export const percentual = (l: Pick<LeituraContexto, 'size' | 'used'>) => Math.max(0, Math.min(100, Math.round((l.used / l.size) * 100)))
/** Acima de 90% o anel fica vermelho. */
export const cheio = (pct: number) => pct > 90

/** Tokens curtos no formato do idioma (Intl, notação compacta): pt-BR "84 mil", "1,2 mi"; en "84K", "1.2M". */
export const tokensCurtos = (idioma: AppLanguage, n: number) => formatarNumero(idioma, n, { notation: 'compact', maximumFractionDigits: 1 })
/** 42 → "42%" no formato do idioma (fr "42 %"). */
export const porcento = (idioma: AppLanguage, pct: number) => formatarNumero(idioma, pct / 100, { style: 'percent' })
/** HH:MM local no formato do idioma. */
export const hora = (idioma: AppLanguage, ms: number) => formatarData(idioma, ms, { hour: '2-digit', minute: '2-digit' })

/** Por que Comprimir agora está desligado; null = pode. Código estável: a view traduz (`contexto.bloqueio.*`). */
export type Bloqueio = 'enviando' | 'semConexao' | 'turno' | 'semMotor'
export function motivoBloqueio({ busy, online, working, enviando, canCompact = true }: { busy: boolean; online: boolean; working: boolean; enviando: boolean; canCompact?: boolean }): Bloqueio | null {
  if (enviando) return 'enviando'
  if (!online) return 'semConexao'
  if (!canCompact) return 'semMotor'
  if (working || busy) return 'turno'
  return null
}

export interface Chamador { call<T>(method: string, body?: unknown): Promise<T> }

/** A conexão em que a leitura foi pedida: resposta de outra (outro Hub, reconexão, outro space) é descartada. */
export interface Origem { client: Chamador; hostId: string; epoch: number; spaceId?: string | null }

const semRpc = (e: unknown) => {
  const status = (e as { status?: unknown })?.status
  return status === 404 || /unknown method|Not available in the web demo/i.test(String((e as { message?: unknown })?.message ?? e))
}

/** Por que o Hub recusou Comprimir agora (C2-R1-02). Código estável, nunca o texto do Hub; a view traduz (`contexto.recusa.*`). */
export type Recusa = 'repetida' | 'ocupada' | 'mudou' | 'indisponivel' | 'falha' | 'antigo' | 'semMotor'
export function codigoRecusa(e: unknown): Recusa {
  const raw = String((e as { message?: unknown })?.message ?? e)
  if (semRpc(e)) return 'antigo'
  if (/compaction_unsupported|context_cannot_compact/.test(raw)) return 'semMotor'
  if (raw.includes('context_repeat_guard')) return 'repetida'
  if (raw.includes('context_busy')) return 'ocupada'
  // O Hub não tem o motor ligado agora ou ainda não mediu a conversa: passageiro, não é "mudou" nem "sem suporte".
  if (/context_engine_unavailable|context_unavailable/.test(raw)) return 'indisponivel'
  if (/context_engine_changed|unknown_session/.test(raw)) return 'mudou'
  return 'falha'
}

/** O que aconteceu depois que o Hub aceitou Comprimir agora (a compressão roda sozinha, fora da resposta do RPC). */
export interface Resultado {
  fase: 'andamento' | 'feito' | 'semEfeito' | 'incerto' | 'mudou'
  /** Tokens usados antes do pedido. */
  antes: number
  /** Tokens usados depois, só quando o Hub já mediu o contexto novo (menor que o de antes). */
  depois: number | null
}
/** Sem sinal de fim depois disto, a compressão é dada como sem efeito (o Hub não confirmou). */
export const ESPERA_MS = 20_000
/** Sem desfecho nem leitura que feche o pedido depois disto, o aviso vira "não deu para confirmar" (relógio próprio, não depende de leitura). */
export const LIMITE_MS = 90_000
/** Quanto tempo o resultado fica no balão. */
export const VALIDADE_MS = 120_000

/** Principal e extras nunca dividem chave: a principal é `null`, nunca o id do bot. */
const chaveDe = (botId: string, sessionId: string | null) => JSON.stringify([botId, sessionId])

/** Uma leitura por conversa+sessão, válida só para a origem em que foi pedida. */
export class ModeloContexto extends Observable {
  #dados = new Map<string, { origem: Origem; scopeId: string | null; epoch: number; leitura: LeituraContexto | null }>()
  #pedidos = new Map<string, number>()
  #seq = 0
  /** Pedido de compressão enviado e ainda sem resposta, por chave. */
  #enviando = new Map<string, { relogio: ReturnType<typeof setTimeout> | null }>()
  /** Última recusa de Comprimir agora, por chave, presa à conexão em que veio. */
  #recusas = new Map<string, { origem: Origem; recusa: Recusa; em: number; relogio: ReturnType<typeof setTimeout> | null }>()
  /** Compressão aceita e ainda sem desfecho/resultado, por chave, presa à conexão. */
  #resultados = new Map<string, { origem: Origem; scopeId: string; epoch: number; desde: number; fim: number | null; antes: number; antesAt: number; visteBusy: boolean; fase: Resultado['fase']; depois: number | null; relogio: ReturnType<typeof setTimeout> | null }>()
  /**
   * "Compactando…" visto na atividade do bot (compactação automática ou de outra janela), por chave: quando começou
   * (relógio do modelo), quantas telas observam (campo e fluxo) e o relógio que redesenha ao vencer o limite. O registro
   * sobrevive à troca de conexão (`descartar`): quem o solta são as telas, ao desmontar ou quando a atividade acaba.
   */
  #atividade = new Map<string, { donos: number; relogio: ReturnType<typeof setTimeout> | null }>()
  /** Hub sem `donnaContext` nesta conexão: nada de poll até outra conexão. */
  #semSuporte: Origem | null = null
  /** A demo da web recusa tudo como "indisponível": não é um Hub antigo e não leva aviso. */
  #demo: Origem | null = null
  readonly #origem: () => Origem | null
  readonly #limiteMs: number
  readonly #relogio: { agora: () => number; agendar: typeof setTimeout; cancelar: typeof clearTimeout }
  #geracao = 0

  constructor(origem: () => Origem | null, limiteMs = LIMITE_MS, relogio = { agora: () => Date.now(), agendar: setTimeout, cancelar: clearTimeout }) {
    super(); this.#origem = origem; this.#limiteMs = limiteMs; this.#relogio = relogio
  }

  /** Cancela recursos da conversa que saiu de cena e invalida respostas ainda em voo. */
  descartar(botId: string, sessionId: string | null) {
    const chave = chaveDe(botId, sessionId)
    this.#geracao++
    this.#pedidos.delete(chave)
    const recusa = this.#recusas.get(chave)
    if (recusa?.relogio) this.#relogio.cancelar(recusa.relogio)
    this.#recusas.delete(chave)
    const resultado = this.#resultados.get(chave)
    if (resultado?.relogio) this.#relogio.cancelar(resultado.relogio)
    this.#resultados.delete(chave)
    this.#soltarEnvio(chave)
  }

  /**
   * O PRIMEIRO sinal de compactação por conversa/sessão (relógio do modelo): o prazo de {@link LIMITE_MS} é absoluto a
   * partir dele. É o único escritor do início: outro sinal (atividade, pedido, aceite tardio), trocar de conexão
   * (`descartar`) ou desmontar a tela NÃO o reiniciam nem o estendem. Só some quando a compactação acaba de verdade
   * (desfecho do pedido, recusa, a atividade deixou de ser "resumindo") sem outro sinal em curso, ou quando um sinal
   * novo chega depois de o anterior já ter vencido.
   */
  #inicios = new Map<string, number>()

  #temSinal(chave: string) {
    return this.#atividade.has(chave) || this.#enviando.has(chave) || this.#resultados.get(chave)?.fase === 'andamento'
  }

  /** Chamar ANTES de registrar o sinal novo. */
  #marcar(chave: string) {
    const antes = this.#inicios.get(chave)
    const agora = this.#relogio.agora()
    if (antes === undefined || (!this.#temSinal(chave) && agora - antes >= this.#limiteMs)) this.#inicios.set(chave, agora)
  }

  /** A compactação acabou de verdade: sem outro sinal em curso, o início some. */
  #fim(chave: string) {
    if (!this.#temSinal(chave)) this.#inicios.delete(chave)
  }

  /** Quanto falta para vencer o prazo do primeiro sinal. */
  #restante(chave: string) {
    const inicio = this.#inicios.get(chave)
    return inicio === undefined ? this.#limiteMs : Math.max(0, this.#limiteMs - (this.#relogio.agora() - inicio))
  }

  /** O bot deixou de resumir nesta conversa/sessão: a compactação acabou. (Desmontar a tela NÃO chama isto.) */
  atividadeAcabou(botId: string, sessionId: string | null) { this.#fim(chaveDe(botId, sessionId)) }

  #soltarEnvio(chave: string) {
    const e = this.#enviando.get(chave)
    if (e?.relogio) this.#relogio.cancelar(e.relogio)
    this.#enviando.delete(chave)
  }

  /**
   * Uma tela passa a observar "o bot está resumindo" nesta conversa/sessão; devolve o que a solta. A primeira observação
   * marca o começo e arma o relógio do limite; as demais dividem o mesmo registro (um relógio só). Soltou a última (a tela
   * desmontou ou a atividade acabou), o registro e o relógio somem e o envio volta.
   */
  observarAtividade(botId: string, sessionId: string | null): () => void {
    const chave = chaveDe(botId, sessionId)
    let a = this.#atividade.get(chave)
    if (!a) {
      this.#marcar(chave)
      a = { donos: 0, relogio: null }
      this.#vigiar(a, this.#restante(chave))
      this.#atividade.set(chave, a)
      this.changed()
    }
    a.donos++
    const registro = a
    let solto = false
    return () => {
      if (solto) return
      solto = true
      if (--registro.donos > 0 || this.#atividade.get(chave) !== registro) return
      if (registro.relogio) this.#relogio.cancelar(registro.relogio)
      this.#atividade.delete(chave)
      this.changed()
    }
  }

  /** Redesenha as telas quando o limite vence, sem depender de leitura nem de evento do Hub. */
  #vigiar(r: { relogio: ReturnType<typeof setTimeout> | null }, ms: number) {
    r.relogio = this.#relogio.agendar(() => { r.relogio = null; this.changed() }, ms)
    ;(r.relogio as { unref?: () => void }).unref?.()
  }

  /**
   * O que o chat mostra e se o envio está bloqueado: junta a atividade do bot com o pedido de Comprimir agora. O limite
   * de {@link LIMITE_MS} conta do PRIMEIRO sinal (atividade ou pedido) e é lido do relógio, então vale mesmo sem leitura,
   * sem evento do Hub e depois de trocar de conexão; um sinal que chega depois não o estende.
   */
  compactacao(botId: string, sessionId: string | null, ativaAgora = true): Compactacao | null {
    const chave = chaveDe(botId, sessionId)
    const agora = this.#relogio.agora()
    // `ativaAgora` é a verdade da tela neste quadro: um registro que ainda não foi solto não prende o envio sozinho.
    const atividade = ativaAgora && this.#atividade.has(chave)
    const pedido = this.#resultados.get(chave)
    const manual = this.#enviando.has(chave) || (!!pedido && pedido.fase === 'andamento' && this.#mesma(pedido.origem, this.#origem()))
    // Um só começo: o do primeiro sinal, que sobrevive a outro sinal, à troca de conexão e à desmontagem.
    const vencido = agora - (this.#inicios.get(chave) ?? agora) >= this.#limiteMs
    return compactacaoDaConversa({
      atividade: atividade ? (vencido ? 'expirou' : 'ativa') : null,
      emAndamento: manual && !vencido,
      resultado: this.resultado(botId, sessionId),
      recusa: this.recusa(botId, sessionId),
    })
  }

  #mesma(a: Origem | null, b: Origem | null) {
    return !!a && !!b && a.client === b.client && a.hostId === b.hostId && a.epoch === b.epoch && (a.spaceId ?? null) === (b.spaceId ?? null)
  }

  /** O Hub desta conexão responde `donnaContext`? (falso só depois de recusar como método desconhecido) */
  suportado() { return !this.#mesma(this.#semSuporte, this.#origem()) }

  /** Hub antigo de verdade (sem `donnaContext`): o medidor aparece avisando, em vez de sumir. Nunca na demo da web. */
  hubAntigo() { return !this.suportado() && !this.#mesma(this.#demo, this.#origem()) }

  /** A leitura atual, só se for desta conexão; de outra, null (nunca números de outro Hub ou sessão). */
  of(botId: string, sessionId: string | null): LeituraContexto | null {
    const r = this.#dados.get(chaveDe(botId, sessionId))
    return r && this.#mesma(r.origem, this.#origem()) ? r.leitura : null
  }

  /**
   * A recusa do último Comprimir agora desta conversa/sessão nesta conexão; de outra, null. "Resumida há pouco"
   * expira sozinha depois de {@link VALIDADE_MS}, como o resultado (um relógio próprio redesenha); as demais ficam
   * até o próximo pedido.
   */
  recusa(botId: string, sessionId: string | null): Recusa | null {
    const r = this.#recusas.get(chaveDe(botId, sessionId))
    if (!r || !this.#mesma(r.origem, this.#origem())) return null
    if (r.recusa === 'repetida' && this.#relogio.agora() - r.em >= VALIDADE_MS) return null
    return r.recusa
  }

  /** O desfecho do último Comprimir agora aceito nesta conversa/sessão e conexão; some depois de {@link VALIDADE_MS}. */
  resultado(botId: string, sessionId: string | null): Resultado | null {
    const r = this.#resultados.get(chaveDe(botId, sessionId))
    if (!r || !this.#mesma(r.origem, this.#origem())) return null
    if (r.fim !== null && this.#relogio.agora() - r.fim > VALIDADE_MS) return null
    return { fase: r.fase, antes: r.antes, depois: r.depois }
  }

  #encerrar(chave: string, r: { fase: Resultado['fase']; fim: number | null; relogio: ReturnType<typeof setTimeout> | null }, fase: Resultado['fase']) {
    r.fase = fase
    r.fim = this.#relogio.agora()
    if (r.relogio) this.#relogio.cancelar(r.relogio)
    r.relogio = null
    this.#fim(chave)
  }

  /**
   * Fecha o pedido aceito. Escopo (ACP/motor/sessão) diferente do pedido: "mudou", sem comparar medidas de escopos
   * diferentes. Época nova no mesmo escopo = compactação concluída, mesmo sem `context` (o Hub o omite até a próxima
   * resposta medir); a medida "depois" só aparece quando existe de verdade.
   */
  #avaliar(chave: string, scopeId: string | null, epoch: number, leitura: LeituraContexto | null) {
    const r = this.#resultados.get(chave)
    if (!r || r.fase !== 'andamento' || scopeId === null) return
    if (scopeId !== r.scopeId) return this.#encerrar(chave, r, 'mudou')
    if (epoch > r.epoch || (leitura?.lastCompaction?.at ?? 0) > r.antesAt) {
      this.#encerrar(chave, r, 'feito')
      r.depois = leitura && leitura.used < r.antes ? leitura.used : null
      return
    }
    if (!leitura) return
    if (leitura.busy) r.visteBusy = true
    if (!leitura.busy && (r.visteBusy || this.#relogio.agora() - r.desde > ESPERA_MS)) this.#encerrar(chave, r, 'semEfeito')
  }

  /** Pedido em curso (RPC enviado ou compressão aceita sem desfecho): o botão fica desligado e outro clique não faz nada. */
  emAndamento(botId: string, sessionId: string | null) {
    const chave = chaveDe(botId, sessionId)
    if (this.#enviando.has(chave)) return true
    const r = this.#resultados.get(chave)
    return !!r && r.fase === 'andamento' && this.#mesma(r.origem, this.#origem())
  }

  enviando(botId: string, sessionId: string | null) { return this.#enviando.has(chaveDe(botId, sessionId)) }

  async ler(botId: string, sessionId: string | null) {
    const origem = this.#origem()
    if (!origem || !this.suportado()) return
    const chave = chaveDe(botId, sessionId)
    const seq = ++this.#seq
    this.#pedidos.set(chave, seq)
    let resposta: Resposta | null
    try {
      resposta = conferirContexto(await origem.client.call(RPC_LER, corpo(botId, sessionId, origem.spaceId ?? null)), { hubId: origem.hostId, botId, sessionId, spaceId: origem.spaceId })
    } catch (e) {
      if (semRpc(e) && this.#mesma(origem, this.#origem())) {
        this.#semSuporte = origem
        if (/Not available in the web demo/i.test(String((e as { message?: unknown })?.message ?? e))) this.#demo = origem
        this.changed()
      }
      // Erro passageiro: mantém a última leitura desta mesma conexão; a próxima volta confere de novo.
      return
    }
    if (this.#pedidos.get(chave) !== seq || !this.#mesma(origem, this.#origem())) return
    const antes = this.#dados.get(chave)
    const mesmaOrigem = !!antes && this.#mesma(antes.origem, origem)
    if (!resposta) {
      // Outro Hub, outra conversa/sessão ou época inválida: oculto, sem aproveitar outra medida.
      this.#dados.set(chave, { origem, scopeId: null, epoch: 0, leitura: null })
    } else {
      // Mesma conexão e mesmo escopo (ACP): época mais velha que a já vista é observação atrasada, descartada.
      // Escopo novo (nova sessão, outro ACP/motor): a medida velha deixa de valer e a época recomeça.
      if (mesmaOrigem && antes!.scopeId === resposta.scopeId && resposta.epoch < antes!.epoch) return
      this.#dados.set(chave, { origem, scopeId: resposta.scopeId, epoch: resposta.epoch, leitura: resposta.leitura })
    }
    // Só uma leitura válida iniciada depois da recusa pode atualizar o suporte.
    if (resposta && this.#recusas.get(chave)?.recusa === 'semMotor') this.#recusas.delete(chave)
    const atual = this.#dados.get(chave)
    this.#avaliar(chave, atual?.scopeId ?? null, atual?.epoch ?? 0, atual?.leitura ?? null)
    this.changed()
  }

  /** Pede a compressão do mesmo motor medido (`contextId`); a leitura seguinte mostra o resultado real. */
  async comprimir(botId: string, sessionId: string | null, contextId: string) {
    const origem = this.#origem()
    const chave = chaveDe(botId, sessionId)
    const geracao = this.#geracao
    // Um pedido por conversa+sessão: enquanto um está em curso, o segundo clique não envia nada.
    if (!origem || this.emAndamento(botId, sessionId)) return
    const base = this.#dados.get(chave)
    // Sem medida de um escopo desta conexão não há o que comparar nem como prender o pedido.
    if (!base || !base.leitura || base.scopeId === null || !this.#mesma(base.origem, origem)) return
    this.#soltarEnvio(chave)
    this.#marcar(chave)
    const envio = { relogio: null as ReturnType<typeof setTimeout> | null }
    this.#vigiar(envio, this.#restante(chave))
    this.#enviando.set(chave, envio)
    const recusaAnterior = this.#recusas.get(chave)
    if (recusaAnterior?.relogio) this.#relogio.cancelar(recusaAnterior.relogio)
    this.#recusas.delete(chave)
    const velho = this.#resultados.get(chave)
    if (velho?.relogio) this.#relogio.cancelar(velho.relogio)
    this.#resultados.delete(chave)
    const antes = base.leitura
    const escopo = base.scopeId
    this.changed()
    try {
      const r = await origem.client.call<{ accepted?: unknown }>(RPC_COMPRIMIR, corpo(botId, sessionId, origem.spaceId ?? null, { contextId }))
      if (r?.accepted !== true) throw new Error('context_compaction_not_started')
      if (geracao === this.#geracao && this.#mesma(origem, this.#origem())) {
        // Referência = época e marcador de compactação de ANTES do envio. O escopo pode ter mudado enquanto o RPC voava (pedido não vale).
        // Um sucesso que o poll já viu antes do aceite continua sendo sucesso: a leitura mais recente é avaliada contra essa referência.
        const agora = this.#dados.get(chave)
        const mudou = !agora || agora.scopeId !== escopo
        // `desde` é o aceite (a espera de 20 s por sinal de fim); o prazo de 90 s é o do primeiro sinal (`#inicios`).
        const reg = { origem, scopeId: escopo, epoch: base.epoch, desde: this.#relogio.agora(), fim: null as number | null, antes: antes.used, antesAt: antes.lastCompaction?.at ?? 0, visteBusy: false, fase: 'andamento' as Resultado['fase'], depois: null as number | null, relogio: null as ReturnType<typeof setTimeout> | null }
        this.#resultados.set(chave, reg)
        if (mudou) this.#encerrar(chave, reg, 'mudou')
        else {
          // Relógio próprio: sem desfecho a tempo (leituras falhando, Hub fora), o aviso fecha em "não deu para confirmar".
          reg.relogio = this.#relogio.agendar(() => { if (reg.fase === 'andamento') { this.#encerrar(chave, reg, 'incerto'); this.changed() } }, this.#restante(chave))
          ;(reg.relogio as { unref?: () => void }).unref?.()
          this.#avaliar(chave, agora.scopeId, agora.epoch, agora.leitura)
        }
      }
    } catch (e) {
      // Recusado: o código fica no balão desta conversa e desta conexão até o próximo pedido, outra conexão ou (só "resumida há pouco") a validade.
      if (geracao === this.#geracao && this.#mesma(origem, this.#origem())) {
        const recusa = codigoRecusa(e)
        // Uma leitura já em voo antecede a recusa e não comprova suporte atual.
        if (recusa === 'semMotor') this.#pedidos.delete(chave)
        const registro = { origem, recusa, em: this.#relogio.agora(), relogio: null as ReturnType<typeof setTimeout> | null }
        this.#recusas.set(chave, registro)
        if (recusa === 'repetida') {
          registro.relogio = this.#relogio.agendar(() => {
            if (this.#recusas.get(chave) !== registro) return
            this.#recusas.delete(chave)
            if (this.#mesma(origem, this.#origem())) this.changed()
          }, VALIDADE_MS)
          ;(registro.relogio as { unref?: () => void }).unref?.()
        }
      }
    } finally {
      if (geracao === this.#geracao) {
        this.#soltarEnvio(chave)
        this.#fim(chave)
        this.changed()
        if (this.#mesma(origem, this.#origem())) await this.ler(botId, sessionId)
      }
    }
  }
}

// --- O que o balão mostra (Mockup do dono, 16/11): função pura, a view só desenha. Códigos estáveis; a view traduz. ---

/** A frase do aviso sob o botão: `contexto.aviso.<id>.titulo` e `.detalhe`. */
export type AvisoId = 'andamento' | 'feito' | 'feitoSemMedida' | 'semEfeito' | 'incerto' | 'mudou' | 'repetida' | 'ocupada'
  | 'indisponivel' | 'falha' | 'antigo' | 'semMotor' | 'semConexao' | 'turno' | 'pausado' | 'semSuporte'
/** `neutro`: o app está certo em recusar (não é erro); `atencao`: pede algo fora dali; `erro`: falha de verdade. */
export type TomAviso = 'neutro' | 'ok' | 'atencao' | 'erro'
export type IconeAviso = 'relogio' | 'ok' | 'info' | 'alerta'
export interface Aviso { id: AvisoId; tom: TomAviso; icone: IconeAviso; vars?: Record<string, string> }

const AVISOS: Record<AvisoId, [TomAviso, IconeAviso]> = {
  andamento: ['neutro', 'relogio'], feito: ['ok', 'ok'], feitoSemMedida: ['ok', 'ok'], semEfeito: ['neutro', 'info'],
  incerto: ['atencao', 'info'], mudou: ['neutro', 'info'], repetida: ['neutro', 'relogio'], ocupada: ['neutro', 'relogio'],
  indisponivel: ['neutro', 'info'], falha: ['erro', 'alerta'], antigo: ['atencao', 'info'], semMotor: ['neutro', 'info'],
  semConexao: ['neutro', 'relogio'], turno: ['neutro', 'relogio'], pausado: ['atencao', 'info'], semSuporte: ['atencao', 'info'],
}
const aviso = (id: AvisoId, vars?: Record<string, string>): Aviso => ({ id, tom: AVISOS[id][0], icone: AVISOS[id][1], ...(vars ? { vars } : {}) })

/** O aviso do Hub antigo (sem `donnaContext`): o medidor fica, sem número, dizendo o que falta. */
export const avisoSemSuporte = (): Aviso => aviso('semSuporte')

/** O desfecho de um pedido aceito como aviso. Sem medida de depois (ou sem tamanho), "Pronto" não inventa números. */
export function avisoResultado(idioma: AppLanguage, resultado: Resultado, size: number | null): Aviso {
  if (resultado.fase !== 'feito') return aviso(resultado.fase)
  if (resultado.depois === null || size === null) return aviso('feitoSemMedida')
  return aviso('feito', {
    antes: tokensCurtos(idioma, resultado.antes), depois: tokensCurtos(idioma, resultado.depois),
    de: porcento(idioma, percentual({ used: resultado.antes, size })), para: porcento(idioma, percentual({ used: resultado.depois, size })),
  })
}

export interface VistaBalao {
  /** Quanto da conversa está ocupada (0–100) e se passou de 90%. */
  pct: number
  alto: boolean
  /** Depois de "Pronto" com medida: onde estava antes (a parte listrada da barra); senão null. */
  antesPct: number | null
  /** Onde a Mexerica resume sozinha, em % da barra; null se não vale (pausado, ou o gatilho não cabe na barra). */
  marca: number | null
  /** O selo de "pronto" ao lado do título. */
  pronto: boolean
  /** A frase de estado em negrito: `contexto.estado.<faixa>`. */
  faixa: 'depois' | 'sobra' | 'ficando' | 'quase'
  /** O que vem depois da frase: a marca ("resume sozinha perto de X%"), o convite a comprimir (quase cheia) ou nada. */
  detalhe: 'marca' | 'quase' | null
  botao: 'comprimir' | 'comprimindo' | 'tentar'
  desligado: boolean
  /** O aviso sob o botão; null = a frase de ajuda do botão. */
  aviso: Aviso | null
}

export interface EntradaVista {
  idioma: AppLanguage
  leitura: LeituraContexto
  online: boolean
  working: boolean
  /** RPC enviado, ainda sem resposta. */
  enviando: boolean
  /** RPC enviado ou compressão aceita sem desfecho. */
  emAndamento: boolean
  recusa: Recusa | null
  resultado: Resultado | null
}

/** Recusas que deixam Comprimir agora desligado: o clique só repetiria a recusa. */
const RECUSA_DESLIGA: readonly Recusa[] = ['repetida', 'antigo', 'semMotor']

export function vistaBalao({ idioma, leitura, online, working, enviando, emAndamento, recusa, resultado }: EntradaVista): VistaBalao {
  const pct = percentual(leitura)
  const bloqueio = motivoBloqueio({ busy: leitura.busy, online, working, enviando: emAndamento, canCompact: leitura.canCompact })
  const medido = resultado?.fase === 'feito' && resultado.depois !== null ? resultado : null
  const marca = !leitura.automaticPaused && leitura.trigger > 0 && leitura.trigger < leitura.size
    ? Math.max(1, Math.min(99, Math.round((leitura.trigger / leitura.size) * 100))) : null
  const base = pct > 90 ? 'quase' : pct > 70 ? 'ficando' : 'sobra'
  const faixa = medido && base === 'sobra' ? 'depois' : base
  const detalhe = base === 'quase' ? 'quase' : marca !== null && pct < marca ? 'marca' : null

  let a: Aviso | null = null
  if (enviando || resultado?.fase === 'andamento') a = aviso('andamento')
  else if (bloqueio && bloqueio !== 'enviando') a = aviso(bloqueio)
  else if (recusa) a = aviso(recusa)
  else if (resultado) a = avisoResultado(idioma, resultado, leitura.size)
  else if (leitura.automaticPaused) a = aviso('pausado')

  return {
    pct,
    alto: cheio(pct),
    antesPct: medido ? percentual({ used: medido.antes, size: leitura.size }) : null,
    marca,
    pronto: !!medido,
    faixa,
    detalhe,
    botao: emAndamento ? 'comprimindo' : recusa === 'falha' ? 'tentar' : 'comprimir',
    desligado: !!bloqueio || (!!recusa && RECUSA_DESLIGA.includes(recusa)),
    aviso: a,
  }
}

/** "Resumida hoje às 12:39" ou, de outro dia, "Resumida em 12 de nov. às 12:39"; nunca resumida: `nunca`. */
export function resumidaEm(idioma: AppLanguage, lc: LeituraContexto['lastCompaction'], agora: number): { id: 'hoje' | 'em' | 'nunca'; vars?: Record<string, string> } {
  if (!lc) return { id: 'nunca' }
  const dia = (ms: number) => new Date(ms).toDateString()
  if (dia(lc.at) === dia(agora)) return { id: 'hoje', vars: { hora: hora(idioma, lc.at) } }
  return { id: 'em', vars: { data: formatarData(idioma, lc.at, { day: 'numeric', month: 'short' }), hora: hora(idioma, lc.at) } }
}

// --- "Compactando…" no chat (desenho aprovado pelo dono, 16/11, opção A): o envio espera, o rascunho fica. ---

/** O que a marca no fluxo do chat mostra. `andamento` é a única fase que bloqueia o envio. */
export interface Compactacao {
  fase: 'andamento' | 'feito' | 'feitoSemMedida' | 'incerto' | 'falha'
  /** Só em `feito`, quando o Hub já mediu o contexto novo. */
  antes?: number
  depois?: number
}

/**
 * Função pura. Bloqueia o envio (`andamento`) enquanto o bot compacta (atividade "resumindo" ou Comprimir agora aceito e
 * sem desfecho). Falha, "não deu para confirmar" e limite de espera NUNCA bloqueiam: a saída é sempre livre. O resultado
 * "feito" e a falha vêm só do pedido manual (a compactação automática não deixa marca depois de terminar).
 */
export function compactacaoDaConversa({ atividade, emAndamento, resultado, recusa }: {
  atividade: 'ativa' | 'expirou' | null
  emAndamento: boolean
  resultado: Resultado | null
  recusa: Recusa | null
}): Compactacao | null {
  if (atividade === 'ativa' || emAndamento) return { fase: 'andamento' }
  if (atividade === 'expirou' || resultado?.fase === 'incerto') return { fase: 'incerto' }
  if (resultado?.fase === 'feito') return resultado.depois === null ? { fase: 'feitoSemMedida' } : { fase: 'feito', antes: resultado.antes, depois: resultado.depois }
  if (recusa === 'falha') return { fase: 'falha' }
  return null
}

/** Compactando: o campo fica só para leitura, com o texto guardado, e Enviar desligado. */
export const bloqueiaEnvioCompactando = (c: Compactacao | null) => c?.fase === 'andamento'

/** A medida da marca "de 60 mil para 22 mil"; sem as duas medidas, null (nunca inventa número). */
export const medidaDaMarca = (idioma: AppLanguage, c: Compactacao): { antes: string; depois: string } | null =>
  c.fase === 'feito' && c.antes !== undefined && c.depois !== undefined ? { antes: tokensCurtos(idioma, c.antes), depois: tokensCurtos(idioma, c.depois) } : null
