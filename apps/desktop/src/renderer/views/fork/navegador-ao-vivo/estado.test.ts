import test from 'node:test'
import assert from 'node:assert/strict'
import { STALE_AFTER_MS, type LiveState } from '../../../../shared/fork-browser-live.ts'
import { aceitaClique, atrasoDoQuadro, chaveDoMotivo, faixaDoEstado } from './estado.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'
import { IDIOMAS_CATALOGO } from '../../../lib/fork-i18n/registro.ts'

const estado = (over: Partial<LiveState> = {}): LiveState => ({ status: 'live', reason: null, controller: 'agent', epoch: 0, target: 'a', targetGeneration: 0, switching: false, ...over })

test('a faixa diz quem controla e só aceita clique com a pessoa no controle e o quadro em dia', () => {
  assert.deepEqual(faixaDoEstado(estado({ status: 'connecting' })), { kind: 'abrindo' })
  assert.deepEqual(faixaDoEstado(estado()), { kind: 'bot' })
  assert.deepEqual(faixaDoEstado(estado({ switching: true })), { kind: 'assumindo' })
  const pessoa = faixaDoEstado(estado({ controller: 'person' }))
  assert.deepEqual(pessoa, { kind: 'pessoa' })
  assert.equal(aceitaClique(pessoa, null, true), true)
  assert.equal(aceitaClique(pessoa, 7, true), false, 'quadro velho bloqueia')
  assert.equal(aceitaClique(pessoa, null, false), false, 'sem quadro não há onde clicar')
  assert.equal(aceitaClique(faixaDoEstado(estado()), null, true), false, 'com o bot no controle não há clique')
  assert.equal(aceitaClique(faixaDoEstado(estado({ controller: 'person', switching: true })), null, true), false, 'sem confirmação do Hub não há clique')
})

test('o atraso só aparece acima de 6 s e é arredondado para baixo', () => {
  assert.equal(atrasoDoQuadro(null), null)
  assert.equal(atrasoDoQuadro(STALE_AFTER_MS), null)
  assert.equal(atrasoDoQuadro(STALE_AFTER_MS + 1), 6)
  assert.equal(atrasoDoQuadro(9_900), 9)
})

test('tela fechada mostra o motivo estável do catálogo, nos seis idiomas', () => {
  for (const reason of ['unsupported', 'denied', 'connectionLost', 'unavailable', 'closed'] as const) {
    const faixa = faixaDoEstado(estado({ status: 'closed', reason }))
    assert.deepEqual(faixa, { kind: 'fechada', reason })
    for (const idioma of IDIOMAS_CATALOGO) assert.ok(traduzir(idioma, chaveDoMotivo(reason)).length > 0)
  }
})
