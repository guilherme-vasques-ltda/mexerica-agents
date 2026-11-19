/** Consumidor real PR-B: somente a geração já admitida, sem criação, SSH ou reparo. */
import { performance } from 'node:perf_hooks'
import { randomBytes } from 'node:crypto'
import { powerMonitor } from 'electron'
import { b64url, unb64url } from './crypto'
import type { Principal } from './principal'
import type { PersonalSshProfile } from './personal-ssh'
import type { PersonalLinkOwnStatus } from '../../shared/fork-cloud'
import { PersonalLinkVault, type PersonalLinkRecord } from './personal-link-vault'
import { PersonalLinkSocket } from './personal-link-socket'
import * as w from './personal-link-wire'
import * as b from './personal-link-channel-wire'
const opaque = () => randomBytes(16).toString('base64url')
const key = (prefix: 'accept' | 'persisted' | 'management', digest: w.J): PersonalLinkRecord => typeof digest === 'string' && /^[a-f0-9]{64}$/.test(digest) ? `${prefix}.${digest.slice(0, 32)}` : b.refused()
export class PersonalLinkChannel {
  private canceled: ((reason: 'refused' | 'expired') => void) | null = null
  private epoch = 0
  private wire: PersonalLinkSocket | null = null
  private vault: PersonalLinkVault | null = null
  private principal: Principal | null = null
  private cipherIn: b.Cipher | null = null
  private cipherOut: b.Cipher | null = null
  private snapshotValue: PersonalLinkOwnStatus | null = null
  private expires = 0
  private timer: ReturnType<typeof setTimeout> | null = null
  private readonly invalidate = () => this.stop()
  constructor() { powerMonitor.on('suspend', this.invalidate); powerMonitor.on('resume', this.invalidate) }
  dispose() { this.stop(); powerMonitor.off('suspend', this.invalidate); powerMonitor.off('resume', this.invalidate) }
  stop(reason: 'refused' | 'expired' = 'refused') {
    const canceled=this.canceled; this.canceled=null; canceled?.(reason)
    this.epoch++; const wire = this.wire; this.wire = null
    this.cipherIn?.dispose(); this.cipherOut?.dispose(); this.cipherIn = null; this.cipherOut = null
    this.snapshotValue = null; this.vault = null; this.principal = null; this.expires = 0
    if (this.timer) clearTimeout(this.timer); this.timer = null
    wire?.close()
  }
  snapshot(): PersonalLinkOwnStatus | null {
    try {
      if (!this.snapshotValue) return null
      if (!this.principal?.current() || performance.now() >= this.expires) { this.stop(); return null }
      this.vault?.ready()
      const value = this.snapshotValue, remaining = Math.floor(this.expires - performance.now())
      if (remaining <= 0) { this.stop(); return null }
      return value.kind === 'ownStatus' ? { ...value, validForMs: remaining } : value
    } catch { this.stop(); return null }
  }
  query(p: Principal, sourceId: string, profile: PersonalSshProfile | null): Promise<PersonalLinkOwnStatus> {
    this.stop()
    const epoch=this.epoch, context=opaque()
    const canceled=new Promise<PersonalLinkOwnStatus>(resolve => { this.canceled=reason => resolve({version:1,kind:'unavailable',context,reason}) })
    this.timer=setTimeout(()=>this.stop('expired'),30000)
    const work=this.runQuery(p,sourceId,profile,epoch,context).then(result=>{
      if(result.kind==='unavailable' && epoch===this.epoch){this.canceled=null;this.stop()}
      return result
    })
    return Promise.race([work,canceled])
  }
  private async runQuery(p: Principal, sourceId: string, profile: PersonalSshProfile | null, epoch: number, context: string): Promise<PersonalLinkOwnStatus> {
    const start = performance.now()
    const unavailable = (reason: Extract<PersonalLinkOwnStatus, { kind: 'unavailable' }>['reason']): PersonalLinkOwnStatus => ({ version: 1, kind: 'unavailable', context, reason })
    let stage: 'notLinked' | 'vaultUnavailable' | 'connectionUnavailable' | 'refused' | 'expired' = 'notLinked'
    const current = () => { if (epoch !== this.epoch || !p.current() || performance.now() - start >= 30000) b.refused() }
    let localWire: PersonalLinkSocket | null = null
    let pendingEphemeral: b.ChannelEphemeral | null = null
    try {
      current()
      if (!profile || profile.id !== sourceId) return unavailable('notLinked')
      const host=profile.host, port=profile.remotePort
      stage = 'vaultUnavailable'
      const vault = await PersonalLinkVault.reopen(p.namespace, sourceId); current()
      this.vault = vault; this.principal = p
      const read = async (name: PersonalLinkRecord) => { current(); const bytes = await vault.read(name); current(); if (!bytes) b.refused(); return w.readJ(bytes) }
      const signer = await vault.signer(false); current()
      stage = 'notLinked'
      const ra = await read('admission'), rc = await read('complete'), m = await read('management'), mr = await read('receipt')
      if (!Array.isArray(ra) || !Array.isArray(ra[0]) || !Array.isArray(rc) || !Array.isArray(rc[0])) b.refused()
      const ac = await read(key('accept', ra[0][12])), pc = await read(key('persisted', rc[0][8])), sealedM = await read(key('management', w.hashJ(m)))
      const root = b.chain([ra, rc, m, mr, ac, pc, sealedM], b64url(signer.publicKey), p.namespace)
      stage = 'connectionUnavailable'
      localWire = await PersonalLinkSocket.connect(host, port, () => { if (epoch === this.epoch) this.stop() }, () => { current(); vault.ready() }, wire => { current(); this.wire = wire }); current()
      this.wire = localWire
      stage = 'refused'
      const ch = b.challenge(w.readJ(await localWire.exchange(w.j(b.open(root)))), root); current(); vault.ready()
      const eph = pendingEphemeral = new b.ChannelEphemeral()
      const cr = b.response(signer, ch, eph); current()
      const received = await localWire.exchange(w.j(cr)); current(); vault.ready()
      const welcome = b.welcome(w.readJ(received), root, ch, cr), wp = welcome.signed[0]
      const session = wp[9] as string, transcript: w.J = [b.domain('channel-final'), 1, ch, cr, welcome.signed], th = w.hashJ(transcript)
      const shared = eph.dh(unb64url(ch[0][9], 32) ?? b.refused()); pendingEphemeral = null
      let out: b.Cipher, input: b.Cipher
      try { current(); vault.ready(); out = new b.Cipher(shared, transcript, session, 'c2h'); this.cipherOut = out; input = new b.Cipher(shared, transcript, session, 'h2c') } finally { shared.fill(0) }
      this.cipherIn = input; this.cipherOut = out
      const ack = input.read(await localWire.exchange(out.seal([b.domain('channel-finish'), 1, session, th, w.hashJ(welcome.signed)]))); current(); vault.ready()
      if (!b.same(ack, [b.domain('channel-ready'), 1, session, th])) b.refused()
      const id = opaque(), beforeQ = performance.now()
      const status = input.read(await localWire.exchange(out.seal([b.domain('link-status-request'), 1, id, session, th]))); current(); vault.ready()
      b.status(status, root, id, session, th, ch[0][6], wp[10])
      // Orçamento iniciado antes de rede/Welcome/Q: latência jamais acrescenta validade.
      this.expires = Math.min(start + Math.min(30000, welcome.ttl), beforeQ + 30000)
      if (performance.now() >= this.expires) { stage = 'expired'; b.refused() }
      const remaining = Math.floor(this.expires - performance.now())
      if (remaining <= 0) {stage='expired';b.refused()}
      const result: PersonalLinkOwnStatus = { version: 1, kind: 'ownStatus', context, hub: root.H, device: root.T, rights: ['devices.manage', 'link.connect'], state: 'active', validForMs: remaining }
      current(); this.snapshotValue = result
      if (this.timer) clearTimeout(this.timer)
      this.timer = setTimeout(() => this.stop(), Math.max(0, this.expires - performance.now()))
      // Close cifrado é opcional; o serviço conserva sessão em RAM somente durante o snapshot.
      return result
    } catch {
      pendingEphemeral?.dispose()
      if (epoch === this.epoch) { this.canceled=null; this.stop() }
      localWire?.close()
      return unavailable(p.current() ? stage : 'refused')
    }
  }
}
