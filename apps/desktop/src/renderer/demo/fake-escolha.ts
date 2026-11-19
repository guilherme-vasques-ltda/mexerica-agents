// G1b, só na demo (`?escolha`): um cartão de escolha numa extra de Byte, no formato que o Hub grava
// (`{text, genuiChoice}`, sem `data.card`), e a resposta simulada da `sessionChoiceAnswer` v1 (recibo 028596a7,
// R3 d883e822). Nada sai daqui; a autoridade real é do Hub.
import type { Entry, EntryData } from '@shared/models'

export const escolhaDemo = () => typeof location !== 'undefined' && new URLSearchParams(location.search).has('escolha')

const where = { botId: 'byte', sessionId: 'byte-guard' }
const questionId = 'demo-escolha'
const snapshot = JSON.stringify({ version: 1, component: 'choice', title: 'Invalid-date guard', summary: 'Two ways to handle a bad date.',
  data: { question: 'Which guard should I ship first?', options: [{ id: 'reject', label: 'Reject the row' }, { id: 'clamp', label: 'Clamp to today' }] } })

/** A pergunta pendente, como entrada `notice` da extra. */
export function perguntaDemo(): Pick<Entry, 'botId' | 'kind' | 'data'> & { sessionId: string } {
  return { ...where, kind: 'notice', data: { author: 'byte', text: 'Which guard should I ship first?', genuiChoice: { version: 1, questionId, revision: 1, state: 'pending', snapshot } } as EntryData }
}

/**
 * A RPC simulada: mesmos seis campos, CAS da sessão e da pergunta e replay idêntico, como o contrato. `pergunta` é a
 * entrada atual; `gravar` atualiza a projeção e acrescenta a resposta `user` na mesma extra.
 */
export function responderDemo(b: Record<string, unknown>, session: { id: string; chatId: string; state: string; revision: number } | undefined, pergunta: Entry | undefined,
  gravar: (data: EntryData, resposta: Pick<Entry, 'botId' | 'kind' | 'data'> & { sessionId: string }) => Entry, fail: (m: string) => unknown) {
  const keys = ['botId', 'sessionId', 'expectedRevision', 'questionId', 'revision', 'answer']
  if (Object.keys(b).some((k) => !keys.includes(k)) || keys.some((k) => !(k in b))) throw fail('invalid_field: pedido de resposta inválido.')
  if (!session || session.chatId !== b.botId) throw fail('unknown_session')
  if (session.state !== 'open') throw fail('session_closed')
  if (session.revision !== b.expectedRevision) throw fail('session_revision_conflict')
  const g = (pergunta?.data as { genuiChoice?: { questionId: string; revision: number; state: string; answer?: unknown } } | undefined)?.genuiChoice
  if (!pergunta || !g || g.questionId !== b.questionId || pergunta.sessionId !== b.sessionId) throw fail('genui_question_scope_mismatch')
  const replay = g.state === 'answered' && g.revision === Number(b.revision) + 1 && JSON.stringify(g.answer) === JSON.stringify(b.answer)
  if (replay) return { session, question: g, entry: pergunta, replayed: true }
  if (g.revision !== b.revision) throw fail('genui_revision_conflict')
  if (g.state !== 'pending') throw fail('genui_final_state')
  const answer = b.answer as { kind: string; id?: string; text?: string }
  const text = answer.kind === 'option' ? (answer.id === 'clamp' ? 'Clamp to today' : 'Reject the row') : answer.kind === 'text' ? String(answer.text ?? '') : 'None of these'
  const question = { ...g, revision: g.revision + 1, state: 'answered', answer, answeredAt: Date.now() }
  const entry = gravar({ ...pergunta.data, genuiChoice: question } as EntryData,
    { ...where, kind: 'user', data: { text, status: 'sent', choice_response: { version: 1, questionId, revision: g.revision } } as EntryData })
  return { session, question, entry, replayed: false }
}

export const perguntaId = 'escolha-demo'
