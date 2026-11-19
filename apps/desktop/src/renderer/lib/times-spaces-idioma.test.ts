import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { CATALOGOS } from './fork-i18n/index.ts'
import { textoDe, type Frase } from './fork-i18n/frase.ts'
import { originRefusal, spaceErrorText } from './fork-spaces-rail.ts'
import { botName, continuityText, conversationLabel, readErrorText } from './fork-space-context.ts'
import { logoProblem } from '../views/fork/spaces/space-tile.ts'
import { MARKETING, planoMarketing } from '../views/fork/times/modelo-marketing.ts'
import { plural } from './fork-i18n/frase.ts'
import { tradutor } from './fork-i18n/index.ts'
import { structureTree } from '../views/fork/times/structure.ts'
import { boardLayout } from '../views/fork/times/board-layout.ts'
import type { SpaceConversation } from '../../shared/fork-spaces.ts'

// D454 (fatia 9c): Times e Spaces no idioma escolhido, com as lições das R1 anteriores. O que as regras e as sessões
// guardam é `Frase` (dado, D507) e se traduz ao desenhar; nomes e títulos vêm como dado; duais do árabe próprios;
// contagens de itens com o zero natural; código e horas LTR; superfície e portais com lang/dir.

const fonte = (a: string) => readFileSync(new URL(`../views/${a}`, import.meta.url), 'utf8')

test('D507: as regras de Spaces devolvem frase como dado; a mesma recusa sai em inglês e em árabe sem reler nada', () => {
  const recusa = spaceErrorText(new Error('invalid_png_dimensions'), null)
  assert.deepEqual(recusa, { chave: 'spaces.recusa.logoDimensoes' })
  assert.deepEqual(structuredClone(recusa), recusa)
  assert.equal(textoDe(recusa, 'en'), 'The logo can be up to 1024 × 1024 pixels.')
  assert.match(textoDe(recusa, 'ar-AE'), /الشعار/)
  const conflito = spaceErrorText(Object.assign(new Error('spaces_revision_conflict'), { status: 409 }), true)
  assert.equal(textoDe(conflito, 'en'), 'Another device changed the spaces while you were editing. The list of spaces was read again. Check and save again.')
  const origem = { hostId: 'a', client: 1, epoch: 1, revision: 1 }
  assert.equal(textoDe(originRefusal({ ...origem, revision: 2 }, origem)!, 'en'), 'The list of spaces changed after this was opened, here or on another device. Nothing was sent: close and open it again to see the current version.')
  assert.deepEqual(logoProblem({ type: 'image/gif', size: 1 }), { chave: 'spaces.recusa.formato' })
})

test('space aberto: nome do bot e título do Hub são dado; continuidade com plural e dual do árabe; erro de leitura como frase', () => {
  const fixa = { id: 'c', primary: true, title: 'x', botId: 'b' } as unknown as SpaceConversation
  assert.equal(textoDe(conversationLabel(fixa, botName('b', { id: 'b', name: 'Ana' }, new Map())), 'en'), 'Pinned conversation with ⁨Ana⁩')
  assert.equal(textoDe(conversationLabel({ ...fixa, primary: false, title: 'Plano' }, botName('z', null, new Map())), 'ar-AE'), 'Plano')
  assert.equal(textoDe(botName('z', null, new Map()), 'en'), 'Bot in this space')
  const conv = (messages: number) => ({ continuity: { mode: 'fresh_reconstructed', coverage: { messages } } }) as unknown as SpaceConversation
  assert.equal(textoDe(continuityText(conv(2))!, 'ar-AE'), 'بدأت بآخر رسالتين مرئيتين من هذه المحادثة، دون مرفقات أو أدوات أو موافقات.')
  assert.match(textoDe(continuityText(conv(12))!, 'en'), /^Started with the last 12 visible messages/)
  const erro = readErrorText(Object.assign(new Error('x'), { status: 0 }))
  assert.equal(erro.kind, 'offline')
  assert.equal(textoDe(erro.text, 'en'), 'No connection to the Hub.')
})

test('modelo Marketing: o bloqueio é frase; as regras da equipe que vão às instruções dos bots ficam em pt-BR, marcadas', () => {
  const graph = { nodes: [{ id: 'm', name: 'Marketing', parentId: null, managerBotId: null, position: 0 }], members: [], routes: [] } as never
  const bloqueio = planoMarketing(graph, [], null).bloqueio as Frase
  assert.equal(textoDe(bloqueio, 'en'), 'There’s already a top-level Marketing area. The template doesn’t change it.')
  assert.match(fonte('fork/times/TimesModeloSheet.tsx'), /className="times-modelo-rules" lang="pt-BR" dir="ltr"/)
})

test('duais do árabe (R1 do #570): a forma two de times.quadro.* e spaces.* não repete o singular nem usa "2 + singular"', () => {
  const ar = CATALOGOS['ar-AE'] as Record<string, string>
  const bases = Object.keys(ar).filter((k) => (k.startsWith('times.quadro.') || k.startsWith('spaces.')) && k.endsWith('.two')).map((k) => k.slice(0, -4))
  assert.ok(bases.length >= 15, String(bases.length))
  for (const b of bases) {
    assert.notEqual(ar[`${b}.two`], ar[`${b}.one`], b)
    assert.doesNotMatch(ar[`${b}.two`]!, /(^|\s)2\s/, b)
  }
})

test('D507 (#580): contagens de itens da 9c têm o zero natural nos 6 idiomas', () => {
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
    const cat = CATALOGOS[idioma] as Record<string, string>
    for (const base of ['times.quadro.contagem.bots', 'times.quadro.contagem.subareas', 'times.quadro.lista.foraAreas', 'times.quadro.barra.semChefeAria']) {
      assert.ok(cat[`${base}.zero`], `${idioma}: ${base}.zero`)
      assert.doesNotMatch(cat[`${base}.zero`]!, /\{n\}/, `${idioma}: ${base}.zero`)
    }
  }
})

test('bidi e superfícies (fonte): horas LTR, nomes e conteúdo do Hub em dir="auto", raízes e portais com lang/dir', () => {
  const view = fonte('fork/spaces/SpaceView.tsx')
  assert.match(view, /<div className="space-view idioma-superficie" \{\.\.\.superficieIdioma\(idioma\)\}>/)
  assert.match(view, /const clock = \(t: number, idioma: AppLanguage\) => ltr\(/)
  assert.match(view, /<AnchoredMenu [\s\S]*?language=\{superficieIdioma\(idioma\)\} \/>/)
  assert.match(view, /<span className="space-view-text" dir="auto">\{text\}<\/span>/)
  assert.doesNotMatch(view, /'pt-BR'/)
  assert.match(fonte('fork/spaces/SpaceIconPicker.tsx'), /className="space-picker idioma-superficie" \{\.\.\.superficieIdioma\(idioma\)\}/)
  const times = fonte('fork/times/TimesView.tsx')
  // O Sheet abre num portal no body (pt-BR/LTR): o conteúdo declara o idioma.
  assert.match(times, /<Sheet [\s\S]*?label=\{tx\(pickerLabel\(picker\)\)\}>\s*\{\/\*[^*]*\*\/\}\s*<div className="idioma-superficie" \{\.\.\.superficie\} style=\{\{ display: 'contents' \}\}>/)
  assert.match(times, /<AnchoredMenu [\s\S]*?language=\{superficie\} \/>/)
  assert.match(times, /<div className="times-page idioma-superficie" \{\.\.\.superficie\}>/)
  assert.match(fonte('fork/times-sidebar/AgentGroups.tsx'), /<AnchoredMenu [\s\S]*?language=\{superficieIdioma\(idioma\)\} \/>/)
  // Todo ModalHeader da fatia passa o fechar do catálogo (lição da R1 do #577).
  for (const a of ['fork/times/TimesAreaSheet.tsx', 'fork/times/TimesPickers.tsx', 'fork/times/TimesModeloSheet.tsx']) {
    const s = fonte(a)
    assert.equal(s.split('<ModalHeader').length - 1, s.split('closeTitle=').length - 1, a)
  }
  for (const a of ['fork/times/TimesBoard.tsx', 'fork/times/TimesInspector.tsx', 'fork/times/TimesList.tsx', 'fork/times-sidebar/TimesSidebarList.tsx', 'fork/spaces/SpaceView.tsx']) {
    assert.doesNotMatch(fonte(a), /toLocale(Time|Date)?String\('pt-BR'/, a)
  }
})

test('R1-02 do #594 (P135): as funções do modelo Marketing aparecem pelo catálogo, nunca o texto fixo do preset', () => {
  const vagas = planoMarketing({ nodes: [], members: [], routes: [] } as never, [], null).vagas
  assert.equal(vagas.length, 1 + MARKETING.membros.length)
  // O mesmo que `rotuloDoPapel` (views/criar-bot/rotulos.ts, que o Node não carrega pelo alias @shared): `botRoles.<id>.nome`.
  const rotuloDoPapel = (t: ReturnType<typeof tradutor>, id: string) => (t as unknown as (k: string) => string)(`botRoles.${id}.nome`)
  for (const v of vagas) {
    const en = rotuloDoPapel(tradutor('en'), v.role.id)
    const ar = rotuloDoPapel(tradutor('ar-AE'), v.role.id)
    assert.ok(en && ar, v.role.id)
    assert.notEqual(ar, v.role.role, `${v.role.id}: ar-AE não repete o preset`)
  }
  assert.equal(rotuloDoPapel(tradutor('en'), 'social'), 'Social media')
  const folha = fonte('fork/times/TimesModeloSheet.tsx')
  assert.match(folha, /const funcao = \(role: \{ id: string; role: string \}\) => rotuloDoPapel\(t, role\.id\) \?\? role\.role/)
  assert.doesNotMatch(folha, /funcao: v\.role\.role/)
})

test('R1-03 do #594: área recolhida sem bots abaixo (só subárea vazia) mostra o zero natural, nunca "+0"', () => {
  const graph = { nodes: [{ id: 'a', name: 'A', parentId: null, managerBotId: null, position: 0 }, { id: 'b', name: 'B', parentId: 'a', managerBotId: null, position: 0 }], members: [], routes: [] } as never
  const layout = boardLayout(structureTree(graph, [], null), graph, { collapsed: new Set(['area:a']), offsets: {}, showRoutes: true })
  const a = layout.frames.find((f) => f.key === 'area:a')!
  assert.equal(a.collapsed, true)
  assert.equal(a.hiddenBelow, 0)
  const esperado = { 'pt-BR': 'nenhum bot', en: 'no bots', es: 'ningún bot', fr: 'aucun bot', 'zh-CN': '没有机器人', 'ar-AE': 'لا بوتات' } as const
  for (const [idioma, texto] of Object.entries(esperado) as [keyof typeof esperado, string][]) {
    assert.equal(textoDe(plural('times.quadro.board.maisBots', a.hiddenBelow), idioma), texto, idioma)
  }
  assert.equal(textoDe(plural('times.quadro.board.maisBots', 2), 'en'), '+2 bots')
})
