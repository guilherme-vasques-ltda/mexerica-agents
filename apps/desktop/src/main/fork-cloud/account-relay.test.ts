import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import test from 'node:test'
import { openAccountRelay } from './account-relay.ts'
import { signerFromSeed, computerIdOf } from './crypto.ts'
import { FakeSocket, hostAccept } from './fake-hub.ts'

const host = signerFromSeed(new Uint8Array(32).fill(1))
const signer = signerFromSeed(new Uint8Array(32).fill(3))
function fixture() {
  const events = new EventEmitter()
  let epoch = 1
  let resolve!: (value: string) => void
  const token = new Promise<string>((done) => { resolve = done })
  const seal = { epoch: 1 }
  const sockets: FakeSocket[] = []
  const account = {
    state: { cloudURL: 'https://api.example.test' }, captureCloudSession: () => seal,
    isCloudSessionCurrent: () => epoch === 1, verifiedDeviceBinding: () => ({ deviceKey: Buffer.from(signer.publicKey).toString('base64url') }), tokenForCloud: () => token,
    on: events.on.bind(events), off: events.off.bind(events),
  }
  const options = { cloudURL: account.state.cloudURL, computerId: computerIdOf(host.publicKey), hostSignPub: host.publicKey, signer,
    socket: (url: string, headers: Record<string, string>) => { const socket = new FakeSocket(url, headers); sockets.push(socket); return socket } }
  return { account: account as never, options, sockets, resolve, change: () => { epoch++; events.emit('epoch', epoch) } }
}

test('troca durante token não cria socket para outra sessão', async () => {
  const f = fixture()
  const pending = openAccountRelay(f.account, f.options)
  f.change(); f.resolve('fixture')
  assert.equal((await pending).kind, 'superseded')
  assert.equal(f.sockets.length, 0)
})

test('troca durante handshake encerra socket e descarta welcome tardio sem reabrir', async () => {
  const f = fixture(); f.resolve('fixture')
  const opened = await openAccountRelay(f.account, f.options)
  assert.equal(opened.kind, 'started')
  if (opened.kind !== 'started') return
  const socket = f.sockets[0]!
  socket.open(); socket.deliver({ t: 'presence', online: true })
  const welcome = hostAccept(host, socket.lastHello()).welcome
  f.change(); socket.deliver(welcome); opened.link.start()
  assert.equal(opened.link.state.kind, 'stopped')
  assert.equal(opened.link.currentChannel, null)
  assert.equal(socket.closedWith, 1000)
  assert.equal(f.sockets.length, 1)
})

test('troca após canal pronto encerra chamada em voo como unknown', async () => {
  const f = fixture(); f.resolve('fixture')
  const opened = await openAccountRelay(f.account, f.options)
  if (opened.kind !== 'started') throw new Error('relay unavailable')
  const socket = f.sockets[0]!
  socket.open(); socket.deliver({ t: 'presence', online: true }); socket.deliver(hostAccept(host, socket.lastHello()).welcome)
  const pending = opened.link.currentChannel!.call('send', { text: 'fixture' })
  f.change()
  assert.deepEqual(await pending, { kind: 'unknown', reason: 'closed' })
  assert.equal(opened.link.currentChannel, null)
})
