import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { registerHooks } from 'node:module'
import { test } from 'node:test'
import type { ForkBootstrapState, ForkCloudAttachment, ForkCloudIntent, ForkCloudResult, ForkCloudState } from '../../../../shared/fork-cloud.ts'

// D171: o modelo da tela e o anexo pela ponte com uma ponte falsa; o Main é só a ponte.
registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
      const url = new URL(`${specifier}.ts`, context.parentURL)
      if (existsSync(url)) return nextResolve(url.href, context)
    }
    return nextResolve(specifier, context)
  },
})

const settle = () => new Promise((resolve) => setImmediate(resolve))
const load = async () => ({ ...(await import('./fork-cloud-model.ts')), ...(await import('./fork-cloud-attachments.ts')) })

interface Deferred<T> { promise: Promise<T>; resolve: (value: T) => void; reject: (error: unknown) => void }
function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((ok, fail) => { resolve = ok; reject = fail })
  return { promise, resolve, reject }
}

const HUB = 'AAAAAAAAAAAAAAAAAAAAAA'
const ATTEMPT = 'BBBBBBBBBBBBBBBBBBBBBB'
const state = (kind: ForkCloudState['kind'], epoch: number, extra: Partial<ForkCloudState> = {}): ForkCloudState => ({ kind, epoch, attemptId: '', ...extra })
const result = (s: ForkCloudState, extra: Partial<ForkCloudResult> = {}): ForkCloudResult => ({ registration: 'registered', state: s, computers: [], ...extra })
const attachment = (connectionId: string, contextId = 'ctx-a'): ForkCloudAttachment => ({ connectionId, computerId: HUB, contextId, hostId: 'host-1', computer: { id: HUB, name: 'Hub', signKey: 'key', urls: [] } })

/** Uma ponte em que cada chamada espera a resposta do teste. */
function bridge() {
  const sent: ForkCloudIntent[] = []
  const answers: Deferred<ForkCloudResult>[] = []
  let emit: (r: ForkCloudResult) => void = () => {}
  return {
    sent, answers,
    emit: (r: ForkCloudResult) => emit(r),
    api: {
      invoke(intent: ForkCloudIntent) { sent.push(intent); const d = deferred<ForkCloudResult>(); answers.push(d); return d.promise },
      onChange(cb: (r: ForkCloudResult) => void) { emit = cb; return () => {} },
    },
  }
}

function accountFake() {
  const listeners = new Set<() => void>()
  return { sessionRevision: 1, subscribe(fn: () => void) { listeners.add(fn); return () => void listeners.delete(fn) }, bump() { this.sessionRevision++; for (const fn of listeners) fn() } }
}

test('sem clique só há leitura: nada é registrado nem reivindicado no login ou na troca de conta', async () => {
  const { ForkCloudModel } = await load()
  const b = bridge(), account = accountFake()
  const cloud = new ForkCloudModel(b.api, account, () => 'Mac')
  cloud.start(); cloud.start()
  b.answers[0].resolve(result(state('signedOut', 1), { registration: 'unregistered' }))
  await settle()
  account.bump()
  b.answers[1].resolve(result(state('signedOut', 2), { registration: 'unregistered' }))
  await settle()
  assert.deepEqual(b.sent.map((i) => i.kind), ['state', 'state'])
  assert.equal(cloud.registration, 'unregistered')
})

test('A→B→A: resposta e evento da sessão anterior não voltam; o anexo some na troca', async () => {
  const { ForkCloudModel } = await load()
  const b = bridge(), account = accountFake()
  const cloud = new ForkCloudModel(b.api, account, () => 'Mac')
  cloud.start()
  b.answers[0].resolve(result(state('ready', 1, { computerId: HUB }), { attachment: attachment('c1') }))
  await settle()
  assert.equal(cloud.readyAttachment?.connectionId, 'c1')
  const late = cloud.discover()
  account.bump()
  assert.equal(cloud.readyAttachment, null)
  // A late event must also stay fenced before the fresh state reply arrives.
  b.emit(result(state('ready', 1, { computerId: HUB }), { attachment: attachment('c1') }))
  assert.equal(cloud.readyAttachment, null)
  b.answers[1].resolve(result(state('ready', 1, { computerId: HUB }), { attachment: attachment('c1') }))
  assert.equal(await late, 'superseded')
  b.answers[2].resolve(result(state('signedOut', 3)))
  await settle()
  b.emit(result(state('ready', 1, { computerId: HUB }), { attachment: attachment('c1') }))
  assert.equal(cloud.readyAttachment, null)
  assert.equal(cloud.state?.kind, 'signedOut')
})

test('ready só conta com o anexo qualificado do mesmo Hub e contexto próprio', async () => {
  const { ForkCloudModel } = await load()
  const b = bridge()
  const cloud = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  cloud.start()
  b.answers[0].resolve(result(state('ready', 1, { computerId: HUB })))
  await settle()
  assert.equal(cloud.readyAttachment, null)
  b.emit(result(state('connecting', 1, { computerId: HUB }), { attachment: attachment('c1') }))
  assert.equal(cloud.readyAttachment, null)
  b.emit(result(state('ready', 1, { computerId: HUB }), { attachment: attachment('c1', 'local') }))
  assert.equal(cloud.readyAttachment, null)
  b.emit(result(state('ready', 1, { computerId: HUB }), { attachment: attachment('c1') }))
  assert.equal((cloud.readyAttachment as ForkCloudAttachment | null)?.contextId, 'ctx-a')
})

test('recusa mantém o estado; falha da ponte fica incerta e nada é reenviado', async () => {
  const { ForkCloudModel, NOT_SENT, UNCONFIRMED } = await load()
  const b = bridge()
  const cloud = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  cloud.start()
  b.answers[0].resolve(result(state('needsLink', 1), { computers: [{ computerId: HUB, name: 'Hub' }] }))
  await settle()
  const refused = cloud.startRemote({ computerId: HUB, name: 'Hub' })
  b.answers[1].resolve(result(state('notSent', 1), { registration: 'unavailable', computers: [], recovery: null }))
  assert.equal(await refused, 'notSent')
  assert.equal(cloud.state?.kind, 'needsLink')
  assert.equal(cloud.flow, null)
  assert.equal(cloud.notice, NOT_SENT)
  assert.equal(cloud.registration, 'registered')
  assert.deepEqual(cloud.computers, [{ computerId: HUB, name: 'Hub' }])
  const lost = cloud.registerDevice()
  b.answers[2].reject(new Error('closed'))
  assert.equal(await lost, 'unconfirmed')
  await settle()
  assert.equal(cloud.notice, UNCONFIRMED)
  assert.equal(b.sent.length, 3)
})

test('A2: confirmar só envia seis dígitos do pedido atual; vencido não sai e A1 nunca confirma', async () => {
  const { ForkCloudModel } = await load()
  const b = bridge()
  const cloud = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  cloud.start()
  b.answers[0].resolve(result(state('needsLink', 1), { computers: [{ computerId: HUB, name: 'Hub' }] }))
  await settle()
  const start = cloud.startRemote({ computerId: HUB, name: 'Hub' })
  b.answers[1].resolve(result(state('linking', 1, { attemptId: ATTEMPT, computerId: HUB, deadline: Date.now() + 60_000 })))
  await start
  assert.equal(await cloud.confirm('123456'), 'notSent')
  b.emit(result(state('waitingOwnerApproval', 1, { attemptId: ATTEMPT, computerId: HUB, code: '654321', deadline: Date.now() + 60_000 })))
  assert.equal(await cloud.confirm('12345'), 'notSent')
  const sent = cloud.confirm('123456')
  b.answers[2].resolve(result(state('waitingOwnerApproval', 1, { attemptId: ATTEMPT, computerId: HUB, code: '654321', deadline: Date.now() + 60_000 })))
  assert.equal(await sent, 'accepted')
  assert.deepEqual(b.sent[2], { kind: 'confirm', attemptId: ATTEMPT, code: '123456' })
  assert.equal(cloud.compared, ATTEMPT)
  b.emit(result(state('waitingOwnerApproval', 1, { attemptId: 'CCCCCCCCCCCCCCCCCCCCCC', computerId: HUB, code: '111111', deadline: Date.now() - 1 })))
  assert.equal(cloud.compared, null)
  assert.equal(await cloud.confirm('111111'), 'expired')
  assert.equal(b.sent.length, 3)

  const local = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  local.start()
  b.answers[3].resolve(result(state('needsLink', 1)))
  await settle()
  const begun = local.startLocal('profile', HUB, 'Hub', false)
  b.answers[4].resolve(result(state('waitingOwnerApproval', 1, { attemptId: ATTEMPT, computerId: HUB, code: '654321', deadline: Date.now() + 60_000 })))
  await begun
  assert.deepEqual(b.sent[4], { kind: 'startLocal', profileId: 'profile', claim: false })
  assert.equal(await local.confirm('654321'), 'notSent')
  assert.equal(b.sent.length, 5)
})

test('anexo: SSH primeiro, só a própria loja sai, e outro canal recria a loja', async () => {
  const { CloudAttachmentBinder } = await load()
  let ready: ForkCloudAttachment | null = attachment('c1')
  const stores = new Map<string, object>()
  const appListeners = new Set<() => void>(), sourceListeners = new Set<() => void>()
  const detached: string[] = []
  const app = {
    store: (id: string) => stores.get(id) ?? null,
    attach: (computer: { id: string }, store: object) => { stores.set(computer.id, store) },
    detach: (id: string) => { detached.push(id); stores.delete(id) },
    subscribe: (fn: () => void) => { appListeners.add(fn); return () => void appListeners.delete(fn) },
  }
  const source = { get readyAttachment() { return ready }, subscribe: (fn: () => void) => { sourceListeners.add(fn); return () => void sourceListeners.delete(fn) } }
  const made: { connectionId: string; contextId: string }[] = []
  const ssh = { kind: 'ssh' }
  stores.set(HUB, ssh)
  const binder = new CloudAttachmentBinder(app, source, (a) => { const s = { connectionId: a.connectionId, contextId: a.contextId }; made.push(s); return s })
  const stop = binder.start()
  assert.equal(made.length, 0)
  assert.equal(stores.get(HUB), ssh)

  stores.delete(HUB); binder.sync()
  assert.deepEqual(made, [{ connectionId: 'c1', contextId: 'ctx-a' }])
  const first = stores.get(HUB)

  stores.set(HUB, ssh); binder.sync()
  ready = attachment('c2'); binder.sync()
  assert.equal(stores.get(HUB), ssh)
  assert.deepEqual(detached, [])

  stores.delete(HUB); binder.sync()
  assert.equal(made.at(-1)?.connectionId, 'c2')
  assert.notEqual(stores.get(HUB), first)
  ready = attachment('c3'); binder.sync()
  assert.deepEqual(detached, [HUB])
  assert.equal(made.at(-1)?.connectionId, 'c3')

  stop()
  assert.deepEqual(detached, [HUB, HUB])
  stores.set(HUB, ssh); binder.sync()
  assert.equal(stores.get(HUB), ssh)
})

test('MX-217 anexo: canal caído e Main reconectando segura a loja; canal novo ao mesmo Hub a retoma; definitiva solta', async () => {
  const { CloudAttachmentBinder } = await load()
  const { LinkTrocavel, LojaSegura } = await import('../../../store/fork-queda-curta.ts')
  let ready: ForkCloudAttachment | null = attachment('c1')
  let temporaria = true
  const stores = new Map<string, object>()
  const appListeners = new Set<() => void>(), sourceListeners = new Set<() => void>()
  const detached: string[] = []
  const app = {
    store: (id: string) => stores.get(id) ?? null,
    attach: (computer: { id: string }, store: object) => { stores.set(computer.id, store) },
    detach: (id: string) => { detached.push(id); stores.delete(id) },
    subscribe: (fn: () => void) => { appListeners.add(fn); return () => void appListeners.delete(fn) },
  }
  const source = { get readyAttachment() { return ready }, subscribe: (fn: () => void) => { sourceListeners.add(fn); return () => void sourceListeners.delete(fn) } }
  const parado = { call: async () => 'parado', stream: () => () => {} }
  type Loja = { link: InstanceType<typeof LinkTrocavel<string>>; log: string[]; restartStream(): void; marcarQueda(d: number | null): void }
  const made: Loja[] = []
  const timers: (() => void)[] = []
  const relogio = { agora: () => 1, depois: (fn: () => void) => { timers.push(fn); return timers.length }, cancelar: (id: unknown) => { timers[(id as number) - 1] = () => {} } }
  const binder = new CloudAttachmentBinder<Loja>(app, source, (a) => {
    const loja: Loja = { link: new LinkTrocavel<string>(a.connectionId, (id) => ({ call: async () => id, stream: () => () => {} }), parado, () => Object.assign(new Error('caído'), { status: 0 }) as never), log: [],
      restartStream() { this.log.push('restart') }, marcarQueda(d) { this.log.push(`queda:${d}`) } }
    made.push(loja)
    return loja
  }, { segura: (loja) => new LojaSegura(loja, loja.link, relogio), temporaria: () => temporaria })
  const stop = binder.start()
  const first = stores.get(HUB) as Loja
  assert.equal(made.length, 1)

  ready = null; binder.sync()
  assert.equal(stores.get(HUB), first)
  assert.equal(first.link.vivo, false)
  assert.deepEqual(detached, [])

  ready = attachment('c2'); binder.sync()
  assert.equal(stores.get(HUB), first)
  assert.equal(await first.link.transporte().call('x', {}, 1), 'c2')
  assert.deepEqual(first.log, ['restart', 'queda:1', 'queda:null', 'restart'])
  assert.equal(made.length, 1)

  // Outro contexto (conta) não reaproveita a loja.
  ready = attachment('c3', 'ctx-b'); binder.sync()
  assert.deepEqual(detached, [HUB])
  assert.equal(made.length, 2)

  // Em queda, o Main desiste (acesso encerrado): solta na hora, sem esperar o prazo.
  ready = null; binder.sync()
  assert.equal(stores.has(HUB), true)
  temporaria = false; binder.sync()
  assert.deepEqual(detached, [HUB, HUB])

  // Fim do prazo: solta.
  temporaria = true; ready = attachment('c4'); binder.sync()
  ready = null; binder.sync()
  timers.at(-1)!()
  assert.deepEqual(detached, [HUB, HUB, HUB])
  stop()
})

test('registro incerto: só o clique confere, sem registrar de novo; recusa não vira registrado', async () => {
  const { ForkCloudModel, NO_REGISTRATION } = await load()
  const b = bridge(), account = accountFake()
  const cloud = new ForkCloudModel(b.api, account, () => 'Mac')
  cloud.start()
  b.answers[0].resolve(result(state('unknown', 1), { registration: 'unregistered', recovery: 'registration' }))
  await settle()
  assert.equal(cloud.recovery, 'registration')
  const refused = cloud.reconcileRegistration()
  b.answers[1].resolve(result(state('notSent', 1), { registration: 'unregistered', recovery: 'registration' }))
  assert.equal(await refused, 'notSent')
  assert.equal(cloud.registration, 'unregistered')
  assert.equal(cloud.state?.kind, 'unknown')
  assert.equal(cloud.recovery, 'registration')
  assert.equal(cloud.notice, NO_REGISTRATION)
  const checked = cloud.reconcileRegistration()
  b.answers[2].resolve(result(state('needsLink', 1), { registration: 'registered', recovery: null }))
  assert.equal(await checked, 'accepted')
  assert.equal(cloud.registration, 'registered')
  assert.equal(cloud.recovery, null)
  b.emit(result(state('unknown', 1), { registration: 'unregistered', recovery: 'registration' }))
  account.bump()
  assert.equal(cloud.recovery, null)
  assert.deepEqual(b.sent.map((i) => i.kind), ['state', 'reconcileRegistration', 'reconcileRegistration', 'state'])
})


test('pin pendente após reinício conserva alvo/fase e só envia a conferência escolhida', async () => {
  const { ForkCloudModel } = await load()
  const b = bridge(), account = accountFake()
  const cloud = new ForkCloudModel(b.api, account, () => 'Mac')
  cloud.start()
  b.answers[0].resolve(result(state('unknown', 1, { computerId: HUB }), { recovery: 'enrollment', recoveryPhase: 'publication' }))
  await settle()
  assert.equal(cloud.currentAttempt, null); assert.equal(cloud.recoveryPhase, 'publication')
  const checking = cloud.reconcilePin()
  assert.deepEqual(b.sent[1], { kind: 'reconcilePin', computerId: HUB })
  b.answers[1].resolve(result(state('linked', 1, { computerId: HUB })))
  assert.equal(await checking, 'accepted'); assert.equal(cloud.state?.kind, 'linked')
  assert.equal(cloud.recoveryPhase, null)
  assert.deepEqual(b.sent.map(x => x.kind), ['state', 'reconcilePin'])
})


test('conferir posse envia somente a intenção de leitura, sem outro claim ou registro', async () => {
  const { ForkCloudModel } = await load()
  const b = bridge(), cloud = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  cloud.start()
  b.answers[0].resolve(result(state('unknown', 1, { computerId: HUB }), { recovery: 'enrollment', recoveryPhase: 'claim' }))
  await settle()
  const checking = cloud.reconcileClaim()
  assert.deepEqual(b.sent[1], { kind: 'reconcileClaim' })
  b.answers[1].resolve(result(state('needsLink', 1, { computerId: HUB })))
  assert.equal(await checking, 'accepted'); assert.equal(cloud.recovery, null)
  assert.deepEqual(b.sent.map(x => x.kind), ['state', 'reconcileClaim'])
})


const BOOT = 'DDDDDDDDDDDDDDDDDDDDDD'
const boot = (phase: ForkBootstrapState['phase'], extra: Partial<ForkBootstrapState> = {}): ForkBootstrapState =>
  ({ bootstrapId: BOOT, phase, canStart: false, canCheck: false, canProve: false, canReconcile: false, ...extra })

test('ACS4: cada passo só pelo clique e pelo can* do Main, com o bootstrapId devolvido; incerto não repete', async () => {
  const { ForkCloudModel, BOOTSTRAP_SAME_STEP } = await load()
  const b = bridge(), account = accountFake()
  const cloud = new ForkCloudModel(b.api, account, () => 'Mac')
  cloud.start()
  b.answers[0].resolve(result(state('needsLink', 1), { bootstrap: boot('available', { bootstrapId: '', canStart: true }) }))
  await settle()
  assert.equal(cloud.bootstrapBlocks, false)
  assert.equal(await cloud.checkBootstrap(), 'notSent')
  const started = cloud.startBootstrap()
  assert.deepEqual(b.sent[1], { kind: 'startBootstrap' })
  b.answers[1].resolve(result(state('needsLink', 1), { bootstrap: boot('requested', { canCheck: true, computerId: HUB, deadline: Date.now() + 60_000 }) }))
  assert.equal(await started, 'accepted')
  // Sem `blocksOtherOperations`, um pedido aberto falha fechado.
  assert.equal(cloud.bootstrapBlocks, true)
  // Sem autorização não há prova; o legado conflitante nem sai desta janela.
  assert.equal(await cloud.proveBootstrap(), 'notSent')
  assert.equal(await cloud.discover(), 'notSent')
  assert.equal(await cloud.connect(HUB, 'Hub'), 'notSent')
  const refused = cloud.checkBootstrap()
  assert.deepEqual(b.sent[2], { kind: 'checkBootstrap', bootstrapId: BOOT })
  b.answers[2].resolve(result(state('notSent', 1), { bootstrap: boot('requested', { canCheck: true, canProve: true }) }))
  assert.equal(await refused, 'notSent')
  assert.equal(cloud.bootstrap?.canProve, false)
  const same = cloud.checkBootstrap()
  b.answers[3].resolve(result(state('needsLink', 1), { bootstrap: boot('requested', { canCheck: true, canProve: true, computerId: HUB }) }))
  assert.equal(await same, 'accepted')
  assert.equal(cloud.notice, BOOTSTRAP_SAME_STEP)
  const proved = cloud.proveBootstrap()
  assert.deepEqual(b.sent[4], { kind: 'proveBootstrap', bootstrapId: BOOT })
  b.answers[4].resolve(result(state('needsLink', 1, { attemptId: BOOT, computerId: HUB }), { bootstrap: boot('proofUnknown', { blocksOtherOperations: true, canCheck: true, computerId: HUB }) }))
  assert.equal(await proved, 'accepted')
  assert.equal(await cloud.proveBootstrap(), 'notSent')
  assert.equal(await cloud.reconcileBootstrap(), 'notSent')
  b.emit(result(state('linked', 1, { attemptId: BOOT, computerId: HUB }), { bootstrap: boot('complete', { canCheck: true, computerId: HUB }) }))
  assert.equal(cloud.bootstrapBlocks, false)
  assert.equal(cloud.bootstrapLinked, HUB)
  assert.equal(cloud.currentAttempt, null)
  account.bump()
  assert.equal(cloud.bootstrap, null)
  assert.deepEqual(b.sent.map((i) => i.kind), ['state', 'startBootstrap', 'checkBootstrap', 'checkBootstrap', 'proveBootstrap', 'state'])
})

test('ACS4: só o false explícito do Main libera as outras ações; vencido não recomeça; preparado reenvia só pelo clique', async () => {
  const { ForkCloudModel, BOOTSTRAP_NO_EFFECT } = await load()
  const b = bridge()
  const expired = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  expired.start()
  b.answers[0].resolve(result(state('needsLink', 1), { bootstrap: boot('requested', { canCheck: true, deadline: Date.now() - 1 }) }))
  await settle()
  // O relógio da tela não libera nada: sem o campo, o pedido aberto continua bloqueando.
  assert.equal(expired.bootstrapBlocks, true)
  assert.equal(await expired.discover(), 'notSent')
  b.emit(result(state('needsLink', 1), { bootstrap: boot('requested', { blocksOtherOperations: false, canCheck: true, deadline: Date.now() - 1 }) }))
  assert.equal(expired.bootstrapBlocks, false)
  assert.equal(expired.bootstrapOpen, true)
  assert.equal(await expired.startBootstrap(), 'notSent')
  assert.equal(await expired.proveBootstrap(), 'notSent')
  const found = expired.discover()
  assert.deepEqual(b.sent[1], { kind: 'discover' })
  b.answers[1].resolve(result(state('noHub', 1), { bootstrap: boot('requested', { blocksOtherOperations: false, canCheck: true, deadline: Date.now() - 1 }) }))
  assert.equal(await found, 'accepted')

  const prepared = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  prepared.start()
  b.answers[2].resolve(result(state('needsLink', 1), { bootstrap: boot('prepared', { blocksOtherOperations: false, canStart: true, readUnavailable: true }) }))
  await settle()
  assert.equal(prepared.bootstrapBlocks, false)
  const again = prepared.startBootstrap()
  assert.deepEqual(b.sent[3], { kind: 'startBootstrap' })
  b.answers[3].resolve(result(state('needsLink', 1, { attemptId: BOOT }), { bootstrap: boot('prepared', { blocksOtherOperations: false, canStart: true, readUnavailable: true }) }))
  assert.equal(await again, 'accepted')
  assert.equal(prepared.notice, BOOTSTRAP_NO_EFFECT)
  assert.deepEqual(b.sent.map((i) => i.kind), ['state', 'discover', 'state', 'startBootstrap'])
})

test('ACS4: confirmação incerta só pelo clique e canReconcile; pré-prova vencida liberada só pelo false do Main; proofUnknown bloqueia', async () => {
  const { ForkCloudModel, BOOTSTRAP_SAME_STEP } = await load()
  const b = bridge()
  const uncertain = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  uncertain.start()
  b.answers[0].resolve(result(state('needsLink', 1), { bootstrap: boot('reconcileUnknown', { blocksOtherOperations: true, canCheck: true, canReconcile: true, computerId: HUB }) }))
  await settle()
  assert.equal(await uncertain.discover(), 'notSent')
  const again = uncertain.reconcileBootstrap()
  assert.deepEqual(b.sent[1], { kind: 'reconcileBootstrap', bootstrapId: BOOT })
  b.answers[1].resolve(result(state('needsLink', 1, { attemptId: BOOT, computerId: HUB }), { bootstrap: boot('reconcileUnknown', { blocksOtherOperations: true, canCheck: true, canReconcile: true, computerId: HUB }) }))
  assert.equal(await again, 'accepted')
  assert.equal(uncertain.notice, BOOTSTRAP_SAME_STEP)

  const lapsed = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  lapsed.start()
  b.answers[2].resolve(result(state('needsLink', 1), { bootstrap: boot('challenged', { blocksOtherOperations: false, canCheck: true, deadline: Date.now() - 1 }) }))
  await settle()
  assert.equal(lapsed.bootstrapBlocks, false)
  assert.equal(await lapsed.proveBootstrap(), 'notSent')
  assert.equal(await lapsed.reconcileBootstrap(), 'notSent')
  const found = lapsed.discover()
  assert.deepEqual(b.sent[3], { kind: 'discover' })
  b.answers[3].resolve(result(state('noHub', 1), { bootstrap: boot('challenged', { blocksOtherOperations: false, canCheck: true, deadline: Date.now() - 1 }) }))
  assert.equal(await found, 'accepted')

  // A prova pode ter saído: prazo vencido no relógio não libera nada.
  const proof = new ForkCloudModel(b.api, accountFake(), () => 'Mac')
  proof.start()
  b.answers[4].resolve(result(state('needsLink', 1), { bootstrap: boot('proofUnknown', { blocksOtherOperations: true, canCheck: true, deadline: Date.now() - 1 }) }))
  await settle()
  assert.equal(proof.bootstrapBlocks, true)
  assert.equal(await proof.discover(), 'notSent')
  assert.equal(await proof.reconcileBootstrap(), 'notSent')
  assert.deepEqual(b.sent.map((i) => i.kind), ['state', 'reconcileBootstrap', 'state', 'discover', 'state'])
})

test('ACS4: encerrado é só histórico, sem ação nem vínculo; só o false explícito do Main libera os outros Hubs', async () => {
  const { ForkCloudModel } = await load()
  const b = bridge(), account = accountFake()
  const cloud = new ForkCloudModel(b.api, account, () => 'Mac')
  cloud.start()
  // Sem o campo, com prazo vencido e can* contraditórios: nada é oferecido e continua fechado.
  b.answers[0].resolve(result(state('needsLink', 1, { attemptId: BOOT, computerId: HUB }), { bootstrap: boot('closed', { terminalReason: 'expired', canStart: true, canCheck: true, canProve: true, canReconcile: true, computerId: HUB, deadline: Date.now() - 1 }) }))
  await settle()
  assert.equal(cloud.bootstrap?.bootstrapId, BOOT)
  assert.equal(cloud.bootstrap?.terminalReason, 'expired')
  assert.equal(cloud.bootstrapClosed, true)
  assert.equal(cloud.bootstrapOpen, false)
  assert.equal(cloud.bootstrapBlocks, true)
  assert.equal(cloud.bootstrapLinked, null)
  assert.equal(await cloud.startBootstrap(), 'notSent')
  assert.equal(await cloud.checkBootstrap(), 'notSent')
  assert.equal(await cloud.reconcileBootstrap(), 'notSent')
  assert.equal(await cloud.discover(), 'notSent')
  assert.equal(await cloud.connect(HUB, 'Hub'), 'notSent')
  b.emit(result(state('needsLink', 1, { attemptId: BOOT, computerId: HUB }), { bootstrap: boot('closed', { blocksOtherOperations: false, terminalReason: 'expired', computerId: HUB }) }))
  assert.equal(cloud.bootstrapBlocks, false)
  assert.equal(cloud.bootstrapHub, null)
  assert.equal(await cloud.checkBootstrap(), 'notSent')
  const found = cloud.discover()
  assert.deepEqual(b.sent[1], { kind: 'discover' })
  b.answers[1].resolve(result(state('noHub', 1), { bootstrap: boot('closed', { blocksOtherOperations: false, terminalReason: 'expired', computerId: HUB }) }))
  assert.equal(await found, 'accepted')
  account.bump()
  assert.equal(cloud.bootstrap, null)
  assert.deepEqual(b.sent.map((i) => i.kind), ['state', 'discover', 'state'])
})

test('R1-01 do #596: sem nome real, connect guarda null; o nome provisório só existe ao desenhar, no idioma de agora', async () => {
  const { ForkCloudModel } = await load()
  const b = bridge(), account = accountFake()
  const cloud = new ForkCloudModel(b.api, account, () => 'Mac')
  cloud.start()
  b.answers[0].resolve(result(state('linked', 1, { computerId: HUB })))
  await settle()
  const going = cloud.connect(HUB, null)
  assert.deepEqual(b.sent.at(-1), { kind: 'connect', computerId: HUB })
  assert.deepEqual(cloud.flow, { kind: 'connect', computerId: HUB, name: null })
  // Nada guardado vira "Your Hub"/"Seu Hub": quem desenha resolve `nuvem.seuHub` no idioma atual.
  assert.equal(cloud.nameOf(HUB), null)
  b.answers.at(-1)!.resolve(result(state('connecting', 1, { computerId: HUB }), { computers: [{ computerId: HUB, name: 'Estúdio' }] }))
  await going
  // Com o nome real do diretório, ele vale mesmo com o fluxo sem nome.
  assert.equal(cloud.nameOf(HUB), 'Estúdio')
})
