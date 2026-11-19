import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SPACE_ICON_IDS } from '../../../../shared/fork-spaces.ts'
import { DUO_ICONS, DUO_PACKS } from './space-icons.ts'
import { clipName, drawable, iconsFor, isLogoURL, isPhotoURL, letters, logoProblem, NAME_MAX, spaceName, validTile } from './space-tile.ts'

// D169 (SpaceIconPicker): a tela só monta escolhas que o contrato aceita; quem valida e guarda é o Hub.
test('os quatro packs do design, com 30 ícones de formas simples e nomes em pt-BR', () => {
  assert.deepEqual(DUO_PACKS.map((p) => p.name), ['Negócios', 'Comércio', 'Tecnologia', 'Pessoal'])
  const ids = DUO_PACKS.flatMap((p) => p.icons)
  assert.equal(ids.length, 30)
  for (const id of ids) {
    const icon = DUO_ICONS[id]!
    assert.ok(icon.name && !/script|foreignObject|href|on[a-z]+=|style=/i.test(icon.back + icon.front), id)
  }
  assert.deepEqual(ids, [...SPACE_ICON_IDS], 'os ids e a ordem são os do Hub (SPACE_ICON_IDS)')
  assert.deepEqual(Object.keys(DUO_ICONS).sort(), [...SPACE_ICON_IDS].sort())
  assert.deepEqual(iconsFor('negocios', ''), DUO_PACKS[0]!.icons)
  assert.deepEqual(iconsFor('negocios', 'predio'), ['building'], 'busca ignora acento e atravessa os packs')
})

test('letras e nome seguem a regra do Hub: letras alfanuméricas, nome aparado até 60 sem cortar caractere', () => {
  assert.equal(letters('gv'), 'GV')
  assert.equal(letters(' ça va '), 'ÇA')
  assert.equal(letters('abc'), 'AB')
  assert.equal(letters('a-'), 'A', 'o Hub recusa pontuação nas letras')
  assert.equal(letters('ß'), 'SS', 'maiúscula antes do corte')
  assert.equal(letters('1a'), '1A')
  assert.equal(NAME_MAX, 60)
  assert.equal(spaceName('  Loja  da Ana  '), 'Loja  da Ana', 'espaços internos ficam: editar não reescreve o nome')
  assert.equal(spaceName('x'.repeat(80)).length, 60)
  const emojis = spaceName('😀'.repeat(61))
  assert.equal([...emojis].length, 60)
  assert.equal(emojis.length, 120, 'nenhum surrogate solto')
  assert.equal(spaceName('A\tB\u0007'), 'A B', 'caractere de controle vira espaço')
})

test('símbolo desconhecido, letras fora do formato ou logo/foto que não seja data URL inline não desenham', () => {
  assert.equal(drawable({ kind: 'icon', value: 'nao-existe' }), null)
  assert.equal(drawable({ kind: 'letters', value: 'abc' }), null)
  assert.equal(drawable({ kind: 'letters', value: '' }), null)
  assert.equal(drawable({ kind: 'letters', value: 'A-' }), null)
  assert.ok(drawable({ kind: 'letters', value: 'ab' }), 'o Hub aceita minúsculas: o tile desenha em vez de virar o padrão')
  assert.equal(drawable({ kind: 'icon', value: 'scales' }), null, 'nome do bundle, não do Hub')
  assert.ok(drawable({ kind: 'icon', value: 'scale' }))
  assert.equal(drawable({ kind: 'logo', value: 'https://exemplo.invalid/logo.png' }), null)
  assert.equal(drawable({ kind: 'logo', value: 'file:///Users/x/logo.svg' }), null)
  assert.ok(isLogoURL('data:image/svg+xml;base64,PHN2Zy8+'))
  assert.ok(!isLogoURL('data:image/svg+xml,<svg onload=1>'))
  assert.ok(!isPhotoURL('data:image/svg+xml;base64,PHN2Zy8+'), 'foto do dono só PNG')
  assert.ok(!isPhotoURL('data:image/jpeg;base64,/9j/4AAQ'), 'o Hub não aceita JPEG')
  assert.ok(isPhotoURL('data:image/png;base64,iVBORw0KGgo='))
  assert.ok(validTile({ color: 'musgo', style: 'soft', symbol: { kind: 'icon', value: 'leaf' } }))
  assert.ok(!validTile({ color: 'rosa' as never, style: 'filled', symbol: { kind: 'icon', value: 'leaf' } }))
})

test('logo: só PNG ou SVG até 256 KB saem da tela', () => {
  assert.equal(logoProblem({ type: 'image/png', size: 1000 }), null)
  assert.deepEqual(logoProblem({ type: 'image/jpeg', size: 1000 }), { chave: 'spaces.recusa.formato' })
  assert.deepEqual(logoProblem({ type: 'image/svg+xml', size: 300 * 1024 }), { chave: 'spaces.recusa.logoGrande' })
})

test('o campo do nome conta pontos de código como o Hub, sem cortar emoji e sem aparar ao digitar (Sol R1-O1)', () => {
  const thirtyOne = '😀'.repeat(31)
  assert.equal(clipName(thirtyOne), thirtyOne, '31 emojis cabem: são 31 caracteres para o Hub')
  const clipped = clipName('😀'.repeat(61))
  assert.equal([...clipped].length, NAME_MAX)
  assert.equal(clipped.length, NAME_MAX * 2, 'nenhum surrogate solto')
  assert.equal(clipName('x'.repeat(61)).length, NAME_MAX)
  assert.equal(clipName('Loja da Ana '), 'Loja da Ana ', 'o espaço do fim só sai ao salvar')
})
