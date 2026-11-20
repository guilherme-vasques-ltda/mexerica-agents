import type { Engine, ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import type { SetupPhase } from '../../../../shared/fork-engine-setup.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { localeDe, traduzir, traduzirPlural } from '../../../lib/fork-i18n/index.ts'
import { chaveDeErro, chaveDeFalha, textosEm } from './textos.ts'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'

/** `traduzir` com uma chave sem parâmetros vinda de um mapa (o tipo genérico pede os parâmetros de cada uma). */
const frase = (idioma: AppLanguage, chave: ChaveTexto) => (traduzir as (i: AppLanguage, c: ChaveTexto) => string)(idioma, chave)

// D164: regras puras da tela de contas (sem I/O, sem React). O Hub decide; a UI só não confia a mais.

export const ENGINES: readonly Engine[] = ['codex', 'claude', 'grok']

export const isEngine = (value: string | null | undefined): value is Engine => value === 'codex' || value === 'claude' || value === 'grok'

/** O programa oficial que o Hub prepara para cada motor. */
// textos-ok: nome próprio do programa, igual em todo idioma.
export const engineName = (engine: Engine) => engine === 'codex' ? 'Codex' : engine === 'grok' ? 'Grok Build' : 'Claude Code'
export const engineCompany = (engine: Engine) => engine === 'codex' ? 'OpenAI' : engine === 'grok' ? 'xAI' : 'Anthropic'
/** Logo do registro ACP já empacotado no app (sem baixar imagem de fora). */
export const engineRegistry = (engine: Engine) => engine === 'codex' ? 'codex-acp' : engine === 'grok' ? 'grok-build' : 'claude-acp'

/**
 * Domínios oficiais das páginas de login. O Hub confere HTTPS e domínio exato antes de mandar o
 * desafio; a UI repete a conferência e, fora desta lista, não abre nada (falha segura).
 */
export const PROVIDER_DOMAINS: Readonly<Record<Engine, readonly string[]>> = {
  codex: ['auth.openai.com'],
  claude: ['claude.ai', 'platform.claude.com', 'console.anthropic.com'],
  grok: ['auth.x.ai', 'accounts.x.ai'],
}

/** Páginas oficiais onde se cria uma chave de API (link do formulário de chave). */
export const KEY_DOMAINS: Readonly<Record<Engine, readonly string[]>> = {
  codex: ['platform.openai.com'],
  claude: ['console.anthropic.com', 'platform.claude.com'],
  grok: [],
}

/** O endereço normalizado se puder abrir: https, sem usuário/senha/porta, host idêntico ao exibido e na lista. */
export function providerUrl(engine: Engine, raw: unknown, displayDomain?: string, domains: Readonly<Record<Engine, readonly string[]>> = PROVIDER_DOMAINS): string | null {
  if (typeof raw !== 'string' || raw.length > 2048 || /[\s\u0000-\u001f\u007f-\u009f]/.test(raw)) return null
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    return null
  }
  if (url.protocol !== 'https:' || url.username || url.password || url.port || url.hash) return null
  const host = url.hostname.toLowerCase()
  if (!domains[engine].includes(host)) return null
  if (engine === 'grok' && [...url.searchParams.keys()].some((key) => key !== 'user_code')) return null
  if (displayDomain !== undefined && host !== displayDomain.trim().toLowerCase()) return null
  return url.href
}

const ESC = 0x1b
/** CSI que só desenham: cores, cursor, apagar/inserir. Nada que faça o terminal responder ou capturar mouse/foco. */
const SAFE_CSI = new Set(['m', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'f', 'd', 'J', 'K', 'X', 'P', '@', 'L', 'M', 'S', 'T'])
/** Introdutores de sequência de texto (OSC, DCS, SOS, PM, APC), em 7 bits depois de ESC. */
const STRING_7BIT = new Set([']', 'P', 'X', '^', '_'])
/** Os mesmos em 8 bits (C1). */
const STRING_C1 = new Set([0x9d, 0x90, 0x98, 0x9e, 0x9f])

function safeCsi(body: string, final: string) {
  if (/[ -/]/.test(body)) return false
  if (/[<=>?]/.test(body)) return (final === 'h' || final === 'l') && body === '?25'
  return SAFE_CSI.has(final)
}

/** Fim de uma sequência de texto: depois de BEL, ST (ESC \\) ou ST de 8 bits; outro ESC encerra antes dele. -1 se ainda não chegou. */
function stringEnd(s: string, from: number) {
  for (let j = from; j < s.length; j++) {
    const c = s.charCodeAt(j)
    if (c === 0x07 || c === 0x9c) return j + 1
    if (c === ESC) {
      if (j + 1 >= s.length) return -1
      return s[j + 1] === '\\' ? j + 2 : j
    }
  }
  return -1
}

/**
 * Filtro da saída do terminal de setup, com estado entre pedaços (uma sequência pode chegar
 * partida). Mantém texto, quebras e cores. Descarta OSC (títulos, links OSC 8, área de
 * transferência OSC 52), DCS/APC/PM/SOS, C1, sinos e pedidos que fariam o xterm responder
 * (posição do cursor, atributos, mouse, foco, colagem). Complementa o filtro do Hub.
 */
export function createTerminalFilter(limit = 4096) {
  let pending = ''
  let skipping = false
  return {
    push(chunk: string): string {
      const s = pending + chunk
      pending = ''
      let out = ''
      let i = 0
      if (skipping) {
        const end = stringEnd(s, 0)
        if (end === -1) {
          if (s.charCodeAt(s.length - 1) === ESC) pending = '\u001b'
          return ''
        }
        skipping = false
        i = end
      }
      // Sequência de texto ainda aberta: guarda até o limite; depois descarta até o terminador.
      const hold = (start: number, end: number) => {
        if (end !== -1) return end
        if (s.length - start > limit) {
          skipping = true
          if (s.charCodeAt(s.length - 1) === ESC) pending = '\u001b'
        } else {
          pending = s.slice(start)
        }
        return s.length
      }
      while (i < s.length) {
        const c = s.charCodeAt(i)
        if (c === ESC) {
          if (i + 1 >= s.length) {
            pending = s.slice(i)
            break
          }
          const next = s[i + 1]
          if (next === '[') {
            let j = i + 2
            while (j < s.length && s.charCodeAt(j) >= 0x20 && s.charCodeAt(j) <= 0x3f) j++
            if (j >= s.length) {
              if (s.length - i <= 64) pending = s.slice(i)
              break
            }
            const f = s.charCodeAt(j)
            if (f >= 0x40 && f <= 0x7e) {
              if (safeCsi(s.slice(i + 2, j), s[j])) out += s.slice(i, j + 1)
              i = j + 1
            } else {
              i = j
            }
          } else if (STRING_7BIT.has(next)) {
            const end = hold(i, stringEnd(s, i + 2))
            if (end === s.length && (pending || skipping)) break
            i = end
          } else if ('()*+-./'.includes(next)) {
            if (i + 2 >= s.length) {
              pending = s.slice(i)
              break
            }
            i += 3
          } else if (s.charCodeAt(i + 1) < 0x20) {
            // ESC seguido de controle (outro ESC, inclusive): só o primeiro sai.
            i += 1
          } else {
            if ('78DEM'.includes(next)) out += s.slice(i, i + 2)
            i += 2
          }
          continue
        }
        if (c >= 0x80 && c <= 0x9f) {
          if (STRING_C1.has(c)) {
            const end = hold(i, stringEnd(s, i + 1))
            if (end === s.length && (pending || skipping)) break
            i = end
          } else if (c === 0x9b) {
            let j = i + 1
            while (j < s.length && s.charCodeAt(j) >= 0x20 && s.charCodeAt(j) <= 0x3f) j++
            if (j >= s.length) {
              if (s.length - i <= 64) pending = s.slice(i)
              break
            }
            i = s.charCodeAt(j) >= 0x40 && s.charCodeAt(j) <= 0x7e ? j + 1 : j
          } else {
            i++
          }
          continue
        }
        if (c < 0x20 || c === 0x7f) {
          if (c === 0x0a || c === 0x0d || c === 0x09 || c === 0x08) out += s[i]
          i++
          continue
        }
        out += s[i]
        i++
      }
      return out
    },
    reset() {
      pending = ''
      skipping = false
    },
  }
}

export type TerminalFilter = ReturnType<typeof createTerminalFilter>

/** Base64 de texto em UTF-8, para `terminal_input`. */
export function utf8ToBase64(text: string) {
  const bytes = new TextEncoder().encode(text)
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary)
}

/** `código: mensagem` do Hub → texto para a tela. Sem código conhecido, a própria mensagem do Hub. */
const rawMessage = (error: unknown) => typeof error === 'object' && error !== null && 'message' in error ? String(error.message) : String(error ?? '')
const CODE = /^(?:Error: )?([a-z][a-z0-9_]{2,40}): /

export function errorCode(error: unknown): string | null {
  return CODE.exec(rawMessage(error))?.[1] ?? null
}

/** DON-917: guarde o erro cru (estado é dado) e traduza ao desenhar, no idioma de agora. */
export function errorMessage(error: unknown, idioma: AppLanguage): string {
  if (typeof error === 'object' && error !== null && 'status' in error && error.status === 0) return traduzir(idioma, 'contas.foraDeAlcance')
  const chave = chaveDeErro(errorCode(error))
  if (chave) return frase(idioma, chave)
  const text = rawMessage(error).replace(CODE, '').trim()
  return text && text.length <= 240 ? text : traduzir(idioma, 'contas.naoConcluiu')
}

/** Mensagem da fase final sem sucesso, pelo `errorCode` da operação. */
export function failureMessage(code: string | null, idioma: AppLanguage): string {
  const chave = chaveDeFalha(code)
  return chave ? frase(idioma, chave) : traduzir(idioma, 'contas.faseFalhou')
}

/** Fases em que nada mais acontece na operação. */
export const isFinal = (phase: SetupPhase) => phase === 'connected' || phase === 'cancelled' || phase === 'expired' || phase === 'failed'

/** A etapa visível (0 Preparando … 3 Conectado) de uma fase ativa. */
export function stepOf(phase: SetupPhase): number | null {
  switch (phase) {
    case 'preparing': return 0
    case 'awaiting_authorization': return 1
    case 'verifying': return 2
    case 'connected': return 3
    default: return null
  }
}

/** `expiresAt` do Hub em milissegundos (aceita segundos de Hub antigo). */
export const expiresAtMs = (value: number) => (value > 0 && value < 1e12 ? value * 1000 : value)

export const clockText = (ms: number, idioma: AppLanguage) => new Date(ms).toLocaleTimeString(localeDe(idioma), { hour: '2-digit', minute: '2-digit' })

/**
 * Quando a cota renova, no idioma do app e na hora do aparelho: hoje só a hora ("14:32"); nos próximos 6 dias o dia da
 * semana e a hora ("sex 14:32"); depois o dia, o mês e a hora ("24 nov 12:00"). Dia e mês vêm do `Intl`, nunca de texto fixo.
 */
export function whenText(value: number, idioma: AppLanguage, now = Date.now()) {
  const date = new Date(expiresAtMs(value))
  const clock = clockText(date.getTime(), idioma)
  const diaInicio = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
  const dias = Math.round((diaInicio(date) - diaInicio(new Date(now))) / 86_400_000)
  if (dias === 0) return clock
  const locale = localeDe(idioma)
  const dia = dias > 0 && dias <= 6
    ? date.toLocaleDateString(locale, { weekday: 'short' })
    : date.toLocaleDateString(locale, { day: 'numeric', month: 'short' })
  return `${dia.replace('.', '')} ${clock}`
}

/** Pedido estável por tentativa (clique duplo e nova tentativa reaproveitam o mesmo). */
export const newRequestId = () => `ui-${crypto.randomUUID()}`

export type AccountKind = 'cli' | 'api'

/** Sem `kind` no DTO: o tipo criado nesta sessão vale; senão, API paga indica chave. */
export const accountKind = (account: ManagedAccount, known?: AccountKind): AccountKind =>
  known ?? (account.billing === 'paid_api' ? 'api' : 'cli')

export type AccountAction = 'connect' | 'reconnect' | 'key' | 'refresh'

/** Nome curto do estado de uma conta. */
export const stateLabel = (state: ManagedAccount['state'], idioma: AppLanguage) => {
  const textos = textosEm(idioma)
  return state === 'connected' ? textos.conectada : state === 'unverified' ? textos.naoConfirmada : state === 'unavailable' ? textos.indisponivel : textos.desconectada
}

export interface AccountView {
  title: string
  state: string
  tone: 'added' | 'secondary'
  attention: boolean
  detail: string
  billing: string
  primary: { action: AccountAction; label: string } | null
  managed: boolean
}

/** Nome da conta como Contas de IA mostra; os seletores do Bot de fundo e da Memória usam o mesmo (com o prefixo traduzido). */
export function accountTitle(account: { origin?: string; label: string; backend: string }, legacy: string): string {
  return account.origin === 'legacy' ? `${legacy} · ${engineName(account.backend as Engine)}` : account.label
}

export function accountView(account: ManagedAccount, kind: AccountKind, idioma: AppLanguage): AccountView {
  const textos = textosEm(idioma)
  const managed = account.origin === 'managed'
  // Cobrança só com a conta conectada (desconectada não prova assinatura); chave de API sempre se identifica.
  const connected = account.state === 'connected'
  const billing = account.billing === 'paid_api' ? (connected ? textos.apiPaga : textos.porChave)
    : !connected ? '' : account.billing === 'subscription' ? (account.backend === 'grok' ? traduzir(idioma, 'contas.planoXai') : textos.assinatura) : textos.cobrancaNaoConfirmada
  const title = accountTitle(account, textos.perfilAntigo)
  const login = (action: 'connect' | 'reconnect') => kind === 'api'
    ? { action: 'key' as const, label: action === 'connect' ? textos.informarChave : textos.trocarChave }
    : { action, label: action === 'connect' ? textos.conectar : textos.reconectar }
  switch (account.state) {
    case 'connected':
      return { title, state: textos.conectada, tone: 'added', attention: false, detail: textos.detalheConectada, billing, primary: null, managed }
    case 'unverified':
      // Conta gerenciada nova nasce não verificada e só é sondada depois do primeiro login.
      if (managed && account.billing === 'unverified') {
        return { title, state: textos.semLogin, tone: 'secondary', attention: true, detail: kind === 'api' ? textos.detalheSemChave : textos.detalheSemLogin, billing, primary: login('connect'), managed }
      }
      return { title, state: textos.naoConfirmada, tone: 'secondary', attention: true, detail: textos.detalheNaoConfirmada, billing, primary: managed ? login('reconnect') : null, managed }
    case 'unavailable':
      return { title, state: textos.indisponivel, tone: 'secondary', attention: true, detail: textos.detalheIndisponivel, billing, primary: { action: 'refresh', label: textos.tentarDeNovo }, managed }
    case 'disconnected':
      return { title, state: textos.desconectada, tone: 'secondary', attention: true, detail: textos.detalheDesconectada, billing, primary: managed ? login('connect') : null, managed }
  }
}

export interface RowActions {
  /** Verificar a conta no Hub, sem mensagem e sem custo. */
  test: boolean
  /** Entrar de novo. `new` = perfil antigo: o Hub não o reconecta, então abre uma conta nova do mesmo motor. */
  connect: { action: AccountAction | 'new'; label: string } | null
  /** `free` = pode excluir com confirmação; `in-use` = bots usam a conta (o Hub recusa; a tela explica); `null` = não se aplica. */
  remove: 'free' | 'in-use' | null
}

/**
 * Ações sempre à vista em cada linha de Contas de IA, pelo estado (o Hub continua decidindo e recusando).
 * Perfil antigo conectado é o login do computador: só testa. Desconectado, pode reconectar (conta nova) e sair da lista.
 */
export function rowActions(account: ManagedAccount, kind: AccountKind, idioma: AppLanguage): RowActions {
  const textos = textosEm(idioma)
  const managed = account.origin === 'managed'
  const noLogin = managed && account.state === 'unverified' && account.billing === 'unverified'
  const test = account.state === 'connected' || account.state === 'unavailable' || (account.state === 'unverified' && !noLogin)
  let connect: RowActions['connect'] = null
  if (managed && account.state === 'disconnected') {
    connect = kind === 'api' ? { action: 'key', label: textos.trocarChave } : { action: 'connect', label: textos.reconectar }
  } else if (noLogin) {
    connect = kind === 'api' ? { action: 'key', label: textos.informarChave } : { action: 'connect', label: textos.conectar }
  } else if (managed && account.state === 'unverified') {
    connect = kind === 'api' ? { action: 'key', label: textos.trocarChave } : { action: 'reconnect', label: textos.reconectar }
  } else if (managed && account.state === 'unavailable') {
    // Conta com problema: além de testar de novo, dá para entrar de novo.
    connect = kind === 'api' ? { action: 'key', label: textos.trocarChave } : { action: 'reconnect', label: textos.reconectar }
  } else if (!managed && account.state === 'disconnected') {
    connect = { action: 'new', label: textos.reconectar }
  }
  const canRemove = managed || account.state === 'disconnected'
  return { test, connect, remove: canRemove ? (account.botIds.length ? 'in-use' : 'free') : null }
}

/** Resultado do Testar, para a linha: a conta que o Hub devolveu depois de verificar. */
export function testResult(account: ManagedAccount, idioma: AppLanguage): { ok: boolean; text: string } {
  const textos = textosEm(idioma)
  if (account.state === 'connected') return { ok: true, text: textos.testeOk }
  const view = accountView(account, accountKind(account), idioma)
  return { ok: false, text: textos.testeFalhou(view.detail) }
}

/**
 * "Berga, Byte e mais 2", só com bots que o app conhece; IDs desconhecidos contam no "mais". A lista sai do
 * `Intl.ListFormat` do idioma, e cada nome vai isolado (FSI…PDI) com a direção dele.
 */
export function botNames(ids: readonly string[], name: (id: string) => string | null, idioma: AppLanguage, max = 3): string {
  if (!ids.length) return ''
  const shown = ids.map(name).filter((n): n is string => !!n).slice(0, max).map((n) => `\u2068${n}\u2069`)
  const rest = ids.length - shown.length
  if (!shown.length) return traduzirPlural(idioma, 'contas.nBots', rest)
  const lista = new Intl.ListFormat(localeDe(idioma), { type: 'conjunction' })
  return lista.format(rest > 0 ? [...shown, traduzirPlural(idioma, 'contas.maisBots', rest)] : shown)
}

/** Rótulo aceito pelo Hub: 1 a 60 caracteres, sem controles. */
export const validLabel = (label: string) => {
  const trimmed = label.trim()
  return trimmed.length > 0 && [...trimmed].length <= 60 && !/[\u0000-\u001f\u007f-\u009f]/.test(trimmed)
}

/** Link de "onde pegar a chave" do formulário: só páginas oficiais de chave. */
export const keyPageUrl = (engine: Engine, raw: unknown) => providerUrl(engine, raw, undefined, KEY_DOMAINS)
