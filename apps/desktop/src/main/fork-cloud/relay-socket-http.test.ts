import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import type { Socket } from 'node:net'
import { test } from 'node:test'
import { computerIdOf, signerFromSeed } from './crypto.ts'
import { nodeSocket, RelayLink } from './relay-socket.ts'

test('recusa HTTP no upgrade sai de connecting mesmo quando o Node só emite error', { timeout: 3_000 }, async (t) => {
  const server = createServer()
  const connections = new Set<Socket>()
  server.on('connection', (socket) => {
    connections.add(socket)
    socket.once('close', () => connections.delete(socket))
  })
  let upgradeRequests = 0
  server.on('upgrade', (_request, socket) => {
    upgradeRequests += 1
    socket.end('HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n')
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  assert(address && typeof address === 'object')
  let signalError!: () => void
  const socketError = new Promise<void>((resolve) => { signalError = resolve })
  const hostKey = signerFromSeed(new Uint8Array(32).fill(8)).publicKey
  const link = new RelayLink({
    cloudURL: `http://127.0.0.1:${address.port}`,
    computerId: computerIdOf(hostKey),
    hostSignPub: hostKey,
    signer: signerFromSeed(new Uint8Array(32).fill(7)),
    socket: (url, headers) => {
      const socket = nodeSocket(url, headers)
      socket.addEventListener('error', signalError)
      return socket
    },
  })
  t.after(async () => {
    link.stop()
    for (const socket of connections) socket.destroy()
    await new Promise<void>((resolve) => server.close(() => resolve()))
  })
  link.start()
  await socketError
  assert.equal(upgradeRequests, 1)
  assert.equal(link.currentChannel, null)
  assert.equal(link.state.kind, 'retrying')
  if (link.state.kind !== 'retrying') assert.fail('recusa de upgrade precisa sair de connecting')
  assert.equal(link.state.cause, 'connectFailed')
  assert.equal(link.state.connectFailures, 1)
})
