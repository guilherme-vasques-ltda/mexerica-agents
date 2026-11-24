import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fixturas } from './fixtures.ts'
import { PRAZO_AMOSTRA_MS, filtrar, projetarAmostra, projetarFatos } from './modelo.ts'

const AGORA = Date.parse('2026-11-23T12:00:00Z')
const ha = (ms: number) => new Date(AGORA - ms).toISOString()
const boa = { cpu: 10, memoria: 20, disco: 30 }

test('amostra fresca mostra números e alerta de disco só acima do limiar', () => {
  assert.deepEqual(projetarAmostra({ ...boa, coletadaEm: ha(1000) }, AGORA), { estado: 'fresca', cpu: 10, memoria: 20, disco: 30, alertaDisco: false })
  assert.deepEqual(projetarAmostra({ ...boa, disco: 95, coletadaEm: ha(1000) }, AGORA), { estado: 'fresca', cpu: 10, memoria: 20, disco: 95, alertaDisco: true })
})

test('amostra obsoleta oculta números e alertas, inclusive com disco cheio', () => {
  assert.deepEqual(projetarAmostra({ ...boa, disco: 99, coletadaEm: ha(PRAZO_AMOSTRA_MS + 1) }, AGORA), { estado: 'obsoleta' })
})

test('amostra faltante, nula ou em formato errado vira ausente ou inválida, sem número', () => {
  assert.deepEqual(projetarAmostra(undefined, AGORA), { estado: 'ausente' })
  assert.deepEqual(projetarAmostra(null, AGORA), { estado: 'ausente' })
  assert.deepEqual(projetarAmostra('70%', AGORA), { estado: 'invalida' })
  assert.deepEqual(projetarAmostra({ ...boa }, AGORA), { estado: 'invalida' })
  assert.deepEqual(projetarAmostra({ ...boa, coletadaEm: 'ontem' }, AGORA), { estado: 'invalida' })
  assert.deepEqual(projetarAmostra({ ...boa, cpu: 140, coletadaEm: ha(1000) }, AGORA), { estado: 'invalida' })
  assert.deepEqual(projetarAmostra({ ...boa, memoria: NaN, coletadaEm: ha(1000) }, AGORA), { estado: 'invalida' })
  assert.deepEqual(projetarAmostra({ cpu: 1, memoria: 1, coletadaEm: ha(1000) }, AGORA), { estado: 'invalida' })
})

test('amostra no horário exato do agora é fresca; 1 ms no futuro já oculta números e alertas', () => {
  assert.deepEqual(projetarAmostra({ ...boa, coletadaEm: ha(0) }, AGORA), { estado: 'fresca', cpu: 10, memoria: 20, disco: 30, alertaDisco: false })
  assert.deepEqual(projetarAmostra({ ...boa, coletadaEm: new Date(AGORA + 1).toISOString() }, AGORA), { estado: 'futura' })
  assert.deepEqual(projetarAmostra({ ...boa, disco: 99, coletadaEm: ha(-30_000) }, AGORA), { estado: 'futura' })
})

test('a virada da fronteira de 5 min é exata: no limite fica fresca, 1 ms depois fica obsoleta', () => {
  const instante = { ...boa, coletadaEm: ha(PRAZO_AMOSTRA_MS) }
  assert.equal(projetarAmostra(instante, AGORA).estado, 'fresca')
  assert.equal(projetarAmostra(instante, AGORA + 1).estado, 'obsoleta')
  assert.equal(projetarAmostra({ ...boa, coletadaEm: ha(PRAZO_AMOSTRA_MS - 1) }, AGORA + 1).estado, 'fresca')
})

test('relógio de validação inválido rejeita até amostra válida', () => {
  const fresca = { ...boa, coletadaEm: ha(1000) }
  assert.deepEqual(projetarAmostra(fresca, Number.NaN), { estado: 'invalida' })
  assert.deepEqual(projetarAmostra(fresca, Number.POSITIVE_INFINITY), { estado: 'invalida' })
})

test('os quatro fatos são projetados um a um, sem deduzir um do outro', () => {
  const rotaSemTunel = { rota: 'configurada', tunel: 'parado', identidade: 'desconhecida', autorizacao: 'desconhecida' } as const
  const estados = projetarFatos(rotaSemTunel).map((f) => `${f.fato}:${f.estado}`)
  assert.deepEqual(estados, ['rota:configurada', 'tunel:parado', 'identidade:desconhecida', 'autorizacao:desconhecida'])
  const tunelSemAutorizacao = projetarFatos({ rota: 'configurada', tunel: 'em_uso', identidade: 'desconhecida', autorizacao: 'desconhecida' })
  assert.equal(tunelSemAutorizacao.find((f) => f.fato === 'tunel')?.tom, 'ok')
  assert.equal(tunelSemAutorizacao.find((f) => f.fato === 'identidade')?.tom, 'neutro')
})

test('autorização negada não rebaixa a rota nem o túnel, e aparece como negada', () => {
  const [cedro] = fixturas(AGORA).filter((c) => c.id === 'demo-cedro')
  const fatos = projetarFatos(cedro.fatos)
  assert.equal(fatos.find((f) => f.fato === 'autorizacao')?.estado, 'negada')
  assert.equal(fatos.find((f) => f.fato === 'autorizacao')?.tom, 'atencao')
  assert.equal(fatos.find((f) => f.fato === 'rota')?.estado, 'configurada')
  assert.equal(fatos.find((f) => f.fato === 'tunel')?.estado, 'em_uso')
})

test('fixtures cobrem rota sem túnel, túnel sem identidade ou autorização, autorização negada e tudo verificado', () => {
  const porId = Object.fromEntries(fixturas(AGORA).map((c) => [c.id, c.fatos]))
  assert.deepEqual(porId['demo-atlas'], { rota: 'configurada', tunel: 'parado', identidade: 'desconhecida', autorizacao: 'desconhecida' })
  assert.equal(porId['demo-boreal']?.tunel, 'em_uso')
  assert.notEqual(porId['demo-boreal']?.identidade, 'verificada')
  assert.equal(porId['demo-cedro']?.autorizacao, 'negada')
  assert.deepEqual(porId['demo-dunas'], { rota: 'configurada', tunel: 'em_uso', identidade: 'verificada', autorizacao: 'confirmada' })
})

test('fixtures mostram amostra fresca, velha, ausente e inválida, e nenhum dado real', () => {
  const porId = Object.fromEntries(fixturas(AGORA).map((c) => [c.id, projetarAmostra(c.amostra, AGORA).estado]))
  assert.equal(porId['demo-atlas'], 'fresca')
  assert.equal(porId['demo-boreal'], 'obsoleta')
  assert.equal(porId['demo-cedro'], 'ausente')
  assert.equal(porId['demo-dunas'], 'invalida')
  for (const c of fixturas(AGORA)) assert.ok(c.id === 'esta' || c.id.startsWith('demo-'), c.id)
})

test('filtros de túnel em uso e de autorização sem confirmação', () => {
  const todos = fixturas(AGORA)
  assert.equal(filtrar(todos, 'todos').length, todos.length)
  assert.deepEqual(filtrar(todos, 'tunel_em_uso').map((c) => c.id), ['demo-boreal', 'demo-cedro', 'demo-dunas'])
  assert.ok(!filtrar(todos, 'sem_autorizacao').some((c) => c.fatos.autorizacao === 'confirmada'))
})
