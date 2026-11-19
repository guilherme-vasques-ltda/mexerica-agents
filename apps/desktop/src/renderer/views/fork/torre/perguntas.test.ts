import { test } from 'node:test'
import assert from 'node:assert/strict'
import { originDraft, originReport } from '../../../../shared/guard-hub.ts'
import { parseTowerSnapshot, groupOf, questionsOf } from '../../../../shared/fork-torre.ts'
import { answerOptions, answerView, questionView, responderOf, type ResponderStore } from './responder.ts'
import type { Entry } from '../../../../shared/models.ts'

// MB-1b (D70): a pergunta do bot no Board. Hub antigo sem o campo = indisponível; pergunta coloca o bot em
// "Esperam você"; responder pelo Board é o MESMO send do composer, com a mesma origem humana, na conversa da pergunta.
const bot = (extra: Record<string, unknown>) => ({ botId: 'fin', name: 'Fin', runtime: { availability: 'unavailable' }, pending: { availability: 'unavailable' }, queue: { availability: 'unavailable' }, usage: {}, ...extra })
const snap = (b: unknown) => parseTowerSnapshot({ version: 1, hubId: 'h', snapshotId: 's', collectedAt: 1, spaceId: 'p', bots: [b] }, { hubId: 'h', spaceId: 'p' })
const question = { entryId: 'q1', conversation: { conversationId: 'fin', revision: 3 }, text: 'Publico agora?', askedAt: 5 }

test('Hub antigo sem questions: indisponível, não lista vazia; com pergunta, Esperam você', () => {
  const old = snap(bot({})).bots[0]!
  assert.equal(questionsOf(old), null)
  const asking = snap(bot({ questions: { availability: 'available', source: 'hub.runtime.pendingQuestions', coverage: 'x', value: [question] } })).bots[0]!
  assert.equal(questionsOf(asking)?.[0]?.text, 'Publico agora?')
  assert.equal(questionsOf(asking)?.[0]?.sessionId, null)
  assert.equal(groupOf(asking), 'waitingYou')
  // Item fora do contrato derruba a observação inteira (nada de lista parcial).
  const broken = snap(bot({ questions: { availability: 'available', source: 's', coverage: 'c', value: [{ ...question, text: 3 }] } })).bots[0]!
  assert.equal(questionsOf(broken), null)
})

/** Como o BotStore: o send grava a entrada local `sending` com o nonce; a falha a marca `failed`. */
function fakeStore() {
  const entries: Entry[] = []
  const sent: [string, string, { nonce: string }][] = []
  const listeners = new Set<() => void>()
  const store: ResponderStore = {
    allEntries: () => entries, respond: () => {}, answering: new Map(), hostName: 'Servidor',
    send: (text, chat, opts) => {
      sent.push([text, chat, opts])
      entries.push({ id: `local-${opts.nonce}`, botId: chat, kind: 'user', data: { text, status: 'sending', clientNonce: opts.nonce } } as unknown as Entry)
    },
    discard: (entry) => { entries.splice(entries.indexOf(entry), 1) },
    subscribe: (l) => { listeners.add(l); return () => listeners.delete(l) },
  }
  const mark = (nonce: string, status: string) => { entries.find((e) => e.data.clientNonce === nonce)!.data.status = status }
  return { store, entries, sent, mark }
}

test('respondida pelo Board: o mesmo send e a mesma origem do composer, na conversa da pergunta', () => {
  const f = fakeStore()
  const r = responderOf(f.store, () => 'aparelho')
  const origin = originDraft('Espera sexta.', 'typed')
  assert.equal(r.answer('fin', '   ', origin), null, 'vazio não envia')
  const nonce = r.answer(question.conversation.conversationId, 'Espera sexta.', origin)!
  assert.equal(f.sent.length, 1)
  const [text, chat, opts] = f.sent[0]!
  assert.equal(text, 'Espera sexta.')
  assert.equal(chat, 'fin')
  assert.deepEqual(opts, { ...answerOptions('Espera sexta.', origin, 'aparelho'), nonce })
  const report = (opts as unknown as { contentOrigin: ReturnType<typeof originReport> }).contentOrigin
  assert.equal(report.channel, 'desktopComposer')
  assert.equal(report.interaction, 'send')
  assert.deepEqual(report.segments, [{ start: 0, end: 13, kind: 'typed' }])
  assert.equal(r.sendState('fin', nonce), 'sending')
  assert.equal(answerView('sending').editable, false)
  f.mark(nonce, 'queued') // o Hub gravou: registrado, não "respondida" até a pergunta sair da lista
  assert.equal(r.sendState('fin', nonce), 'accepted')
})

test('B1: falha de envio devolve o campo com o texto e o aviso; tentar de novo usa o MESMO nonce', () => {
  const f = fakeStore()
  const r = responderOf(f.store, () => 'aparelho')
  const origin = originDraft('Reserva o do centro.', 'typed')
  const nonce = r.answer('rota', 'Reserva o do centro.', origin)!
  f.mark(nonce, 'failed')
  assert.equal(r.sendState('rota', nonce), 'failed')
  const view = answerView(r.sendState('rota', nonce))
  assert.equal(view.editable, true, 'campo liberado; o componente mantém o texto digitado')
  assert.equal(view.alert, 'torre.resp.naoEnviada', 'o mesmo aviso do chat')
  assert.equal(view.button, 'torre.tentarDeNovo')
  assert.equal(r.answer('rota', 'Reserva o do centro.', origin, nonce), nonce)
  assert.equal(f.entries.length, 1, 'a entrada falha sai antes do reenvio: uma mensagem só')
  assert.equal(f.sent[1]![2].nonce, nonce)
  assert.equal(r.sendState('rota', nonce), 'sending')
})

test('fora do Pessoal ou sem conexão, só Abrir conversa', () => {
  assert.equal(questionView({ live: true, personal: true }).kind, 'answer')
  assert.equal(questionView({ live: false, personal: true }).kind, 'openOnly')
  assert.equal(questionView({ live: true, personal: false }).kind, 'openOnly')
})
