/** WebSocket direto estrito. Sem redirects, HTTP API, extensão ou remontagem de fragmentos. */
import { createConnection, type Socket } from 'node:net'
import { createHash, randomBytes } from 'node:crypto'
import { LIMIT } from './personal-link-wire.ts'
import { refused } from './personal-link-channel-wire.ts'
const HTTP_MAX = 4096
const TIMEOUT = 5000
export class PersonalLinkSocket {
  private buffer = Buffer.alloc(0)
  private upgraded = false
  private ended = false
  private pending: { resolve(v: Buffer): void; reject(e: Error): void; timer: ReturnType<typeof setTimeout> } | null = null
  private accept: string
  private readonly socket: Socket
  private readyResolve!: () => void
  private readyReject!: (e: Error) => void
  private readonly ready: Promise<void>
  private readonly onClose: () => void
  private readonly live: () => void
  private constructor(host: string, port: number, onClose: () => void, live: () => void) {
    this.onClose = onClose
    this.live = live
    const key = randomBytes(16).toString('base64')
    this.accept = createHash('sha1').update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest('base64')
    this.ready = new Promise((resolve, reject) => { this.readyResolve = resolve; this.readyReject = reject })
    this.socket = createConnection({ host, port })
    this.socket.on('error', () => this.close())
    this.socket.on('close', () => this.close())
    this.socket.on('end', () => this.close())
    this.socket.on('data', bytes => {
      try { this.data(bytes) } catch { this.close() }
    })
    this.socket.once('connect', () => { try { this.live(); this.socket.write(`GET /personal-link/channel HTTP/1.1\r\nHost: ${host}:${port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: ${key}\r\n\r\n`) } catch { this.close() } })
  }
  static async connect(host: string, port: number, onClose: () => void, live: () => void, capture: (wire: PersonalLinkSocket) => void): Promise<PersonalLinkSocket> {
    if (!/^[A-Za-z0-9.-]{1,253}$/.test(host) || !Number.isInteger(port) || port < 1 || port > 65535) refused()
    const wire = new PersonalLinkSocket(host, port, onClose, live)
    capture(wire)
    const timer = setTimeout(() => wire.close(), TIMEOUT)
    try { await wire.ready; return wire } catch (e) { wire.close(); throw e } finally { clearTimeout(timer) }
  }
  private data(bytes: Buffer) {
    this.live()
    if (this.ended || this.buffer.length + bytes.length > LIMIT + HTTP_MAX) refused()
    this.buffer = Buffer.concat([this.buffer, bytes])
    if (!this.upgraded) {
      const end = this.buffer.indexOf('\r\n\r\n')
      if (end < 0) { if (this.buffer.length >= HTTP_MAX) refused(); return }
      if (end + 4 > HTTP_MAX) refused()
      const lines = this.buffer.subarray(0, end).toString('ascii').split('\r\n')
      if (lines.shift() !== 'HTTP/1.1 101 Switching Protocols') refused()
      const headers = new Map<string, string>()
      for (const line of lines) {
        const colon = line.indexOf(':'); if (colon <= 0) refused()
        const name = line.slice(0, colon).toLowerCase(); if (headers.has(name)) refused()
        headers.set(name, line.slice(colon + 1).trim())
      }
      if (headers.get('upgrade')?.toLowerCase() !== 'websocket' || headers.get('connection')?.toLowerCase() !== 'upgrade' || headers.get('sec-websocket-accept') !== this.accept || headers.has('sec-websocket-extensions') || headers.has('sec-websocket-protocol')) refused()
      this.buffer = this.buffer.subarray(end + 4); this.upgraded = true; this.readyResolve()
    }
    while (this.buffer.length >= 2) {
      if (this.buffer[0] !== 0x82 || this.buffer[1] & 0x80) refused()
      const small = this.buffer[1] & 127
      if (small === 127) refused()
      if (small === 126 && this.buffer.length < 4) return
      const length = small === 126 ? this.buffer.readUInt16BE(2) : small, head = small === 126 ? 4 : 2
      if (!length || (small === 126 && length < 126) || head + length > LIMIT) refused()
      if (this.buffer.length < head + length) return
      const value = Buffer.from(this.buffer.subarray(head, head + length)); this.buffer = this.buffer.subarray(head + length)
      if (this.pending) { const p = this.pending; this.pending = null; clearTimeout(p.timer); p.resolve(value) }
      else refused()
    }
  }
  private receive(): Promise<Buffer> {
    if (this.ended || this.pending) return Promise.reject(new Error('personalLinkChannelClosed'))
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => this.close(), TIMEOUT)
      this.pending = { resolve, reject, timer }
    })
  }
  exchange(bytes: Buffer): Promise<Buffer> {
    const response = this.receive()
    try { this.send(bytes) } catch { this.close() }
    return response
  }
  send(bytes: Buffer) {
    this.live()
    if (this.ended || !this.upgraded || bytes.length + 8 > LIMIT || this.socket.writableLength > LIMIT) refused()
    const head = Buffer.alloc(bytes.length < 126 ? 6 : 8), mask = randomBytes(4)
    head[0] = 0x82
    if (bytes.length < 126) head[1] = 0x80 | bytes.length
    else { head[1] = 0xfe; head.writeUInt16BE(bytes.length, 2) }
    mask.copy(head, head.length - 4)
    const body = Buffer.from(bytes); for (let i = 0; i < body.length; i++) body[i] ^= mask[i % 4]
    this.socket.write(Buffer.concat([head, body]))
  }
  close() {
    if (this.ended) return
    this.ended = true; this.buffer = Buffer.alloc(0)
    const error = new Error('personalLinkChannelClosed')
    this.readyReject(error)
    if (this.pending) { const p = this.pending; this.pending = null; clearTimeout(p.timer); p.reject(error) }
    this.socket.destroy(); this.onClose()
  }
}
