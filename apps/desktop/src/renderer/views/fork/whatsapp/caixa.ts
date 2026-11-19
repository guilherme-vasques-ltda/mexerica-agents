// whatsapp R5c (DON-861/DON-867): a caixa do WhatsApp do bot como o app mostra, a partir das 3 funções do Hub
// (R3, #321: `donnaWhatsappChats`, `donnaWhatsappMessages`, `donnaWhatsappMarkRead`) e dos campos do #372 (fixada,
// chamada, tipo de mídia, contato salvo). Puro, sem React nem I/O, para testar sem DOM.

import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { formatarData, traduzir, type ChaveTexto, type Tradutor } from '../../../lib/fork-i18n/index.ts'
import type { BotWhatsapp } from './regras.ts'

export interface UltimaMensagem {
  senderName: string
  senderContact: boolean
  /** Texto, ou o rótulo da mídia/chamada que o Hub já escreve ("Foto", "Chamada de voz"…). */
  body: string
  media: string | null
  call: { media: string | null; durationSecs: number | null } | null
  sentAt: number
  offline: boolean
}

export interface ConversaWa {
  whatsappChatId: string
  kind: 'individual' | 'group'
  name: string | null
  phone: string | null
  pinned: boolean
  /** MX-18: o dono marcou para o modo "sozinho" responder. Ausente em Hub antigo. */
  marked?: boolean
  /** Autônomo explicitamente ligado no grupo; ausente em Hub antigo. */
  groupAuto?: boolean
  last: UltimaMensagem
  unread: number
}

/** R3b: transcrição de uma mensagem de voz (conteúdo de fora, D125). Ausente = não transcreve. */
export interface TranscricaoWa {
  state: 'pending' | 'done' | 'too_large' | 'no_key' | 'unsupported' | 'failed'
  text: string | null
  provider: 'openai' | 'gemini' | null
}

export interface MensagemWa {
  seq: number
  messageId: string
  from: { name: string; phone: string | null; contact: boolean }
  call: { media: string | null; durationSecs: number | null } | null
  sentAt: number
  body: string
  media: string | null
  offline: boolean
  read: boolean
  /** Hubs antes do R3b não mandam. */
  transcript?: TranscricaoWa | null
  /** R8: chegada ao Hub (ms); ausente em Hub antigo. */
  receivedAt?: number
}

export interface PaginaWa {
  whatsappChatId: string
  kind: 'individual' | 'group'
  name: string | null
  /** Da mais recente para a mais antiga (como o Hub manda). */
  messages: MensagemWa[]
  next: { sentAt: number; seq: number } | null
  /** R8: o que chegou até aqui já venceu pelo prazo do bot; ausente em Hub antigo. */
  keepAfter?: number
  /** R8: versão do prazo do bot; mudou = o que o app tinha carregado deixa de valer. Ausente em Hub antigo. */
  retentionRev?: number
}

export type Filtro = 'todas' | 'naoLidas' | 'privadas' | 'grupos'

/** Resposta de outro formato é erro, nunca uma lista vazia. */
export function conferirConversas(raw: unknown): ConversaWa[] {
  const v = raw as { chats?: unknown } | null
  if (!v || !Array.isArray(v.chats) || !v.chats.every((c) => c && typeof c.whatsappChatId === 'string' && c.last && typeof c.unread === 'number')) {
    throw new Error('resposta_invalida')
  }
  return v.chats as ConversaWa[]
}

export function conferirPagina(raw: unknown): PaginaWa {
  const v = raw as PaginaWa | null
  if (!v || typeof v.whatsappChatId !== 'string' || !Array.isArray(v.messages)) throw new Error('resposta_invalida')
  return v
}

const PROVEDOR: Record<string, string> = { openai: 'OpenAI', gemini: 'Gemini' }

/** R3b: a nota embaixo da voz, como chave do catálogo (`whatsapp.voz.*`); a view traduz. */
export type NotaVoz = 'transcrito' | 'transcritoProvedor' | 'semFala' | 'transcrevendo' | 'longa' | 'semChave' | 'formato' | 'falhou'

/**
 * R3b: o que a bolha de voz mostra embaixo de "Mensagem de voz": o texto transcrito e quem transcreveu, ou o estado.
 * `provedor` é o nome da marca (não se traduz). `null` = nada (não é voz, ou "Transcrever" estava desligado quando chegou).
 */
export function transcricao(m: Pick<MensagemWa, 'media' | 'transcript'>): { texto: string | null; nota: NotaVoz; provedor: string | null; erro: boolean } | null {
  const t = m.transcript
  if (m.media !== 'audio' || !t) return null
  const so = (nota: NotaVoz, erro = false) => ({ texto: null, nota, provedor: null, erro })
  switch (t.state) {
    case 'done': {
      const texto = t.text?.trim() || null
      if (!texto) return so('semFala')
      const provedor = PROVEDOR[t.provider ?? ''] ?? null
      return { texto, nota: provedor ? 'transcrito' : 'transcritoProvedor', provedor, erro: false }
    }
    case 'pending': return so('transcrevendo')
    case 'too_large': return so('longa')
    case 'no_key': return so('semChave', true)
    case 'unsupported': return so('formato')
    case 'failed': return so('falhou', true)
  }
}

/** Estados do número em que o bot tem número ligado a ele (a aba aparece; fora de "conectado", com a faixa). */
const COM_NUMERO = new Set(['connected', 'connecting', 'disconnected', 'needs_reconnect', 'stopped'])
export const temNumero = (b: BotWhatsapp | null | undefined) => !!b && COM_NUMERO.has(b.number.state)
/** D350: a aba só com o plugin ligado em Ajustes → Plugins E número ligado ao bot (desligado, o Hub diz "parado"). */
export const mostraAba = (pluginLigado: boolean, b: BotWhatsapp | null | undefined) => pluginLigado && temNumero(b)
/** "Ver conversas" e "Ver grupos" desligados: a aba abre com o aviso. */
export const verLigado = (b: BotWhatsapp) => b.settings.readChats || b.settings.readGroups

/** O que o topo da aba diz do número, e a faixa quando não está conectado. */
export interface Faixa { titulo: ChaveTexto; texto: ChaveTexto }
/** `phone`: só com o número conectado; a tela o mostra escondido (D374, NumeroBot), nunca no `texto`. Textos como chaves. */
export function estadoNumero(b: BotWhatsapp): { texto: ChaveTexto; phone: string | null; ruim: boolean; faixa: Faixa | null } {
  const phone = b.number.state === 'connected' ? b.number.phone ?? null : null
  switch (b.number.state) {
    case 'connected': return { texto: 'whatsapp.estados.numero.connected', phone, ruim: false, faixa: null }
    case 'connecting': return { texto: 'whatsapp.estados.numero.connecting', phone, ruim: false, faixa: null }
    case 'stopped': return { texto: 'whatsapp.estados.numero.stopped', phone, ruim: true, faixa: { titulo: 'whatsapp.estados.faixa.parado.titulo', texto: 'whatsapp.estados.faixa.parado.texto' } }
    default: return { texto: 'whatsapp.estados.numero.desconectado', phone, ruim: true, faixa: { titulo: 'whatsapp.estados.faixa.desconectado.titulo', texto: 'whatsapp.estados.faixa.desconectado.texto' } }
  }
}

/** "+5511900000107" → "+55 11 90000-0107"; fora do Brasil, como veio. */
export function formatarTelefone(phone: string): string {
  const d = phone.replace(/\D/g, '')
  const m = /^55(\d{2})(\d{4,5})(\d{4})$/.exec(d)
  return m ? `+55 ${m[1]} ${m[2]}-${m[3]}` : phone
}

/** Só o nome, como no WhatsApp; o número só quando não há nome. Sem os dois, "sem nome" no idioma de `t` (sem `t`, vazio). */
export function nomeDa(c: Pick<ConversaWa, 'name' | 'phone' | 'kind'>, t?: Tradutor): string {
  const nome = c.name?.trim()
  if (nome) return nome
  if (c.phone) return formatarTelefone(c.phone)
  return t ? t(c.kind === 'group' ? 'whatsapp.estados.semNome.grupo' : 'whatsapp.estados.semNome.contato') : ''
}

/** D387: forma de telefone ("+55 11 90000-0107", "5511900000107"): o que o app mostra escondido. */
export function pareceTelefone(s: string | null | undefined): s is string {
  const v = (s ?? '').trim()
  return /^\+?[\d\s().-]+$/.test(v) && v.replace(/\D/g, '').length >= 8
}

/** O número que aparece no lugar do nome da conversa (sem nome salvo), ou `null`. */
export const telefoneDaConversa = (c: Pick<ConversaWa, 'name' | 'phone'>) => (!c.name?.trim() && c.phone ? c.phone : null)

/** Remetente em grupo: "~Nome" quando não é contato salvo (como o WhatsApp). */
export const remetente = (name: string, contact: boolean) => (contact ? name : `~${name}`)

/** A prévia da linha: em grupo, "~Nome: texto"; mídia e chamada, o rótulo do Hub, sem duração de áudio. */
export function previa(c: ConversaWa): string {
  const corpo = c.last.body
  return c.kind === 'group' ? `${remetente(c.last.senderName, c.last.senderContact)}: ${corpo}` : corpo
}

/** O ícone da prévia/balão (SF Symbol; Lucide fora do Mac). */
export function iconeDa(m: { media: string | null; call: { media: string | null } | null }): string | null {
  if (m.call) return m.call.media === 'video' ? 'video' : 'phone'
  switch (m.media) {
    case 'audio': return 'mic'
    case 'image': return 'camera'
    case 'video': case 'gif': return 'video'
    case 'document': return 'doc'
    case 'sticker': return 'face.smiling'
    case 'location': case 'live_location': return 'mappin'
    default: return null
  }
}

/** Duração de chamada ("2:05"); o áudio não tem duração no wacli, então nunca "(0:14)". */
export function duracao(secs: number | null | undefined): string | null {
  if (!secs || secs <= 0) return null
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, '0')}`
}

/** Iniciais (até 2) do nome; sem nome (só número), nada: o círculo leva o ícone de pessoa. */
export function iniciais(c: Pick<ConversaWa, 'name'>): string {
  const partes = (c.name ?? '').replace(/\(.*?\)/g, ' ').trim().split(/\s+/).filter((p) => /\p{L}/u.test(p))
  if (!partes.length) return ''
  const a = partes[0]!.match(/\p{L}/u)![0]
  return a.toLocaleUpperCase('pt-BR')
}

const CORES = ['azul', 'jade', 'ambar', 'rosa', 'violeta', 'grafite'] as const
/** A cor do círculo, estável por conversa. */
export function corDa(id: string): (typeof CORES)[number] {
  let h = 0
  for (const ch of id) h = (h * 31 + ch.charCodeAt(0)) >>> 0
  return CORES[h % CORES.length]!
}

/** As contagens das pílulas: conversas com não lidas, privadas e grupos. */
export function contagens(chats: readonly ConversaWa[]) {
  return {
    naoLidas: chats.filter((c) => c.unread > 0).length,
    privadas: chats.filter((c) => c.kind === 'individual').length,
    grupos: chats.filter((c) => c.kind === 'group').length,
  }
}

const sem = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase('pt-BR')

/** Pílula e busca (nome, número ou prévia). A ordem do Hub fica: fixadas primeiro, depois a mais recente. */
export function filtrar(chats: readonly ConversaWa[], filtro: Filtro, busca: string): ConversaWa[] {
  const q = sem(busca.trim())
  const qd = busca.replace(/\D/g, '')
  return chats.filter((c) => {
    if (filtro === 'naoLidas' && c.unread <= 0) return false
    if (filtro === 'privadas' && c.kind !== 'individual') return false
    if (filtro === 'grupos' && c.kind !== 'group') return false
    if (!q) return true
    return sem(nomeDa(c)).includes(q) || sem(previa(c)).includes(q) || (qd.length >= 3 && (c.phone ?? '').replace(/\D/g, '').includes(qd))
  })
}

/** Quantos dias de calendário entre `at` e `now` (0 = hoje). */
function diasAtras(at: number, now: Date): number {
  const dia = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime()
  return Math.round((dia(now) - dia(new Date(at))) / 86_400_000)
}

/** Hora da lista como no WhatsApp, no idioma do app: hoje "10:42", ontem "ontem", na semana "ter", antes "02/11". */
export function horaLista(at: number, now = new Date(), idioma: AppLanguage = 'pt-BR'): string {
  if (!at) return ''
  const dias = diasAtras(at, now)
  if (dias <= 0) return formatarData(idioma, at, { hour: '2-digit', minute: '2-digit' })
  if (dias === 1) return traduzir(idioma, 'whatsapp.estados.hora.ontem')
  if (dias < 7) return formatarData(idioma, at, { weekday: 'short' }).replace('.', '')
  return formatarData(idioma, at, { day: '2-digit', month: '2-digit' })
}

export const horaMensagem = (at: number, idioma: AppLanguage = 'pt-BR') => formatarData(idioma, at, { hour: '2-digit', minute: '2-digit' })

/** O separador de dia dos balões, no idioma do app. */
export function diaDe(at: number, now = new Date(), idioma: AppLanguage = 'pt-BR'): string {
  const dias = diasAtras(at, now)
  if (dias <= 0) return traduzir(idioma, 'whatsapp.estados.dia.hoje')
  if (dias === 1) return traduzir(idioma, 'whatsapp.estados.dia.ontem')
  return formatarData(idioma, at, { day: 'numeric', month: 'long' })
}

export type ItemConversa =
  | { tipo: 'dia'; id: string; texto: string }
  | { tipo: 'desligado'; id: string }
  | { tipo: 'msg'; id: string; m: MensagemWa }

/** Os balões em ordem de leitura (mais antiga em cima), com o dia e o aviso "chegaram com o computador desligado". */
export function itensConversa(msgs: readonly MensagemWa[], now = new Date(), idioma: AppLanguage = 'pt-BR'): ItemConversa[] {
  const ordem = [...msgs].sort((a, b) => a.sentAt - b.sentAt || a.seq - b.seq)
  const out: ItemConversa[] = []
  let dia = ''
  let antesOffline = false
  for (const m of ordem) {
    const d = diaDe(m.sentAt, now, idioma)
    if (d !== dia) {
      dia = d
      out.push({ tipo: 'dia', id: `dia:${m.seq}`, texto: d })
      antesOffline = false
    }
    if (m.offline && !antesOffline) out.push({ tipo: 'desligado', id: `off:${m.seq}` })
    antesOffline = m.offline
    out.push({ tipo: 'msg', id: m.messageId, m })
  }
  return out
}

/** Junta páginas sem repetir (a mesma mensagem pode vir de novo depois de um aviso). */
export function juntar(atual: readonly MensagemWa[], nova: readonly MensagemWa[], keepAfter?: number): MensagemWa[] {
  const por = new Map(atual.map((m) => [m.messageId, m]))
  for (const m of nova) por.set(m.messageId, m)
  // R8: o que venceu pelo prazo (ou o prazo encurtou) sai também das páginas já carregadas.
  const vale = (m: MensagemWa) => keepAfter === undefined || m.receivedAt === undefined || m.receivedAt > keepAfter
  return [...por.values()].filter(vale).sort((a, b) => b.sentAt - a.sentAt || b.seq - a.seq)
}

/**
 * Foco ao abrir e voltar de uma conversa (nunca BODY): abrir pela linha foca o "‹" no painel e o título da conversa
 * na tela grande (lá não há "‹"); voltar foca a linha da conversa que estava aberta.
 */
export const ALVO_AO_ABRIR = { painel: '.wa-cvh .back', grande: '.wa-cvh .wa-cvh-t' } as const
/**
 * D387-SOL-01: a referência da linha no DOM é opaca (`c1`, `c2`…, só nesta sessão, em memória). O JID da conversa
 * individual tem o telefone inteiro e nunca vai para atributo.
 */
const refs = new Map<string, string>()
export const refDaConversa = (id: string) => { let r = refs.get(id); if (!r) refs.set(id, (r = `c${refs.size + 1}`)); return r }
export const seletorDaLinha = (id: string) => `.wa-cv[data-chat="${refDaConversa(id)}"]`
