import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Space, SpacesSnapshot } from '../../shared/fork-spaces.ts'
import { failureOf, movedOrder, originRefusal as recusaDaOrigem, railSpaces, railStore, spaceErrorText as textoDoErro } from './fork-spaces-rail.ts'
import { textoDe, type Frase } from './fork-i18n/frase.ts'

// D454 (fatia 9c) e D507: as regras devolvem `Frase`; aqui o texto pt-BR dela, sem as marcas de isolamento.
const pt = (f: Frase | null) => (f ? textoDe(f, 'pt-BR').replace(/[\u2066-\u2069]/g, '') : null)
const spaceErrorText = (e: unknown, reread: boolean | null) => pt(textoDoErro(e, reread))!
const originRefusal = (...a: Parameters<typeof recusaDaOrigem>) => pt(recusaDaOrigem(...a))

// D169 (SpaceRail): a faixa mostra os spaces de um Hub só, manda ao Hub a ordem inteira com Pessoal primeiro e
// explica a recusa em pt-BR; sem resposta ou com revisão velha, relê em vez de repetir.

const PERSONAL = 'p'
const space = (id: string, position: number): Space => ({
  id, kind: id === PERSONAL ? 'personal' : 'space', name: id, position, revision: 1,
  tile: id === PERSONAL ? null : { color: 'musgo', style: 'filled', symbol: { kind: 'icon', value: 'leaf' } },
  appearanceConfigured: true, appearance: { wallpaper: 'none', blur: 0, transparency: 50, veilLight: 0.18, veilDark: 0.32 }, owner: null,
})
const list = (ids: string[]): SpacesSnapshot => ({ version: 1, personalSpaceId: PERSONAL, revision: 7, spaces: ids.map((id, i) => space(id, i)) })
const hub = (hostId: string, spaces: SpacesSnapshot | null, contract = 1) => ({ hello: { hostId, spacesContract: contract }, spaces })

test('o Hub da faixa: o selecionado com spaces, senão o único; dois sem seleção não mostram nenhum', () => {
  const a = hub('a', list([PERSONAL, 'x']))
  const b = hub('b', list([PERSONAL]))
  const old = hub('c', list([PERSONAL]), 0)
  const empty = hub('d', null)
  assert.equal(railStore([a, old, empty], null), a)
  assert.equal(railStore([a, b], b), b)
  assert.equal(railStore([a, b], null), null)
  assert.equal(railStore([a, b], old), null, 'um Hub sem o contrato de Spaces não entra')
})

test('spaces depois do Pessoal, na ordem do dono; mover manda a lista inteira com Pessoal primeiro', () => {
  const snapshot = list([PERSONAL, 'x', 'y', 'z'])
  snapshot.spaces.reverse()
  assert.deepEqual(railSpaces(snapshot).map((s) => s.id), ['x', 'y', 'z'])
  assert.deepEqual(movedOrder(snapshot, 'y', -1), [PERSONAL, 'y', 'x', 'z'])
  assert.deepEqual(movedOrder(snapshot, 'y', 1), [PERSONAL, 'x', 'z', 'y'])
  assert.equal(movedOrder(snapshot, 'x', -1), null)
  assert.equal(movedOrder(snapshot, 'z', 1), null)
  assert.equal(movedOrder(snapshot, PERSONAL, 1), null, 'o Pessoal não sai do lugar')
})

test('sem resposta ou revisão velha relê; a recusa vira pt-BR sem o código', () => {
  const unknown = Object.assign(new Error('timeout'), { status: 0 })
  const conflict = Object.assign(new Error('spaces_revision_conflict'), { status: 409 })
  assert.equal(failureOf(unknown), 'unknown')
  assert.equal(failureOf(conflict), 'conflict')
  assert.equal(failureOf(new Error('invalid_tile_icon')), 'refused')
  assert.match(spaceErrorText(unknown, true), /não respondeu.*foi relida.*antes de tentar de novo/)
  assert.match(spaceErrorText(unknown, false), /não pôde ser relida/)
  assert.match(spaceErrorText(conflict, true), /Outro aparelho.*foi relida.*salve de novo/)
  assert.equal(spaceErrorText(new Error('invalid_png_dimensions'), null), 'O logo pode ter até 1024 × 1024 pixels.')
  assert.equal(spaceErrorText(new Error('invalid_png_crc'), null), 'O Hub não conseguiu ler esse PNG. Exporte o logo de novo.')
  assert.match(spaceErrorText(new Error('remote_setup_unavailable: gestão remota de provedores ainda indisponível.'), null), /conexão local com o Hub/)
  assert.match(spaceErrorText(new Error('space_principal_fixed'), null), /Berga/)
  assert.match(spaceErrorText(new Error('space_delete_pending'), null), /exclusão.*não.*realizada/)
  const other = spaceErrorText(new Error('algo_novo_do_hub'), null)
  assert.equal(other, 'O Hub recusou o pedido. Confira e tente de novo.')
  assert.ok(!/_/.test(other))
})

test('a origem da folha fica fixa: outro Hub, link novo ou lista mais nova não enviam nada (Sol R1-F1/F2)', () => {
  const linkA = { link: 'A' }
  const origin = { hostId: 'hub-a', client: linkA, epoch: 3, revision: 7 }
  assert.equal(originRefusal({ ...origin }, origin), null, 'sem troca, o envio segue com a revisão da abertura')
  // R1-F1: o mesmo store aceitou o hello do Hub B antes de Salvar.
  assert.match(originRefusal({ hostId: 'hub-b', client: { link: 'B' }, epoch: 4, revision: 2 }, origin) ?? '', /outro Hub.*Nada foi enviado/)
  assert.match(originRefusal({ ...origin, client: { link: 'A2' }, epoch: 4 }, origin) ?? '', /conexão com o Hub mudou.*Nada foi enviado/)
  assert.match(originRefusal({ ...origin, client: null }, origin) ?? '', /Sem conexão.*Nada foi enviado/)
  // R1-F2: rascunho da revisão 7; um evento (ou a releitura depois de um 409) trouxe a 8.
  assert.match(originRefusal({ ...origin, revision: 8 }, origin) ?? '', /lista de spaces mudou.*Nada foi enviado/)
  assert.match(originRefusal({ ...origin, revision: null }, origin) ?? '', /Nada foi enviado/)
})
