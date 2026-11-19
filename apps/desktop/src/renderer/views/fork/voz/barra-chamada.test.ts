import { test } from 'node:test'
import assert from 'node:assert/strict'
import { ESTADO_CHAMADA, estadoDaBarra, tempoDaChamada, tempoMostrado } from './barra-chamada.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'

test('D249 F1: estado da barra da chamada, na ordem de prioridade', () => {
  // Fatia 6b (D454): a barra guarda a chave; o teste confere o texto pt-BR da tela.
  const e = (c: Partial<Parameters<typeof estadoDaBarra>[0]>) => traduzir('pt-BR', ESTADO_CHAMADA[estadoDaBarra({ encerrada: false, fase: 'listening', mudo: false, trabalhando: false, ...c })])
  assert.equal(e({ fase: null }), 'Conectando…')
  assert.equal(e({ fase: 'starting' }), 'Conectando…')
  assert.equal(e({}), 'Ouvindo')
  assert.equal(e({ trabalhando: true }), 'Pensando')
  assert.equal(e({ fase: 'speaking', trabalhando: true }), 'Falando', 'falando vence pensando')
  assert.equal(e({ fase: 'failed', trabalhando: true }), 'Não consigo ouvir', 'a falha nunca vira Pensando')
  assert.equal(e({ mudo: true, trabalhando: true }), 'Microfone desligado')
  assert.equal(e({ encerrada: true, fase: 'speaking' }), 'Chamada encerrada')
})

test('D249 F1: tempo da chamada em mm:ss', () => {
  assert.equal(tempoDaChamada(0), '00:00')
  assert.equal(tempoDaChamada(42_900), '00:42')
  assert.equal(tempoDaChamada(12 * 60_000 + 5_000), '12:05')
  assert.equal(tempoDaChamada(-5), '00:00')
})

test('D249 F1: encerrada (limite, falha sem reserva ou fim pela voz), o tempo para no instante do fim', () => {
  const inicio = 1_000_000
  assert.equal(tempoMostrado(inicio, inicio + 42_000, null), '00:42', 'em curso, conta até agora')
  // Falha sem reserva: finish() aos 42 s; a barra continua na tela e o relógio segue chamado a cada segundo.
  assert.equal(tempoMostrado(inicio, inicio + 60_000, inicio + 42_000), '00:42', 'não passa de 00:42 depois do fim')
  assert.equal(tempoMostrado(inicio, inicio + 31 * 60_000, inicio + 30 * 60_000), '30:00', 'teto de 30 min')
})
