// D340 na barra (DON-865): a equipe com chefe gravado e sem outros membros aparece só pela linha do chefe, como no
// protótipo; a equipe sem chefe mantém a linha genérica com o aviso. A regra de gravar o chefe é do Hub (DON-835);
// aqui só a projeção (lib/times-roster) e a renderização real de TimesSidebarList, com estado e ícones isolados.
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { createElement, type ReactNode } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import ts from 'typescript'
import type { TimesMember, TimesSnapshot } from '../../../../shared/fork-times.ts'
import * as timesHelpers from '../../../lib/times-roster.ts'
import * as fraseMod from '../../../lib/fork-i18n/frase.ts'
import * as i18nMod from '../../../lib/fork-i18n/index.ts'
import * as bidiMod from '../../../../shared/bidi.ts'

const { emptyTimesPresentation, projectTimes, setTimesOpen, teamKey } = timesHelpers
const bot = (id: string) => ({ bot: { id, name: id, pinned: false, hidden: false, status: 'idle', unread: 0, title: '' } })
const member = (botId: string, teamId: string): TimesMember => ({ botId, teamId, parentBotId: null, role: 'executor', reportsTo: { kind: 'owner' }, ownerContact: 'reports' })
/** Duas equipes de topo: Solo (chefe gravado, sem outros membros) e Dupla (dois bots, sem chefe). */
const snapshot = (): TimesSnapshot => ({ version: 1, spaceId: 'personal', revision: 1, catalogVersion: 'c1', primaryBotId: 'donna', bots: [], routes: [],
  nodes: [
    { id: 'solo', name: 'Solo', parentId: null, managerBotId: 'chefe', position: 0 },
    { id: 'dupla', name: 'Dupla', parentId: null, managerBotId: null, position: 1 },
  ],
  members: [member('chefe', 'solo'), member('a', 'dupla'), member('b', 'dupla')] })
const items = () => ['chefe', 'a', 'b'].map(bot)

type Component = (props: Record<string, unknown>) => ReturnType<typeof createElement>
/** O idioma das telas carregadas (pt-BR nos testes antigos; R1-01 do #594 troca para ar-AE). */
let idiomaTeste: 'pt-BR' | 'en' | 'ar-AE' = 'pt-BR'
/** Transpila e carrega um componente da barra com estado e ícones isolados. */
function load(file: string, globals: Record<string, unknown> = {}): Record<string, Component> {
  const require = createRequire(import.meta.url)
  const source = readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')
  const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2023, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } })
  const exports: Record<string, Component> = {}
  runInNewContext(outputText, { ...globals, exports, require: (path: string) => {
    if (path === 'react' || path === 'react/jsx-runtime') return require(path)
    if (path === '../../../lib/times-roster') return timesHelpers
    if (path === '../../../lib/observable') return { useModel: (model: unknown) => model }
    // D454 (fatia 9c): textos do catálogo de verdade, no pt-BR.
    if (path === '../../../lib/fork-i18n/estado') return { useIdioma: () => idiomaTeste, idiomaAtual: () => idiomaTeste, superficieIdioma: i18nMod.superficieIdioma }
    if (path === '../../../lib/fork-i18n/frase') return fraseMod
    if (path === '../../../lib/fork-i18n/index') return i18nMod
    if (path === '@shared/bidi') return bidiMod
    if (path === '../../../components/Icon') return { Icon: () => null }
    if (path === '../../../components/Avatar') return { BotAvatar: () => null }
    if (path === '../../../components/Overlay') return { useEscape: () => {} }
    if (path === './TimesSidebarList') return load('TimesSidebarList.tsx')
    if (path === './Fold') return { warmFold: () => {}, Fold: ({ className, children }: { className: string; children: ReactNode }) => createElement('div', { className }, children) }
    if (path === '../times/navigation') return { timesScreen: { show: () => {} } }
    if (path === './prefs') return { updatePresentation: () => { throw new Error('renderizar não grava preferência') } }
    if (path === './times-sidebar.css') return {}
    throw new Error(`Dependência não prevista: ${path}`)
  } })
  return exports
}

function html(graph: TimesSnapshot, list: ReturnType<typeof items>, open: string[]) {
  const { TimesSidebarList } = load('TimesSidebarList.tsx')
  const pref = open.reduce((p, id) => setTimesOpen(p, teamKey(id), true), emptyTimesPresentation())
  const projection = projectTimes(list, graph, pref)
  return { projection, markup: renderToStaticMarkup(createElement(TimesSidebarList!, {
    store: { computer: { id: 'hub' }, timesRoster: { phase: 'ready', snapshot: graph, domain: 'd', scope: 1, retry: () => {} } },
    projection, compact: false, onEnter: () => {}, onOpen: () => {},
    renderRow: (item: { bot: { id: string } }, options: { lead: boolean }) => createElement('div', { key: item.bot.id, 'data-roster-row': '', 'data-lead': String(options.lead) }, item.bot.id),
  })) }
}

function render(open: boolean) {
  const { projection, markup } = html(snapshot(), items(), open ? ['solo', 'dupla'] : [])
  // Cada equipe de topo é um `.times-team`; separa o HTML de cada uma.
  const [solo, dupla] = markup.split('<div class="times-team"').slice(1)
  return { projection, solo: solo!, dupla: dupla! }
}

test('D340: equipe com chefe gravado e sem outros membros é só a linha do chefe', () => {
  for (const open of [false, true]) {
    const { projection, solo } = render(open)
    const group = projection.groups.find((g) => g.id === teamKey('solo'))!
    assert.equal(group.chiefId, 'chefe')
    assert.equal(group.checkChief, false)
    assert.deepEqual(group.rows.map((i) => i.bot.id), ['chefe'])
    assert.deepEqual([group.members.length, group.below.length], [0, 0])
    assert.match(solo, /class="times-lead has-chief"/)
    assert.match(solo, /data-lead="true">chefe</)
    assert.doesNotMatch(solo, /times-teamrow|times-team-tile/, 'sem a linha genérica com ícone de grupo')
    assert.doesNotMatch(solo, /Confira o chefe no quadro|Sem chefe definido/)
    assert.doesNotMatch(solo, /times-exp|aria-expanded/, 'sem seta de expandir quando não há membros')
    assert.doesNotMatch(solo, /times-members/, 'sem corpo vazio embaixo do chefe')
  }
})

test('D340: equipe com 2 bots e sem chefe mantém a linha genérica com o aviso', () => {
  for (const open of [false, true]) {
    const { projection, dupla } = render(open)
    const group = projection.groups.find((g) => g.id === teamKey('dupla'))!
    assert.equal(group.chiefId, null)
    assert.equal(group.checkChief, false)
    assert.deepEqual(group.members.map((i) => i.bot.id), ['a', 'b'])
    assert.match(dupla, /class="times-lead no-chief"/)
    assert.match(dupla, /class="times-teamrow"/)
    assert.match(dupla, /times-team-tile/)
    assert.match(dupla, /Sem chefe definido\./)
    assert.match(dupla, /class="times-exp"/)
    assert.match(dupla, /data-lead="false">a<.*data-lead="false">b</)
  }
})

const joioGraph = (): TimesSnapshot => ({ ...snapshot(),
  nodes: [{ id: 'youtube', name: 'Youtube', parentId: null, managerBotId: 'joio', position: 0 }, { id: 'roteiros', name: 'Roteiros', parentId: 'youtube', managerBotId: 'joio', position: 0 }],
  members: [member('joio', 'youtube'), member('crivo', 'roteiros')] })

test('Joio chefe de Youtube e de Roteiros: Youtube pela linha dele, Roteiros pela referência; oculto volta ao Confira', () => {
  const graph = joioGraph
  const { markup } = html(graph(), ['joio', 'crivo'].map(bot), ['youtube', 'roteiros'])
  const [youtube, roteiros] = markup.split('<div class="times-team"').slice(1)
  assert.match(youtube!, /data-lead="true">joio</)
  assert.match(roteiros!, /class="times-teamrow times-ref"/)
  assert.match(roteiros!, /times-ref-name" dir="auto">joio</)
  assert.match(roteiros!, /Chefe · em \u2068Youtube\u2069/, 'o nome da equipe entra isolado (D454)')
  assert.match(roteiros!, /data-lead="false">crivo</)
  assert.equal(markup.match(/data-roster-row/g)?.length, 2, 'o Joio é uma linha de bot só; a referência não é linha de membro')
  assert.doesNotMatch(markup, /Confira o chefe no quadro|Sem chefe definido/)
  const hidden = html(graph(), [{ bot: { ...bot('joio').bot, hidden: true } }, bot('crivo')], ['youtube', 'roteiros']).markup
  assert.equal(hidden.match(/Confira o chefe no quadro/g)?.length, 2)
  assert.doesNotMatch(hidden, /joio/, 'não revela o nome do chefe oculto')
})

test('Sala de Roteiros: a referência do Joio no topo, sem "Sem chefe definido"', () => {
  const { TeamRoom } = load('TeamRoom.tsx')
  const graph = joioGraph()
  const projection = projectTimes(['joio', 'crivo'].map(bot), graph, { ...emptyTimesPresentation(), open: [teamKey('youtube'), teamKey('roteiros')] })
  const markup = renderToStaticMarkup(createElement(TeamRoom!, { projection, teamId: teamKey('roteiros'), filtering: false, query: '',
    onGo: () => {}, onOrganize: () => {}, onOpen: () => {},
    renderRow: (item: { bot: { id: string } }) => createElement('div', { key: item.bot.id, 'data-roster-row': '' }, item.bot.id) }))
  assert.match(markup, /class="times-teamrow times-ref".*times-ref-name" dir="auto">joio<.*Chefe · em \u2068Youtube\u2069.*data-roster-row="">crivo</)
  assert.doesNotMatch(markup, /Sem chefe definido|Confira o chefe/)
})

test('Sala: ⌥↑/↓ na referência não chega ao atalho de mover o bot selecionado', () => {
  class El { ref = false; matches() { return false } closest(sel: string) { return sel === '.times-ref' && this.ref ? this : null } }
  const { roomKey } = load('TeamRoom.tsx', { HTMLElement: El }) as unknown as { roomKey: (e: object, up: () => void) => void }
  const fire = (key: string, ref: boolean) => {
    const seen = { stopped: false, prevented: false, up: false }
    const target = Object.assign(new El(), { ref })
    roomKey({ key, altKey: true, target, preventDefault: () => { seen.prevented = true }, stopPropagation: () => { seen.stopped = true } }, () => { seen.up = true })
    return seen
  }
  assert.deepEqual(fire('ArrowUp', true), { stopped: true, prevented: true, up: false })
  assert.deepEqual(fire('ArrowDown', true), { stopped: true, prevented: true, up: false })
  assert.deepEqual(fire('ArrowUp', false), { stopped: false, prevented: false, up: false }, 'linha de membro: segue para o atalho da lista')
})

test('R1-01 do #594: a lista de Equipes e a sala declaram o idioma na raiz (a barra as monta fora da superfície)', () => {
  idiomaTeste = 'ar-AE'
  try {
    const { markup } = html(snapshot(), items(), ['solo', 'dupla'])
    assert.match(markup, /^<div class="times-sidebar idioma-superficie" lang="ar-AE" dir="rtl" role="group" aria-label="الفرق">/)
    const { TeamRoom } = load('TeamRoom.tsx')
    const projection = projectTimes(items(), snapshot(), emptyTimesPresentation())
    const sala = renderToStaticMarkup(createElement(TeamRoom!, {
      projection, teamId: teamKey('dupla'), filtering: false, query: '', onGo: () => {}, onOrganize: () => {}, onOpen: () => {},
      renderRow: (item: { bot: { id: string } }) => createElement('div', { key: item.bot.id, 'data-roster-row': '' }, item.bot.id),
    }))
    assert.match(sala, /^<div class="team-room idioma-superficie" lang="ar-AE" dir="rtl">/)
    assert.match(sala, /لم يُحدَّد قائد/)
  } finally {
    idiomaTeste = 'pt-BR'
  }
})
