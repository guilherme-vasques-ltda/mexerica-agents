import assert from 'node:assert/strict'
import { test } from 'node:test'
import { FUNCOES_EXEMPLO, erroDaFuncao, funcoesProprias, limparFuncao } from './fork-bot-funcao.ts'
import { normalizeBot } from './models.ts'
import { camposAvatarDoBot } from './fork-avatar-config.ts'

test('a função vai ao Hub numa linha só e recusa o que esconderia texto', () => {
  assert.equal(limparFuncao('  Chefe \t de\n gabinete '), 'Chefe de gabinete')
  assert.equal(erroDaFuncao('Prospecção BR'), null)
  assert.match(erroDaFuncao('Vendas‮sotnemagap') ?? '', /invisível/)
  assert.match(erroDaFuncao('Ven​das') ?? '', /invisível/)
  assert.equal(erroDaFuncao('ã'.repeat(40)), null)
  assert.match(erroDaFuncao('ã'.repeat(41)) ?? '', /40/)
})

test('as funções criadas pelo dono voltam como opção, sem repetir exemplos nem grupos', () => {
  assert.equal(FUNCOES_EXEMPLO.length, 30)
  const bots = [
    normalizeBot({ id: 'a', name: 'Rafa', title: 'Prospecção BR', createdAt: 1 }),
    normalizeBot({ id: 'b', name: 'Lia', title: 'prospecção  br', createdAt: 2 }),
    normalizeBot({ id: 'c', name: 'Giro', title: 'Estoque', createdAt: 3 }),
    normalizeBot({ id: 'd', name: 'Sala', kind: 'group', title: 'Vendas B2B', createdAt: 4 }),
    normalizeBot({ id: 'e', name: 'Velho', createdAt: 5 }),
  ]
  const proprias = funcoesProprias(bots)
  assert.deepEqual(proprias.map((f) => f.nome), ['prospecção br'], 'o bot mais recente dá o nome e a aparência')
  assert.equal(proprias[0]!.config, bots[1]!.avatarConfig)
  assert.equal(proprias[0]!.propria, true)
})

test('fase 2a: a função própria leva o avatar v2 do bot de origem inteiro (estilo, peças ou conjunto 3D)', () => {
  const v2 = { v: 2, estilo: '3d', modeloId: 'pomarca', conjuntoId: 'pomarca' } as const
  const [funcao] = funcoesProprias([{ kind: 'bot', name: 'Ana', description: '', title: 'Curadoria', ...camposAvatarDoBot(v2), createdAt: 1 }])
  assert.deepEqual(funcao!.config, v2)
})
