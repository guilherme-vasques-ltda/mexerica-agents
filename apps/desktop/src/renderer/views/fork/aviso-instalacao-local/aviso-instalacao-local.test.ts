import assert from 'node:assert/strict'
import { test } from 'node:test'
import { CATALOGOS } from '../../../lib/fork-i18n/index.ts'
import { avisaPrimeiraAgendada, ehHubDesteComputador } from './aviso-instalacao-local.ts'

const local = { computer: { id: 'computador-1' } }
const deste = { computer: { id: 'computador-1' } }
const remoto = { computer: { id: 'vps-1' } }
const bot = { id: 'bot-1' }
const cobertura = { teams: [{ botIds: ['bot-1'] }] }
const cron = { type: 'cron' }
const manual = { type: 'webhook' }

const visao = (routines: { triggers: { type: string }[] }[], teams = cobertura.teams) => ({ routines, teams })

const pode = (overrides: Partial<Parameters<typeof avisaPrimeiraAgendada>[0]>) => avisaPrimeiraAgendada({
  hubLocal: true,
  visao: visao([]),
  botsDaTela: [bot],
  ...overrides,
})

test('o aviso vale só para o Hub deste computador', () => {
  assert.equal(ehHubDesteComputador(local, deste), true)
  assert.equal(ehHubDesteComputador(local, remoto), false)
  assert.equal(ehHubDesteComputador(null, deste), false)
})

test('sem rotina agendada no overview completo, o aviso de criação aparece', () => {
  assert.equal(pode({}), true)
  assert.equal(pode({ visao: visao([{ triggers: [manual] }]) }), true)
})

test('uma rotina agendada ligada ou desligada impede o aviso de criação', () => {
  assert.equal(pode({ visao: visao([{ triggers: [cron] }]) }), false)
  assert.equal(pode({ visao: visao([{ triggers: [manual, cron] }]) }), false)
})

test('Hub remoto e overview ausente não mostram o aviso de criação', () => {
  assert.equal(pode({ hubLocal: false }), false)
  assert.equal(pode({ visao: null }), false)
})

test('um bot da tela fora do overview deixa a cobertura incompleta', () => {
  assert.equal(pode({ botsDaTela: [bot, { id: 'bot-fora' }] }), false)
})

test('o texto do aviso existe nos seis idiomas, sem marcador sobrando', () => {
  for (const [idioma, catalogo] of Object.entries(CATALOGOS)) {
    const texto = catalogo['avisoLocal.texto']
    assert.ok(texto && texto.trim().length > 0, idioma)
    assert.ok(!/[{}]/.test(texto), idioma)
  }
})
