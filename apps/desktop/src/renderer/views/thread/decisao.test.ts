import assert from 'node:assert/strict'
import test from 'node:test'
import type { Entry } from '@shared/models'
import { lastDecision, statusDoBalao } from './decisao.ts'

const options = [
  { optionId: 'allow', name: 'Allow', kind: 'allow_once' },
  { optionId: 'deny', name: 'Deny', kind: 'reject_once' },
]
const entry = (kind: string, data: Record<string, unknown> = {}) => ({ kind: 'entry', entry: { kind, data } as unknown as Entry })
const card = (data: Record<string, unknown>) => entry('permission', { options, ...data })

test('uma autorização só: o balão diz o que a pessoa escolheu no último cartão do turno', () => {
  assert.equal(lastDecision([entry('user'), card({ status: 'answered', selected: 'allow' }), entry('tool')]), 'permitiu')
  assert.equal(lastDecision([entry('user'), card({ status: 'answered', selected: 'deny' })]), 'recusou')
  // O mais novo vale.
  assert.equal(lastDecision([entry('user'), card({ status: 'answered', selected: 'deny' }), card({ status: 'answered', selected: 'allow' })]), 'permitiu')
})

test('cartão pendente, cancelado ou de um turno anterior não diz nada', () => {
  assert.equal(lastDecision([entry('user'), card({ status: 'pending' })]), null)
  assert.equal(lastDecision([entry('user'), card({ status: 'cancelled' })]), null)
  assert.equal(lastDecision([card({ status: 'answered', selected: 'allow' }), entry('user'), entry('agent')]), null)
  assert.equal(lastDecision([{ kind: 'time' }]), null)
})

test('a tela permitida sem efeito no Hub não vira "Você permitiu"', () => {
  const tela = { status: 'answered', selected: 'allow', toolKind: 'screen_access' }
  assert.equal(lastDecision([entry('user'), card({ ...tela, screenAccess: { effect: false } })]), null)
  assert.equal(lastDecision([entry('user'), card({ ...tela, screenAccess: { effect: true } })]), 'permitiu')
})

test('o status do balão sai do estado: aprovação aberta com o cartão na tela, ou a decisão', () => {
  assert.equal(statusDoBalao(true, true, null), 'esperando')
  // Um pedido novo aberto esconde a decisão anterior; fora da tela, o balão não promete "acima".
  assert.equal(statusDoBalao(true, true, 'permitiu'), 'esperando')
  assert.equal(statusDoBalao(true, false, 'permitiu'), null)
  assert.equal(statusDoBalao(false, false, 'permitiu'), 'permitiu')
  assert.equal(statusDoBalao(false, true, 'recusou'), 'recusou')
  assert.equal(statusDoBalao(false, false, null), null)
})
