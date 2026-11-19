import test from 'node:test'
import assert from 'node:assert/strict'
import { Channel, ChannelBroken, MAX_REQUESTS, MAX_SUBSCRIPTIONS } from './channel.ts'
import { CHUNK, random } from './crypto.ts'
import { HostCodec } from './fake-hub.ts'

function pair() {
  const keys = { d2h: random(32), h2d: random(32) }
  const frames: { t: 'f'; c: number; d: string }[] = []
  const channel = new Channel(keys, { send: (f) => frames.push(f) })
  return { channel, frames, hub: new HostCodec(keys) }
}

test('mensagens grandes saem e chegam em pedaços de 256 KiB; acima de 1 MiB nada sai', async () => {
  const { channel, frames, hub } = pair()
  const text = 'x'.repeat(CHUNK + 10)
  void channel.call('send', { text })
  assert.equal(frames.length, 2)
  const inner = frames.map((f) => hub.open(f)).filter(Boolean)[0] as { id: number; b: { text: string } }
  assert.equal(inner.b.text, text)
  const big = await channel.call('send', { text: 'y'.repeat(1024 * 1024) })
  assert.deepEqual(big, { kind: 'notSent', reason: 'tooLarge' })
  assert.equal(frames.length, 2, 'nada foi enviado')

  const pending = channel.call('history', {})
  const id = (frames.slice(2).map((f) => hub.open(f)).filter(Boolean)[0] as { id: number }).id
  for (const f of hub.seal({ id, ok: { big: 'z'.repeat(CHUNK * 2) } })) channel.receive(f)
  const r = await pending
  assert.equal(r.kind, 'ok')
  channel.close()
})

test('prazo vencido: resultado unknown, cancelamento enviado, sem repetir a chamada', async () => {
  const { channel, frames, hub } = pair()
  const r = await channel.call('deleteBot', { botId: 'b' }, 20)
  assert.deepEqual(r, { kind: 'unknown', reason: 'timeout' })
  const inner = frames.map((f) => hub.open(f)).filter(Boolean) as Record<string, unknown>[]
  assert.equal(inner.length, 2)
  assert.equal(inner[0]!.m, 'deleteBot')
  assert.deepEqual(inner[1], { id: inner[0]!.id, cancel: true })
  // A resposta que chega depois não ressuscita a chamada.
  for (const f of hub.seal({ id: inner[0]!.id, ok: {} })) channel.receive(f)
})

test('limites de requests e assinaturas; erro do Hub chega como err', async () => {
  const { channel, frames, hub } = pair()
  const calls = Array.from({ length: MAX_REQUESTS }, () => channel.call('usage', {}))
  assert.deepEqual(await channel.call('usage', {}), { kind: 'notSent', reason: 'busy' })
  const subs = Array.from({ length: MAX_SUBSCRIPTIONS }, () => channel.subscribe('events', {}, () => {}))
  assert.ok(subs.every(Boolean))
  assert.equal(channel.subscribe('events', {}, () => {}), null)
  const first = (frames.map((f) => hub.open(f)).filter(Boolean)[0] as { id: number }).id
  for (const f of hub.seal({ id: first, err: { status: 403, message: 'This can only be done at the computer itself.' } })) channel.receive(f)
  assert.deepEqual(await calls[0], { kind: 'err', status: 403, message: 'This can only be done at the computer itself.' })
  channel.close()
  const rest = await Promise.all(calls.slice(1))
  assert.ok(rest.every((r) => r.kind === 'unknown'))
  assert.deepEqual(await channel.call('usage', {}), { kind: 'notSent', reason: 'notReady' })
})

test('quadro fora de ordem ou adulterado quebra o canal', () => {
  const { channel, hub } = pair()
  const [a, b] = [...hub.seal({ id: 1, ev: 1 }), ...hub.seal({ id: 1, ev: 2 })]
  assert.throws(() => channel.receive(b!), (e: unknown) => e instanceof ChannelBroken && e.closeCode === 4002)
  const other = pair()
  const [frame] = other.hub.seal({ id: 1, ev: 1 })
  assert.throws(() => channel.receive({ c: 0, d: frame!.d }), ChannelBroken)
  void a
})
