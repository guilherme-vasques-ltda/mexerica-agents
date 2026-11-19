import assert from 'node:assert/strict'
import test from 'node:test'
import type { BrowserStatus } from '../../../../shared/fork-browser.ts'
import { motivoDoIntegrado, nomeDoComputador, preferenciaConfirmadaDoNavegador, preferenciaDoNavegador, situacaoDoComputador } from './navegador.ts'

const tela = (computerUse: boolean) => ({ computerUse })
const acesso = (name: string, available: boolean) => ({ target: name, name, available, enabled: true, notice: '' })

test('a escolha efetiva é o rascunho do editor; sem rascunho, o confirmado pelo Hub', () => {
  const bot = { browserPreference: 'computer' as const }
  assert.equal(preferenciaDoNavegador(null, bot), 'computer')
  assert.equal(preferenciaDoNavegador({}, bot), 'computer', 'rascunho sem o campo não apaga a preferência')
  assert.equal(preferenciaDoNavegador({ browserPreference: 'off' }, bot), 'off')
  assert.equal(preferenciaDoNavegador({ browserPreference: 'integrated' }, bot), 'integrated')
})

test('o estado exibido só segue o confirmado pelo Hub; rascunho pendente ou falho não desliga', () => {
  const bot = { browserPreference: 'computer' as const }
  // Sem editor lido, o mirror do bot é a fonte confirmada.
  assert.equal(preferenciaConfirmadaDoNavegador(null, bot), 'computer')
  assert.equal(preferenciaConfirmadaDoNavegador(undefined, bot), 'computer')
  // O perfil devolvido pelo save é o confirmado; o rascunho pode divergir sem efeito.
  assert.equal(preferenciaConfirmadaDoNavegador({ browserPreference: 'off' }, bot), 'off')
  assert.equal(preferenciaConfirmadaDoNavegador({}, bot), 'computer', 'perfil sem o campo não apaga a preferência')
  // Gravação recusada: o rascunho segue 'off', mas o confirmado continua o anterior.
  assert.equal(preferenciaDoNavegador({ browserPreference: 'off' }, bot), 'off')
  assert.equal(preferenciaConfirmadaDoNavegador({ browserPreference: 'computer' }, bot), 'computer')
})

test('o nome da máquina vem do estado real do Hub: screenAccess disponível, depois o hello', () => {
  const bot = { screenAccess: [acesso('servidor-do-hub', true), acesso('outro', false)] }
  assert.equal(nomeDoComputador(bot, 'nome-do-hello'), 'servidor-do-hub')
  assert.equal(nomeDoComputador({ screenAccess: [acesso('indisponivel', false)] }, 'nome-do-hello'), 'nome-do-hello')
  assert.equal(nomeDoComputador({ screenAccess: null }, null), null, 'sem fonte, sem nome inventado')
  assert.equal(nomeDoComputador({ screenAccess: null }, undefined), null)
})

test('Meu computador: pronto só com controle ligado E tela admitida; Hub em servidor cai em semTela', () => {
  assert.equal(situacaoDoComputador({ computer: true }, tela(true)), 'pronto')
  assert.equal(situacaoDoComputador({ computer: false }, tela(true)), 'semUso')
  assert.equal(situacaoDoComputador({ computer: true }, tela(false)), 'semTela', 'ligado sem tela não é pronto')
  assert.equal(situacaoDoComputador({ computer: false }, tela(false)), 'semTela')
  assert.equal(situacaoDoComputador({ computer: true }, null), 'semTela', 'Hub sem tela informada')
  assert.equal(situacaoDoComputador({ computer: false }, undefined), 'semTela')
})

test('o motivo do integrado vem do reason estável; consulta falha ou resposta estranha vira genérico', () => {
  const status = (reason: string) => ({ reason }) as BrowserStatus
  assert.equal(motivoDoIntegrado(status('backend_absent')), 'backend_absent')
  assert.equal(motivoDoIntegrado(status('client_unsupported')), 'client_unsupported')
  assert.equal(motivoDoIntegrado(null), 'generico')
  assert.equal(motivoDoIntegrado(undefined), 'generico')
  assert.equal(motivoDoIntegrado(status('outra_coisa')), 'generico')
})
