import assert from 'node:assert/strict'
import test from 'node:test'
import { ALL, filterRoutines, mergeOrder } from './busca.ts'

test('a busca de rotinas acha pelo nome da rotina, do bot e do Time, sem acento', () => {
  const routines = [
    { id: 'a', botId: 'fin', name: 'Boletos da semana', teamId: 'pessoal' },
    { id: 'b', botId: 'mkt', name: 'Agenda de posts', teamId: 'pessoal' },
  ]
  const teams = [{ id: 'pessoal', name: 'Pessoal', botIds: ['fin', 'mkt'] }]
  const names: Record<string, string> = { fin: 'Finanças', mkt: 'Marketing' }
  const find = (query: string, botId = ALL) =>
    filterRoutines(routines, teams, { teamId: ALL, botId, query }, (id) => names[id]!).map((r) => r.id)
  assert.deepEqual(find('boletos'), ['a'])
  assert.deepEqual(find('financas'), ['a'], 'sem acento acha "Finanças"')
  assert.deepEqual(find('POSTS marketing'), ['b'], 'todas as palavras, sem caixa')
  assert.deepEqual(find('pessoal'), ['a', 'b'], 'o nome do Time também conta')
  assert.deepEqual(find('boletos', 'mkt'), [], 'a busca soma com o filtro de bot')
})

test('reordenar com a busca ativa só troca as linhas visíveis e deixa as outras no lugar', () => {
  assert.deepEqual(mergeOrder(['a', 'b', 'c', 'd'], ['d', 'b']), ['a', 'd', 'c', 'b'])
  assert.deepEqual(mergeOrder(['a', 'b'], ['b', 'a']), ['b', 'a'])
})
