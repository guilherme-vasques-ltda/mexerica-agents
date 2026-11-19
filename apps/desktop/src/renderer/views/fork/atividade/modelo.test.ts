import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { HistoryEntry } from '../bot-de-fundo/model.ts'
import { Atividade, filtrar, maisNovasPrimeiro, paginaPorTecla, paginar, porDia, resumo, tokens, type Transport } from './modelo.ts'

const entrada = (p: Partial<HistoryEntry>): HistoryEntry => ({ id: 'a', botId: 'b', function: 'memory', status: 'applied', at: 0, effectiveModels: [], usage: null, failureStage: null, ...p })
const bruto = (i: number, status = 'applied') => ({ id: `r${i}`, botId: 'bot', function: 'memory', status, at: 1_000 + i, effectiveModels: [] })

test('agrupa por dia local na ordem do Hub: hoje, ontem, data e sem data', () => {
  const agora = new Date(2026, 10, 11, 15, 0).getTime()
  const g = porDia([entrada({ id: '1', at: agora - 60_000 }), entrada({ id: '2', at: new Date(2026, 10, 10, 23, 0).getTime() }), entrada({ id: '3', at: new Date(2026, 10, 8, 9, 0).getTime() }), entrada({ id: '4', at: 0 })], agora)
  assert.deepEqual(g.map((x) => x.dia), ['hoje', 'ontem', new Date(2026, 10, 8).getTime(), 'semData'])
})

test('filtro de falhas, resumo e tokens (sem uso informado é null, não zero)', () => {
  const lista = [entrada({ id: '1' }), entrada({ id: '2', status: 'failed' }), entrada({ id: '3', status: 'failed' })]
  assert.deepEqual(filtrar(lista, 'falhas').map((e) => e.id), ['2', '3'])
  assert.equal(filtrar(lista, 'todas').length, 3)
  assert.deepEqual(resumo(lista), { total: 3, falhas: 2 })
  assert.equal(tokens(entrada({})), null)
  assert.equal(tokens(entrada({ usage: { inputTokens: 10, outputTokens: 5, cachedInputTokens: 99, cachedWriteTokens: 1 } })), 15)
})

test('lê até 50 registros; sem conexão, Hub recusando e resposta inválida viram estados próprios', async () => {
  let resposta: unknown = Array.from({ length: 50 }, (_, i) => bruto(i))
  let falha = false
  const link: Transport = { call: async <T,>() => { if (falha) throw new Error('local_only'); return resposta as T } }
  const a = new Atividade(() => link)
  await a.ler()
  assert.equal(a.getSnapshot().entradas?.length, 50)
  assert.equal(a.getSnapshot().entradas?.[0]?.botId, 'bot')
  resposta = Array.from({ length: 51 }, (_, i) => bruto(i))
  await a.ler()
  assert.deepEqual([a.getSnapshot().entradas?.length, a.getSnapshot().erro], [50, 'invalido'])
  falha = true
  await a.ler()
  assert.deepEqual([a.getSnapshot().entradas?.length, a.getSnapshot().erro], [50, 'indisponivel'])
  // Primeira carga que falha: sem linhas, só o erro.
  const nova = new Atividade(() => link)
  await nova.ler()
  assert.deepEqual([nova.getSnapshot().entradas, nova.getSnapshot().erro], [null, 'indisponivel'])
  const sem = new Atividade(() => null)
  await sem.ler()
  assert.equal(sem.getSnapshot().erro, 'semConexao')
})

test('resposta de uma leitura antiga ou de outra conexão é descartada', async () => {
  let soltar: (v: unknown) => void = () => {}
  const lenta: Transport = { call: <T,>() => new Promise<T>((r) => { soltar = r as (v: unknown) => void }) }
  let atual: Transport | null = lenta
  const a = new Atividade(() => atual)
  const primeira = a.ler()
  atual = { call: async <T,>() => [bruto(1, 'failed')] as T }
  await a.ler()
  soltar([bruto(2)])
  await primeira
  assert.deepEqual(a.getSnapshot().entradas?.map((e) => e.status), ['failed'])
})

test('revisão R1: o Hub manda do mais antigo ao mais novo (acrescenta no fim); a Atividade mostra o mais novo primeiro', async () => {
  const agora = new Date(2026, 10, 11, 15, 0).getTime()
  // Ordem real do Hub: anteontem, ontem, hoje 9h, hoje 14h.
  const doHub = [new Date(2026, 10, 9, 10, 0), new Date(2026, 10, 10, 20, 0), new Date(2026, 10, 11, 9, 0), new Date(2026, 10, 11, 14, 0)]
    .map((d, i) => ({ ...bruto(i), at: d.getTime() }))
  const link: Transport = { call: async <T,>() => doHub as T }
  const a = new Atividade(() => link)
  await a.ler()
  const lidas = a.getSnapshot().entradas!
  assert.deepEqual(lidas.map((e) => e.id), ['r3', 'r2', 'r1', 'r0'])
  const dias = porDia(lidas, agora)
  assert.deepEqual(dias.map((g) => g.dia), ['hoje', 'ontem', new Date(2026, 10, 9).getTime()])
  assert.deepEqual(dias[0]!.entradas.map((e) => e.id), ['r3', 'r2'])
  assert.deepEqual(doHub.map((e) => e.id), ['r0', 'r1', 'r2', 'r3'], 'a resposta do Hub não é alterada')
  assert.deepEqual(maisNovasPrimeiro([1, 2, 3]), [3, 2, 1])
})

test('decisão do dono: 10 por página, "X–Y de N", página fora do intervalo vai para a mais próxima', () => {
  const cinquenta = Array.from({ length: 50 }, (_, i) => i)
  const p1 = paginar(cinquenta, 1)
  assert.deepEqual([p1.pagina, p1.paginas, p1.de, p1.ate, p1.total, p1.itens.length], [1, 5, 1, 10, 50, 10])
  const p3 = paginar(cinquenta, 3)
  assert.deepEqual([p3.de, p3.ate, p3.itens[0]], [21, 30, 20])
  assert.equal(paginar(cinquenta, 9).pagina, 5)
  assert.equal(paginar(cinquenta, 0).pagina, 1)
  const curta = paginar(Array.from({ length: 23 }, (_, i) => i), 3)
  assert.deepEqual([curta.paginas, curta.de, curta.ate, curta.itens.length], [3, 21, 23, 3])
  const nada = paginar([], 4)
  assert.deepEqual([nada.pagina, nada.paginas, nada.de, nada.ate], [1, 1, 0, 0])
  // Filtro "Com falha" com 5 falhas: uma página só; quem estava na página 3 cai na 1.
  assert.equal(paginar(filtrar(Array.from({ length: 50 }, (_, i) => entrada({ id: String(i), status: i % 10 === 8 ? 'failed' : 'applied' })), 'falhas'), 3).pagina, 1)
})

test('paginador pelo teclado: setas andam uma página (espelhadas no RTL), Home e End vão às pontas', () => {
  assert.equal(paginaPorTecla('ArrowRight', 2, 5, false), 3)
  assert.equal(paginaPorTecla('ArrowLeft', 2, 5, false), 1)
  assert.equal(paginaPorTecla('ArrowRight', 2, 5, true), 1)
  assert.equal(paginaPorTecla('ArrowLeft', 2, 5, true), 3)
  assert.equal(paginaPorTecla('Home', 4, 5, false), 1)
  assert.equal(paginaPorTecla('End', 1, 5, true), 5)
  assert.equal(paginaPorTecla('ArrowLeft', 1, 5, false), null)
  assert.equal(paginaPorTecla('End', 5, 5, false), null)
  assert.equal(paginaPorTecla('Enter', 2, 5, false), null)
})

test('recarga: as linhas antigas ficam na tela até a resposta; com erro, ficam e o erro aparece', async () => {
  let soltar: (v: unknown) => void = () => {}
  let recusar: (e: unknown) => void = () => {}
  let primeira = true
  const link: Transport = { call: <T,>() => primeira ? (primeira = false, Promise.resolve([bruto(1)] as T)) : new Promise<T>((ok, erro) => { soltar = ok as (v: unknown) => void; recusar = erro }) }
  const a = new Atividade(() => link)
  await a.ler()
  assert.deepEqual(a.getSnapshot().entradas?.map((e) => e.id), ['r1'])
  // Recarga em curso: as linhas continuam, com o indicador de leitura.
  const segunda = a.ler()
  assert.deepEqual([a.getSnapshot().entradas?.map((e) => e.id), a.getSnapshot().lendo, a.getSnapshot().erro], [['r1'], true, null])
  soltar([bruto(1), bruto(2)])
  await segunda
  assert.deepEqual([a.getSnapshot().entradas?.map((e) => e.id), a.getSnapshot().lendo], [['r2', 'r1'], false])
  // Recarga que falha: as linhas da última leitura ficam e o erro aparece.
  const terceira = a.ler()
  assert.deepEqual(a.getSnapshot().entradas?.length, 2)
  recusar(new Error('local_only'))
  await terceira
  assert.deepEqual([a.getSnapshot().entradas?.map((e) => e.id), a.getSnapshot().lendo, a.getSnapshot().erro], [['r2', 'r1'], false, 'indisponivel'])
  // A próxima recarga boa limpa o erro.
  const quarta = a.ler()
  assert.equal(a.getSnapshot().erro, null)
  soltar([bruto(3)])
  await quarta
  assert.deepEqual([a.getSnapshot().entradas?.map((e) => e.id), a.getSnapshot().erro], [['r3'], null])
})
