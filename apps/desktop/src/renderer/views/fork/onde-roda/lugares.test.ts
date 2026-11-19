import assert from 'node:assert/strict'
import test from 'node:test'
import { decodeGuideReply, decodeManifest, decodeReply, methods, type Intent, type Operation, type Place, type Reply, type WorkplaceGuide } from '../../../../shared/fork-workplaces.ts'
import { Workplaces, type Source } from '../../../store/fork-workplaces.ts'
import { createPlaceDraft, reconcilePlaceDraft } from './formulario.ts'
import { FakeWorkplaces } from '../../../demo/fake-workplaces.ts'
import { placeFocusScope, trackStepFocus } from './foco.ts'
import { GuideReading, type GuideContent } from './guia-local.ts'

const place: Place = { id: 'place-fixture', hubId: 'hub-a', resourceId: 'bx_fixture', name: 'Oficina', kind: 'cloud', provider: 'boat',
  revision: 1, admissionRevision: 2, connectionRevision: 3, capacityRevision: 0, prepared: true, pinned: true, connected: true, helpers: 'indisponivel' }
const operation: Operation = { id: 'op-fixture', placeId: place.id, hubId: 'hub-a', authority: 'local', intent: 'intent-fixture', admissionRevision: 2, connectionRevision: 3,
  deadline: 60_000, observedAt: null, phase: 'verificando', reason: null, observation: { connection: 'nao_verificado', identity: 'nao_verificado', preparation: 'nao_verificado', account: 'nao_verificado', helpers: 'indisponivel' } }
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((r, fail) => { resolve = r; reject = fail })
  return { promise, resolve, reject }
}
class Fixture implements Source {
  hello: { hostId: string } | null = { hostId: 'hub-a' }
  principalEpoch = 1
  listeners = new Set<() => void>()
  handler: (method: string, body: unknown) => Promise<unknown> = async (method) => ({ hubId: 'hub-a', ...(method === 'donnaWorkplaceAccess'
    ? { available: true, origin: 'local', helpers: 'indisponivel' } : { places: [place], operations: [] }) })
  connectedClient: Source['connectedClient'] = { call: <T>(method: string, body: unknown) => this.handler(method, body) as Promise<T> }
  subscribe(listener: () => void) { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  changed() { for (const listener of this.listeners) listener() }
  disconnect() { this.connectedClient = null; this.changed() }
}
test('resposta de outro Hub e enums/campos extras não entram no estado privado', () => {
  assert.throws(() => decodeReply({ hubId: 'hub-b', places: [] }, 'hub-a'))
  assert.throws(() => decodeReply({ hubId: 'hub-a', places: [{ ...place, endpoint: 'forbidden' }] }, 'hub-a'))
  assert.throws(() => decodeReply({ hubId: 'hub-a', operation: { ...operation, observation: { ...operation.observation, account: 'connected' } } }, 'hub-a'))
  assert.throws(() => decodeReply({ hubId: 'hub-a', operation: { ...operation, observation: { ...operation.observation, helpers: 'ready' } } }, 'hub-a'))
  assert.throws(() => decodeManifest({ schema: 1, protocol: 1, journal: 1, os: 'linux', arch: 'x86_64', commit: 'a'.repeat(40), sha256: 'b'.repeat(64), size: 1, libc: 'fixture', driver: { kind: 'shell', command: 'forbidden' } }))
})
test('resposta tardia do cadastro não preenche outra conexão nem conserva chaves públicas', async () => {
  const source = new Fixture(); const model = new Workplaces(source)
  await model.load()
  const late = deferred<Reply>()
  source.handler = () => late.promise
  const preparing = model.mutate({ type: 'prepare_connection', rotate: false }, place, '55555555-5555-4555-8555-555555555555')
  source.disconnect()
  late.resolve({ hubId: 'hub-a', place, publicMaterial: { hubId: 'hub-a', placeId: place.id, resourceId: place.resourceId, runnerPort: 47631,
    sshPublic: 'ssh-ed25519 fictional', controlPublic: 'A'.repeat(43), runnerPin: null, manifest: null } })
  assert.equal(await preparing, null)
  assert.equal(model.material, null); assert.deepEqual(model.places, []); assert.equal(model.available, false)
  model.dispose()
})
test('mesmo objeto de transporte com novo hello/epoch invalida resposta antiga', async () => {
  const source = new Fixture(); const model = new Workplaces(source); await model.load()
  const late = deferred<Reply>(); source.handler = () => late.promise
  const changing = model.mutate({ type: 'rename', name: 'Novo' }, place, '66666666-6666-4666-8666-666666666666')
  source.hello = { hostId: 'hub-b' }; source.principalEpoch++; source.changed()
  late.resolve({ hubId: 'hub-a', place: { ...place, name: 'Novo' } })
  assert.equal(await changing, null); assert.deepEqual(model.places, []); assert.equal(model.operation, null)
  model.dispose()
})
test('cancelamento não é sobrescrito por poll anterior e API nunca vira assinatura', async () => {
  const source = new Fixture(); const model = new Workplaces(source); await model.load()
  model.operations = [operation]
  const poll = deferred<Reply>()
  source.handler = (method) => method === 'donnaWorkplaceInspectStatus' ? poll.promise : Promise.resolve({ hubId: 'hub-a', operation: { ...operation, phase: 'cancelada_incerta' } })
  const pending = model.status(operation)
  await model.cancel(operation)
  poll.resolve({ hubId: 'hub-a', operation: { ...operation, phase: 'concluida', observation: { ...operation.observation, account: 'api_nao_admitida' } } })
  assert.equal(await pending, null); assert.equal(model.operationFor(place.id)?.phase, 'cancelada_incerta')
  const response = decodeReply({ hubId: 'hub-a', operation: { ...operation, observation: { ...operation.observation, account: 'api_nao_admitida' } } }, 'hub-a')
  assert.equal(response.operation?.observation.account, 'api_nao_admitida')
  model.dispose()
})
test('erro de poll antigo após rotação não apaga cadastro ou públicas da revisão atual', async () => {
  const source = new Fixture(); const model = new Workplaces(source)
  try {
    await model.load()
    model.operation = operation; model.operations = [operation]
    const poll = deferred<Reply>()
    const rotated = { ...place, admissionRevision: place.admissionRevision + 1 }
    const material = { hubId: place.hubId, placeId: place.id, resourceId: place.resourceId, runnerPort: 47631 as const,
      sshPublic: 'ssh-ed25519 fictional-new', controlPublic: 'B'.repeat(43), runnerPin: null, manifest: null }
    source.handler = async (method) => {
      if (method === 'donnaWorkplaceInspectStatus') return poll.promise
      assert.equal(method, 'donnaWorkplacePrepareConnection')
      return { hubId: place.hubId, place: rotated, publicMaterial: material }
    }
    const generation = model.generation
    const pending = model.status(operation)
    const reply = await model.mutate({ type: 'prepare_connection', rotate: true }, place)
    assert.ok(reply?.publicMaterial)
    assert.equal(model.places[0].admissionRevision, rotated.admissionRevision)
    assert.equal(model.material, reply.publicMaterial)
    assert.equal(model.operationFor(place.id), null)
    const currentPlaces = model.places
    const currentMaterial = model.material
    poll.reject(Error('operation_invalidated'))
    assert.equal(await pending, null)
    assert.equal(model.available, true)
    assert.equal(model.generation, generation)
    assert.equal(model.places, currentPlaces)
    assert.equal(model.material, currentMaterial)
    assert.equal(model.error, null)
    assert.equal(model.busy, false)
    assert.equal(model.operation, null)
    assert.deepEqual(model.operations, [])
  } finally { model.dispose() }
})
test('erro de poll da operação ainda atual recusa e apaga projeções', async () => {
  const source = new Fixture(); const model = new Workplaces(source)
  try {
    await model.load()
    model.operation = operation; model.operations = [operation]
    model.material = { hubId: place.hubId, placeId: place.id, resourceId: place.resourceId, runnerPort: 47631,
      sshPublic: 'ssh-ed25519 fictional', controlPublic: 'A'.repeat(43), runnerPin: null, manifest: null }
    const poll = deferred<Reply>()
    source.handler = (method) => { assert.equal(method, 'donnaWorkplaceInspectStatus'); return poll.promise }
    const pending = model.status(operation)
    assert.equal(model.operationFor(place.id)?.phase, 'verificando')
    poll.reject(Error('operation_invalidated'))
    assert.equal(await pending, null)
    assert.equal(model.available, false)
    assert.deepEqual(model.places, [])
    assert.deepEqual(model.operations, [])
    assert.equal(model.material, null)
    assert.equal(model.operation, null)
    assert.equal(model.error, 'respostaInvalida')
  } finally { model.dispose() }
})
test('recusa de capacidade apaga projeções e não sugere elevar Control', async () => {
  const source = new Fixture(); const model = new Workplaces(source); await model.load()
  model.operations = [operation]
  source.handler = async () => { throw Error('origin_not_admitted') }
  await model.load()
  assert.equal(model.available, false); assert.deepEqual(model.operations, []); assert.deepEqual(model.places, [])
  assert.equal(model.error, 'soDireto')
  model.dispose()
})
test('Hub sem o recurso: método desconhecido ao abrir vira semRecurso, não erro de cadastro', async () => {
  const source = new Fixture(); const model = new Workplaces(source)
  source.handler = async () => { throw Error('unknown_method') }
  await model.load()
  assert.equal(model.available, false); assert.equal(model.error, 'semRecurso')
  model.dispose()
})
test('Access válido + List com timeout ou falha interna não vira semRecurso', async () => {
  for (const falha of ['Request timed out', 'workplace_request_failed']) {
    const source = new Fixture(); const model = new Workplaces(source)
    const normal = source.handler
    source.handler = async (method, body) => { if (method === 'donnaWorkplaceList') throw Error(falha); return normal(method, body) }
    await model.load()
    assert.equal(model.available, false); assert.equal(model.error, 'generico', falha)
    model.dispose()
  }
})
test('página desconectada não cria gerações ou recarregamentos sem nova conexão', async () => {
  const source = new Fixture(); source.connectedClient = null; source.hello = null
  const model = new Workplaces(source)
  const generation = model.generation
  const version = model.getVersion()
  await model.load(); await model.load(); source.changed()
  assert.equal(model.generation, generation); assert.equal(model.getVersion(), version)
  assert.equal(model.available, false); assert.deepEqual(model.places, [])
  model.dispose()
})
test('PinRunner e Refix reconciliam apenas o passo alterado e invalidam confirmação do CAS anterior', () => {
  const original = { ...createPlaceDraft(place), name: 'Nome ainda em edição', pin: 'pin anterior', manifest: 'manifesto anterior',
    host: '192.0.2.10', port: '2222', known: 'linha pública em edição',
    confirmation: { intent: { type: 'remove' as const }, target: { ...place } } }
  const pinned = { ...place, admissionRevision: place.admissionRevision + 1 }
  const afterPin = reconcilePlaceDraft(original, pinned, null)
  assert.equal(afterPin.name, original.name)
  assert.equal(afterPin.host, original.host); assert.equal(afterPin.port, original.port); assert.equal(afterPin.known, original.known)
  assert.equal(afterPin.pin, ''); assert.equal(afterPin.manifest, ''); assert.equal(afterPin.confirmation, null)
  const editingPin = { ...afterPin, pin: 'nova pública em edição', manifest: 'novo manifesto em edição' }
  const fixed = { ...pinned, connectionRevision: pinned.connectionRevision + 1 }
  const afterRefix = reconcilePlaceDraft(editingPin, fixed, null)
  assert.equal(afterRefix.name, original.name)
  assert.equal(afterRefix.pin, editingPin.pin); assert.equal(afterRefix.manifest, editingPin.manifest)
  assert.equal(afterRefix.host, ''); assert.equal(afterRefix.port, '22'); assert.equal(afterRefix.known, '')
  const otherHub = reconcilePlaceDraft(editingPin, { ...pinned, hubId: 'hub-b' }, null)
  assert.equal(otherHub.name, place.name); assert.equal(otherHub.pin, ''); assert.equal(otherHub.manifest, ''); assert.equal(otherHub.host, '')
})
test('reabrir a demo recupera só a consulta terminal mais recente sem perder consulta por ID', async (context) => {
  context.mock.timers.enable({ apis: ['setTimeout', 'Date'], now: 1_700_000_000_000 })
  const fake = new FakeWorkplaces(() => 'hub-a')
  fake.enabled = true
  const source = new Fixture()
  source.handler = (method, body) => fake.call(method, body as Record<string, unknown>)
  const model = new Workplaces(source)
  await model.load()
  await model.mutate({ type: 'create', name: 'Oficina', resource_id: 'bx_fixture' }, null)
  const prepared = await model.mutate({ type: 'prepare_connection', rotate: false }, model.places[0])
  assert.ok(prepared?.publicMaterial)
  await model.mutate({ type: 'pin_runner', pin: 'A'.repeat(43), rotate: false,
    manifest: { schema: 1, commit: 'a'.repeat(40), sha256: 'b'.repeat(64), size: 1, os: 'linux', arch: 'x86_64', libc: 'fixture', protocol: 1, journal: 1, driver: { kind: 'indisponivel' } },
  }, model.places[0])
  await model.mutate({ type: 'refix', package: { sandbox_id: 'bx_fixture', host: '192.0.2.10', port: 22,
    known_hosts_line: `bx_fixture ${prepared.publicMaterial.sshPublic.split(' ').slice(0, 2).join(' ')}`,
  } }, model.places[0])
  const placeId = model.places[0].id
  const first = await model.mutate({ type: 'inspect_start' }, model.places[0])
  assert.ok(first?.operation)
  context.mock.timers.tick(2200)
  await model.status(first.operation)
  assert.equal(model.operationFor(placeId)?.phase, 'concluida')
  fake.scenario = 'slow'
  const second = await model.mutate({ type: 'inspect_start' }, model.places[0])
  assert.ok(second?.operation)
  await model.cancel(second.operation)
  assert.equal(model.operationFor(placeId)?.phase, 'cancelada_incerta')
  model.dispose()

  const reopened = new Workplaces(source)
  try {
    await reopened.load()
    assert.equal(reopened.operations.length, 1)
    assert.equal(reopened.operationFor(placeId)?.id, second.operation.id)
    assert.equal(reopened.operationFor(placeId)?.phase, 'cancelada_incerta')
    const oldById = decodeReply(await fake.call('donnaWorkplaceInspectStatus', { operationId: first.operation.id }), 'hub-a')
    const latestById = decodeReply(await fake.call('donnaWorkplaceInspectStatus', { operationId: second.operation.id }), 'hub-a')
    assert.equal(oldById.operation?.phase, 'concluida')
    assert.equal(latestById.operation?.phase, 'cancelada_incerta')
  } finally { reopened.dispose() }
})
test('sucesso de Pin/Refix recupera foco de BODY com preventScroll sem roubar Tab/clique ou outro alvo', () => {
  class FocusDocument extends EventTarget {
    body = {}
    documentElement = {}
    activeElement: unknown = {}
  }
  const cases = ['pin', 'refix', 'tab', 'click', 'changed-focus', 'reconnected', 'other-place', 'new-revision', 'unmounted'] as const
  for (const scenario of cases) {
    const document = new FocusDocument()
    const origin = placeFocusScope(place, 1)
    let actual = { ...origin }
    const focus = trackStepFocus(document, origin, () => actual)
    // Desabilitar o botão enviador remove o foco sem intervenção humana.
    document.activeElement = document.body
    const expected = { ...origin, admissionRevision: origin.admissionRevision + (scenario === 'refix' ? 0 : 1),
      connectionRevision: origin.connectionRevision + (scenario === 'refix' ? 1 : 0) }
    actual = { ...expected }
    let called: { preventScroll: true } | null = null
    const target = { isConnected: true, focus: (options: { preventScroll: true }) => { called = options } }
    if (scenario === 'tab') {
      const event = new Event('keydown')
      Object.defineProperty(event, 'key', { value: 'Tab' })
      document.dispatchEvent(event)
      document.activeElement = {} // Controle alcançado por Tab durante await.
    }
    if (scenario === 'click') document.dispatchEvent(new Event('pointerdown'))
    if (scenario === 'changed-focus') document.activeElement = {}
    if (scenario === 'reconnected') actual.generation++
    if (scenario === 'other-place') actual.placeId = 'another-place'
    if (scenario === 'new-revision') actual.admissionRevision++
    if (scenario === 'unmounted') focus.cancel()
    assert.equal(focus.restore(target, expected), scenario === 'pin' || scenario === 'refix', scenario)
    assert.deepEqual(called, scenario === 'pin' || scenario === 'refix' ? { preventScroll: true } : null, scenario)
    assert.equal(focus.active(), false)
    assert.equal(focus.restore(target, expected), false)
  }
})
test('demo isola lista, material e todas as intenções por Hub sem alterar histórico de outro', async (context) => {
  context.mock.timers.enable({ apis: ['setTimeout', 'Date'], now: 1_700_000_000_000 })
  let hub = 'hub-a'
  const fake = new FakeWorkplaces(() => hub)
  fake.enabled = true
  const manifest = decodeManifest({ schema: 1, commit: 'a'.repeat(40), sha256: 'b'.repeat(64), size: 1, os: 'linux', arch: 'x86_64', libc: 'fixture', protocol: 1, journal: 1, driver: { kind: 'indisponivel' } })
  const mutate = async (intent: Intent, target: Place | null) => decodeReply(await fake.call(methods[intent.type], { clientRequestId: crypto.randomUUID(), target, intent }), hub)
  const prepare = async () => {
    const created = await mutate({ type: 'create', name: `Oficina ${hub}`, resource_id: 'bx_shared' }, null)
    assert.ok(created.place)
    const prepared = await mutate({ type: 'prepare_connection', rotate: false }, created.place)
    assert.ok(prepared.place && prepared.publicMaterial)
    const pinned = await mutate({ type: 'pin_runner', pin: 'A'.repeat(43), manifest, rotate: false }, prepared.place)
    assert.ok(pinned.place)
    const fixed = await mutate({ type: 'refix', package: { sandbox_id: 'bx_shared', host: '192.0.2.10', port: 22,
      known_hosts_line: `bx_shared ${prepared.publicMaterial.sshPublic.split(' ').slice(0, 2).join(' ')}` } }, pinned.place)
    assert.ok(fixed.place)
    return mutate({ type: 'prepare_connection', rotate: false }, fixed.place)
  }
  const ownA = await prepare()
  assert.ok(ownA.place)
  fake.scenario = 'slow'
  const inspectingA = await mutate({ type: 'inspect_start' }, ownA.place)
  assert.ok(inspectingA.operation)
  const snapshotA = decodeReply(await fake.call('donnaWorkplaceList', {}), hub)
  hub = 'hub-b'
  assert.deepEqual(decodeReply(await fake.call('donnaWorkplaceList', {}), hub).places, [])
  assert.deepEqual(decodeReply(await fake.call('donnaWorkplaceList', {}), hub).operations, [])
  assert.equal(decodeGuideReply(await fake.call('donnaWorkplaceGuide', {}), hub).hubVersion, 'demo-04')
  await assert.rejects(fake.call('donnaWorkplaceGuide', { target: ownA.place }), /workplace_request_failed/)
  const foreignIntents: Intent[] = [{ type: 'rename', name: 'FOREIGN' }, { type: 'prepare_connection', rotate: false },
    { type: 'prepare_connection', rotate: true }, { type: 'pin_runner', pin: 'B'.repeat(43), manifest, rotate: true },
    { type: 'refix', package: { sandbox_id: 'bx_shared', host: '192.0.2.11', port: 22, known_hosts_line: 'foreign' } },
    { type: 'inspect_start' }, { type: 'remove' }]
  for (const intent of foreignIntents) {
    await assert.rejects(fake.call(methods[intent.type], { clientRequestId: crypto.randomUUID(), target: { ...ownA.place, hubId: hub }, intent }), /target_invalidated/)
  }
  for (const method of ['donnaWorkplaceInspectStatus', 'donnaWorkplaceInspectCancel']) {
    await assert.rejects(fake.call(method, { operationId: inspectingA.operation.id, clientRequestId: crypto.randomUUID() }), /operation_invalidated/)
  }
  await assert.rejects(fake.call('donnaWorkplaceCreate', { target: ownA.place, intent: { type: 'create', name: 'FOREIGN', resource_id: 'bx_foreign' } }), /target_not_allowed/)
  // O mesmo ID lógico no outro Hub não consulta nem revela o cadastro do primeiro.
  const ownB = await prepare()
  assert.ok(ownB.place)
  fake.scenario = 'error'
  const inspectingB = await mutate({ type: 'inspect_start' }, ownB.place)
  assert.ok(inspectingB.operation)
  hub = 'hub-a'
  assert.deepEqual(decodeReply(await fake.call('donnaWorkplaceList', {}), hub), snapshotA)
  assert.deepEqual((await mutate({ type: 'prepare_connection', rotate: false }, ownA.place)).publicMaterial, ownA.publicMaterial)
  hub = 'hub-b'
  context.mock.timers.tick(2200)
  assert.equal(decodeReply(await fake.call('donnaWorkplaceInspectStatus', { operationId: inspectingB.operation.id }), hub).operation?.phase, 'falhou')
  context.mock.timers.tick(45_000)
  hub = 'hub-a'
  const completedA = decodeReply(await fake.call('donnaWorkplaceInspectStatus', { operationId: inspectingA.operation.id }), hub)
  assert.equal(completedA.operation?.phase, 'concluida')
  assert.equal(completedA.operation?.observation.account, 'assinatura_oficial')
  assert.deepEqual(decodeReply(await fake.call('donnaWorkplaceList', {}), hub).places?.map((item) => item.id), [ownA.place.id])
  await assert.rejects(fake.call('donnaWorkplaceInspectStatus', { operationId: inspectingB.operation.id }), /operation_invalidated/)
})
test('guia tem decoder fechado, versão e limite UTF-8 sem interpretar o texto', () => {
  const guide: WorkplaceGuide = { schema: 1, hubVersion: '2.20.3', text: '<script>literal</script>\nRoteiro público' }
  assert.deepEqual(decodeGuideReply({ hubId: 'hub-a', guide }, 'hub-a'), guide)
  for (const invalid of [{ ...guide, schema: 2 }, { ...guide, hubVersion: '' }, { ...guide, text: '' },
    { ...guide, text: 'á'.repeat(32_769) }, { ...guide, text: 'a\0b' }, { ...guide, path: '/not-admitted' }]) {
    assert.throws(() => decodeGuideReply({ hubId: 'hub-a', guide: invalid }, 'hub-a'))
  }
  assert.throws(() => decodeGuideReply({ hubId: 'hub-b', guide }, 'hub-a'))
  assert.throws(() => decodeGuideReply({ hubId: 'hub-a', guide, extra: true }, 'hub-a'))
  assert.equal(decodeGuideReply({ hubId: 'hub-a', guide: { ...guide, text: 'a'.repeat(65_536) } }, 'hub-a').text.length, 65_536)
})
test('demo só admite objeto vazio em Guide, Access e List', async () => {
  const fake = new FakeWorkplaces(() => 'hub-a'); fake.enabled = true
  for (const method of ['donnaWorkplaceGuide', 'donnaWorkplaceAccess', 'donnaWorkplaceList']) {
    for (const body of [[], null, { path: 'not-admitted' }]) {
      await assert.rejects(fake.call(method, body as unknown as Record<string, unknown>), /workplace_request_failed/)
    }
    assert.ok(await fake.call(method, {}))
  }
})
test('leitura do guia não apaga material/progresso e descarta resposta de origem antiga', async () => {
  const source = new Fixture(); const model = new Workplaces(source); await model.load()
  const material = { hubId: 'hub-a', placeId: place.id, resourceId: place.resourceId, runnerPort: 47631 as const,
    sshPublic: 'ssh-ed25519 fixture', controlPublic: 'A'.repeat(43), runnerPin: null, manifest: null }
  model.material = material; model.operation = operation; model.operations = [operation]
  source.handler = async (method, body) => {
    assert.equal(method, 'donnaWorkplaceGuide'); assert.deepEqual(body, {})
    return { hubId: 'hub-a', guide: { schema: 1, hubVersion: '2.20.3', text: 'Roteiro fixo' } }
  }
  assert.equal((await model.readGuide())?.text, 'Roteiro fixo')
  assert.equal(model.material, material); assert.equal(model.operation, operation); assert.equal(model.operations[0], operation)
  assert.equal(model.busy, false); assert.equal(model.error, null)
  source.handler = async () => { throw Error('guide_failed') }
  await assert.rejects(model.readGuide(), /guide_failed/)
  assert.equal(model.material, material); assert.equal(model.operation, operation); assert.equal(model.available, true); assert.equal(model.error, null)
  const late = deferred<unknown>(); source.handler = () => late.promise
  const pending = model.readGuide()
  source.hello = { hostId: 'hub-b' }; source.principalEpoch++; source.changed()
  late.resolve({ hubId: 'hub-a', guide: { schema: 1, hubVersion: '2.20.3', text: 'Antigo' } })
  assert.equal(await pending, null)
  model.dispose()
})
test('fechar/reabrir o guia ou trocar origem descarta carregamento tardio sem reabrir painel', async () => {
  const reading = new GuideReading()
  const applied: GuideContent[] = []
  const old = deferred<WorkplaceGuide | null>()
  const source = { generation: 1, readGuide: () => old.promise }
  const pending = reading.read(source, (content) => applied.push(content))
  reading.cancel()
  old.resolve({ schema: 1, hubVersion: 'old', text: 'Antigo' })
  await pending
  assert.equal(applied.length, 0)
  const older = deferred<WorkplaceGuide | null>()
  source.readGuide = () => older.promise
  const obsolete = reading.read(source, (content) => applied.push(content))
  source.readGuide = async () => ({ schema: 1, hubVersion: 'new', text: 'Novo' })
  await reading.read(source, (content) => applied.push(content))
  older.resolve({ schema: 1, hubVersion: 'old', text: 'Antigo' }); await obsolete
  assert.deepEqual([...applied], [{ kind: 'ready', guide: { schema: 1, hubVersion: 'new', text: 'Novo' } }])
  const prior = deferred<WorkplaceGuide | null>(); source.readGuide = () => prior.promise
  const staleOrigin = reading.read(source, (content) => applied.push(content))
  source.generation++
  prior.resolve({ schema: 1, hubVersion: 'old', text: 'Outra origem' }); await staleOrigin
  assert.equal(applied.length, 1)
})
