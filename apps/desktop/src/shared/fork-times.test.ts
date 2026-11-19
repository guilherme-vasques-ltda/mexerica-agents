import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  addArea, localRefusal, moveBot, parseTimesSnapshot, reconcileRoster, removeArea, reportOfferRefusal, setReportsTo, singleLeader, timesErrorText, timesFailure, updateArea, upstreamManager,
  type TimesGraph, type TimesMember,
} from './fork-times.ts'
import { ErroFrase, textoDe, type Frase } from '../renderer/lib/fork-i18n/frase.ts'

// D454 (fatia 10b): as recusas são frases do catálogo; aqui, lidas em pt-BR.
const pt = (f: Frase | null | undefined) => (f ? textoDe(f, 'pt-BR') : '')

// Times (times-03): o app só lê o DTO mínimo do Hub, recusa resposta de outro space e monta mudanças que conservam
// o que a pessoa não pediu para mudar. As regras locais só orientam a tela; o Hub confere de novo.

const AREA = '6f1c2b8e-1d2a-4c3b-9e4f-5a6b7c8d9e0f'
const member = (botId: string, extra: Partial<TimesMember> = {}): TimesMember => ({
  botId, teamId: null, parentBotId: null, role: 'executor', reportsTo: { kind: 'owner' }, ownerContact: 'reports', ...extra,
})
const graph = (): TimesGraph => ({
  nodes: [{ id: AREA, name: 'Marketing', parentId: null, managerBotId: 'hype', position: 0 }],
  members: [
    member('donna', { role: 'manager' }),
    member('hype', { role: 'manager', teamId: AREA }),
    member('clipe', { teamId: AREA, reportsTo: { kind: 'bot', botId: 'hype' }, ownerContact: 'on_request' }),
    member('lingo', { teamId: AREA, parentBotId: 'clipe', reportsTo: { kind: 'bot', botId: 'hype' }, ownerContact: 'on_request' }),
    member('ata'),
  ],
  routes: [],
})
const snapshotJson = (spaceId: string) => ({
  version: 1, spaceId, revision: 3, catalogVersion: 'v1:abc', primaryBotId: 'donna',
  bots: [{ id: 'donna', name: 'Donna', spaceId, title: 'Secretária', avatarColor: 'blue', avatarShape: 'blob', hidden: false, prompt: 'segredo', memory: ['x'] }],
  ...graph(),
})

test('o snapshot do Hub é lido campo a campo, sem campos fora do DTO mínimo', () => {
  const s = parseTimesSnapshot(snapshotJson('p'), 'p')
  assert.equal(s.revision, 3)
  assert.deepEqual(Object.keys(s.bots[0]!).sort(), ['avatarColor', 'avatarShape', 'hidden', 'id', 'name', 'spaceId', 'title'])
})

test('resposta de outro space ou malformada é recusada', () => {
  assert.throws(() => parseTimesSnapshot(snapshotJson('outro'), 'p'), /cross_space_response/)
  assert.throws(() => parseTimesSnapshot({ ...snapshotJson('p'), members: [{ botId: 'x', role: 'chefe' }] }, 'p'), /times_invalid_answer/)
  // Sem space pedido (Pessoal padrão), o do Hub vale; um bot de outro space no DTO, não.
  const json = snapshotJson('p')
  assert.throws(() => parseTimesSnapshot({ ...json, bots: [{ ...json.bots[0], spaceId: 'q' }] }, null), /times_invalid_answer/)
})

test('Mover leva a subárvore e não troca Reporta a; só a opção explícita troca', () => {
  const g = graph()
  const moved = moveBot(g, 'clipe', { teamId: null, parentBotId: null })
  const clipe = moved.members.find((m) => m.botId === 'clipe')!
  const lingo = moved.members.find((m) => m.botId === 'lingo')!
  assert.deepEqual(clipe.reportsTo, { kind: 'bot', botId: 'hype' })
  assert.equal(clipe.teamId, null)
  assert.equal(lingo.parentBotId, 'clipe', 'o subbot vai junto')
  assert.equal(lingo.teamId, null)
  assert.deepEqual(g.members.find((m) => m.botId === 'clipe')!.teamId, AREA, 'o grafo anterior não muda')
  const withReport = moveBot(g, 'ata', { teamId: null, parentBotId: 'hype' }, 'hype')
  assert.deepEqual(withReport.members.find((m) => m.botId === 'ata')!.reportsTo, { kind: 'bot', botId: 'hype' })
  assert.equal(withReport.members.find((m) => m.botId === 'ata')!.teamId, AREA)
})

test('Reporta a Você leva o contato para Relatórios, como o Hub exige', () => {
  const g = setReportsTo(graph(), 'clipe', { kind: 'owner' })
  const clipe = g.members.find((m) => m.botId === 'clipe')!
  assert.equal(clipe.ownerContact, 'reports')
  assert.equal(localRefusal(g, 'donna'), null)
})

test('as regras locais espelham as recusas do Hub: gerente, principal e ciclos', () => {
  assert.equal(localRefusal(graph(), 'donna'), null)
  assert.match(pt(localRefusal(setReportsTo(graph(), 'ata', { kind: 'bot', botId: 'clipe' }), 'donna')), /Gerente/)
  // Composto: Hype reporta a Lingo, que está abaixo de Clipe, na área de Hype. Lingo vira gerente para isolar o ciclo.
  const lingoManager = graph()
  lingoManager.members = lingoManager.members.map((m) => (m.botId === 'lingo' ? { ...m, role: 'manager' } : m))
  const hypeBelow = moveBot(lingoManager, 'hype', { teamId: null, parentBotId: 'lingo' })
  assert.notEqual(localRefusal(hypeBelow, 'donna'), null)
  assert.equal(localRefusal(setReportsTo(graph(), 'ata', { kind: 'bot', botId: 'hype' }), 'donna'), null, 'reportar a um gerente de outra parte da árvore é permitido')
  // Só o composto fecha o círculo: Hype reporta a Lingo, que está abaixo de Clipe, na área de Hype; Lingo reporta a Você.
  const lingoToOwner = { ...lingoManager, members: lingoManager.members.map((m) => (m.botId === 'lingo' ? { ...m, reportsTo: { kind: 'owner' as const }, ownerContact: 'reports' as const } : m)) }
  assert.equal(localRefusal(lingoToOwner, 'donna'), null)
  const compound = setReportsTo(lingoToOwner, 'hype', { kind: 'bot', botId: 'lingo' })
  assert.match(pt(localRefusal(compound, 'donna')), /abaixo dele/)
  assert.match(pt(localRefusal(moveBot(graph(), 'donna', { teamId: AREA, parentBotId: null }), 'donna')), /principal/)
})

test('área nova nasce com o gerente como Gerente; remover área com bots dentro é recusado', () => {
  const id = '0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d'
  const g = addArea(graph(), { id, name: '  Finanças ', parentId: null, managerBotId: 'ata' })
  assert.equal(g.nodes.find((n) => n.id === id)!.name, 'Finanças')
  assert.equal(g.members.find((m) => m.botId === 'ata')!.role, 'manager')
  assert.throws(() => removeArea(graph(), AREA), (e: unknown) => e instanceof ErroFrase && pt(e.frase).startsWith('Mova os bots'))
  assert.equal(removeArea(g, id).nodes.length, 1)
})

test('falhas do Hub viram texto; sem resposta é desconhecido, nunca recusa', () => {
  assert.equal(timesFailure(Object.assign(new Error('offline'), { status: 0 })), 'unknown')
  assert.equal(timesFailure(new Error('times_conflict: o quadro mudou')), 'conflict')
  assert.equal(timesFailure(new Error('catalog_conflict: a principal mudou')), 'catalog')
  assert.equal(timesFailure(new Error('not_allowed: só o dono')), 'owner')
  assert.equal(timesFailure(Object.assign(new Error('Control scope required'), { status: 403 })), 'owner')
  assert.equal(timesFailure(Object.assign(new Error('Unauthorized'), { status: 401 })), 'owner')
  assert.equal(pt(timesErrorText(new Error('compound_cycle: ligação cria um ciclo'))), 'Um bot reportaria a alguém que está abaixo dele.')
  assert.doesNotMatch(pt(timesErrorText(new Error('report_cycle: x'))), /report_cycle/)
})

test('reconciliar o elenco: bot novo entra como o Hub projetou; principal trocada ou bot que saiu voltam como motivo', () => {
  const draft = setReportsTo(graph(), 'ata', { kind: 'bot', botId: 'hype' })
  const json = snapshotJson('p')
  const withNew = parseTimesSnapshot({ ...json, members: [...json.members, member('nova')] }, 'p')
  const fitted = reconcileRoster(draft, withNew)
  assert.ok('graph' in fitted)
  assert.deepEqual(fitted.graph.members.find((m) => m.botId === 'ata')!.reportsTo, { kind: 'bot', botId: 'hype' }, 'a edição fica')
  assert.deepEqual(fitted.graph.members.find((m) => m.botId === 'nova'), member('nova'))
  const newPrimary = parseTimesSnapshot({ ...json, primaryBotId: 'ata', members: json.members.map((m) => (m.botId === 'ata' ? member('ata', { role: 'manager' }) : m)) }, 'p')
  assert.ok('problem' in reconcileRoster(draft, newPrimary))
  const gone = parseTimesSnapshot({ ...json, members: json.members.filter((m) => m.botId !== 'lingo') }, 'p')
  assert.ok('problem' in reconcileRoster(draft, gone))
})

test('D249 R4: recusa sem código conhecido muda de texto entre leitura e gravação; códigos conhecidos ficam', () => {
  const unknown = Object.assign(new Error('codigo_novo_do_hub: algo'), { status: 400 })
  assert.equal(pt(timesErrorText(unknown, 'read')), 'Não foi possível ler as Equipes agora.')
  assert.equal(pt(timesErrorText(unknown, 'save')), 'Essa mudança não foi aceita.')
  assert.equal(pt(timesErrorText(unknown)), 'Essa mudança não foi aceita.', 'o padrão continua o de gravação')
  assert.equal(pt(timesErrorText(new Error('report_cycle: x'), 'read')), 'Os reportes voltariam em círculo.')
  assert.equal(pt(timesErrorText(Object.assign(new Error('x'), { status: 0 }), 'read')), 'O computador dos seus bots não respondeu.')
})

// Reporta a automático (três níveis): o padrão oferecido é o nível de cima; a tela marca, a pessoa confirma.
const SOCIAL = '7a2d3c9f-2e3b-4d4c-8f5a-6b7c8d9e0f1a'
const tres = (): TimesGraph => ({
  nodes: [
    { id: AREA, name: 'Marketing', parentId: null, managerBotId: 'ana', position: 0 },
    { id: SOCIAL, name: 'Social', parentId: AREA, managerBotId: 'angela', position: 1 },
  ],
  members: [
    member('donna', { role: 'manager' }),
    member('ana', { role: 'manager', teamId: AREA }),
    member('angela', { role: 'manager', teamId: SOCIAL, reportsTo: { kind: 'bot', botId: 'ana' }, ownerContact: 'on_request' }),
    member('marisa', { role: 'manager' }),
    member('angelica'),
  ],
  routes: [],
})

test('o nível de cima é o bot de cima ou o gerente da área; a oferta válida não tem motivo', () => {
  const g = tres()
  assert.equal(upstreamManager(g, { teamId: SOCIAL, parentBotId: null }), 'angela')
  assert.equal(upstreamManager(g, { teamId: null, parentBotId: 'ana' }), 'ana')
  assert.equal(upstreamManager(g, { teamId: null, parentBotId: null }), null)
  const moved = moveBot(g, 'angelica', { teamId: SOCIAL, parentBotId: null })
  assert.equal(reportOfferRefusal(moved, 'angelica', 'angela', 'donna'), null)
  // Desmarcar: Mover sem o destinatário mantém o Reporta a anterior.
  assert.deepEqual(moved.members.find((m) => m.botId === 'angelica')!.reportsTo, { kind: 'owner' })
})

test('a oferta fica desligada para o próprio bot, quem não é gerente, círculo e a principal', () => {
  const g = tres()
  assert.equal(pt(reportOfferRefusal(g, 'ana', 'ana', 'donna')), 'É o próprio bot.')
  assert.equal(pt(reportOfferRefusal(g, 'marisa', 'angelica', 'donna', 'Angelica')), '\u2068Angelica\u2069 não é gerente.')
  assert.match(pt(reportOfferRefusal(g, 'ana', 'angela', 'donna')), /círculo|abaixo/)
  assert.match(pt(reportOfferRefusal(g, 'donna', 'ana', 'donna')), /principal/)
})

test('gerente de subárea nova passa a reportar ao gerente de cima; editar sem mudar gerente/pai não altera nada', () => {
  const g = tres()
  const INSTA = '8b3e4d0a-3f4c-4e5d-9a6b-7c8d9e0f1a2b'
  let next = addArea(g, { id: INSTA, name: 'Instagram', parentId: SOCIAL, managerBotId: 'marisa' })
  const up = upstreamManager(next, { teamId: SOCIAL, parentBotId: null })
  assert.equal(up, 'angela')
  assert.equal(reportOfferRefusal(next, 'marisa', up!, 'donna'), null)
  next = setReportsTo(next, 'marisa', { kind: 'bot', botId: up! })
  assert.deepEqual(next.members.find((m) => m.botId === 'marisa')!.reportsTo, { kind: 'bot', botId: 'angela' })
  assert.equal(localRefusal(next, 'donna'), null)
  const renamed = updateArea(g, SOCIAL, { name: 'Redes' })
  assert.deepEqual(renamed.members, g.members, 'só o nome muda')
})

test('D340: membro único sem chefe vira o chefe na prévia e é o "de cima" da oferta; 0 ou 2+ não ganham chefe', () => {
  const YT = '1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d'
  const RT = '2b3c4d5e-6f7a-4b8c-9d0e-1f2a3b4c5d6e'
  // Caso real: Youtube só com o Joio e a subequipe Roteiros só com o Crivo; subequipe não conta como membro.
  const g: TimesGraph = {
    nodes: [
      { id: YT, name: 'Youtube', parentId: null, managerBotId: null, position: 0 },
      { id: RT, name: 'Roteiros', parentId: YT, managerBotId: null, position: 1 },
    ],
    members: [member('donna', { role: 'manager' }), member('joio', { teamId: YT }), member('crivo', { teamId: RT }), member('ata')],
    routes: [],
  }
  const led = singleLeader(g, g, 'donna')
  assert.deepEqual(led.nodes.map((n) => n.managerBotId), ['joio', 'crivo'])
  assert.equal(led.members.find((m) => m.botId === 'crivo')!.role, 'manager')
  assert.deepEqual(led.members.find((m) => m.botId === 'crivo')!.reportsTo, { kind: 'owner' }, 'Reporta a fica com a oferta')
  assert.equal(upstreamManager(g, { teamId: YT, parentBotId: null }), 'joio')
  assert.equal(reportOfferRefusal(led, 'crivo', 'joio', 'donna'), null)
  // Entra o 2º: o chefe continua; sai o único (chefe): a área fica sem chefe e o papel fica.
  const two = singleLeader(moveBot(led, 'ata', { teamId: RT, parentBotId: null }), led, 'donna')
  assert.equal(two.nodes[1]!.managerBotId, 'crivo')
  const out = singleLeader(moveBot(led, 'crivo', { teamId: null, parentBotId: null }), led, 'donna')
  assert.equal(out.nodes[1]!.managerBotId, null)
  assert.equal(out.members.find((m) => m.botId === 'crivo')!.role, 'manager')
  // Dois sem chefe não ganham chefe; chefe informado não muda.
  const pair = { ...g, nodes: [g.nodes[0]!], members: g.members.map((m) => (m.botId === 'crivo' ? { ...m, teamId: YT } : m)) }
  assert.equal(singleLeader(pair).nodes[0]!.managerBotId, null)
  assert.deepEqual(singleLeader(graph()), graph())
})

test('D454 (fatia 10b): recusas e erros de Times são frases e saem no idioma pedido', () => {
  const recusa = reportOfferRefusal({ nodes: [], members: [member('ana'), member('angelica', { role: 'executor' })], routes: [] }, 'ana', 'angelica', null, 'Angelica')
  assert.equal(textoDe(recusa, 'en'), '\u2068Angelica\u2069 isn’t a manager.')
  assert.equal(textoDe(timesErrorText(new Error('report_cycle: x')), 'en'), 'Reports would go around in a circle.')
  assert.notEqual(textoDe(timesErrorText(new Error('x'), 'read'), 'ar-AE'), pt(timesErrorText(new Error('x'), 'read')))
})
