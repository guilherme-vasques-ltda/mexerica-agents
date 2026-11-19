// D70 (DON-907 C4): só a fronteira do fluxo alterado — qual espera o clique atinge e quais botões o estado do Hub permite.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Entry } from '@shared/models'
import { acoesDaEspera, alvoDaEspera, FORMA_INVALIDA, lerLimites } from './limites.ts'

const aviso = (wait: NonNullable<Entry['data']['subscriptionWait']>, extra: Partial<Entry> = {}): Entry =>
  ({ id: 'n1', seq: 1, botId: 'crivo', rev: 1, kind: 'notice', turn: 1, createdAt: 0, updatedAt: 0, data: { author: 'crivo', subscriptionWait: wait }, ...extra })

test('o clique leva o aviso, a espera e o destino; a principal vai sem sessão', () => {
  assert.deepEqual(alvoDaEspera(aviso({ waitId: 'w1' }, { sessionId: 'crivo' })), { botId: 'crivo', chatId: 'crivo', sessionId: null, entryId: 'n1', waitId: 'w1' })
  assert.equal(alvoDaEspera(aviso({ waitId: 'w1' }, { sessionId: 's2' }))?.sessionId, 's2')
  assert.equal(alvoDaEspera(aviso({ retry: true })), null)
})

test('os botões seguem retry e cancelled do Hub', () => {
  assert.deepEqual(acoesDaEspera(aviso({ retry: true })), [], 'Hub antigo: aviso de sempre')
  assert.deepEqual(acoesDaEspera(aviso({ waitId: 'w', retry: false })), ['naoRetomar'])
  assert.deepEqual(acoesDaEspera(aviso({ waitId: 'w', retry: true })), ['retomarAgora', 'naoRetomar'])
  assert.deepEqual(acoesDaEspera(aviso({ waitId: 'w', cancelled: true, retry: true })), ['retomarAgora'])
  assert.deepEqual(acoesDaEspera(aviso({ waitId: 'w', cancelled: true, retry: false, alreadyStarted: true })), [])
})

test('ajustes de forma estranha são recusados com código estável, sem texto', () => {
  assert.deepEqual(lerLimites({ autoResume: true, snooze: false }), { autoResume: true, snooze: false })
  assert.throws(() => lerLimites({ autoResume: 'sim', snooze: false }), { message: FORMA_INVALIDA })
})

// MX-245: a linha da conversa diz "Parou pelo limite do plano; volta às HH:MM" (hora local, idioma do app) com o `resumeAt` do Hub.
test('a parada pelo limite usa o resumeAt da espera; estados do Hub mantêm o texto dele', async () => {
  const { paradaDaEspera } = await import('./limites.ts')
  const { tradutor } = await import('../../../lib/fork-i18n/index.ts')
  const t = tradutor('pt-BR')
  const agora = Date.parse('2026-11-11T18:00:00Z') // 15:00 em São Paulo
  const SP = 'America/Sao_Paulo'
  const com = (wait: NonNullable<Entry['data']['subscriptionWait']>, style?: 'error' | 'info') => ({ ...aviso(wait), data: { author: 'crivo', text: 'texto do Hub', style, subscriptionWait: wait } }) as Entry
  assert.equal(paradaDaEspera(t, 'pt-BR', com({ waitId: 'w', resumeAt: Date.parse('2026-11-11T20:30:00Z') }), agora, SP), 'Parou pelo limite do plano; volta às 17:30')
  assert.equal(paradaDaEspera(t, 'pt-BR', com({ resumeAt: Date.parse('2026-11-12T12:00:00Z') }, 'info'), agora, SP), 'Parou pelo limite do plano; volta amanhã às 09:00', 'Hub antigo, sem waitId')
  assert.equal(paradaDaEspera(t, 'pt-BR', com({ waitId: 'w', resumeAt: null }), agora, SP), 'Parou pelo limite do plano; o provedor não informou quando volta')
  assert.equal(paradaDaEspera(t, 'pt-BR', com({ waitId: 'w', resumeAt: Date.parse('2026-11-11T20:30:00Z'), cancelled: true }), agora, SP), null)
  assert.equal(paradaDaEspera(t, 'pt-BR', com({ retry: true, resumeAt: Date.parse('2026-11-11T20:30:00Z') }, 'error'), agora, SP), null, 'pausada continua aviso de erro')
  assert.equal(paradaDaEspera(t, 'pt-BR', com({ waitId: 'w', resumeAt: Date.parse('2026-11-11T17:00:00Z') }), agora, SP), null, 'espera vencida pelo relógio, sem ready: fica o texto do Hub')
  assert.equal(paradaDaEspera(t, 'pt-BR', com({ resumeAt: Date.parse('2026-11-11T17:00:00Z') }, 'info'), agora, SP), null, 'idem, Hub antigo')
  assert.equal(paradaDaEspera(t, 'pt-BR', { ...aviso({}), data: { text: 'x' } } as Entry, agora, SP), null, 'aviso sem espera')
  const rotina = { ...aviso({ waitId: 'w', resumeAt: Date.parse('2026-11-11T20:30:00Z') }), data: { author: 'crivo', text: 'Rotina «Resumo» aguardando o limite da assinatura; volto às 17:30.', subscriptionWait: { waitId: 'w', resumeAt: Date.parse('2026-11-11T20:30:00Z') }, routineWait: { routineId: 'r1', runId: 'run1' } } } as Entry
  assert.equal(paradaDaEspera(t, 'pt-BR', rotina, agora, SP), null, 'espera de rotina conserva o nome dela no texto do Hub')
})
