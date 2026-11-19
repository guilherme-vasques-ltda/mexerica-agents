import { Observable } from '../../../lib/observable.ts'
import { conferirConversas, conferirPagina, juntar, type ConversaWa, type MensagemWa } from './caixa.ts'
import { EstadoWhatsapp, estadoDe, type Chamador } from './estado.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'
import { erroDoHub, SEM_CONEXAO_MSG } from './regras.ts'

// whatsapp R5c: a leitura da caixa do bot pelo app (só o dono). Uma instância por Hub, compartilhada pela aba do
// painel e pela tela grande (a mesma conversa nos dois). Erro de leitura fica na tela com "Tentar de novo"; nunca
// vira lista vazia. Uma resposta antiga não sobrescreve a atual.

interface Lista { conversas: ConversaWa[] | null; erro: Mensagem | null; carregando: boolean; geracao: number }
interface Conversa { msgs: MensagemWa[]; next: { sentAt: number; seq: number } | null; erro: Mensagem | null; carregando: boolean; antigas: boolean; geracao: number; rev?: number }

const PAGINA = 50
/** Código (nunca texto) de "sem cliente" ao apagar uma conversa; a view traduz. */
export const SEM_CONEXAO = 'sem_conexao'
const chave = (bot: string, chat: string) => `${bot}\n${chat}`

export class CaixaWhatsapp extends Observable {
  /** A leitura `donnaWhatsapp` (número, ajustes, `inbox.unread`). */
  readonly leitura: EstadoWhatsapp
  #listas = new Map<string, Lista>()
  #conversas = new Map<string, Conversa>()
  #marcando = new Set<string>()
  // O Hub avisa após o primeiro DELETE, antes de confirmar o cleanup. A linha continua
  // disponível para repetir; não repomos mensagens que o prazo do Hub já retirou.
  #semExclusaoConfirmada = new Map<string, ConversaWa>()
  readonly #cliente: () => Chamador | null

  /** `leitura`: a compartilhada do Hub (D350, `estadoDe`), a mesma de Ajustes → Plugins e das Configurações do bot. */
  constructor(cliente: () => Chamador | null, leitura = new EstadoWhatsapp(cliente)) {
    super()
    this.#cliente = cliente
    this.leitura = leitura
    this.leitura.subscribe(() => this.changed())
  }

  bot(botId: string) { return this.leitura.leitura?.bots.find((b) => b.botId === botId) ?? null }
  lista(botId: string): Lista | null { return this.#listas.get(botId) ?? null }
  conversa(botId: string, chat: string): Conversa | null { return this.#conversas.get(chave(botId, chat)) ?? null }

  async lerConversas(botId: string) {
    const antes = this.#listas.get(botId)
    const geracao = (antes?.geracao ?? 0) + 1
    const c = this.#cliente()
    if (!c) { this.#listas.set(botId, { conversas: null, erro: SEM_CONEXAO_MSG, carregando: false, geracao }); return this.changed() }
    this.#listas.set(botId, { conversas: antes?.conversas ?? null, erro: null, carregando: true, geracao })
    this.changed()
    try {
      const conversas = conferirConversas(await c.call('donnaWhatsappChats', { botId }))
      for (const [k, linha] of this.#semExclusaoConfirmada) {
        if (k === chave(botId, linha.whatsappChatId) && !conversas.some((x) => x.whatsappChatId === linha.whatsappChatId)) conversas.push(linha)
      }
      if (this.#listas.get(botId)?.geracao === geracao) this.#listas.set(botId, { conversas, erro: null, carregando: false, geracao })
    } catch (e) {
      if (this.#listas.get(botId)?.geracao === geracao) this.#listas.set(botId, { conversas: null, erro: erroDoHub(e), carregando: false, geracao })
    }
    this.changed()
  }

  /** A página mais recente (abrir, aviso do Hub); `antigas`: a página antes da mais antiga que já está aqui. */
  async lerMensagens(botId: string, chat: string, antigas = false) {
    const k = chave(botId, chat)
    const antes = this.#conversas.get(k)
    if (antigas && (!antes?.next || antes.antigas)) return
    const geracao = (antes?.geracao ?? 0) + 1
    const c = this.#cliente()
    const base = { msgs: antes?.msgs ?? [], next: antes?.next ?? null, geracao, rev: antes?.rev }
    if (!c) { this.#conversas.set(k, { ...base, erro: SEM_CONEXAO_MSG, carregando: false, antigas: false }); return this.changed() }
    this.#conversas.set(k, { ...base, erro: null, carregando: !antigas, antigas })
    this.changed()
    try {
      const body = { botId, whatsappChatId: chat, limit: PAGINA, ...(antigas ? { before: antes!.next } : {}) }
      const p = conferirPagina(await c.call('donnaWhatsappMessages', body))
      const atual = this.#conversas.get(k)
      if (atual?.geracao !== geracao) return
      // R8: o prazo do bot mudou desde o que está carregado (`retentionRev`): o Hub pode ter apagado mensagens que o
      // corte do prazo novo não alcança (alongar depois de encurtar). Nada do que estava aqui vale: troca, sem juntar.
      const trocou = p.retentionRev !== undefined && atual.rev !== undefined && p.retentionRev !== atual.rev
      if (trocou && antigas) {
        this.#conversas.delete(k)
        this.changed()
        return void this.lerMensagens(botId, chat)
      }
      // A primeira página não esquece as antigas já carregadas; o `next` continua o da mais antiga.
      const next = trocou || antigas || !antes?.msgs.length ? p.next : atual.next
      const msgs = juntar(trocou ? [] : atual.msgs, p.messages, p.keepAfter)
      this.#conversas.set(k, { msgs, next, erro: null, carregando: false, antigas: false, geracao, rev: p.retentionRev ?? atual.rev })
    } catch (e) {
      const atual = this.#conversas.get(k)
      if (atual?.geracao === geracao) this.#conversas.set(k, { ...atual, erro: erroDoHub(e), carregando: false, antigas: false })
    }
    this.changed()
  }

  /** Abrir a conversa marca como lida. O Hub avisa (`whatsappChanged`) e a lista e o ponto da aba se atualizam. */
  async marcarLida(botId: string, chat: string) {
    const k = chave(botId, chat)
    const c = this.#cliente()
    if (!c || this.#marcando.has(k)) return
    this.#marcando.add(k)
    try {
      await c.call('donnaWhatsappMarkRead', { botId, whatsappChatId: chat })
      // Sem esperar o aviso: a linha perde a bolinha já.
      const l = this.#listas.get(botId)
      if (l?.conversas) l.conversas = l.conversas.map((x) => (x.whatsappChatId === chat ? { ...x, unread: 0 } : x))
      this.changed()
    } catch {
      // Falhar ao marcar não esconde nada: a bolinha continua e a próxima abertura tenta de novo.
    } finally {
      this.#marcando.delete(k)
    }
  }

  /** MX-18: o dono marca/desmarca a conversa para o modo "sozinho" responder. */
  async marcarConversa(botId: string, chat: string, marked: boolean) {
    const c = this.#cliente()
    if (!c) throw new Error(SEM_CONEXAO)
    await c.call('donnaWhatsappChatMark', { botId, whatsappChatId: chat, marked })
    const l = this.#listas.get(botId)
    if (l?.conversas) {
      l.conversas = l.conversas.map((x) => (x.whatsappChatId === chat ? { ...x, marked, groupAuto: marked ? x.groupAuto : false } : x))
      this.changed()
    }
  }

  async alterarAutonomoGrupo(botId: string, chat: string, auto: boolean) {
    const c = this.#cliente()
    if (!c) throw new Error(SEM_CONEXAO)
    await c.call('donnaWhatsappGroupAuto', { botId, whatsappChatId: chat, auto })
    const l = this.#listas.get(botId)
    if (l?.conversas) {
      l.conversas = l.conversas.map((x) => (x.whatsappChatId === chat ? { ...x, groupAuto: auto } : x))
      this.changed()
    }
  }

  /** MX-18/D379: o dono escreve no mini WhatsApp — sai na hora, fora da fila, do silêncio e dos limites. */
  async enviarDono(botId: string, chat: string, text: string) {
    const c = this.#cliente()
    if (!c) throw new Error(SEM_CONEXAO)
    const r = (await c.call('donnaWhatsappSend', { botId, whatsappChatId: chat, text })) as { state?: string }
    if (r?.state === 'uncertain') return 'uncertain' as const
    return 'sent' as const
  }

  /** R8: `wacli: failed` não confirmou a saída. A view mostra o aviso neutro; a conversa continua na lista. */
  #wacliFicou = new Set<string>()
  wacliFicou(botId: string) { return this.#wacliFicou.has(botId) }

  /** "Apagar esta conversa deste computador". Sucesso tira da lista na hora. `failed` mantém a conversa e a leitura aberta. Erro do Hub ou `sem_conexao` volta cru. */
  async apagarConversa(botId: string, chat: string) {
    const c = this.#cliente()
    if (!c) throw new Error(SEM_CONEXAO)
    const k = chave(botId, chat)
    const linha = this.#listas.get(botId)?.conversas?.find((x) => x.whatsappChatId === chat)
    if (linha) this.#semExclusaoConfirmada.set(k, linha)
    const r = (await c.call('donnaWhatsappChatErase', { botId, whatsappChatId: chat })) as { wacli?: string }
    if (r?.wacli === 'failed') {
      this.#wacliFicou.add(botId)
      this.changed()
      return
    }
    this.#semExclusaoConfirmada.delete(k)
    this.#wacliFicou.delete(botId)
    // Nova geração: uma leitura da lista que saiu antes de apagar não traz a conversa de volta.
    const l = this.#listas.get(botId)
    if (l) this.#listas.set(botId, { ...l, conversas: l.conversas?.filter((x) => x.whatsappChatId !== chat) ?? null, carregando: false, geracao: l.geracao + 1 })
    this.#conversas.delete(chave(botId, chat))
    this.changed()
  }

  #avisoVisto: number | null = null
  /** Cada aviso do Hub uma vez só, mesmo com a aba e a tela grande montadas. A 1ª chamada (montar) só anota. */
  sincronizar(aviso: { seq: number; botId: string | null }) {
    if (aviso.seq === this.#avisoVisto) return
    const antes = this.#avisoVisto
    this.#avisoVisto = aviso.seq
    // Mais de um aviso entre duas leituras (bots diferentes): relê tudo o que está aberto.
    if (antes !== null) this.aviso(aviso.seq - antes === 1 ? aviso.botId : null)
  }

  /** O aviso `whatsappChanged` do Hub (só o ID do bot): relê a lista e as conversas abertas deste bot. */
  aviso(botId: string | null) {
    // A leitura `donnaWhatsapp` compartilhada já relê a cada aviso (useWhatsapp, partes.tsx); aqui, só a caixa.
    for (const b of this.#listas.keys()) if (!botId || b === botId) void this.lerConversas(b)
    for (const k of this.#conversas.keys()) {
      const [b, chat] = k.split('\n') as [string, string]
      if (!botId || b === botId) void this.lerMensagens(b, chat)
    }
  }
}

const porCliente = new WeakMap<object, CaixaWhatsapp>()

/** Uma caixa por loja do app (um Hub): a aba e a tela grande leem a mesma. */
export function caixaDe(dono: object, cliente: () => Chamador | null): CaixaWhatsapp {
  let c = porCliente.get(dono)
  if (!c) porCliente.set(dono, (c = new CaixaWhatsapp(cliente, estadoDe(dono, cliente))))
  return c
}
