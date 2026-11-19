import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { SIDEBAR_SECTIONS, SIDEBAR_WIDTH, clampSidebarWidth, isStringList, moveSection, navigableRows, rowTime, sectionOrder, splitChannels, visibleSections } from './sidebar-layout.ts'

// D165 fase C: ordem, visibilidade e navegação da barra lateral, sem perder linhas.

test('ordem salva ignora ids desconhecidos e completa as seções que faltam', () => {
  assert.deepEqual(sectionOrder([]), ['channels', 'teams', 'agents'])
  assert.deepEqual(sectionOrder(['agents', 'x', 'agents']), ['agents', 'channels', 'teams'])
  assert.deepEqual(visibleSections(['teams', 'channels'], ['channels']), ['teams', 'agents'])
})

test('Barra04: quem tinha Bots guardado em ordem, escondido ou recolhido leva Equipes e Agentes juntas', () => {
  assert.deepEqual(sectionOrder(['bots', 'channels']), ['teams', 'agents', 'channels'])
  assert.deepEqual(visibleSections([], ['bots']), ['channels'])
  assert.deepEqual(navigableRows(['channels', 'teams', 'agents'], ['bots'], { channels: ['g'], teams: ['t'], agents: ['a'] }), ['g'])
})

test('mover seção respeita as pontas', () => {
  assert.deepEqual(moveSection([], 'teams', -1), ['teams', 'channels', 'agents'])
  assert.deepEqual(moveSection([], 'channels', -1), ['channels', 'teams', 'agents'])
  assert.deepEqual(moveSection([], 'agents', 1), ['channels', 'teams', 'agents'])
})

test('canais são os grupos, na ordem de origem, e a navegação segue a ordem e o recolhimento', () => {
  const rows = [{ bot: { kind: 'agent', id: 'a' } }, { bot: { kind: 'group', id: 'g' } }, { bot: { kind: 'agent', id: 'b' } }]
  const { channels, bots } = splitChannels(rows)
  assert.deepEqual(channels.map((r) => r.bot.id), ['g'])
  assert.deepEqual(bots.map((r) => r.bot.id), ['a', 'b'])
  assert.deepEqual(navigableRows(['channels', 'teams', 'agents'], [], { channels, teams: bots.slice(0, 1), agents: bots.slice(1) }).map((r) => r.bot.id), ['g', 'a', 'b'])
  assert.deepEqual(navigableRows(['teams', 'agents', 'channels'], ['teams'], { channels, teams: bots.slice(0, 1), agents: bots.slice(1) }).map((r) => r.bot.id), ['b', 'g'])
})

test('vocabulário (dono, 05/11/2026): a conversa com vários bots se chama canal; o dado continua `group`', () => {
  assert.deepEqual(SIDEBAR_SECTIONS.map((s) => s.label), ['Canais', 'Equipes', 'Agentes'])
  const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')
  const telas = ['../views/BotRoster.tsx', '../views/ChatWindow.tsx', '../views/bots/NewBotView.tsx', '../views/bots/GroupEditorView.tsx', '../components/Avatar.tsx', './fork-i18n/pt-BR/navegacao/janela.ts', './fork-i18n/pt-BR/navegacao/barra.ts', './fork-i18n/pt-BR/bots/editor.ts'] // fatias 1 e 3 (D454): os textos dessas telas vêm do catálogo pt-BR
  const texto = telas.map(read).join('\n')
  for (const novo of ['"Canais"', 'Novo canal', 'Nenhum canal ainda.', 'Configurações do canal…', 'Apagar canal e a conversa', 'Nome do canal', 'No que este canal trabalha', 'Canal: ']) assert.ok(texto.includes(novo), novo)
  for (const antigo of ['title="Grupos"', 'Novo grupo', 'Nenhum grupo', 'do grupo…', 'Apagar grupo', 'Nome do grupo', 'este grupo', 'Grupo: ']) assert.ok(!texto.includes(antigo), antigo)
})

test('hora da linha: hoje, ontem e data curta', () => {
  const now = new Date(2026, 9, 24, 15, 0)
  assert.equal(rowTime(new Date(2026, 9, 24, 9, 12).getTime(), now), '09:12')
  assert.equal(rowTime(new Date(2026, 9, 23, 22, 0).getTime(), now), 'ontem')
  assert.equal(rowTime(new Date(2026, 9, 14, 8, 0).getTime(), now), '14 out')
  assert.equal(rowTime(0, now), '')
})

test('preferência com JSON válido de tipo errado não derruba a barra (C3)', () => {
  for (const bad of [null, 'bots', {}, [1, 'bots'], 3]) assert.equal(isStringList(bad), false)
  assert.ok(isStringList([]))
  assert.ok(isStringList(['bots', 'teams']))
  assert.deepEqual(sectionOrder(null as unknown as string[]), ['channels', 'teams', 'agents'])
  assert.deepEqual(visibleSections(['agents'], {} as unknown as string[]), ['agents', 'channels', 'teams'])
})

test('largura da barra: padrão 280, arrasto de 200 a 360', () => {
  assert.equal(SIDEBAR_WIDTH.default, 280)
  assert.equal(clampSidebarWidth(150), 200)
  assert.equal(clampSidebarWidth(400), 360)
  assert.equal(clampSidebarWidth(287.6), 288)
  assert.equal(clampSidebarWidth(Number.NaN), 280)
})
