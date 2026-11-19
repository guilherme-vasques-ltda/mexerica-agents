import assert from 'node:assert/strict'
import test from 'node:test'
import type { WebContents } from 'electron'
import { generalWindowProvider, isGeneralContext, markPrivateContext } from './contexts.ts'

function contents(id: number) {
  let destroyed = false
  const sent: unknown[][] = []
  return {
    value: { id, isDestroyed: () => destroyed, send: (...args: unknown[]) => sent.push(args) } as unknown as WebContents,
    destroy: () => { destroyed = true }, sent,
  }
}

test('fornecedor real exclui privado de lista e broadcast; identidade não acompanha ID reutilizado', () => {
  const general = contents(1)
  const privateContext = contents(2)
  let windows = [general, privateContext].map(context => ({ webContents: context.value }))
  const provider = generalWindowProvider(() => windows)
  markPrivateContext(privateContext.value)
  markPrivateContext(privateContext.value)
  assert.deepEqual(provider().map(window => window.webContents), [general.value])
  for (const window of provider()) window.webContents.send('host:change', { state: 'fixture' })
  assert.equal(general.sent.length, 1)
  assert.equal(privateContext.sent.length, 0)
  privateContext.destroy()
  general.destroy()
  const replacement = contents(2)
  windows = [...windows, { webContents: replacement.value }]
  assert.deepEqual(provider().map(window => window.webContents), [replacement.value])
  assert.equal(isGeneralContext(privateContext.value), false)
  assert.equal(isGeneralContext(replacement.value), true)
})
