// MB-1 (docs/features/mission-board.md, "Responder no Board"): o que o Board faz com cada permissão original que o
// Hub listou em `pending`. Regra pura. Responder usa a MESMA intenção da conversa (`BotStore.respond` →
// `respondPermission {botId, entryId, entryRev, optionId}`): o Hub aceita só a primeira resposta e recusa cartão
// que não está mais pendente ou com revisão antiga. Nada é respondido sem o cartão original carregado.

import type { Entry } from '../../../../shared/models.ts'
import { originDevice, originPasted, originReport, type OriginDraft } from '../../../../shared/guard-hub.ts'
import { bloqueioSenha, type ResultadoEnvio } from '../../../store/fork-senha-bloqueio.ts'
/** DON-917 (D507): as notas e rótulos são chaves do catálogo; a tela traduz ao desenhar. */
type Nota = `torre.resp.${'mesmoCartao' | 'soPessoal' | 'semCartao' | 'jaRespondido' | 'naoEspera' | 'semConexao' | 'notaPergunta' | 'enviando' | 'enviandoConversa' | 'naoEnviada' | 'enviar' | 'registrada'}` | 'torre.tentarDeNovo'

export type PendingEntry = Pick<Entry, 'id' | 'kind'> & { data: { status?: string; toolKind?: string } }

export type PendingView =
  /** O cartão original, com as próprias opções. */
  | { kind: 'card'; entry: PendingEntry; nota: Nota }
  /** Já resolvido (aqui ou na conversa): o cartão mostra o resultado, sem botões. */
  | { kind: 'settled'; entry: PendingEntry; nota: Nota }
  /** Sem como responder aqui agora: só Abrir conversa, com o motivo. */
  | { kind: 'openOnly'; nota: Nota | null }

export const SAME_CARD = 'torre.resp.mesmoCartao' satisfies Nota

export function pendingView(entry: PendingEntry | null, ctx: { live: boolean; personal: boolean }): PendingView {
  if (!ctx.personal) return { kind: 'openOnly', nota: 'torre.resp.soPessoal' }
  if (!entry || entry.kind !== 'permission') return { kind: 'openOnly', nota: 'torre.resp.semCartao' }
  const status = entry.data.status
  if (status === 'answered') return { kind: 'settled', entry, nota: 'torre.resp.jaRespondido' }
  if (status !== 'pending') return { kind: 'settled', entry, nota: 'torre.resp.naoEspera' }
  if (entry.data.toolKind === 'open_local_command') return { kind: 'openOnly', nota: null }
  if (!ctx.live) return { kind: 'openOnly', nota: 'torre.resp.semConexao' }
  return { kind: 'card', entry, nota: SAME_CARD }
}

/**
 * MB-1b: pergunta do bot ao dono. Responder é escrever na conversa dela: o Board usa o MESMO `send` do composer, com
 * o mesmo relatório de origem humana (`desktopComposer`), e o Hub marca respondidas, na mesma transação, todas as
 * perguntas pendentes daquela conversa. Fora do Pessoal ou sem conexão, só Abrir conversa.
 */
export type QuestionView = { kind: 'answer'; nota: Nota } | { kind: 'openOnly'; nota: Nota }

export const QUESTION_NOTE = 'torre.resp.notaPergunta' satisfies Nota

export function questionView(ctx: { live: boolean; personal: boolean }): QuestionView {
  if (!ctx.personal) return { kind: 'openOnly', nota: 'torre.resp.soPessoal' }
  if (!ctx.live) return { kind: 'openOnly', nota: 'torre.resp.semConexao' }
  return { kind: 'answer', nota: QUESTION_NOTE }
}

/**
 * B1: o estado do MESMO envio, pela entrada local do `send` (mesmo `clientNonce`). `accepted` quer dizer que o Hub
 * gravou a mensagem; respondida é só quando a pergunta sai de `questions`. Falha mantém o texto e libera o campo.
 */
export type SendState = 'sending' | 'failed' | 'accepted'

export function sendStateOf(entries: readonly Pick<Entry, 'data'>[], nonce: string): SendState | null {
  const entry = entries.find((e) => e.data.clientNonce === nonce)
  if (!entry) return null
  if (entry.data.status === 'sending') return 'sending'
  if (entry.data.status === 'failed') return 'failed'
  return 'accepted'
}

export interface AnswerView { editable: boolean; busy: boolean; button: Nota; nota: Nota | null; alert: Nota | null }

/** Sem envio: campo livre. Enviando: campo travado com o texto. Falhou: o mesmo aviso do chat, texto mantido. */
export function answerView(state: SendState | null): AnswerView {
  switch (state) {
    case 'sending': return { editable: false, busy: true, button: 'torre.resp.enviando', nota: 'torre.resp.enviandoConversa', alert: null }
    case 'failed': return { editable: true, busy: false, button: 'torre.tentarDeNovo', nota: null, alert: 'torre.resp.naoEnviada' }
    case 'accepted': return { editable: false, busy: false, button: 'torre.resp.enviar', nota: 'torre.resp.registrada', alert: null }
    default: return { editable: true, busy: false, button: 'torre.resp.enviar', nota: null, alert: null }
  }
}

/** Os mesmos campos que o Composer manda em `send`; texto vazio não envia nada. */
export function answerOptions(text: string, origin: OriginDraft | undefined, deviceId: string) {
  return { pasted: originPasted(text, origin), contentOrigin: originReport(text, origin, deviceId) }
}

/** O que o Board usa do store: exatamente o cartão e a intenção da conversa. */
export interface Responder {
  entry(conversationId: string, entryId: string): Entry | null
  respond(entry: Entry, option: string | null): void
  answering(entryId: string): string | null
  hostName: string
  /** MB-1b: escreve na conversa `chatId`, pelo mesmo `send` do composer. Devolve o nonce do envio (null se vazio).
   *  Com `nonce` de um envio que falhou, tira a entrada local falha e reenvia com o MESMO nonce: se a primeira
   *  tentativa chegou ao Hub, ele devolve a mesma mensagem em vez de gravar outra. */
  answer(chatId: string, text: string, origin: OriginDraft | undefined, nonce?: string): string | null
  sendState(chatId: string, nonce: string): SendState | null
  subscribe(listener: () => void): () => void
  /** MX-214: o aviso de senha colada desta conversa está aberto; responder por texto fica desligado e o motivo aparece. */
  envioBloqueado?(chatId: string): boolean
  subscribeBloqueio?(listener: () => void): () => void
}

/** A parte do BotStore que o Board usa; `respond` é a mesma função do PermissionCard da conversa e da Ilha. */
export interface ResponderStore {
  allEntries(chatId: string): Entry[]
  respond(entry: Entry, option: string | null): void
  answering: ReadonlyMap<string, string>
  hostName: string
  send(text: string, botId: string, options: ReturnType<typeof answerOptions> & { nonce: string }): ResultadoEnvio | void
  envioBloqueado?(chatId: string): boolean
  avisarEnvioBloqueado?(botId: string): void
  discard(entry: Entry): void
  subscribe(listener: () => void): () => void
}

export function responderOf(store: ResponderStore, device: () => string = originDevice): Responder {
  return {
    entry: (conversationId, entryId) => store.allEntries(conversationId).find((e) => e.id === entryId) ?? null,
    respond: (entry, option) => store.respond(entry, option),
    answering: (entryId) => store.answering.get(entryId) ?? null,
    hostName: store.hostName,
    answer: (chatId, text, origin, previous) => {
      if (!text.trim()) return null
      // MX-214: com o aviso de senha aberto, nada é descartado nem registrado: a falha, o rascunho e o nonce ficam como estão.
      if (store.envioBloqueado?.(chatId)) { store.avisarEnvioBloqueado?.(chatId); return null }
      const failed = previous ? store.allEntries(chatId).find((e) => e.data.clientNonce === previous && e.data.status === 'failed') : undefined
      if (failed) store.discard(failed)
      const nonce = failed ? previous! : crypto.randomUUID()
      if (store.send(text, chatId, { ...answerOptions(text, origin, device()), nonce }) === 'bloqueado') return null
      return nonce
    },
    sendState: (chatId, nonce) => sendStateOf(store.allEntries(chatId), nonce),
    subscribe: (listener) => store.subscribe(listener),
    envioBloqueado: (chatId) => store.envioBloqueado?.(chatId) ?? false,
    subscribeBloqueio: (listener) => bloqueioSenha.subscribe(listener),
  }
}
