import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { test } from 'node:test'
import type { IpcMain } from 'electron'
import { requireOfficialSession } from './fork-official-ipc.ts'

test('official IPC denies privileged and unknown channels before login and after revocation', () => {
  type Event = { entry?: boolean; returnValue?: unknown }
  type Handler = (event: Event) => unknown
  const handlers = new Map<string, Handler>()
  const ipc = Object.assign(new EventEmitter(), { handle: (channel: string, handler: Handler) => handlers.set(channel, handler) })
  let confirmed = false
  let starts = 0
  requireOfficialSession(ipc as unknown as IpcMain, () => confirmed, event => (event as unknown as Event).entry === true)
  ipc.handle('account:state', () => 'state')
  ipc.handle('host:call', () => 'hub')
  ipc.handle('account:newPrivilegedFeature', () => 'future')
  ipc.on('host:install', () => { starts++ })
  for (const channel of ['speech:available', 'island:supported']) {
    ipc.on(channel, (event: Event) => { event.returnValue = true })
    const entry: Event = { entry: true }
    const other: Event = {}
    ipc.emit(channel, entry)
    ipc.emit(channel, other)
    assert.equal(entry.returnValue, true)
    assert.equal(other.returnValue, null)
  }
  ipc.on('speech:start', () => { starts++ })
  ipc.emit('speech:start', { entry: true })
  const opening = { entry: true }
  assert.equal(handlers.get('account:state')!(opening), 'state')
  assert.throws(() => handlers.get('account:state')!({}), /account_required/)
  for (const channel of ['host:call', 'account:newPrivilegedFeature']) assert.throws(() => handlers.get(channel)!(opening), /account_required/)
  ipc.emit('host:install', { entry: true })
  assert.equal(starts, 0)
  confirmed = true
  assert.equal(handlers.get('host:call')!(opening), 'hub')
  ipc.emit('host:install', { entry: true })
  confirmed = false
  assert.throws(() => handlers.get('host:call')!(opening), /account_required/)
  ipc.emit('host:install', { entry: true })
  assert.equal(starts, 1)
})
