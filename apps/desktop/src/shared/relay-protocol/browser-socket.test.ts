import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { b64url, unb64url } from './protocol.ts'
import { browserSocket } from './relay-socket.ts'

// WebSocket falso: captura url e subprotocolos e guarda a superfície de SocketLike.
class FakeWebSocket {
  static instances: { url: string; protocols: unknown }[] = []
  sent: string[] = []
  closed: [number?, string?] | null = null
  listeners = new Map<string, ((event?: never) => void)[]>()
  constructor(url: string, protocols?: string | string[]) {
    FakeWebSocket.instances.push({ url, protocols })
  }
  send(data: string) {
    this.sent.push(data)
  }
  close(code?: number, reason?: string) {
    this.closed = [code, reason]
  }
  addEventListener(type: string, listener: (event?: never) => void) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener])
  }
}

test('browserSocket abre com subprotocolo codync-sig.v1 e o Codync-Sig em base64url', () => {
  FakeWebSocket.instances = []
  const g = globalThis as unknown as { WebSocket: unknown }
  const previous = g.WebSocket
  g.WebSocket = FakeWebSocket
  try {
    const value = 'v=1,kid=abcd,ts=1730000000000,nonce=n123,sig=sig456'
    const socket = browserSocket('wss://api.example.test/v1/relay/device/AAAAAAAAAAAAAAAAAAAAAA?v=1', {
      'Codync-Sig': value,
    }) as unknown as FakeWebSocket
    assert.equal(FakeWebSocket.instances.length, 1)
    const call = FakeWebSocket.instances[0]!
    assert.equal(call.url, 'wss://api.example.test/v1/relay/device/AAAAAAAAAAAAAAAAAAAAAA?v=1')
    assert.deepEqual(call.protocols, ['codync-sig.v1', b64url(new TextEncoder().encode(value))])
    // O objeto devolvido é um SocketLike utilizável pela RelayLink.
    socket.send('{"t":"ping"}')
    assert.deepEqual(socket.sent, ['{"t":"ping"}'])
    socket.close(1000, 'fim')
    assert.deepEqual(socket.closed, [1000, 'fim'])
  } finally {
    g.WebSocket = previous
  }
})

test('browserSocket recusa abrir sem o cabeçalho Codync-Sig', () => {
  FakeWebSocket.instances = []
  const g = globalThis as unknown as { WebSocket: unknown }
  const previous = g.WebSocket
  g.WebSocket = FakeWebSocket
  try {
    assert.throws(() => browserSocket('wss://x.test/', {}), /sigMissing/)
    assert.equal(FakeWebSocket.instances.length, 0)
  } finally {
    g.WebSocket = previous
  }
})

// ---- contrato com o servidor da ponte (PR #742, MX-212) ----

const fixture = JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/browser-socket-subprotocol.json', import.meta.url), 'utf8')) as {
  value: string
  protocols: string[]
  secWebSocketProtocol: string
  maxDecodedBytes: number
}
const relayVectors = JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json', import.meta.url), 'utf8'))

/**
 * Cópia da lógica de `sigFromSubprotocol` de cloud/src/auth.ts no head 77b5d457 do PR #742 (sem
 * acoplar os PRs): exatamente dois itens, `codync-sig.v1` (caixa exata) e base64url canônico de
 * utf8 até 512 bytes, ASCII imprimível. `null` quando recusa.
 */
function serverParse(header: string | null): string | null {
  if (!header) return null
  const items = header.split(',').map((x) => x.trim())
  if (items.length !== 2 || items[0] !== 'codync-sig.v1' || !items[1]) return null
  const packed = unb64url(items[1])
  if (!packed || packed.length > 512) return null
  try {
    const value = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(packed)
    return /^[\x21-\x7e](?:[\x20-\x7e]*[\x21-\x7e])?$/.test(value) ? value : null
  } catch {
    return null
  }
}

function openWith(value: string) {
  FakeWebSocket.instances = []
  const g = globalThis as unknown as { WebSocket: unknown }
  const previous = g.WebSocket
  g.WebSocket = FakeWebSocket
  try {
    browserSocket('wss://api.example.test/v1/relay/device/AAAAAAAAAAAAAAAAAAAAAA?v=1', { 'Codync-Sig': value })
    return FakeWebSocket.instances[0]!.protocols as string[]
  } finally {
    g.WebSocket = previous
  }
}

test('vetor compartilhado: o que o browserSocket produz é o que o servidor do #742 aceita', () => {
  assert.equal(`Codync-Sig: ${fixture.value}`, relayVectors.requestSig.header, 'o vetor vem do Codync-Sig oficial')
  const protocols = openWith(fixture.value)
  assert.deepEqual(protocols, fixture.protocols)
  // O navegador junta a oferta com vírgula e espaço no Sec-WebSocket-Protocol.
  assert.equal(protocols.join(', '), fixture.secWebSocketProtocol)
  assert.equal(serverParse(fixture.secWebSocketProtocol), fixture.value)
  assert.equal(serverParse(protocols.join(',')), fixture.value, 'também sem espaço depois da vírgula')
})

test('o parse do servidor recusa o que o contrato não permite (ordem, caixa, itens, limite)', () => {
  const [name, packed] = fixture.protocols as [string, string]
  assert.equal(serverParse(`${packed}, ${name}`), null, 'ordem trocada')
  assert.equal(serverParse(`Codync-Sig.V1, ${packed}`), null, 'caixa diferente')
  assert.equal(serverParse(`${name}, ${packed}, extra`), null, 'item a mais')
  assert.equal(serverParse(name), null, 'sem o valor')
  assert.equal(serverParse(`${name}, ${b64url(new TextEncoder().encode('x'.repeat(513)))}`), null, 'acima de 512 bytes')
  assert.equal(serverParse(`${name}, ${b64url(new TextEncoder().encode('x'.repeat(512)))}`), 'x'.repeat(512), 'exatamente 512 bytes passa')
})

test('browserSocket só oferece valores que o servidor aceitaria: limite de 512 bytes e ASCII imprimível', () => {
  assert.equal(serverParse(openWith('x'.repeat(512)).join(', ')), 'x'.repeat(512))
  for (const bad of ['x'.repeat(513), 'v=1\nkid=a', ' v=1', 'v=1 ', 'v=1,kid=ação', '', 'v=1\u0000']) {
    assert.throws(() => openWith(bad), /sigInvalid/, JSON.stringify(bad).slice(0, 30))
  }
})
