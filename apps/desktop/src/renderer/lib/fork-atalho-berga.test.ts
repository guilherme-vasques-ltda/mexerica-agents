import test from 'node:test'
import assert from 'node:assert/strict'
import { acceptsBergaShortcut, bergaTarget, type BergaTargetSource } from './fork-atalho-berga.ts'
import type { SpaceConversation } from '../../shared/fork-spaces.ts'

const key = { key: ' ', code: 'Space', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, repeat: false, isComposing: false, keyCode: 32, defaultPrevented: false }
test('só Mac, opt-in e janela focada; não rouba edição, IME, repetição ou atalhos de outros controles', () => {
  assert.equal(acceptsBergaShortcut(key, 'darwin', true, true, false), true)
  for (const platform of ['linux', 'win32']) assert.equal(acceptsBergaShortcut(key, platform, true, true, false), false)
  assert.equal(acceptsBergaShortcut(key, 'darwin', false, true, false), false)
  assert.equal(acceptsBergaShortcut(key, 'darwin', true, false, false), false)
  assert.equal(acceptsBergaShortcut(key, 'darwin', true, true, true), false)
  for (const patch of [{ isComposing: true }, { keyCode: 229 }, { repeat: true }, { defaultPrevented: true }, { ctrlKey: true }, { altKey: true }, { shiftKey: true }, { metaKey: false }, { key: 'a' }, { code: 'KeyA' }]) {
    assert.equal(acceptsBergaShortcut({ ...key, ...patch }, 'darwin', true, true, false), false, JSON.stringify(patch))
  }
})
const conversation = (patch: Partial<SpaceConversation> = {}): SpaceConversation => ({ id: 'chat-space', spaceId: 'work', botId: 'berga-work', primary: true, closed: false, ...patch }) as SpaceConversation
const source = (): BergaTargetSource => ({
  space: { id: 'work', ready: true, live: true, principalId: 'berga-work', conversations: [conversation({ id: 'extra', primary: false }), conversation()] },
  personal: { computerId: 'hub', ready: true, principalId: 'berga-personal', hasBot: true },
})
test('abre somente a conversa principal do Berga do Space ativo', () => {
  assert.deepEqual(bergaTarget(source()), { kind: 'space', id: 'chat-space' })
  for (const patch of [{ principalId: null }, { ready: false }, { live: false }, { conversations: [] },
    { conversations: [conversation({ spaceId: 'personal' })] }, { conversations: [conversation({ primary: false })] },
    { conversations: [conversation({ closed: true })] }, { conversations: [conversation({ botId: 'other' })] }]) {
    const s = source()
    Object.assign(s.space!, patch)
    assert.equal(bergaTarget(s), null, JSON.stringify(patch))
  }
})
test('Pessoal só é destino quando ele próprio está ativo e tem principal confirmada', () => {
  const s = source(); s.space = null
  assert.deepEqual(bergaTarget(s), { kind: 'personal', computerId: 'hub', botId: 'berga-personal' })
  for (const patch of [{ ready: false }, { principalId: null }, { hasBot: false }]) {
    assert.equal(bergaTarget({ ...s, personal: { ...s.personal!, ...patch } }), null)
  }
  assert.equal(bergaTarget({ space: null, personal: null }), null)
})
