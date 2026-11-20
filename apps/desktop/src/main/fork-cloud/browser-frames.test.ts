import test from 'node:test'
import assert from 'node:assert/strict'
import { Channel, ChannelBroken } from './channel.ts'
import type { Channel as ChannelType } from './channel.ts'
import { computerIdOf, random, signerFromSeed } from './crypto.ts'
import { FakeSocket, HostCodec, hostAccept } from './fake-hub.ts'
import { RelayLink } from './relay-socket.ts'
import { encodeFrame, FRAME_HEADER, FRAME_MAX_WIRE, FRAMES_CAPABILITY, type BrowserFrame } from '../../shared/relay-protocol/browser-frames.ts'

// B2: o app negocia browserFramesV1 e recebe quadros binários dentro dos frames cifrados do canal.

const host = signerFromSeed(new Uint8Array(32).fill(1))
const device = signerFromSeed(new Uint8Array(32).fill(3))
const computerId = computerIdOf(host.publicKey)

const quadro = (sequence = 1, bytes = 4): BrowserFrame => ({
  session: 's1',
  target: 'aba',
  sequence,
  targetGeneration: 0,
  image: { width: 960, height: 540 },
  viewport: { width: 1280, height: 720 },
  jpeg: Uint8Array.from([0xff, 0xd8, ...new Uint8Array(bytes), 0xff, 0xd9]),
})

function par(negociado: boolean) {
  const keys = { d2h: random(32), h2d: random(32) }
  const frames: { t: 'f'; c: number; d: string }[] = []
  const channel = new Channel(keys, { send: (f) => frames.push(f) }, negociado)
  return { channel, frames, hub: new HostCodec(keys) }
}

function link(frames: boolean | undefined) {
  const sockets: FakeSocket[] = []
  const channels: (ChannelType | null)[] = []
  const l = new RelayLink({
    cloudURL: 'https://api.example.test',
    computerId,
    hostSignPub: host.publicKey,
    signer: device,
    random: () => 1,
    ...(frames === undefined ? {} : { browserFrames: frames }),
    socket: (url, headers) => {
      const s = new FakeSocket(url, headers)
      sockets.push(s)
      return s
    },
    onChannel: (c) => channels.push(c),
  })
  l.start()
  const socket = sockets[0]!
  socket.open()
  socket.deliver({ t: 'presence', online: true, lastSeenAt: null })
  return { l, socket, channels }
}

test('o hello só oferece browserFramesV1 no canal de dados que pede; os demais ficam como eram', () => {
  assert.deepEqual(link(true).socket.lastHello().caps, [FRAMES_CAPABILITY])
  assert.equal('caps' in link(undefined).socket.lastHello(), false)
  assert.equal('caps' in link(false).socket.lastHello(), false)
})

test('o Hub aceita a capacidade só se ela foi oferecida e ele a devolve no welcome', () => {
  const aceito = link(true)
  const a = hostAccept(host, aceito.socket.lastHello(), host, true)
  aceito.socket.deliver(a.welcome)
  assert.equal(aceito.channels[0]?.framesNegotiated, true)

  const antigo = link(true)
  antigo.socket.deliver(hostAccept(host, antigo.socket.lastHello(), host, false).welcome)
  assert.equal(antigo.channels[0]?.framesNegotiated, false, 'Hub antigo: sem caps no welcome')

  const naoPediu = link(false)
  naoPediu.socket.deliver({ ...hostAccept(host, naoPediu.socket.lastHello(), host, false).welcome, caps: [FRAMES_CAPABILITY] })
  assert.equal(naoPediu.channels[0]?.framesNegotiated, false, 'welcome com caps sem oferta não liga quadros')
})

test('o quadro binário chega decodificado ao consumidor, sem passar por JSON', () => {
  const { channel, hub } = par(true)
  const recebidos: BrowserFrame[] = []
  const sub = channel.subscribeFrames({ onFrame: (f) => recebidos.push(f), onEnd: () => {} })
  assert.ok(sub)
  const wire = encodeFrame(quadro(7, 300_000))
  assert.ok(wire.length > 256 * 1024, 'o quadro atravessa mais de um pedaço cifrado')
  for (const f of hub.sealBytes(wire)) channel.receive(f)
  assert.equal(recebidos.length, 1)
  assert.equal(recebidos[0]!.sequence, 7)
  assert.equal(recebidos[0]!.session, 's1')
  assert.deepEqual(recebidos[0]!.jpeg, quadro(7, 300_000).jpeg)
  // Mensagem JSON no meio continua funcionando.
  for (const f of hub.seal({ id: 99, ok: 1 })) channel.receive(f)
  for (const f of hub.sealBytes(encodeFrame(quadro(8)))) channel.receive(f)
  assert.deepEqual(recebidos.map((f) => f.sequence), [7, 8])
})

test('Hub que não negociou mandando quadro quebra o canal (4002) e nada chega ao consumidor', () => {
  const { channel, hub } = par(false)
  assert.equal(channel.subscribeFrames({ onFrame: () => assert.fail('nada deve chegar'), onEnd: () => {} }), null)
  const [f] = hub.sealBytes(encodeFrame(quadro()))
  assert.throws(() => channel.receive(f!), (e: unknown) => e instanceof ChannelBroken && e.closeCode === 4002)
})

test('quadro corrompido quebra o canal (4002); gigante é recusado antes de juntar os pedaços (4013)', () => {
  const corrompido = encodeFrame(quadro())
  corrompido[corrompido.length - 1] = 0
  const a = par(true)
  a.channel.subscribeFrames({ onFrame: () => assert.fail('corrompido'), onEnd: () => {} })
  assert.throws(() => a.channel.receive(a.hub.sealBytes(corrompido)[0]!), (e: unknown) => e instanceof ChannelBroken && e.closeCode === 4002)

  const gigante = new Uint8Array(FRAME_MAX_WIRE + 256 * 1024)
  gigante.set(encodeFrame(quadro()).subarray(0, FRAME_HEADER), 0)
  const b = par(true)
  b.channel.subscribeFrames({ onFrame: () => assert.fail('gigante'), onEnd: () => {} })
  const pedacos = b.hub.sealBytes(gigante)
  assert.ok(pedacos.length >= 3)
  b.channel.receive(pedacos[0]!)
  b.channel.receive(pedacos[1]!)
  assert.throws(() => b.channel.receive(pedacos[2]!), (e: unknown) => e instanceof ChannelBroken && e.closeCode === 4013)
})

test('um consumidor por canal; cancelar libera; fechar o canal encerra o consumidor', () => {
  const { channel, hub } = par(true)
  let fim: unknown = 'aberto'
  const sub = channel.subscribeFrames({ onFrame: () => {}, onEnd: (e) => (fim = e) })!
  assert.equal(channel.subscribeFrames({ onFrame: () => {}, onEnd: () => {} }), null)
  sub.cancel()
  const outros: BrowserFrame[] = []
  const novo = channel.subscribeFrames({ onFrame: (f) => outros.push(f), onEnd: (e) => (fim = e) })
  assert.ok(novo)
  for (const f of hub.sealBytes(encodeFrame(quadro()))) channel.receive(f)
  assert.equal(outros.length, 1)
  channel.close()
  assert.equal((fim as { status: number }).status, 0)
  assert.equal(channel.subscribeFrames({ onFrame: () => {}, onEnd: () => {} }), null)
})

test('browserStream* só sai se o Hub aceitou a capacidade neste canal; os outros métodos seguem como eram', async () => {
  const sem = par(false)
  assert.deepEqual(await sem.channel.call('browserStreamSubscribe', {}), { kind: 'notSent', reason: 'notReady' })
  assert.deepEqual(await sem.channel.call('browserStreamInput', {}), { kind: 'notSent', reason: 'notReady' })
  assert.equal(sem.frames.length, 0, 'nada vai para o fio')
  void sem.channel.call('send', {}, 1_000)
  assert.equal(sem.frames.length, 1, 'chamadas comuns continuam saindo')

  const com = par(true)
  void com.channel.call('browserStreamSubscribe', {}, 1_000)
  assert.equal(com.frames.length, 1)
  com.channel.close()
})

test('revogação do acesso chega ao consumidor de quadros como 403; queda comum continua sendo 0', () => {
  const revogado = link(true)
  revogado.socket.deliver(hostAccept(host, revogado.socket.lastHello(), host, true).welcome)
  const fins: unknown[] = []
  revogado.channels[0]!.subscribeFrames({ onFrame: () => {}, onEnd: (e) => fins.push(e) })
  revogado.socket.drop(4003, 'revoked')
  assert.equal((fins[0] as { status: number }).status, 403)
  assert.equal(fins.length, 1)

  const queda = link(true)
  queda.socket.deliver(hostAccept(host, queda.socket.lastHello(), host, true).welcome)
  const outros: unknown[] = []
  queda.channels[0]!.subscribeFrames({ onFrame: () => {}, onEnd: (e) => outros.push(e) })
  queda.socket.drop(1006, 'queda')
  assert.equal((outros[0] as { status: number }).status, 0)
})
