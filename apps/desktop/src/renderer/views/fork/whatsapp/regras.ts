import type { ChaveTexto, Tradutor } from '../../../lib/fork-i18n/index.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'
// whatsapp R5 (D297): o que a tela mostra a partir da leitura do Hub (`donnaWhatsapp`, R1) e as regras que
// ela repete para não oferecer o que o Hub recusaria. Quem decide continua sendo o Hub
// (host/src/fork/whatsapp/store.rs); aqui é só leitura, texto e limite de controle. Sem React nem I/O.

export type Modo = 'donna_only' | 'per_bot'
/** MX-18: "peço antes" (cartão) ou "sozinho" dentro dos limites, só em conversa marcada. */
export type ModoResposta = 'approval' | 'autonomous'

export interface AjustesBot {
  readChats: boolean
  replyChats: boolean
  readGroups: boolean
  replyGroups: boolean
  replyMode: ModoResposta
  delaySeconds: number
  hourlyLimit: number
  /** MX-18: envios por dia (janela móvel de 24 h). Ausente em Hub antigo = 100. */
  dailyLimit?: number
  /** MX-18: silêncio por número; segurado até a abertura. Ausente em Hub antigo = ligado. */
  quietEnabled?: boolean
  /** Minutos do dia, 0–1439, no fuso do dono (ou `quietTimeZone`). */
  quietStart?: number
  quietEnd?: number
  quietTimeZone?: string | null
  transcribeAudio: boolean
  /** R8: por quantos dias a caixa deste bot guarda as mensagens (7, 30 ou 90). Ausente em Hub antigo = 30. */
  retentionDays?: number
}

export type PatchBot = Partial<AjustesBot>

export interface BotWhatsapp {
  botId: string
  isDonna: boolean
  paused: boolean
  settings: AjustesBot
  number: Numero
  botCan: { readChats: boolean; replyChats: boolean; readGroups: boolean; replyGroups: boolean }
  sending: { state: string }
  /** Não lidas da caixa do bot (R3); ausente em Hub antigo. */
  inbox?: { unread: number }
}

export interface Transcricao {
  provider: string | null
  model: string | null
  source: 'chosen' | 'default' | 'none'
  keyOnHub: boolean
}

export interface LeituraWhatsapp {
  /** `wacli` desde o R2; ausente num Hub do R1. */
  plugin: { enabled: boolean; mode: Modo; wacli?: Wacli }
  transcription: Transcricao
  bots: BotWhatsapp[]
}

/** As regras da conexão (R4b, D357): o Hub recusa fora disso. */
export const ESPERA = { min: 8, max: 300 } as const
export const LIMITE = { min: 1, max: 200 } as const
/** MX-18: o limite por dia (padrão do dono: 100) e o silêncio (22h–8h), ajustáveis. */
export const LIMITE_DIA = { min: 1, max: 500 } as const
export const LIMITE_DIA_PADRAO = 100
export const SILENCIO_PADRAO = { start: 22 * 60, end: 8 * 60 } as const
export const limiteDiaDe = (s: AjustesBot) => s.dailyLimit ?? LIMITE_DIA_PADRAO
export const silencioDe = (s: AjustesBot) => ({
  on: s.quietEnabled ?? true,
  start: s.quietStart ?? SILENCIO_PADRAO.start,
  end: s.quietEnd ?? SILENCIO_PADRAO.end,
})
/** Minutos do dia → "22:00"; minutos → "08:00". */
export const horaDeMinutos = (min: number) => `${String(Math.floor(min / 60) % 24).padStart(2, '0')}:${String(min % 60).padStart(2, '0')}`
/** "22:00" → 1320; inválido → `null`. */
export const minutosDeHora = (texto: string): number | null => {
  const m = /^(\d{1,2}):(\d{2})$/.exec(texto.trim())
  if (!m) return null
  const h = Number(m[1]), min = Number(m[2])
  return h <= 23 && min <= 59 ? h * 60 + min : null
}

// Fatia 6 (D454): os textos saem do catálogo (whatsapp/estados) no idioma do app; aqui só chaves.
export const AVISO_FIXO = 'whatsapp.estados.avisoFixo' satisfies ChaveTexto
export const AVISO_14_DIAS = 'whatsapp.estados.aviso14' satisfies ChaveTexto
export const FRASE_ESTADOS = 'whatsapp.estados.fraseEstados' satisfies ChaveTexto
export const PAUSADO = 'marca.whatsapp.pausado' satisfies ChaveTexto

export const clamp = (valor: number, faixa: { min: number; max: number }) =>
  Math.min(faixa.max, Math.max(faixa.min, Math.round(Number.isFinite(valor) ? valor : faixa.min)))

/** Uma leitura com outro formato é erro, nunca uma lista vazia ou "pronto". */
export function conferirLeitura(raw: unknown): LeituraWhatsapp {
  const v = raw as LeituraWhatsapp | null
  const ok = !!v && typeof v === 'object' && !!v.plugin && typeof v.plugin.enabled === 'boolean'
    && (v.plugin.mode === 'donna_only' || v.plugin.mode === 'per_bot') && Array.isArray(v.bots)
    && v.bots.every((b) => b && typeof b.botId === 'string' && !!b.settings && !!b.botCan && !!b.number)
  if (!ok) throw new Error('resposta_invalida')
  return v
}

/** Sem cliente do Hub: a mesma mensagem nas telas e nos estados (guardada como chave, D507). */
export const SEM_CONEXAO_MSG: Mensagem = { chave: 'whatsapp.estados.erro.semConexao' }

/**
 * Erro do Hub como dado (D507): a chave do catálogo quando o app sabe o que houve; a frase do Hub, como veio, quando
 * ela vem pronta ("código: frase para leigo", R1). A tela traduz ao desenhar (`textoDaMensagem`).
 */
export function erroDoHub(error: unknown): Mensagem {
  const raw = (error instanceof Error ? error.message : typeof error === 'object' && error && 'message' in error ? String(error.message) : String(error)).replace(/^Error: /, '')
  if (/unknown method|Not available in the web demo/i.test(raw)) return { chave: 'marca.whatsapp.hubAntigo' }
  if (raw === 'resposta_invalida') return { chave: 'whatsapp.estados.erro.respostaInvalida' }
  if (/timed? ?out|timeout/i.test(raw)) return { chave: 'whatsapp.estados.erro.demorou' }
  const m = /^([a-z_]+): (.+)$/s.exec(raw)
  if (m) {
    const [, codigo, frase] = m
    if (codigo === 'not_allowed' && /editor/.test(frase!)) return { chave: 'whatsapp.estados.erro.soDono' }
    return { texto: frase!.charAt(0).toUpperCase() + frase!.slice(1) }
  }
  return { chave: 'whatsapp.estados.erro.semFalar' }
}

/** Pode mexer nas opções deste bot agora? `null` quando pode; senão, a chave do porquê. */
export function bloqueioDoBot(leitura: LeituraWhatsapp, bot: BotWhatsapp): ChaveTexto | null {
  if (!leitura.plugin.enabled) return 'whatsapp.estados.bloqueio.pluginDesligado'
  if (bot.paused) return PAUSADO
  return null
}

/** "Responder" só com "Ver" ligado: o bot não responde o que não pode ver. */
/** R8 (D302): os prazos de guarda que o dono escolhe; o padrão é 30 dias. */
export const PRAZOS = [7, 30, 90] as const
export const PRAZO_PADRAO = 30
export const prazoDe = (s: AjustesBot) => s.retentionDays ?? PRAZO_PADRAO
export const podeResponder = (s: AjustesBot, onde: 'chats' | 'groups') => (onde === 'chats' ? s.readChats : s.readGroups)

/** Desligar "Ver" desliga "Responder" junto, no mesmo pedido (o Hub faz o mesmo). */
export function patchVer(onde: 'chats' | 'groups', ligado: boolean): PatchBot {
  if (onde === 'chats') return ligado ? { readChats: true } : { readChats: false, replyChats: false }
  return ligado ? { readGroups: true } : { readGroups: false, replyGroups: false }
}

/** O que o bot pode, em uma frase inteira do catálogo (estado 3, separado de plugin, número e envio). */
export function oQuePode(c: BotWhatsapp['botCan'], sozinho = false): ChaveTexto {
  const conversas = c.readChats ? (c.replyChats ? 'r' : 'v') : '0'
  const grupos = c.readGroups ? (c.replyGroups ? 'r' : 'v') : '0'
  if (conversas === '0' && grupos === '0') return 'whatsapp.estados.pode.nada'
  // MX-18: no modo "sozinho" a frase diz que a resposta sai sem cartão, só nas marcadas.
  if (sozinho && (conversas === 'r' || grupos === 'r')) return `whatsapp.estados.podeAuto.${conversas}${grupos}` as ChaveTexto
  return `whatsapp.estados.pode.${conversas}${grupos}` as ChaveTexto
}

/**
 * O que este bot pode (estado 3). Sem número ligado ao bot, nada chega: com a opção ligada, a tela diz para
 * conectar. Vale antes e depois do R3 (#321), quando `botCan` passa a vir da concessão e fica falso sem número.
 */
export function oQueOBotPode(bot: BotWhatsapp): ChaveTexto {
  if (bot.paused) return 'whatsapp.estados.pausado'
  const s = bot.settings
  const quer = s.readChats || s.replyChats || s.readGroups || s.replyGroups
  if (quer && !numeroLigado(bot.number)) return bot.number.state === 'needs_reconnect' ? 'whatsapp.estados.conecteDeNovo' : 'whatsapp.estados.conecte'
  return oQuePode(bot.botCan, s.replyMode === 'autonomous')
}

/** O número é deste bot (conectado, conectando, caiu ou parado), mesmo sem estar de pé agora. */
const numeroLigado = (n: Numero) => n.state === 'connected' || n.state === 'connecting' || n.state === 'disconnected' || n.state === 'stopped'

/** O número (estado 2), como o Hub do R2 manda (`number.rs::view`). */
export function numeroTexto(n: Numero): ChaveTexto {
  return NUMERO_TEXTO[n.state] ?? 'whatsapp.estados.numero.desconhecido'
}

/** Cada envio (estado 4): no modo "sozinho" sai sem cartão, só nas marcadas (MX-18). */
export const envioTexto = (sozinho = false): ChaveTexto => (sozinho ? 'whatsapp.estados.envio.sozinho' : 'whatsapp.estados.envio.espera')

/** Os 4 estados como chaves do catálogo; `ligado` decide o ícone do plugin. */
export interface Estados { ligado: boolean; plugin: ChaveTexto; numero: ChaveTexto; pode: ChaveTexto; envio: ChaveTexto }

/** Os 4 estados do contrato de plugins, sempre separados. Sem bot: o resumo do Hub. */
export function quatroEstados(leitura: LeituraWhatsapp, bot?: BotWhatsapp): Estados {
  const ligado = leitura.plugin.enabled
  const plugin = ligado ? 'whatsapp.estados.ligado' : 'whatsapp.estados.desligado'
  const sozinho = bot?.settings.replyMode === 'autonomous'
  const envio = bot ? envioTexto(sozinho) : 'whatsapp.estados.envio.espera'
  if (bot) return { ligado, plugin, numero: numeroTexto(bot.number), pode: oQueOBotPode(bot), envio }
  const algum = leitura.bots.some((b) => Object.values(b.botCan).some(Boolean))
  return {
    ligado,
    plugin,
    numero: leitura.plugin.mode === 'per_bot' ? 'whatsapp.estados.resumo.porBot' : 'marca.whatsapp.soDoBerga',
    pode: algum ? 'whatsapp.estados.resumo.vejaCada' : 'whatsapp.estados.resumo.tudoDesligado',
    envio,
  }
}

export const MODO_TEXTO: Record<Modo, ChaveTexto> = {
  donna_only: 'marca.whatsapp.soBergaUsa',
  per_bot: 'whatsapp.estados.modo.porBot',
}

/** A transcrição do computador, numa linha (o nome do provedor e o modelo são técnicos; a frase, do catálogo). */
export function transcricaoTexto(t: Tradutor, tr: Transcricao): string {
  if (tr.source === 'none' || !tr.provider) return t('whatsapp.estados.transcricao.semChave')
  const provedor = tr.provider === 'openai' ? 'OpenAI' : tr.provider === 'gemini' ? 'Gemini' : tr.provider
  const modelo = tr.model ?? t('whatsapp.estados.transcricao.modeloPadrao')
  const padrao = tr.source === 'default'
  const chave = tr.keyOnHub
    ? (padrao ? 'whatsapp.estados.transcricao.comChavePadrao' : 'whatsapp.estados.transcricao.comChave')
    : (padrao ? 'whatsapp.estados.transcricao.semChavePadrao' : 'whatsapp.estados.transcricao.semChaveLinha')
  return t(chave, { provedor, modelo })
}

// MARK: número (R2)

export type EstadoNumero = 'none' | 'connecting' | 'connected' | 'disconnected' | 'stopped' | 'needs_reconnect' | 'pairing' | 'not_installed'
export type ResultadoPareamento = 'waiting' | 'connected' | 'expired' | 'refused' | 'cancelled'

export interface Pareamento {
  method: 'code' | 'qr'
  result: ResultadoPareamento
  /** Código de 8 letras ou texto do QR: só enquanto espera. Nunca vai para log nem analytics. */
  code?: string
  qr?: string
}

/** `phone` só com o número conectado e confirmado pelo wacli (leitura `donnaWhatsapp`). */
export interface Numero { state: EstadoNumero | (string & {}); pairing?: Pareamento; phone?: string }

export interface Logout { logout: 'done' | 'failed' | 'skipped'; warning?: string }

const NUMERO_TEXTO: Record<string, ChaveTexto> = {
  none: 'whatsapp.estados.numero.none',
  pairing: 'whatsapp.estados.numero.pairing',
  connecting: 'whatsapp.estados.numero.connecting',
  connected: 'whatsapp.estados.numero.connected',
  disconnected: 'whatsapp.estados.numero.disconnected',
  stopped: 'whatsapp.estados.numero.stopped',
  needs_reconnect: 'whatsapp.estados.numero.needs_reconnect',
  not_installed: 'whatsapp.estados.numero.not_installed',
}

/** A cor do ponto ao lado do estado. */
export function numeroTom(n: Numero): 'ok' | 'atencao' | 'neutro' {
  if (n.state === 'connected') return 'ok'
  if (n.state === 'disconnected' || n.state === 'needs_reconnect' || n.state === 'not_installed') return 'atencao'
  return 'neutro'
}

/** A chave da frase de ajuda embaixo do estado, quando ele pede ação (as com `{bot}` levam o nome isolado). */
export function numeroAjuda(n: Numero): ChaveTexto | null {
  switch (n.state) {
    case 'needs_reconnect': return 'whatsapp.estados.ajuda.reconectar'
    case 'disconnected': return 'whatsapp.estados.ajuda.caiu'
    case 'stopped': return 'whatsapp.estados.ajuda.parado'
    case 'not_installed': return 'whatsapp.estados.ajuda.semWacli'
    default: return null
  }
}

/** O que dá para fazer com o número agora. */
export function acoesDoNumero(n: Numero) {
  const s = n.state
  return {
    conectar: s === 'none' || s === 'needs_reconnect' || s === 'pairing',
    rotuloConectar: (s === 'needs_reconnect' ? 'whatsapp.estados.acao.conectarDeNovo' : s === 'pairing' ? 'whatsapp.estados.acao.continuar' : 'whatsapp.estados.acao.conectar') as ChaveTexto,
    desconectar: s === 'connected' || s === 'connecting' || s === 'disconnected' || s === 'stopped' || s === 'needs_reconnect',
  }
}

/** Por que "Conectar" não pode agora (chave do catálogo); `null` quando pode. */
export function bloqueioConectar(leitura: LeituraWhatsapp, bot: BotWhatsapp): ChaveTexto | null {
  const travado = bloqueioDoBot(leitura, bot)
  if (travado) return travado
  const w = leitura.plugin.wacli
  if (bot.number.state === 'not_installed' || w?.installed === false) {
    if (w?.state === 'downloading') return 'whatsapp.estados.bloqueio.baixando'
    if (w?.state === 'unsupported') return 'whatsapp.estados.bloqueio.semSuporte'
    if (w?.state === 'failed') return 'whatsapp.estados.bloqueio.falhou'
    return 'whatsapp.estados.bloqueio.semWacli'
  }
  return null
}

/** "ABCDEFGH" → "ABCD-EFGH", como o celular mostra. */
export const codigoFormatado = (codigo: string) => {
  const limpo = codigo.replace(/[^0-9A-Za-z]/g, '').toUpperCase()
  return limpo.length === 8 ? `${limpo.slice(0, 4)}-${limpo.slice(4)}` : codigo
}

/** Conferência leve do número antes de pedir o código; o Hub confere de verdade. */
export const telefoneParece = (telefone: string) => {
  const digitos = telefone.replace(/\D/g, '')
  return digitos.length >= 10 && digitos.length <= 15
}

export const RESULTADO_TEXTO: Record<Exclude<ResultadoPareamento, 'waiting'>, { titulo: ChaveTexto; detalhe: ChaveTexto }> = {
  connected: { titulo: 'whatsapp.estados.resultado.connected.titulo', detalhe: 'whatsapp.estados.resultado.connected.detalhe' },
  expired: { titulo: 'whatsapp.estados.resultado.expired.titulo', detalhe: 'whatsapp.estados.resultado.expired.detalhe' },
  refused: { titulo: 'whatsapp.estados.resultado.refused.titulo', detalhe: 'whatsapp.estados.resultado.refused.detalhe' },
  cancelled: { titulo: 'whatsapp.estados.resultado.cancelled.titulo', detalhe: 'whatsapp.estados.resultado.cancelled.detalhe' },
}

/** Quando o aviso ao WhatsApp falhou (R2 `Logout::Failed`) e o Hub não mandou a frase dele. */
export const LOGOUT_FALHOU: Mensagem = { chave: 'whatsapp.estados.logoutFalhou' }

// MARK: wacli (R7)

/** `plugin.wacli`: `installed` desde o R2; `state` e o resto desde o R7 (`binario.rs::follow`). */
export interface Wacli {
  installed: boolean
  state?: 'off' | 'downloading' | 'ready' | 'failed' | 'unsupported' | (string & {})
  version?: string
  /** Só em `failed`: "wacli_download: não foi possível baixar do GitHub." */
  reason?: string
}

export interface AvisoWacli { titulo: string; detalhe: string; tom: 'ok' | 'atencao' | 'neutro'; tentar: boolean }

/** O que a tela diz do wacli, no idioma de `t`; `null` com o plugin desligado ou num Hub sem nada a dizer. */
export function avisoWacli(t: Tradutor, w: Wacli | undefined): AvisoWacli | null {
  if (!w) return null
  switch (w.state) {
    case 'downloading': return { titulo: t('whatsapp.estados.wacli.baixando.titulo'), detalhe: t('whatsapp.estados.wacli.baixando.detalhe'), tom: 'neutro', tentar: false }
    case 'ready': return { titulo: t('whatsapp.estados.wacli.pronto.titulo'), detalhe: w.version ? t('whatsapp.estados.wacli.pronto.detalheVersao', { versao: w.version }) : t('whatsapp.estados.wacli.pronto.detalhe'), tom: 'ok', tentar: false }
    case 'failed': return { titulo: t('whatsapp.estados.wacli.falhou.titulo'), detalhe: motivo(t, w.reason), tom: 'atencao', tentar: true }
    case 'unsupported': return { titulo: t('whatsapp.estados.wacli.semSuporte.titulo'), detalhe: t('whatsapp.estados.wacli.semSuporte.detalhe'), tom: 'atencao', tentar: false }
    case 'off': return null
    default: return w.installed ? null : { titulo: t('whatsapp.estados.wacli.naoInstalado.titulo'), detalhe: t('whatsapp.estados.wacli.naoInstalado.detalhe'), tom: 'atencao', tentar: false }
  }
}

/** "wacli_download: não foi possível baixar do GitHub." → "Não foi possível baixar do GitHub." (a frase do Hub, como veio). */
function motivo(t: Tradutor, reason?: string) {
  const frase = reason?.replace(/^[a-z_]+: /, '').trim()
  return frase ? frase.charAt(0).toUpperCase() + frase.slice(1) : t('whatsapp.estados.wacli.motivoDesconhecido')
}

/** Enquanto baixa, a tela relê de tempos em tempos: o Hub não avisa quando o download termina. */
export const relerEmMs = (l: LeituraWhatsapp | null) => (l?.plugin.enabled && l.plugin.wacli?.state === 'downloading' ? 3000 : null)

/** Mac ou PC desligam e dormem; um Hub Linux costuma ser servidor. Só com o que o Hub já diz (`hello.os`). */
export const computadorPessoal = (os: string | undefined) => os === 'macos' || os === 'windows'

/** Estados em que o número está guardado neste computador. */
export const numeroNoComputador = (n: Numero) => ['connected', 'connecting', 'disconnected', 'stopped'].includes(n.state)
