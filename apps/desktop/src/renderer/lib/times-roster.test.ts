import test from 'node:test'
import assert from 'node:assert/strict'
import type { TimesSnapshot, TimesMember } from '../../shared/fork-times.ts'
import { chiefTitle, emptyTimesPresentation, moveTimesGroup, moveTimesMember, nextTimesSibling, NO_TEAM, parseTimesPresentation, projectTimes, setTimesOpen, teamKey, teamsToReveal, type TimesRow } from './times-roster.ts'
import { navigableRows, splitChannels, visibleSections } from './sidebar-layout.ts'
import { TimesRosterPreferences, TIMES_PRESENTATION_KEY } from './times-roster-preferences.ts'
import { mergeOrder } from './busca.ts'

const row = (id: string, pinned = false, hidden = false) => ({ bot: { id, name: id, pinned, hidden, kind: 'bot' } })
const member = (id: string, teamId: string | null = 'marketing', patch: Partial<TimesMember> = {}): TimesMember => ({ botId: id, teamId, parentBotId: null, role: 'executor', reportsTo: { kind: 'owner' }, ownerContact: 'reports', ...patch })
function graph(): TimesSnapshot {
  return { version: 1, spaceId: 'personal', revision: 4, catalogVersion: 'c1', primaryBotId: 'donna', bots: [], routes: [],
    nodes: [
      { id: 'marketing', name: 'Marketing', parentId: null, managerBotId: 'chief', position: 0 },
      { id: 'social', name: 'Social', parentId: 'marketing', managerBotId: null, position: 1 },
      { id: 'finance', name: 'Finanças', parentId: null, managerBotId: null, position: 2 },
    ], members: [member('chief'), member('pin'), member('member'), member('sub', 'social', { parentBotId: 'member' }), member('free', null), member('donna', null)] }
}
const ids = (items: TimesRow[]) => items.map((i) => i.bot.id)
const bots = () => [row('pin', true), row('member'), row('chief'), row('sub'), row('free'), row('donna')]
/** Todas as equipes do exemplo abertas, como depois de a pessoa abrir cada uma. */
const openAll = () => ({ ...emptyTimesPresentation(), open: [teamKey('marketing'), teamKey('social'), teamKey('finance')] })

test('teamId manda; Chefe explícito antes de pin, subárea com caminho, Donna fora e Sem time ao fim', () => {
  const p = projectTimes(bots(), graph(), openAll())
  assert.deepEqual(ids(p.groups[0]!.rows), ['chief', 'pin', 'member'])
  assert.equal(p.groups[0]!.chiefId, 'chief')
  assert.equal(p.groups[1]!.path, 'Marketing')
  assert.deepEqual(p.groups.map((g) => g.level), [1, 2, 1])
  assert.deepEqual(ids(p.groups[1]!.rows), ['sub'])
  assert.ok(!p.groups.some((g) => g.id === NO_TEAM))
  assert.deepEqual(ids(p.simple), ['free'])
  assert.deepEqual(ids(p.rows), [...p.groups.flatMap((g) => ids(g.rows)), 'free'])
  assert.equal(new Set(ids(p.rows)).size, p.rows.length)
})

test('Chefe oculto/principal (fora do alcance da barra): Confira, sem nome, sem duplicar nem referência', () => {
  for (const kind of ['oculto', 'principal'] as const) {
    const g = graph(), items = bots()
    if (kind === 'oculto') items.find((i) => i.bot.id === 'chief')!.bot.hidden = true
    if (kind === 'principal') g.nodes[0]!.managerBotId = 'donna'
    const p = projectTimes(items, g, openAll())
    assert.equal(p.groups[0]!.chiefId, null, kind)
    assert.equal(p.groups[0]!.chiefRef, null, kind)
    assert.equal(p.groups[0]!.checkChief, true, kind)
    assert.equal(p.groups[0]!.name, 'Marketing')
    assert.equal(new Set(ids(p.rows)).size, p.rows.length)
    assert.ok(!ids(p.rows).includes(kind === 'oculto' ? 'chief' : 'donna'))
  }
})

test('Chefe de fora ou de outra equipe vira a linha de referência, sem Confira e sem duplicar', () => {
  const fora = graph()
  fora.members.find((m) => m.botId === 'chief')!.teamId = null
  const p = projectTimes(bots(), fora, openAll())
  assert.equal(p.groups[0]!.chiefId, null)
  assert.equal(p.groups[0]!.checkChief, false)
  assert.deepEqual([p.groups[0]!.chiefRef?.item.bot.id, p.groups[0]!.chiefRef?.home], ['chief', 'Agentes'])
  assert.ok(ids(p.simple).includes('chief'))
  assert.equal(new Set(ids(p.rows)).size, p.rows.length)
  // Compartilhado: chefe-membro de Marketing e chefe gravado de Finanças.
  const shared = graph()
  shared.nodes[2]!.managerBotId = 'chief'
  const q = projectTimes(bots(), shared, openAll())
  assert.equal(q.groups[0]!.chiefId, 'chief')
  assert.deepEqual([q.groups[2]!.chiefRef?.item.bot.id, q.groups[2]!.chiefRef?.home, q.groups[2]!.checkChief], ['chief', 'Marketing', false])
  assert.deepEqual(ids(q.rows).filter((id) => id === 'chief'), ['chief'])
})

test('Joio chefe de Youtube (membro) e de Roteiros (só o Crivo): os dois pela linha do chefe', () => {
  const g: TimesSnapshot = { version: 1, spaceId: 'personal', revision: 1, catalogVersion: 'c1', primaryBotId: 'donna', bots: [], routes: [],
    nodes: [{ id: 'youtube', name: 'Youtube', parentId: null, managerBotId: 'joio', position: 0 }, { id: 'roteiros', name: 'Roteiros', parentId: 'youtube', managerBotId: 'joio', position: 0 }],
    members: [member('joio', 'youtube'), member('crivo', 'roteiros')] }
  const items = [row('joio'), row('crivo')]
  const p = projectTimes(items, g, { ...emptyTimesPresentation(), open: [teamKey('youtube'), teamKey('roteiros')] })
  const [youtube, roteiros] = p.groups
  assert.deepEqual([youtube!.chiefId, youtube!.chiefRef, youtube!.checkChief], ['joio', null, false])
  assert.deepEqual([roteiros!.chiefId, roteiros!.chiefRef?.item.bot.id, roteiros!.chiefRef?.home, roteiros!.checkChief], [null, 'joio', 'Youtube', false])
  // A referência não é membro: fora de items/members/below (contagem), rows (ordem, setas, arraste) e da ordem local.
  assert.deepEqual(ids(roteiros!.items), ['crivo'])
  assert.deepEqual(ids(roteiros!.members), ['crivo'])
  assert.deepEqual(ids(roteiros!.below), ['crivo'])
  assert.deepEqual(ids(youtube!.below), ['crivo'])
  assert.deepEqual(ids(p.rows), ['joio', 'crivo'])
  assert.equal(nextTimesSibling(p, 'crivo', -1), null)
  assert.deepEqual(moveTimesMember(p, emptyTimesPresentation(), 'crivo', 'joio'), emptyTimesPresentation())
  // Busca: o Joio aparece uma vez (Youtube); Roteiros só entra pelo Crivo, com a referência como contexto.
  const joio = projectTimes(items, g, emptyTimesPresentation(), 'joio')
  assert.deepEqual(ids(joio.rows), ['joio'])
  assert.deepEqual(joio.groups.map((x) => x.id), [teamKey('youtube')])
  const crivo = projectTimes(items, g, emptyTimesPresentation(), 'crivo')
  assert.deepEqual(ids(crivo.rows), ['joio', 'crivo'])
  assert.equal(crivo.groups[1]!.chiefRef?.item.bot.id, 'joio')
  // Joio oculto: as duas equipes voltam ao Confira, sem nome.
  const hidden = projectTimes([{ bot: { ...row('joio').bot, hidden: true } }, row('crivo')], g, emptyTimesPresentation())
  assert.deepEqual(hidden.groups.map((x) => [x.chiefId, x.chiefRef, x.checkChief]), [[null, null, true], [null, null, true]])
  assert.ok(!ids(hidden.rows).includes('joio'))
})

test('role manager e primeiro membro não viram Chefe; DTO administrativo não amplia visibilidade', () => {
  const g = graph()
  g.nodes[0]!.managerBotId = null
  g.members[0]!.role = 'manager'
  g.bots.push({ id: 'oculto-admin', name: 'Nome sigiloso', hidden: true, spaceId: 'personal', title: '', avatarColor: '', avatarShape: '' })
  g.members.push(member('oculto-admin'))
  const p = projectTimes(bots(), g, emptyTimesPresentation())
  assert.equal(p.groups[0]!.chiefId, null)
  assert.equal(p.groups[0]!.checkChief, false)
  assert.equal(ids(p.rows).includes('oculto-admin'), false)
})

test('catálogo cresce antes da releitura: desconhecido fica simples, só null conhecido vira Sem time', () => {
  const g = graph(), items = [...bots(), row('new')]
  const before = projectTimes(items, g, openAll())
  assert.deepEqual(ids(before.simple), ['free', 'new'])
  assert.ok(ids(before.rows).includes('new'))
  g.catalogVersion = 'c2'; g.members.push(member('new', 'social')) // sem mudar revision
  const after = projectTimes(items, g, openAll())
  assert.deepEqual(ids(after.simple), ['free'])
  assert.deepEqual(ids(after.groups[1]!.items), ['sub', 'new'])
})

test('sem snapshot não finge Sem time, não usa preferência de filtro antiga nem esconde bot autorizado', () => {
  const p = projectTimes(bots(), null, { ...emptyTimesPresentation(), filter: 'mk' }, '', 'donna')
  assert.deepEqual(p.groups, [])
  assert.deepEqual(ids(p.simple), ['pin', 'member', 'chief', 'sub', 'free'])
})

test('recolhida é o padrão: a equipe é só o chefe, o `folded` antigo é ignorado e abrir fica guardado', () => {
  const legacy = { ...emptyTimesPresentation(), folded: [teamKey('finance')] }
  const p = projectTimes(bots(), graph(), legacy)
  assert.deepEqual(p.groups.map((g) => [g.id, g.open, g.shown]), [[teamKey('marketing'), false, true], [teamKey('social'), false, false], [teamKey('finance'), false, true]])
  assert.deepEqual(ids(p.rows), ['chief', 'free'], 'Social fica fora da tela sob a equipe-mãe recolhida; Finanças sem chefe não tem linha de bot')
  assert.deepEqual(ids(p.groups[0]!.members), ['pin', 'member'], 'o corpo recolhido continua montado (revisão #355): os membros vêm mesmo fechado')
  assert.deepEqual(ids(p.groups[1]!.rows), [], 'equipe fora da tela não entra na navegação')
  assert.deepEqual(ids(p.groups[0]!.below), ['pin', 'member', 'sub'], 'a linha recolhida representa membros e subequipes')
  const opened = setTimesOpen(legacy, teamKey('marketing'), true)
  assert.deepEqual(opened.open, [teamKey('marketing')])
  assert.deepEqual(ids(projectTimes(bots(), graph(), opened).rows), ['chief', 'pin', 'member', 'free'])
  assert.deepEqual(setTimesOpen(opened, teamKey('marketing'), false).open, [])
  assert.deepEqual(parseTimesPresentation({ order: [], folded: [], members: {}, filter: null }).open, [], 'apresentação antiga, sem `open`, abre recolhida')
  assert.equal(chiefTitle(1), 'Gerente'); assert.equal(chiefTitle(2), 'Coordenação'); assert.equal(chiefTitle(5), 'Responsável')
})

test('busca abre temporariamente, mostra o chefe para contexto, não muda preferência e navega o desenhado', () => {
  const pref = emptyTimesPresentation()
  const original = JSON.stringify(pref)
  const p = projectTimes(bots(), graph(), pref, 'MEMBER')
  assert.equal(p.groups[0]!.temporary, true)
  assert.equal(p.groups[0]!.open, true)
  assert.deepEqual(ids(p.rows), ['chief', 'member'])
  const deep = projectTimes(bots(), graph(), pref, 'sub')
  assert.deepEqual(deep.groups.map((g) => [g.id, g.temporary, g.shown]), [[teamKey('marketing'), true, true], [teamKey('social'), true, true]])
  assert.deepEqual(ids(deep.rows), ['chief', 'sub'])
  assert.deepEqual(teamsToReveal(graph(), 'sub'), [teamKey('social'), teamKey('marketing')])
  assert.deepEqual(teamsToReveal(graph(), 'chief'), [], 'o chefe é a linha da equipe: nada a abrir')
  assert.deepEqual(teamsToReveal(graph(), 'member'), [teamKey('marketing')])
  assert.equal(JSON.stringify(pref), original)
  const filtered = projectTimes(bots(), graph(), { ...pref, open: [teamKey('marketing')], filter: teamKey('marketing') })
  assert.deepEqual(ids(filtered.rows), ['chief', 'pin', 'member'])
  assert.deepEqual(filtered.groups.map((g) => g.id), [teamKey('marketing'), teamKey('social')])
  assert.deepEqual(ids(projectTimes(bots(), graph(), { ...pref, filter: NO_TEAM }).rows), ['free'], 'filtro Agentes')
  assert.deepEqual(ids(projectTimes(bots(), graph(), { ...pref, filter: teamKey('social') }).rows), ['chief', 'free'], 'subequipe não é filtro: volta a Todas')
})

test('ordem local usa IDs, sobrevive renomeação e ignora IDs removidos/duplicados sem criar linhas', () => {
  const pref = { ...openAll(), order: ['team:missing', teamKey('finance'), teamKey('marketing'), teamKey('marketing')],
    members: { [teamKey('marketing')]: ['deleted', 'member', 'pin', 'chief'] } }
  const g = graph()
  const p = projectTimes(bots(), g, pref)
  assert.equal(p.groups[0]!.id, teamKey('finance'))
  assert.deepEqual(ids(p.groups[1]!.rows), ['chief', 'pin', 'member'])
  g.nodes[0]!.name = 'Nome novo'
  assert.equal(projectTimes(bots(), g, pref).groups[1]!.name, 'Nome novo')
  assert.equal(new Set(ids(p.rows)).size, p.rows.length)
})

test('mover só irmãos/pin; Chefe, outro Time e subbot com outro pai não são alvos', () => {
  const g = graph(), items = [...bots(), row('peer'), row('child')]
  g.members.push(member('peer'), member('child', 'marketing', { parentBotId: 'chief' }))
  const pref = openAll(), p = projectTimes(items, g, pref)
  assert.equal(moveTimesMember(p, pref, 'member', 'chief'), pref)
  assert.equal(moveTimesMember(p, pref, 'chief', 'member'), pref)
  assert.equal(moveTimesMember(p, pref, 'member', 'pin'), pref)
  assert.equal(moveTimesMember(p, pref, 'member', 'sub'), pref)
  assert.equal(moveTimesMember(p, pref, 'member', 'child'), pref)
  const next = moveTimesMember(p, pref, 'member', 'peer')
  assert.deepEqual(ids(projectTimes(items, g, next).groups[0]!.rows), ['chief', 'pin', 'peer', 'member', 'child'])
  assert.equal(g.members.find((m) => m.botId === 'member')!.teamId, 'marketing')
})

test('Times só ordenam áreas irmãs; Sem time fica ao fim e subárea segue o caminho', () => {
  const pref = openAll(), p = projectTimes(bots(), graph(), pref)
  assert.equal(moveTimesGroup(p, pref, teamKey('marketing'), teamKey('social')), pref)
  assert.equal(moveTimesGroup(p, pref, teamKey('marketing'), NO_TEAM), pref)
  const next = moveTimesGroup(p, pref, teamKey('marketing'), teamKey('finance'))
  assert.deepEqual(projectTimes(bots(), graph(), next).groups.map((g) => g.id), [teamKey('finance'), teamKey('marketing'), teamKey('social')])
})

test('Canais conserva slots/reorder e navegação com Canais/Bots ocultos ou recolhidos; legado não muda', () => {
  const legacy = { donna_bot_sections: '[{"id":"mk","name":"mk"}]', donnabotCollapsedBotSections: '["mk"]' }
  const storage = new Map(Object.entries(legacy))
  const writes: string[] = []
  const prefs = new TimesRosterPreferences({ getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => { writes.push(key); storage.set(key, value) } })
  const domainA = JSON.stringify(['conta', 'pc', 'hub-A', 'personal'])
  const domainB = JSON.stringify(['conta', 'pc', 'hub-B', 'personal'])
  prefs.change(domainA, (p) => ({ ...p, folded: [teamKey('marketing')] }))
  prefs.change(domainB, (p) => ({ ...p, order: [teamKey('finance'), teamKey('marketing')] }))
  assert.deepEqual(writes, [TIMES_PRESENTATION_KEY, TIMES_PRESENTATION_KEY])
  for (const [key, value] of Object.entries(legacy)) assert.equal(storage.get(key), value)
  assert.deepEqual(parseTimesPresentation(prefs.get()[domainA]).folded, [teamKey('marketing')])
  assert.deepEqual(parseTimesPresentation(prefs.get()[domainB]).folded, [])
  const group1 = { bot: { ...row('grupo1').bot, kind: 'group' } }, group2 = { bot: { ...row('grupo2').bot, kind: 'group' } }
  const { channels, bots: direct } = splitChannels([group1, ...bots(), group2])
  const p = projectTimes(direct, graph(), emptyTimesPresentation())
  assert.deepEqual(ids(channels), ['grupo1', 'grupo2'])
  assert.deepEqual(mergeOrder(['grupo1', 'pin', 'grupo2'], ['grupo2', 'grupo1']), ['grupo2', 'pin', 'grupo1'])
  const order = visibleSections(['channels', 'bots'], [])
  const parts = { channels, teams: p.groups.flatMap((g) => g.rows), agents: p.simple }
  assert.deepEqual(ids(navigableRows(order, ['bots'], parts)), ['grupo1', 'grupo2'])
  assert.deepEqual(ids(navigableRows(visibleSections([], ['channels']), [], parts)), ids(p.rows))
  prefs.change(domainA, (pref) => moveTimesMember(p, pref, 'member', 'pin'))
  for (const [key, value] of Object.entries(legacy)) assert.equal(storage.get(key), value)
  assert.ok(!p.groups.some((g) => g.name === 'mk'))
})

test('preferência inválida volta ao padrão e namespace não usa nome para ordem/fold', () => {
  for (const bad of [null, [], { order: 'mk' }, { ...emptyTimesPresentation(), folded: [1] }, { ...emptyTimesPresentation(), members: { x: ['a', 1] } }]) {
    assert.deepEqual(parseTimesPresentation(bad), emptyTimesPresentation())
  }
  assert.deepEqual(parseTimesPresentation({ ...emptyTimesPresentation(), open: [1] }), emptyTimesPresentation())
  const pref = { ...emptyTimesPresentation(), open: [teamKey('marketing')] }
  const g = graph(); g.nodes[0]!.name = 'Finanças'
  assert.equal(projectTimes(bots(), g, pref).groups.find((g) => g.id === teamKey('marketing'))!.open, true)
  assert.equal(projectTimes(bots(), g, pref).groups.find((g) => g.id === teamKey('finance'))!.open, false)
})


test('alvo irmão escolhido não dispensa revalidação do vínculo corrente pelo mutador', () => {
  const g = graph(), items = [row('chief'), row('member'), row('child'), row('peer')]
  g.members.push(member('child', 'marketing', { parentBotId: 'member' }), member('peer'))
  const pref = openAll()
  const before = projectTimes(items, g, pref)
  const target = nextTimesSibling(before, 'member', 1)
  assert.equal(target?.bot.id, 'peer', 'o filho intercalado não é o alvo')
  assert.equal(nextTimesSibling(before, 'peer', -1)?.bot.id, 'member')
  g.members.find((m) => m.botId === 'peer')!.parentBotId = 'chief'
  const current = projectTimes(items, g, pref)
  assert.equal(moveTimesMember(current, pref, 'member', target!.bot.id), pref, 'seleção antiga não autoriza mover quem mudou de pai')
  assert.equal(nextTimesSibling(current, 'member', 1), null, 'sem outro irmão válido não há fallback')
  assert.equal(nextTimesSibling(before, 'chief', 1), null, 'Chefe nunca é origem de reordenação')
  assert.equal(nextTimesSibling(before, 'member', -1), null, 'Chefe não é alvo nem há volta pelo fim')
  assert.equal(nextTimesSibling(before, 'peer', 1), null, 'o último irmão não dá a volta na lista')
  assert.equal(nextTimesSibling(before, 'member', 0), null, 'somente direções acima/abaixo são válidas')
})
