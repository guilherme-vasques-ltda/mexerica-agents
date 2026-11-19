import { SpaceClient, type SpaceTransport } from '../../shared/fork-spaces-client.ts'
import type { SpaceConversation } from '../../shared/fork-spaces.ts'
import { Observable } from '../lib/observable.ts'
import type { HostClient } from '../client/host-client'
import { parseSessionActions, type SessionIntent } from './fork-sessions.ts'
import { choiceSendError, type ChoiceEntry, type ChoiceSendError } from '../views/fork/genui/choice-entry.ts'
import type { ChoiceIntent, ChoiceSubmission } from '../views/fork/genui/escolha-model.ts'

// UI generativa no chat, G1b (D273): o envio da resposta ao cartão de escolha. A view entrega a intenção
// do ChoiceCard; aqui ela vai ao Hub: numa extra UI01 (`sessionId` ≠ `botId` brutos da entrada) pela
// `sessionChoiceAnswer` (contrato v1 congelado, recibo 028596a7, R3 d883e822), só com o anúncio `choice`
// deste link/época; senão pela RPC de conversa existente (`donnaConversationChoiceAnswer`), no Space e na
// conversa da própria entrada. Este modelo guarda só o envio transitório (enviando ou erro
// com código estável): Respondida, Encerrada e Expirada vêm apenas do estado do Hub no fio. Com sucesso
// o cartão continua "Enviando…" até a entrada atualizada chegar. Nada é durável, reenviado sozinho ou
// tentado na conversa principal (o Hub não tem cartão nela).

/** O que o modelo lê do store do computador: o Hub conectado agora e o link dele. Nunca escreve. */
export interface ChoiceSource {
  readonly hello: { hostId: string } | null
  readonly connectedClient: SpaceTransport | null
  /** O Pessoal deste Hub: destino do histórico legado, que não traz `spaceId`. */
  readonly spaces?: { personalSpaceId: string } | null
  /** O catálogo de sessões (store/fork-sessions.ts): intenção `choice` do link/época atuais e sua validade. */
  readonly sessions?: SessionChoiceSource | null
}

/** O pedaço de `ChatSessionsModel` que o envio numa extra usa; nada é escrito nele. */
export interface SessionChoiceSource {
  intent(chatId: string, sessionId: string, action: 'choice'): SessionIntent | null
  current(intent: SessionIntent, client: HostClient): boolean
  of(chatId: string): readonly { id: string; state: string }[] | null
}

/** O que o Hub devolve à `sessionChoiceAnswer` (contrato v1). Só é conferido; o estado vem pelo fio. */
interface SessionChoiceResult { session?: { id?: unknown; chatId?: unknown }; question?: { questionId?: unknown }; entry?: { botId?: unknown; sessionId?: unknown }; replayed?: unknown }

interface Sent { submission: ChoiceSubmission; error: ChoiceSendError | null; hostId: string }

export class ChoiceAnswers extends Observable {
  private sent = new Map<string, Sent>()

  /** O envio em curso ou falho desta pergunta, se pertence ao Hub conectado agora. */
  submission(source: ChoiceSource, questionId: string): { submission: ChoiceSubmission; error: ChoiceSendError | null } | null {
    const s = this.sent.get(questionId)
    return s && s.hostId === source.hello?.hostId ? { submission: s.submission, error: s.error } : null
  }

  /** Envia a intenção do cartão. Um envio por pergunta de cada vez; o resultado só muda o estado transitório. */
  async answer(source: ChoiceSource, card: ChoiceEntry, intent: ChoiceIntent): Promise<void> {
    if (intent.questionId !== card.questionId) return
    const current = this.sent.get(intent.questionId)
    if (current?.submission.status === 'sending' && current.hostId === source.hello?.hostId) return
    const hostId = source.hello?.hostId ?? ''
    const client = source.connectedClient
    const spaceId = card.spaceId ?? source.spaces?.personalSpaceId ?? null
    const mine: Sent = { submission: { ...intent, status: 'sending' }, error: null, hostId }
    this.sent.set(intent.questionId, mine)
    this.changed()
    const fail = (error: ChoiceSendError) => {
      if (this.sent.get(intent.questionId) !== mine) return
      this.sent.set(intent.questionId, { ...mine, submission: { ...intent, status: 'error' }, error })
      this.changed()
    }
    if (card.sessionId && card.sessionId !== card.botId) return this.answerSession(source, card, intent, fail)
    if (!hostId || !client) return fail('link')
    if (!spaceId) return fail('unsupported')
    let space: SpaceClient | null = null
    let conversation: ReturnType<SpaceClient['conversation']> | null = null
    try {
      space = new SpaceClient(hostId, spaceId, client)
      // A revisão da conversa (CAS do Hub) e se ela é adicional: a principal (`id === botId`) não recebe RPC.
      const list = await space.conversations()
      const target = list.conversations.find((c: SpaceConversation) => c.id === card.conversationId)
      if (!target || target.id === target.botId) return fail('unsupported')
      if (target.closed) return fail('closed')
      conversation = space.conversation(card.conversationId)
      await conversation.answerChoice({ expectedRevision: target.revision, questionId: intent.questionId, revision: intent.revision, answer: intent.answer })
      // Sucesso: continua "Enviando…"; o estado do Hub no fio decide o que o cartão mostra.
    } catch (e) {
      fail(choiceSendError(e))
    } finally {
      conversation?.dispose()
      space?.dispose()
    }
  }

  /**
   * Extra UI01: a intenção `choice` (link e época) é capturada antes do primeiro await; o catálogo é relido no
   * mesmo link para o anúncio e a revisão atuais; a validade é conferida em volta da releitura, antes da RPC e
   * antes de aceitar o resultado. Sem anúncio, sem link ou fora da extra, nada é enviado (nunca cai na principal).
   */
  private async answerSession(source: ChoiceSource, card: ChoiceEntry, intent: ChoiceIntent, fail: (e: ChoiceSendError) => void): Promise<void> {
    const { botId } = card
    const sessionId = card.sessionId!
    const sessions = source.sessions
    const si = sessions?.intent(botId, sessionId, 'choice') ?? null
    if (!sessions || !si) return fail(sessions?.of(botId)?.find((s) => s.id === sessionId)?.state === 'closed' ? 'closed' : 'unsupported')
    const client = si.client
    try {
      if (!sessions.current(si, client)) return fail('link')
      const res = await client.call<{ sessions?: unknown; sessionActions?: unknown }>('sessions', { botId })
      if (!sessions.current(si, client)) return fail('link')
      if (!parseSessionActions(res.sessionActions).has('choice')) return fail('unsupported')
      const row = (Array.isArray(res.sessions) ? res.sessions : []).find((s) => (s as { id?: unknown } | null)?.id === sessionId) as
        { chatId?: unknown; principal?: unknown; state?: unknown; revision?: unknown } | undefined
      if (!row || row.chatId !== botId || row.principal !== false) return fail('scope')
      if (row.state !== 'open') return fail('closed')
      if (typeof row.revision !== 'number' || !Number.isSafeInteger(row.revision)) return fail('generic')
      if (!sessions.current(si, client)) return fail('link')
      const out = await client.call<SessionChoiceResult>('sessionChoiceAnswer', {
        botId, sessionId, expectedRevision: row.revision, questionId: intent.questionId, revision: intent.revision, answer: intent.answer,
      })
      if (!sessions.current(si, client)) return fail('link')
      // Destino: a resposta tem de ser desta extra e desta pergunta (como `misrouted` no envio de extra).
      const ok = out?.entry?.botId === botId && out.entry.sessionId === sessionId && out.session?.id === sessionId && out.session.chatId === botId
        && out.question?.questionId === intent.questionId && typeof out.replayed === 'boolean'
      if (!ok) return fail('scope')
      // Sucesso: continua "Enviando…"; o estado do Hub no fio decide o que o cartão mostra.
    } catch (e) {
      // Rejeição tardia (releitura ou RPC) depois de hello/troca de link: o erro é da época velha, não é adotado.
      fail(sessions.current(si, client) ? choiceSendError(e) : 'link')
    }
  }
}

const models = new WeakMap<object, ChoiceAnswers>()

/** Um modelo por store de computador (o envio pertence ao Hub daquele store). */
export function choiceAnswersOf(store: object): ChoiceAnswers {
  let model = models.get(store)
  if (!model) models.set(store, (model = new ChoiceAnswers()))
  return model
}
