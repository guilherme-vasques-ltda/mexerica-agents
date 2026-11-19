import assert from 'node:assert/strict'
import test from 'node:test'
import type { Bot } from '../../../../shared/models.ts'
import { canRemoveMember, channelDraft, leaderOf } from './canal.ts'

const channel = (leader: string | null = null) => ({ id: 'ship', kind: 'group', name: 'ship', description: 'd', members: ['ana', 'caio'], pinned: false, leader }) as unknown as Bot

test('canal: sem líder ninguém vira líder; o líder não sai antes de trocar', () => {
  assert.equal(leaderOf(channel()), null)
  assert.equal(leaderOf(channel(null)), null)
  assert.equal(leaderOf(channel('ana')), 'ana')
  assert.equal(leaderOf(channel('fora')), null)
  assert.equal(canRemoveMember(channel('ana'), 'ana'), false)
  assert.equal(canRemoveMember(channel('ana'), 'caio'), true)
})

test('canal: o draft só manda `leader` quando muda; null tira o líder', () => {
  assert.equal('leader' in channelDraft(channel('ana'), { name: 'novo' }), false)
  assert.equal(channelDraft(channel('ana'), { name: 'novo' }).name, 'novo')
  assert.equal(channelDraft(channel('ana'), { leader: 'caio' }).leader, 'caio')
  assert.equal(channelDraft(channel('ana'), { leader: null }).leader, null)
})
