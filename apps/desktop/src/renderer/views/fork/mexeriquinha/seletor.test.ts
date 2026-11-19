import assert from 'node:assert/strict'
import { test } from 'node:test'
import { CATEGORIAS, MODELOS, modeloPorId } from '../../../lib/mexeriquinhas/index.ts'
import { camposAvatarDoBot, projetarAvatarLegado, randomAvatar, validAvatar } from '../../../../shared/fork-avatar-config.ts'
import { camposDaCriacao, comAvatar, comParte, configCriada, configDoPronto, filtrarModelos, gruposVisiveis, prontoMarcado, simplesDe } from './seletor.ts'

const berga = modeloPorId('berga')!

test('catálogo: 119 nomes em 17 categorias; busca sem acento e filtro por categoria', () => {
  assert.equal(filtrarModelos('').length, MODELOS.length)
  assert.equal(MODELOS.length, 119)
  assert.equal(gruposVisiveis('').length, 17)
  assert.equal(CATEGORIAS.length, 17)
  assert.deepEqual(filtrarModelos('BÉRGA').map((m) => m.id), ['berga'])
  const diretoria = filtrarModelos('', 'diretoria')
  assert.ok(diretoria.length > 0 && diretoria.every((m) => m.categoriaId === 'diretoria'))
  assert.deepEqual(gruposVisiveis('nada-com-esse-nome'), [])
})

test('pronto grava só aparência, nos dois estilos, sem nome/função/instruções do bot', () => {
  const s = configDoPronto(berga, 'simples')
  assert.deepEqual(Object.keys(s).sort(), ['aparencia', 'estilo', 'modeloId', 'v'])
  assert.deepEqual(Object.keys(s.estilo === 'simples' ? s.aparencia : {}).sort(), ['acessorio', 'cor', 'objeto', 'olhos', 'roupa'])
  const d = configDoPronto(berga, '3d')
  assert.deepEqual(d, { v: 2, estilo: '3d', modeloId: 'berga', conjuntoId: 'berga' })
  // Cópia: mexer no resultado não altera o catálogo.
  if (s.estilo === 'simples') s.aparencia.cor = 'roxo'
  assert.notEqual(berga.config.cor, 'roxo')
})

test('Crie o seu e peças: sem chave, sem nome pronto; voltar às peças do nome recupera o nome', () => {
  assert.deepEqual(configCriada(), { v: 2, estilo: 'simples', aparencia: { cor: 'laranja', olhos: 'ponto', roupa: 'nenhuma', acessorio: 'nenhum', objeto: 'nenhum' } })
  const pronto = configDoPronto(berga, 'simples')
  if (pronto.estilo !== 'simples') throw new Error('estilo')
  const editado = comParte(pronto, 'cor', 'roxo')
  assert.equal(editado.modeloId, undefined)
  assert.equal(editado.aparencia.cor, 'roxo')
  assert.equal(comParte(editado, 'cor', pronto.aparencia.cor).modeloId, undefined, 'sem nome de origem, continua criado')
  assert.equal(comParte(pronto, 'cor', pronto.aparencia.cor).modeloId, 'berga')
})

test('marcação e ponto de partida do editor entre estilos', () => {
  const d = configDoPronto(berga, '3d')
  assert.equal(prontoMarcado(d, '3d'), 'berga')
  assert.equal(prontoMarcado(d, 'simples'), null)
  assert.deepEqual(simplesDe(d), configDoPronto(berga, 'simples'))
  assert.deepEqual(simplesDe({ v: 2, estilo: '3d', conjuntoId: 'ceo' }), configCriada())
})

test('criação: Hub com v2 recebe legado + mexeriquinha coerentes; Hub antigo (só avatarEditing) recebe só o legado', () => {
  const v2 = configDoPronto(berga, '3d')
  const legado = randomAvatar()
  assert.deepEqual(camposDaCriacao(camposAvatarDoBot(v2), { avatarConfigVersion: 2 }), { avatarConfig: projetarAvatarLegado(v2), mexeriquinha: v2 })
  const antigo = camposDaCriacao(camposAvatarDoBot(v2), {})
  assert.deepEqual(Object.keys(antigo), ['avatarConfig'])
  assert.ok(validAvatar(antigo.avatarConfig))
  assert.deepEqual(camposDaCriacao({ avatarConfig: legado }, { avatarConfigVersion: 1 }), { avatarConfig: legado }, 'legado vai como está')
  assert.equal(camposDaCriacao({ avatarConfig: legado }, { avatarConfigVersion: 2 }).mexeriquinha?.v, 2)
})

test('comAvatar troca os dois campos juntos; legado tira a mexeriquinha antiga', () => {
  const v2 = configDoPronto(berga, 'simples')
  const comV2 = comAvatar({ name: 'Ana', avatarConfig: randomAvatar() }, v2)
  assert.deepEqual(comV2.mexeriquinha, v2)
  assert.equal(comV2.name, 'Ana')
  const legado = randomAvatar()
  assert.deepEqual(comAvatar(comV2, legado), { name: 'Ana', avatarConfig: legado })
})
