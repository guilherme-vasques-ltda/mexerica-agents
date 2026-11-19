import assert from 'node:assert/strict'
import test from 'node:test'
import { planoDeQueda, planoDoMotor } from './plano-motor.ts'
import { avisoLeituraGratis, escolhaParaGuardar, fixarMinis, fonteDaChave, isMini, modeloSumiu, ouvirInicial, podeGerirNoHub, semChave } from './voz-model.ts'

const online = { kind: 'online' }

test('a gestão da chave do Hub só aparece no acesso local ou pelo túnel SSH, nunca pela nuvem', () => {
  assert.equal(podeGerirNoHub({ isLoopback: true, connection: online }), true, 'Hub local ou por SSH (loopback)')
  assert.equal(podeGerirNoHub({ isLoopback: false, connection: online }), false, 'Hub pela nuvem (Device): sem setVoiceKey')
  assert.equal(podeGerirNoHub({ isLoopback: true, connection: { kind: 'connecting' } }), false)
  assert.equal(podeGerirNoHub(null), false)
})

test('a fonte vem só do Hub informado, por provedor; sem resposta não confirma chave', () => {
  const status = { providers: [{ provider: 'openai' as const, configured: false }, { provider: 'gemini' as const, configured: true }] }
  assert.equal(fonteDaChave(status, 'gemini'), 'hub')
  assert.equal(fonteDaChave(status, 'openai'), 'nenhuma')
  assert.equal(fonteDaChave(null, 'gemini'), 'desconhecida')
})

test('Voz para texto nasce com o que já valia e o aviso de leitura grátis só cabe na Voz do macOS com provedor pago', () => {
  const modos: Record<string, string> = { openai: 'speech', gemini: 'realtime' }
  assert.equal(ouvirInicial('openai', (p) => modos[p]!), 'openai', 'modo Fala já transcrevia pela OpenAI')
  assert.equal(ouvirInicial('gemini', (p) => modos[p]!), 'device')
  assert.equal(ouvirInicial('device', (p) => modos[p]!), 'device')
  assert.equal(avisoLeituraGratis('device', 'openai'), true)
  assert.equal(avisoLeituraGratis('device', 'device'), false)
  assert.equal(avisoLeituraGratis('openai', 'gemini'), false)
})

test('o mini nunca sobe sozinho', () => {
  assert.equal(isMini('gpt-4o-mini-tts'), true)
  assert.equal(isMini('gpt-realtime-mini'), true)
  assert.equal(isMini('gpt-realtime-2.1'), false)
  assert.equal(isMini('gemini-3.8-flash-lite'), true)
  // Padrão novo completo não substitui o mini em uso, quando a pessoa não escolheu outro.
  const emUso = { realtime: 'gpt-realtime-2.1', transcribe: 'gpt-transcribe', speech: 'gpt-4o-mini-tts' }
  const novos = { realtime: 'gpt-realtime-3', transcribe: 'gpt-transcribe-2', speech: 'gpt-tts-2' }
  assert.deepEqual(fixarMinis(emUso, novos, () => null), { speech: 'gpt-4o-mini-tts' })
  assert.deepEqual(fixarMinis(emUso, novos, (t) => (t === 'speech' ? 'gpt-tts-hd' : null)), {}, 'escolha da pessoa vale')
  assert.deepEqual(fixarMinis(emUso, { ...novos, speech: 'gpt-5-mini-tts' }, () => null), {}, 'mini para mini segue o padrão')
  // Escolher um mini igual ao padrão grava a escolha; escolher o completo padrão continua seguindo o mais novo.
  assert.equal(escolhaParaGuardar('gpt-4o-mini-tts', 'gpt-4o-mini-tts'), 'gpt-4o-mini-tts')
  assert.equal(escolhaParaGuardar('gpt-realtime-2.1', 'gpt-realtime-2.1'), null)
  assert.equal(escolhaParaGuardar('gpt-realtime-mini', 'gpt-realtime-2.1'), 'gpt-realtime-mini')
  // Mini que saiu do catálogo: avisa, sem trocar.
  assert.equal(modeloSumiu('gpt-realtime-mini', ['gpt-realtime-2.1']), true)
  assert.equal(modeloSumiu('gpt-realtime-mini', []), false, 'sem lista ainda, sem aviso')
})

test('a chamada nunca escolhe um provedor pago que a pessoa não escolheu', () => {
  assert.deepEqual(planoDoMotor({ aoVivo: null, fala: false, ouvir: 'device', ditado: false }), { tipo: 'sem-voz' }, 'Windows/Linux sem escolha: avisa e para')
  assert.deepEqual(planoDoMotor({ aoVivo: null, fala: false, ouvir: 'openai', ditado: false }), { tipo: 'sem-voz' })
  assert.deepEqual(planoDeQueda(false), { tipo: 'sem-voz' }, 'sem Ditado, a queda não vai para outro provedor')
  assert.deepEqual(planoDeQueda(true), { tipo: 'local' })
  assert.deepEqual(planoDoMotor({ aoVivo: null, fala: false, ouvir: 'device', ditado: true }), { tipo: 'local' })
  assert.deepEqual(planoDoMotor({ aoVivo: null, fala: false, ouvir: 'gemini', ditado: true }), { tipo: 'local-ouvindo', ouvir: 'gemini' })
  assert.deepEqual(planoDoMotor({ aoVivo: 'openai', fala: false, ouvir: 'gemini', ditado: true }), { tipo: 'tempo-real', provider: 'openai', fonte: 'hub' })
  assert.deepEqual(planoDoMotor({ aoVivo: 'gemini', fala: true, ouvir: 'device', ditado: true }), { tipo: 'fala-ditado', ler: 'gemini' })
  assert.deepEqual(planoDoMotor({ aoVivo: 'gemini', fala: true, ouvir: 'device', ditado: false }), { tipo: 'sem-ouvir' }, 'sem Ditado, o Fala não ouve pago por tabela (revisão r1, bloqueio 3)')
  assert.deepEqual(planoDoMotor({ aoVivo: 'gemini', fala: true, ouvir: 'gemini', ditado: false }), { tipo: 'fala', ouvir: 'gemini', ler: 'gemini' }, 'com escolha explícita, ouve por ela')
  assert.deepEqual(planoDoMotor({ aoVivo: 'gemini', fala: true, ouvir: 'openai', ditado: true }), { tipo: 'fala', ouvir: 'openai', ler: 'gemini' })
})

test('textos de erro não repetem a chave digitada nem algo com forma de chave', () => {
  const chave = 'sk-proj-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'
  for (const msg of [
    `Incorrect API key provided: ${chave}`,
    `Incorrect API key provided: ${chave.slice(0, 20)}`,
    `chave ${chave.slice(-12)} recusada`,
    'Gemini: API key AIzaSyA1234567890abcdefghij not valid',
  ]) {
    const limpo = semChave(msg, chave)
    assert.ok(!limpo.includes(chave.slice(0, 12)) && !limpo.includes(chave.slice(-8)) && !limpo.includes('AIzaSy'), limpo)
  }
  assert.equal(semChave('A OpenAI recusou a chave.', chave), 'A OpenAI recusou a chave.')
})
