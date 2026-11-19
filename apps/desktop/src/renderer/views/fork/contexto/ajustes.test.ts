// DON-906 (C3): o ajuste de compressão do Hub e a pausa da compressão automática (DON-910) no medidor.
import assert from 'node:assert/strict'
import test from 'node:test'
import { codigoFalha, conferirAjustes, falhaFixa, lerCampo, MINIMO_MIL, paraCampo } from './ajustes.ts'
import { conferirContexto, porcento, tokensCurtos } from './estado.ts'
import { traduzir } from '../../../lib/fork-i18n/index.ts'

// Intl usa espaços especiais (NBSP, NNBSP); compara com espaço comum.
const sp = (s: string) => s.replace(/[\u00a0\u202f]/g, ' ')

test('DTO do Hub: null = Automático; número só inteiro seguro, múltiplo de 1000, a partir de 50 mil; idle fechado', () => {
  assert.deepEqual(conferirAjustes({ triggerTokens: null, idle: 'beforeExpiry' }), { triggerTokens: null, idle: 'beforeExpiry' })
  assert.deepEqual(conferirAjustes({ triggerTokens: 120_000, idle: 'never' }), { triggerTokens: 120_000, idle: 'never' })
  for (const triggerTokens of [undefined, 49_999, 120_500, 1.5, '120000', Number.MAX_SAFE_INTEGER + 1, -1]) {
    assert.equal(conferirAjustes({ triggerTokens, idle: 'never' }), null, `triggerTokens ${triggerTokens}`)
  }
  for (const idle of [undefined, 'sempre', null]) assert.equal(conferirAjustes({ triggerTokens: null, idle }), null, `idle ${idle}`)
  assert.equal(conferirAjustes(null), null)
})

test('campo em mil tokens: piso de 50 mil, só inteiros, separador de milhar de qualquer idioma', () => {
  assert.deepEqual(lerCampo('150'), { tokens: 150_000 })
  assert.deepEqual(lerCampo(' 150 '), { tokens: 150_000 })
  for (const t of ['1.000', '1,000', '1 000', '1\u202f000']) assert.deepEqual(lerCampo(t), { tokens: 1_000_000 }, t)
  assert.deepEqual(lerCampo('50'), { tokens: 50_000 })
  assert.deepEqual(lerCampo('49'), { erro: 'minimo' })
  for (const t of ['', 'abc', '-80', '1,5', '12e4', '150 mil']) assert.deepEqual(lerCampo(t), { erro: 'numero' }, t)
  assert.deepEqual(lerCampo('99999999999999999'), { erro: 'grande' })
  assert.equal(paraCampo(120_000), '120')
})

test('recusas viram código, sem texto cru do Hub; só o 400 ao salvar afirma que nada mudou', () => {
  assert.equal(codigoFalha({ status: 404, message: 'x' }, 'ler'), 'hubAntigo')
  assert.equal(codigoFalha(new Error('unknown method: donnaContextSettings'), 'salvar'), 'hubAntigo')
  assert.equal(codigoFalha({ status: 403, message: 'forbidden' }, 'ler'), 'soDono')
  assert.equal(codigoFalha({ status: 401, message: 'x' }, 'salvar'), 'soDono')
  assert.equal(codigoFalha({ status: 400, message: 'invalid_trigger' }, 'salvar'), 'recusado')
  assert.equal(codigoFalha({ status: 400, message: 'x' }, 'ler'), 'ler')
  assert.equal(codigoFalha({ status: 500, message: 'db locked at host/src/x.rs' }, 'salvar'), 'salvar', 'pode ter gravado')
  assert.equal(codigoFalha(new Error('network down'), 'salvar'), 'salvar')
  assert.deepEqual(['hubAntigo', 'soDono', 'recusado', 'ler', 'salvar', 'resposta'].filter((f) => falhaFixa(f as never)), ['hubAntigo', 'soDono'])
  assert.equal(traduzir('pt-BR', 'contexto.ajustes.falha.salvar'), 'Não foi possível confirmar o ajuste. Atualize e confira.')
  assert.equal(traduzir('pt-BR', 'contexto.ajustes.falha.recusado'), 'O Hub recusou esse valor. Nada mudou.')
})

test('textos da C3 seguem o idioma: pausa, piso e regra automática com número da base', () => {
  assert.equal(traduzir('pt-BR', 'contexto.aviso.pausado.titulo'), 'O resumo automático está pausado')
  assert.equal(traduzir('en', 'contexto.aviso.pausado.titulo'), 'Automatic summary is paused')
  const minimo = (i: 'pt-BR' | 'en') => sp(traduzir(i, 'contexto.ajustes.erro.minimo', { min: tokensCurtos(i, MINIMO_MIL * 1000) }))
  assert.equal(minimo('pt-BR'), 'O mínimo é 50 mil tokens.')
  assert.equal(minimo('en'), 'The minimum is 50K tokens.')
  assert.equal(sp(traduzir('en', 'contexto.ajustes.automaticoDetalhe', { pct: porcento('en', 75), maximo: tokensCurtos('en', 200_000) })), 'Near 75% of the bot’s memory, at most 200K')
  assert.equal(sp(traduzir('pt-BR', 'contexto.anel.ariaPausado', { pct: porcento('pt-BR', 42) })), 'Espaço da conversa: 42% cheia; resumo automático pausado')
})

test('pausa da compressão automática chega no context; ausente ou inválida = não pausada', () => {
  const base = { hubId: 'h', botId: 'a', sessionId: 'a', scopeId: 'A', epoch: 0 }
  const ctx = { used: 84_000, size: 200_000, trigger: 150_000, contextId: 'c1' }
  const ler = (extra: Record<string, unknown>) => conferirContexto({ ...base, context: { ...ctx, ...extra } }, { hubId: 'h', botId: 'a', sessionId: null })?.leitura
  assert.deepEqual([ler({ automaticPaused: true, automaticFailures: 2 })?.automaticPaused, ler({ automaticPaused: true, automaticFailures: 2 })?.automaticFailures], [true, 2])
  assert.deepEqual([ler({})?.automaticPaused, ler({})?.automaticFailures], [false, 0], 'Hub sem os campos')
  assert.deepEqual([ler({ automaticPaused: 'true', automaticFailures: -1 })?.automaticPaused, ler({ automaticPaused: 'true', automaticFailures: 1.5 })?.automaticFailures], [false, 0])
})
