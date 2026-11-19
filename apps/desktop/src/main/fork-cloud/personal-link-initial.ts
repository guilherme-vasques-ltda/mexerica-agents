/** D430 PR-A, consumidor estreito do Main: procedimento inicial pela sessão SSH PREVIAMENTE
 * ancorada (âncora D430 embutida), dois gestos separados (raiz connect-only; gestão inicial),
 * T no cofre da plataforma e recibos selados/reabertos. O renderer só envia intenções; nunca
 * bytes para assinar. Sem âncora, fonte de tempo qualificada no Hub ou cofre: FECHA. */
import { spawn } from 'node:child_process'
import { promises as fs } from 'node:fs'
import { randomBytes } from 'node:crypto'
import { join } from 'node:path'
import { app } from 'electron'
import { b64url, type DeviceSigner } from './crypto'
import { certificate, privateFile, protectedParents, type Certificate, type PersonalSshProfile } from './personal-ssh'
import { personalLinkAnchor, type PersonalLinkAnchor } from './personal-link-trust'
import { PersonalLinkVault, type PersonalLinkRecord } from './personal-link-vault'
import * as w from './personal-link-wire'
import type { Principal } from './principal'
import type { PersonalLinkState } from '../../shared/fork-cloud'

type Errors = 'clockUnqualified' | 'quarantine' | 'reopened' | 'refused'
function closed(reason: string): never { throw new Error(reason) }

/** Enquadramento u32 + J do procedimento; uma operação em voo. */
class Procedure {
  private buffer = Buffer.alloc(0)
  private pending: { id: string; op: string; resolve(v: w.J): void; reject(e: Error): void; timer: ReturnType<typeof setTimeout> } | null = null
  private alive = true
  private readonly input: NodeJS.ReadableStream
  private readonly output: NodeJS.WritableStream
  private readonly stop: () => void
  constructor(input: NodeJS.ReadableStream, output: NodeJS.WritableStream, stop: () => void) {
    this.input = input; this.output = output; this.stop = stop
    input.on('data', this.data); input.on('end', this.close); input.on('error', this.close); output.on('error', this.close)
  }
  private readonly data = (chunk: Buffer) => {
    try {
      const pending = this.pending
      if (!this.alive || !pending || this.buffer.length + chunk.length > w.LIMIT + 4) throw new Error('frame')
      this.buffer = Buffer.concat([this.buffer, chunk])
      if (this.buffer.length < 4) return
      const size = this.buffer.readUInt32BE(0)
      if (size < 1 || size > w.LIMIT || this.buffer.length > size + 4) throw new Error('frame')
      if (this.buffer.length !== size + 4) return
      const v = w.readJ(this.buffer.subarray(4)) as readonly w.J[]
      // Valida domínio/versão/id/forma/op ANTES de soltar o pedido: erro fecha e rejeita o original.
      if (v[0] !== w.D430.procedure || v[1] !== 1 || v[2] !== pending.id) throw new Error('frame')
      const refused = v.length === 5 && v[3] === 'error'
      if (!refused && !(v.length === 6 && v[3] === 'ok' && v[4] === pending.op)) throw new Error('frame')
      this.buffer = Buffer.alloc(0); this.pending = null; clearTimeout(pending.timer)
      if (refused) pending.reject(new Error(['clockUnqualified', 'quarantine', 'reopened'].includes(v[4] as string) ? `personalLink:${v[4] as Errors}` : 'personalLink:refused'))
      else pending.resolve(v[5])
    } catch { this.close() }
  }
  async call(op: string, ...args: w.J[]): Promise<w.J> {
    if (!this.alive || this.pending) closed('personalLinkUnavailable')
    const id = randomBytes(16).toString('base64url'), bytes = w.j([w.D430.procedure, 1, id, op, ...args]), head = Buffer.alloc(4)
    head.writeUInt32BE(bytes.length)
    return await new Promise<w.J>((resolve, reject) => {
      this.pending = { id, op, resolve, reject, timer: setTimeout(this.close, 20_000) }
      this.output.write(Buffer.concat([head, bytes]))
    })
  }
  readonly close = () => {
    if (!this.alive) return
    this.alive = false
    const pending = this.pending; this.pending = null
    if (pending) { clearTimeout(pending.timer); pending.reject(new Error('personalLinkUnknown')) }
    this.input.removeListener('data', this.data)
    this.stop()
  }
}

/** Sessão SSH só contra âncora já comprovada (CA embutida); sem forwarding, sem TOFU/accept-new. */
async function anchored(profile: PersonalSshProfile, anchor: Readonly<PersonalLinkAnchor>) {
  if (!/^[A-Za-z0-9_./=-]+( [A-Za-z0-9_./=-]+){0,5}$/.test(anchor.command) || anchor.host !== profile.host || anchor.port !== (profile.port ?? 22) || anchor.user !== profile.user || !profile.identityFile?.startsWith('/')) closed('personalLinkOriginUnavailable')
  const binary = await fs.lstat('/usr/bin/ssh'); if (!binary.isFile() || binary.uid !== 0 || binary.mode & 0o022) closed('personalLinkOriginUnavailable')
  await protectedParents(app.getPath('userData'))
  const dir = await fs.mkdtemp(join(app.getPath('userData'), 'personal-link-ssh-')); await fs.chmod(dir, 0o700)
  const store = join(dir, 'ca'), log = join(dir, 'diagnostic'), storeBytes = Buffer.from(`@cert-authority ${anchor.principal} ${anchor.ca}\n`)
  await fs.writeFile(store, storeBytes, { flag: 'wx', mode: 0o600 }); await fs.writeFile(log, '', { flag: 'wx', mode: 0o600 })
  const ca = await privateFile(store, 4096)
  const options = ['StrictHostKeyChecking=yes', 'HostKeyAlgorithms=ssh-ed25519-cert-v01@openssh.com', `UserKnownHostsFile=${store}`, 'GlobalKnownHostsFile=/dev/null', 'KnownHostsCommand=none', `HostKeyAlias=${anchor.principal}`, 'UpdateHostKeys=no', 'VerifyHostKeyDNS=no', 'CanonicalizeHostname=no', 'ProxyCommand=none', 'ProxyJump=none', 'ControlMaster=no', 'ControlPath=none', 'ForwardAgent=no', 'ForwardX11=no', 'ClearAllForwardings=yes', 'BatchMode=yes', 'IdentitiesOnly=yes', 'IdentityAgent=none', 'PermitLocalCommand=no', 'RequestTTY=no', 'LogLevel=DEBUG2']
  const child = spawn('/usr/bin/ssh', ['-F', '/dev/null', '-E', log, ...options.flatMap(o => ['-o', o]), '-p', String(anchor.port), '-l', anchor.user, '-i', profile.identityFile!, anchor.host, anchor.command], { stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, TZ: 'UTC' } })
  child.stderr.on('data', () => {})
  let observed: Certificate | null = null
  const stop = () => { child.kill(); void fs.rm(dir, { recursive: true, force: true }).catch(() => {}) }
  const procedure = new Procedure(child.stdout, child.stdin, stop)
  child.on('exit', procedure.close); child.on('error', procedure.close)
  /** Depois da primeira resposta: certificado do MESMO processo contra a CA ancorada. */
  const check = async () => {
    const [now, own] = await Promise.all([privateFile(store, 4096), privateFile(log, 256 * 1024)])
    if (now.dev !== ca.dev || now.ino !== ca.ino || !now.bytes.equals(storeBytes) || child.exitCode !== null) closed('personalLinkCaptureChanged')
    const cert = certificate(own.bytes.toString('ascii'), anchor, store)
    if (observed && JSON.stringify(observed) !== JSON.stringify(cert)) closed('personalLinkCaptureChanged')
    if (Date.now() >= cert.until || Date.now() >= anchor.admittedUntil) closed('personalLinkCertificateExpired')
    observed = cert
  }
  return { procedure, check, close: procedure.close }
}

type Step = { kind: 'root'; ch: w.Signed } | { kind: 'management'; payload: readonly w.J[]; ra: w.Signed }
interface Attempt { readonly id: string; readonly p: Principal; readonly gen: number; readonly vault: PersonalLinkVault; readonly signer: DeviceSigner; readonly session: Awaited<ReturnType<typeof anchored>>; step: Step | null }
const view = (a: Attempt, phase: PersonalLinkState['phase'], H: string, T: string): PersonalLinkState => ({ attemptId: a.id, phase, hub: H, hubFingerprint: w.fingerprint(H), device: T, deviceFingerprint: w.fingerprint(T) })
const key = (prefix: 'accept' | 'persisted' | 'management', digest: w.J) => typeof digest === 'string' && /^[a-f0-9]{64}$/.test(digest) ? `${prefix}.${digest.slice(0, 32)}` as const : closed('personalLinkRefused')
const payloadOf = (v: w.J): readonly w.J[] => Array.isArray(v) && Array.isArray(v[0]) ? v[0] as readonly w.J[] : closed('personalLinkRefused')

/** Captura ORIGINAL (Principal + geração) atravessa todos os awaits: antes de assinar, enviar,
 * selar e publicar estado. `stop()` encerra inclusive abertura em voo, sem repopular nada. */
export class PersonalLinkInitial {
  private attempt: Attempt | null = null
  private state: PersonalLinkState | null = null
  private gen = 0
  snapshot(): PersonalLinkState | null { return this.state }

  stop() { this.gen++; this.attempt?.session.close(); this.attempt = null; this.state = null }
  private begin() { this.gen++; this.attempt?.session.close(); this.attempt = null; this.state = null; return this.gen }
  private live(a: Attempt) { if (this.attempt !== a || a.gen !== this.gen || !a.p.current()) closed('personalLinkCaptureChanged') }
  private async call(a: Attempt, op: string, ...args: w.J[]) { this.live(a); const v = await a.session.procedure.call(op, ...args); this.live(a); return v }
  private async checked(a: Attempt) { this.live(a); await a.session.check(); this.live(a) }
  private async read(a: Attempt, name: PersonalLinkRecord) { this.live(a); const v = await a.vault.read(name); this.live(a); return v }
  private async seal(a: Attempt, name: PersonalLinkRecord, bytes: Uint8Array) { this.live(a); const v = await a.vault.seal(name, bytes); this.live(a); return v }
  private async has(a: Attempt, prefix: 'accept' | 'management') { this.live(a); const v = await a.vault.has(prefix); this.live(a); return v }
  private publish(a: Attempt, state: PersonalLinkState) { this.live(a); this.state = state }
  private finish(a: Attempt, state: PersonalLinkState) { this.live(a); a.session.close(); this.attempt = null; this.state = state }
  private fail(gen: number, a: Attempt | null, e: unknown) {
    a?.session.close()
    if (gen !== this.gen) return
    this.attempt = null; this.state = { attemptId: a?.id ?? '', phase: 'closed', reason: reason(e) }
  }

  private async open(gen: number, p: Principal, sourceId: string, profile: PersonalSshProfile | null, create: boolean): Promise<Attempt | null> {
    const anchor = personalLinkAnchor(sourceId)
    if (!p.current() || !anchor || !profile || profile.id !== sourceId) closed('personalLinkOriginUnavailable')
    const stale = () => gen !== this.gen || !p.current()
    // C: namespace selecionado aqui e autorizado pelo gesto; o login só ajuda a escolhê-lo.
    const vault = await PersonalLinkVault.open(p.namespace, anchor.qualificationId)
    if (stale()) return null
    const signer = await vault.signer(create)
    if (stale()) return null
    const session = await anchored(profile, anchor)
    if (stale()) { session.close(); return null }
    const a: Attempt = { id: randomBytes(16).toString('base64url'), p, gen, vault, signer, session, step: null }
    this.attempt = a
    return a
  }

  /** Gesto 1a. Reconcilia primeiro: admissão já commitada (inclusive resposta perdida) retoma
   * pelos MESMOS envelopes, conferidos contra o material selado antes do envio. */
  async startRoot(p: Principal, sourceId: string, profile: PersonalSshProfile | null): Promise<void> {
    const gen = this.begin()
    let a: Attempt | null = null
    try {
      a = await this.open(gen, p, sourceId, profile, true)
      if (!a) return
      const saved = await this.read(a, 'admission')
      if (saved) {
        if (await this.resume(a, w.readJ(saved) as w.Signed)) return
        a = await this.newGeneration(a)
      } else {
        const r = await this.call(a, 'reconcile', b64url(a.signer.publicKey)) as readonly w.J[]
        await this.checked(a)
        // SEG-A-R2-01: `closed` só abre geração nova depois de conferir a RA contra o aceite selado (D456).
        if (r[0] === 'closed') {
          await this.admissionObserved(a, r)
          a = await this.newGeneration(a)
        }
        else if (r[0] !== 'none') { await this.recoverAdmission(a, r); return }
        // `none` não prova que um aceite em voo nunca terá efeito: fica incerto, sem novo desafio/op/bytes;
        // só um reconcile futuro que observe o resultado exato conclui.
        else if (await this.has(a, 'accept')) closed('personalLinkUncertain')
      }
      const T = b64url(a.signer.publicKey)
      const nT = randomBytes(32).toString('base64url')
      const ch = w.checkChallenge(await this.call(a, 'root-challenge', T, p.namespace, nT), { T, C: p.namespace, nT })
      a.step = { kind: 'root', ch }
      this.publish(a, view(a, 'confirmRoot', ch[0][2] as string, T))
    } catch (e) { this.fail(gen, a, e) }
  }

  /** D456: RA pendente fechada pela reabertura do Hub. No MESMO gesto explícito do dono, geração local nova com
   * T novo (a origem antiga é imutável); os registros antigos ficam intactos e nada antigo é reenviado. */
  private async newGeneration(a: Attempt): Promise<Attempt> {
    this.live(a)
    const vault = await a.vault.next()
    this.live(a)
    const signer = await vault.signer(true)
    this.live(a)
    const b: Attempt = { ...a, vault, signer, step: null }
    this.attempt = b
    return b
  }

  /** Resultado do reconcile conferido contra challenge/accept ORIGINAIS selados: forma exata, admissionId e RA
   * (assinatura H, H/G/C/T e ligação ao aceite). Sem material ou com divergência, fecha sem efeito. */
  private async admissionObserved(a: Attempt, r: readonly w.J[]): Promise<w.Signed> {
    const arity: Readonly<Record<string, number>> = { pending: 3, closed: 3, complete: 4, managed: 7 }
    if (typeof r[0] !== 'string' || r.length !== arity[r[0]]) closed('personalLinkRefused')
    const ra = r[2] as w.Signed, sealed = await this.read(a, key('accept', payloadOf(ra)[12]))
    if (!sealed) closed('personalLinkRefused')
    const [ch, ac] = w.readJ(sealed) as readonly [w.Signed, w.Signed]
    const checked = w.checkAdmission(ra, ch, ac)
    if (r[1] !== checked[0][10]) closed('personalLinkRefused')
    return checked
  }

  private async recoverAdmission(a: Attempt, r: readonly w.J[]) {
    const checked = await this.admissionObserved(a, r)
    await this.seal(a, 'admission', w.j(checked))
    if (!await this.resume(a, checked)) closed('personalLinkRefused')
  }

  /** Gesto 1b: aceite connect-only (P0), selado ANTES do envio; RA selado e reaberto antes de PC. */
  private async confirmRoot(a: Attempt) {
    if (a.step?.kind !== 'root') closed('personalLinkUnavailable')
    const ch = a.step.ch; a.step = null
    this.live(a)
    const ac = w.accept(a.signer, ch)
    await this.seal(a, key('accept', w.hashJ(ac)), w.j([ch, ac]))
    const ra = w.checkAdmission(await this.call(a, 'root-accept', ac), ch, ac)
    await this.seal(a, 'admission', w.j(ra))
    if (!await this.resume(a, ra)) closed('personalLinkRefused')
  }

  /** RC só com o PC selado antes do envio; resposta perdida volta pelo reconcile. */
  private async complete(a: Attempt, ra: w.Signed, rc: w.J) {
    const pc = await this.read(a, key('persisted', payloadOf(rc)[8]))
    if (!pc) closed('personalLinkRefused')
    return w.checkComplete(rc, ra, w.readJ(pc) as w.Signed)
  }

  /** `false` = D456: o Hub reabriu e fechou esta RA pendente (não retoma, não reenvia nada antigo). */
  private async resume(a: Attempt, ra: w.Signed): Promise<boolean> {
    const T = b64url(a.signer.publicKey)
    if (ra[0][7] !== T) closed('personalLinkVaultMismatch')
    const saved = await this.read(a, 'complete')
    if (saved) await this.complete(a, ra, w.readJ(saved))
    else {
      const r = await this.call(a, 'reconcile', T) as readonly w.J[]
      await this.checked(a)
      if (r[0] === 'none' || !w.j(r[2]).equals(w.j(ra))) closed('personalLinkRefused')
      if (r[0] === 'closed') return false
      let rc: w.Signed
      if (r[0] === 'pending') {
        let response: w.J
        try { response = await this.call(a, 'persist-challenge', T, ra[0][10]) } catch (e) {
          if (e instanceof Error && e.message === 'personalLink:reopened') return false
          throw e
        }
        this.live(a)
        const pc = w.persisted(a.signer, ra, response)
        await this.seal(a, key('persisted', w.hashJ(pc)), w.j(pc))
        rc = w.checkComplete(await this.call(a, 'persist-confirm', pc), ra, pc)
      } else rc = await this.complete(a, ra, r[3])
      await this.seal(a, 'complete', w.j(rc))
    }
    this.finish(a, view(a, 'rootComplete', ra[0][2] as string, T))
    return true
  }

  /** Gesto 2a, separado: gestão inicial (P1, sem delegação). MR já commitado volta pelo reconcile. */
  async startManagement(p: Principal, sourceId: string, profile: PersonalSshProfile | null): Promise<void> {
    const gen = this.begin()
    let a: Attempt | null = null
    try {
      a = await this.open(gen, p, sourceId, profile, false)
      if (!a) return
      const T = b64url(a.signer.publicKey)
      const raBytes = await this.read(a, 'admission'), rcBytes = await this.read(a, 'complete')
      if (!raBytes || !rcBytes) closed('personalLinkUnavailable')
      const ra = w.readJ(raBytes) as w.Signed
      const rc = await this.complete(a, ra, w.readJ(rcBytes))
      const r = await this.call(a, 'reconcile', T) as readonly w.J[]
      await this.checked(a)
      if (r[0] === 'managed') { await this.recoverManagement(a, ra, rc, r); return }
      if (r[0] !== 'complete' || !w.j(r[2]).equals(w.j(ra)) || !w.j(r[3]).equals(w.j(rc)) || await this.read(a, 'receipt')) closed('personalLinkRefused')
      if (await this.has(a, 'management')) closed('personalLinkUncertain')
      const payload = w.checkManagement(await this.call(a, 'management-challenge', T, w.hashJ(ra)), ra)
      a.step = { kind: 'management', payload, ra }
      this.publish(a, view(a, 'confirmManagement', ra[0][2] as string, T))
    } catch (e) { this.fail(gen, a, e) }
  }

  private async recoverManagement(a: Attempt, ra: w.Signed, rc: w.Signed, r: readonly w.J[]) {
    const T = ra[0][7] as string
    if (!w.j(r[2]).equals(w.j(ra)) || !w.j(r[3]).equals(w.j(rc))) closed('personalLinkRefused')
    const m = w.verifySigned(r[4], w.D430.management, 14, T)
    const sealed = await this.read(a, key('management', w.hashJ(m)))
    if (!sealed || !sealed.equals(w.j(m))) closed('personalLinkRefused')
    w.checkManagement(m[0], ra)
    const mr = w.checkReceipt(r[5], ra, m)
    if (r[6] !== w.chainDigestT(ra, rc, m, mr)) closed('personalLinkReceiptMismatch')
    if (!await this.read(a, 'management')) await this.seal(a, 'management', w.j(m))
    if (!await this.read(a, 'receipt')) await this.seal(a, 'receipt', w.j(mr))
    this.finish(a, view(a, 'managed', ra[0][2] as string, T))
  }

  /** Gesto 2b: M selado ANTES do envio; depois M/MR finais e chainDigestT conferido com H. */
  private async confirmManagement(a: Attempt) {
    if (a.step?.kind !== 'management') closed('personalLinkUnavailable')
    const { payload, ra } = a.step; a.step = null
    this.live(a)
    const m = w.signT(a.signer, payload)
    await this.seal(a, key('management', w.hashJ(m)), w.j(m))
    const mr = w.checkReceipt(await this.call(a, 'management-accept', m), ra, m)
    const rcBytes = await this.read(a, 'complete')
    if (!rcBytes) closed('personalLinkVaultMismatch')
    const rc = await this.complete(a, ra, w.readJ(rcBytes))
    const r = await this.call(a, 'reconcile', ra[0][7]) as readonly w.J[]
    if (r[0] !== 'managed' || !w.j(r[5]).equals(w.j(mr))) closed('personalLinkReceiptMismatch')
    await this.recoverManagement(a, ra, rc, r)
  }

  /** Um gesto confirma só o passo exibido para ESTE attemptId e para o MESMO principal capturado. */
  async confirm(p: Principal, attemptId: string): Promise<void> {
    const a = this.attempt
    if (!a || a.id !== attemptId || p.namespace !== a.p.namespace || p.seal.sessionId !== a.p.seal.sessionId || p.seal.epoch !== a.p.seal.epoch) closed('personalLinkUnavailable')
    const gen = a.gen
    try {
      if (a.step?.kind === 'root') await this.confirmRoot(a)
      else await this.confirmManagement(a)
    } catch (e) { this.fail(gen, a, e) }
  }

  cancel(attemptId: string) { if (this.attempt?.id === attemptId) { const a = this.attempt; this.gen++; a.session.close(); this.attempt = null; this.state = { attemptId, phase: 'closed', reason: 'canceled' } } }
}

function reason(e: unknown): PersonalLinkState['reason'] {
  const m = e instanceof Error ? e.message : ''
  if (m === 'personalLink:clockUnqualified') return 'clockUnqualified'
  if (m === 'personalLink:quarantine') return 'quarantine'
  if (m === 'personalLinkOriginUnavailable') return 'originUnavailable'
  if (m === 'personalLinkVaultUnavailable') return 'vaultUnavailable'
  if (m === 'personalLinkUncertain') return 'uncertain'
  return 'refused'
}
