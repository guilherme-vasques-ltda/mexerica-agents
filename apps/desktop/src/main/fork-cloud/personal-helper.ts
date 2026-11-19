/** Enquadramento helper externo v1. Uma operação em voo na mesma captura. */
import { randomBytes } from 'node:crypto'
import type { Readable, Writable } from 'node:stream'
import { personalCanonical } from './personal-protocol'
import { personalDomain } from './personal-wire'
const D = personalDomain('helper')
export type HelperRequest = readonly ['ticket', 'command' | 'device', unknown] | readonly ['consent', unknown, unknown, unknown] | readonly ['retain', unknown] | readonly ['status', string | null] | readonly ['time']
export class PersonalHelper {
  private busy = false
  private alive = true
  private buffer = Buffer.alloc(0)
  private pending: { id: string; op: string; resolve(value: unknown): void; reject(error: Error): void; timer: ReturnType<typeof setTimeout> } | null = null
  constructor(private readonly input: Readable, private readonly output: Writable, private readonly current: () => boolean, private readonly terminate: () => void) {
    input.on('data', this.data); input.on('error', this.failed); input.on('end', this.failed); output.on('error',this.failed)
  }
  private failed = () => { this.close() }
  private data = (chunk: Buffer) => {
    try {
      if (!this.alive || !this.pending || !this.current() || this.buffer.length + chunk.length > 16388) throw new Error('personalHelperFrame')
      this.buffer = Buffer.concat([this.buffer, chunk])
      if (this.buffer.length < 4) return
      const size = this.buffer.readUInt32BE(0)
      if (size < 1 || size > 16384 || this.buffer.length > size + 4) throw new Error('personalHelperFrame')
      if (this.buffer.length !== size + 4) return
      const value = personalCanonical(this.buffer.subarray(4))
      const pending = this.pending
      if (!Array.isArray(value) || value[0] !== D || value[1] !== 1 || value[2] !== pending.id) throw new Error('personalHelperIdentity')
      if (value.length === 5 && value[3] === 'error' && ['invalid','unauthorizedPeer','unavailable','conflict','expired','clockUnavailable','identityMismatch','unsupportedVersion'].includes(value[4])) throw new Error('personalHelperRefused')
      if (value.length !== 6 || value[3] !== 'ok' || value[4] !== pending.op) throw new Error('personalHelperResponse')
      this.buffer = Buffer.alloc(0); this.pending = null; clearTimeout(pending.timer); pending.resolve(value[5])
    } catch { this.close() }
  }
  currentSession() { return this.alive && this.current() }
  async request(request: HelperRequest): Promise<unknown> {
    if (this.busy || !this.currentSession()) throw new Error('personalHelperUnavailable')
    this.busy = true
    try {
      const id = randomBytes(16).toString('base64url'), bytes = Buffer.from(JSON.stringify([D,1,id,...request]))
      if (bytes.length > 16384) throw new Error('personalHelperFrame')
      const header = Buffer.alloc(4); header.writeUInt32BE(bytes.length)
      const value = await new Promise<unknown>((resolve,reject) => {
        const timer = setTimeout(() => this.close(),5000)
        this.pending = { id, op:request[0], resolve,reject,timer }
        this.output.write(Buffer.concat([header,bytes]), e => { if (e) this.close() })
      })
      if (!this.currentSession()) throw new Error('personalCaptureChanged')
      return value
    } finally { this.busy = false }
  }
  close() {
    if (!this.alive) return
    this.alive = false; const pending = this.pending; this.pending = null
    if (pending) { clearTimeout(pending.timer); pending.reject(new Error('personalHelperUnknown')) }
    this.input.off('data',this.data); this.input.off('error',this.failed); this.input.off('end',this.failed); this.output.off('error',this.failed)
    this.terminate()
  }
}
