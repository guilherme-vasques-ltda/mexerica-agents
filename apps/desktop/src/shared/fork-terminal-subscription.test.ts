import assert from 'node:assert/strict'
import test from 'node:test'
import { cloudSubscription } from './fork-cloud.ts'
const term = '11111111-2222-4333-8444-555555555555'
test('terminal de login tem assinatura fechada, distinta de eventos gerais', () => {
  assert.deepEqual(cloudSubscription({ since: 0, client: 'login', kind: 'term', term }), { since: 0, client: 'login', kind: 'term', term })
  for (const body of [ { since: 0, client: 'login', kind: 'term', term: '../events' }, { since: 0, client: 'login', kind: 'term', term, replay: true }, { since: 0, client: 'login', kind: 'term', term, spaceId: 's1' }, { since: 0, client: 'web', term } ]) assert.equal(cloudSubscription(body), null)
  assert.deepEqual(cloudSubscription({ since: 3, client: 'web', spaceId: 's1' }), { since: 3, client: 'web', spaceId: 's1' })
})
