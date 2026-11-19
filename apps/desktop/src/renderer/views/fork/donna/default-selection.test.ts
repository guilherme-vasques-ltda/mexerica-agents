import { test } from 'node:test'
import assert from 'node:assert/strict'
import { defaultSelection, initialTrack, trackSelection, type DefaultHub, type DefaultInput, type Row, type SelectionTrack } from './default-selection.ts'
import { firstConversation, principalConversation } from '../../../lib/fork-space-context.ts'
import type { SpaceConversation } from '../../../../shared/fork-spaces.ts'

// Abertura no Berga (dono, 03/11/2026): o app abre na principal; "Novo bot" só sem nenhum bot depois da conexão.

const row = (botId: string, group = false, computerId = 'mac'): Row => ({ ref: { computerId, botId }, group })
const hub = (extra: Partial<DefaultHub> = {}): DefaultHub => ({ computerId: 'mac', principalId: 'donna', phase: 'ready', reachable: true, ...extra })
const donna = row('donna'), research = row('research'), joio = row('joio'), canal = row('canal', true)
const input = (extra: Partial<DefaultInput> = {}): DefaultInput => ({
  selection: null, items: [donna, canal, research, joio], rows: [canal, research, joio], hub: hub(), connecting: false, settled: true, manual: false, pinned: false, ...extra,
})

test('sem seleção, abre a Donna do Hub do trilho', () => {
  assert.deepEqual(defaultSelection(input()), { kind: 'select', ref: donna.ref })
  // A principal guardada abre antes de a consulta terminar.
  assert.deepEqual(defaultSelection(input({ hub: hub({ phase: 'loading' }) })), { kind: 'select', ref: donna.ref })
})

test('sem Donna, abre a primeira conversa direta com bot, pulando grupos', () => {
  assert.deepEqual(defaultSelection(input({ hub: hub({ principalId: null }) })), { kind: 'select', ref: research.ref })
  // Erro na consulta sem principal guardada: também o primeiro bot.
  assert.deepEqual(defaultSelection(input({ hub: hub({ principalId: null, phase: 'error' }) })), { kind: 'select', ref: research.ref })
  // Grupo só quando não há bot nenhum.
  assert.deepEqual(defaultSelection(input({ hub: hub({ principalId: null }), items: [canal], rows: [canal] })), { kind: 'select', ref: canal.ref })
})

test('sem bots confirmado, a tela vazia; conectando, "Conectando…"', () => {
  assert.deepEqual(defaultSelection(input({ items: [], rows: [] })), { kind: 'empty' })
  assert.deepEqual(defaultSelection(input({ items: [], rows: [], connecting: true })), { kind: 'connecting' })
  assert.deepEqual(defaultSelection(input({ items: [], rows: [], hub: null })), { kind: 'empty' })
})

test('R1-2: lista vazia sem ausência confirmada (principal unknown/loading, stream recém-online) é wait, nunca "Novo bot"', () => {
  for (const phase of ['unknown', 'loading'] as const) {
    assert.deepEqual(defaultSelection(input({ items: [], rows: [], hub: hub({ principalId: null, phase }), settled: false })), { kind: 'wait' })
  }
  // Stream online antes dos eventos dos bots, principal ainda sem resposta: espera.
  assert.deepEqual(defaultSelection(input({ items: [], rows: [], hub: hub({ principalId: 'donna', phase: 'loading' }), settled: false })), { kind: 'wait' })
  // Um computador mostrado ainda sem stream impede a confirmação, mesmo com a principal do trilho pronta.
  assert.deepEqual(defaultSelection(input({ items: [], rows: [], hub: hub({ principalId: null }), settled: false })), { kind: 'wait' })
})

test('a seleção válida é preservada, inclusive de outro computador', () => {
  assert.deepEqual(defaultSelection(input({ selection: joio.ref, manual: true })), { kind: 'keep' })
  const other = row('vitrine', false, 'servidor')
  assert.deepEqual(defaultSelection(input({ selection: other.ref, items: [donna, other], manual: true })), { kind: 'keep' })
  // A pessoa escolheu outro bot: nem a consulta confirmando a principal o troca.
  assert.deepEqual(defaultSelection(input({ selection: joio.ref, manual: true })), { kind: 'keep' })
})

test('o bot aberto sumiu (apagado, ocultado, computador escondido ou Hub trocado): volta para a Donna', () => {
  assert.deepEqual(defaultSelection(input({ selection: row('crivo').ref })), { kind: 'select', ref: donna.ref })
  // Mesmo id em outro computador não conta como o mesmo bot.
  assert.deepEqual(defaultSelection(input({ selection: row('joio', false, 'antigo').ref })), { kind: 'select', ref: donna.ref })
})

test('aberto de propósito fora da lista (computador escondido no filtro) continua; se sumir do Hub, volta para a Donna', () => {
  const escondido = row('vitrine', false, 'servidor')
  assert.deepEqual(defaultSelection(input({ selection: escondido.ref, pinned: true })), { kind: 'keep' })
  assert.deepEqual(defaultSelection(input({ selection: escondido.ref, pinned: false })), { kind: 'select', ref: donna.ref })
})

test('fase desconhecida com bots: painel neutro, nunca "Novo bot"', () => {
  for (const phase of ['unknown', 'loading'] as const) {
    assert.deepEqual(defaultSelection(input({ hub: hub({ principalId: null, phase }) })), { kind: 'wait' })
  }
  // Hub fora do ar sem principal guardada: não há consulta para esperar.
  assert.deepEqual(defaultSelection(input({ hub: hub({ principalId: null, phase: 'unknown', reachable: false }) })), { kind: 'select', ref: research.ref })
})

test('a consulta confirma outra principal: troca só enquanto a seleção ainda é a automática', () => {
  const nova = hub({ principalId: 'research' })
  assert.deepEqual(defaultSelection(input({ hub: nova, selection: donna.ref })), { kind: 'select', ref: research.ref })
  assert.deepEqual(defaultSelection(input({ hub: { ...nova, phase: 'loading' }, selection: donna.ref })), { kind: 'keep' })
  assert.deepEqual(defaultSelection(input({ hub: nova, selection: donna.ref, manual: true })), { kind: 'keep' })
})

const at = (seq: number, key: string | null, manual = false, outside = false): SelectionTrack => ({ seq, key, manual, outside })

test('R1-1: abrir de novo a MESMA conversa (notificação, Buscar, clique) a torna manual', () => {
  // A escolha automática abriu o Berga salvo (seq 1 = autoSeq).
  const auto = trackSelection(at(0, null), { seq: 1, key: 'mac/donna', listed: true, autoSeq: 1 })
  assert.equal(auto.manual, false)
  // A pessoa abre a mesma Donna: a chave não muda, mas o select conta.
  const again = trackSelection(auto, { seq: 2, key: 'mac/donna', listed: true, autoSeq: 1 })
  assert.equal(again.manual, true)
  // A consulta confirma outra principal: a escolha da pessoa fica.
  assert.deepEqual(defaultSelection(input({ hub: hub({ principalId: 'research' }), selection: donna.ref, manual: again.manual })), { kind: 'keep' })
  // Mesma referência reaberta pela notificação com o computador já escondido no filtro: fica fixada.
  const hidden = trackSelection(again, { seq: 3, key: 'mac/donna', listed: false, autoSeq: 1 })
  assert.deepEqual(hidden, at(3, 'mac/donna', true, true))
  // Render sem select não muda nada; vista de novo na lista, deixa de ser "fora da lista".
  assert.equal(trackSelection(hidden, { seq: 3, key: 'mac/donna', listed: false, autoSeq: 1 }), hidden)
  assert.equal(trackSelection(hidden, { seq: 3, key: 'mac/donna', listed: true, autoSeq: 1 }).outside, false)
  // Seleção que some sem select (computador removido) nunca é da pessoa.
  assert.deepEqual(trackSelection(hidden, { seq: 3, key: null, listed: false, autoSeq: 1 }), at(3, null))
})

test('R2-2: seleção anterior à primeira montagem (openBot com o acesso ao computador na tela) nasce manual e, filtrada, fixada', () => {
  const escondido = row('vitrine', false, 'servidor')
  // openBot selecionou um bot de computador escondido no filtro antes de a barra montar (seq 1, nenhuma escolha automática).
  const first = initialTrack({ seq: 1, key: 'servidor/vitrine', listed: false, autoSeq: -1 })
  assert.deepEqual(first, at(1, 'servidor/vitrine', true, true))
  // O gancho fixa (o bot existe em app.roster): o Berga não substitui a notificação.
  assert.deepEqual(defaultSelection(input({ selection: escondido.ref, manual: first.manual, pinned: first.outside })), { kind: 'keep' })
  // Renders seguintes sem select mantêm a fixação.
  assert.equal(trackSelection(first, { seq: 1, key: 'servidor/vitrine', listed: false, autoSeq: -1 }), first)
  // Na lista (computador mostrado), nasce manual sem fixação; a consulta com outra principal não a troca.
  const shown = initialTrack({ seq: 1, key: 'mac/joio', listed: true, autoSeq: -1 })
  assert.deepEqual(shown, at(1, 'mac/joio', true, false))
  assert.deepEqual(defaultSelection(input({ selection: joio.ref, manual: shown.manual })), { kind: 'keep' })
  // Remontagem depois de uma escolha automática (seq === autoSeq): continua automática.
  assert.deepEqual(initialTrack({ seq: 4, key: 'mac/donna', listed: true, autoSeq: 4 }), at(4, 'mac/donna'))
  // Sem seleção: nada a proteger.
  assert.deepEqual(initialTrack({ seq: 0, key: null, listed: false, autoSeq: -1 }), at(0, null))
})

test('R1-3: queda de A com B online não troca de Hub; a conversa em cache de A continua', () => {
  const a = row('donna', false, 'a'), b = row('donna', false, 'b')
  const items = [a, row('joio', false, 'a'), b]
  // A caiu, o trilho passou a B (pronto, com outra Donna): a seleção automática de A é mantida.
  assert.deepEqual(defaultSelection(input({ items, selection: a.ref, hub: hub({ computerId: 'b', principalId: 'donna' }) })), { kind: 'keep' })
  // No mesmo Hub, a principal alterada ainda troca.
  assert.deepEqual(defaultSelection(input({ items, selection: a.ref, hub: hub({ computerId: 'a', principalId: 'joio' }) })), { kind: 'select', ref: { computerId: 'a', botId: 'joio' } })
})

const conversation = (id: string, botId: string, extra: Partial<SpaceConversation> = {}): SpaceConversation => ({
  id, spaceId: 's', botId, title: id, revision: 1, stateRevision: 1, primary: true, closed: false, readRevision: 0,
  engineAccountId: null, engineSelection: null, accountRevision: null, continuity: null, ...extra,
})

test('trocar de Space abre a conversa fixa da Donna; sem ela, a primeira aberta', () => {
  const list = [conversation('crivo-fixa', 'crivo'), conversation('donna-fixa', 'donna'), conversation('extra', 'donna', { primary: false })]
  assert.equal(principalConversation(list, 'donna'), 'donna-fixa')
  assert.equal(principalConversation([conversation('donna-fixa', 'donna', { closed: true })], 'donna'), null)
  assert.equal(principalConversation(list, null), null)
  assert.equal(principalConversation([conversation('crivo-fixa', 'crivo')], 'donna') ?? firstConversation([conversation('crivo-fixa', 'crivo')]), 'crivo-fixa')
})
