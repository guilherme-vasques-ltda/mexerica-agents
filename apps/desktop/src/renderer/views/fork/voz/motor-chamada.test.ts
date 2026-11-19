import assert from 'node:assert/strict'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'
import { textosVoz } from './frases.ts'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'

// Revisão r1 do voz-03: motores reais montados por `criarMotor`, preferências reais e textos reais da tela.
// Só o Hub, o microfone, o áudio e o socket são fictícios; nenhuma rede nem API paga.
const bundle = buildSync({
  stdin: {
    contents: `export { criarMotor, depoisDaFalha, planoDoMotor } from './views/fork/voz/motor-chamada';
      export { CHAVES_FALHA } from './views/fork/voz/falha-segura';
      export { linhaOuvirLocal, rodapeChavesNoHub } from './views/fork/voz/voz-model';
      export { onDeviceAvailable, onDeviceUnavailableReason, VoiceSettings } from './views/call/voice';
      export { chosenListen, engine, mode } from './views/settings/voice-settings';
      export { TEXTOS_CHAMADA } from './views/fork/voz/textos-chamada';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false,
  define: { 'process.env.NODE_ENV': '"production"' },
})

type Modulo = {
  criarMotor: typeof import('./motor-chamada').criarMotor
  depoisDaFalha: typeof import('./motor-chamada').depoisDaFalha
  planoDoMotor: typeof import('./motor-chamada').planoDoMotor
  CHAVES_FALHA: typeof import('./falha-segura').CHAVES_FALHA
  linhaOuvirLocal: typeof import('./voz-model').linhaOuvirLocal
  rodapeChavesNoHub: typeof import('./voz-model').rodapeChavesNoHub
  onDeviceAvailable: typeof import('../../call/voice').onDeviceAvailable
  onDeviceUnavailableReason: typeof import('../../call/voice').onDeviceUnavailableReason
  VoiceSettings: typeof import('../../call/voice').VoiceSettings
  chosenListen: typeof import('../../settings/voice-settings').chosenListen
  engine: typeof import('../../settings/voice-settings').engine
  mode: typeof import('../../settings/voice-settings').mode
  TEXTOS_CHAMADA: typeof import('./textos-chamada').TEXTOS_CHAMADA
}

// Fatia 6b (D454, D507): falhas e avisos são dado; o teste confere a chave e, nos textos, a frase pt-BR da tela.
const pt = (partes: Mensagem[], mac = true) => textosVoz(tradutor('pt-BR'), partes, undefined, mac)

const CHAVE = 'sk-proj-FAKEKEY0123456789abcdefghijklmnopqrstuv'
const logs: string[] = []

/** Microfone fictício: o teste empurra quadros pelo `onaudioprocess` do processador. */
const mic: { node: { onaudioprocess: ((e: unknown) => void) | null } | null } = { node: null }
class FakeAudioContext {
  state = 'running'
  destination = {}
  createMediaStreamSource() { return { connect() {}, disconnect() {} } }
  createScriptProcessor() {
    const node = { onaudioprocess: null, connect() {}, disconnect() {} }
    mic.node = node
    return node
  }
  close() { return Promise.resolve() }
}

class FakeSocket {
  static OPEN = 1
  static todos: FakeSocket[] = []
  readyState = 1
  enviados: string[] = []
  onopen: (() => void) | null = null
  onmessage: ((e: { data: string }) => void) | null = null
  onclose: ((e: { reason: string }) => void) | null = null
  constructor() { FakeSocket.todos.push(this) }
  send(s: string) { this.enviados.push(s) }
  close() {
    if (this.readyState === 3) return
    this.readyState = 3
    queueMicrotask(() => this.onclose?.({ reason: '' }))
  }
  /** O servidor fecha. */
  fechar(reason: string) {
    this.readyState = 3
    this.onclose?.({ reason })
  }
  receber(msg: unknown) { this.onmessage?.({ data: JSON.stringify(msg) }) }
}

class FakeAudio {
  onended: (() => void) | null = null
  onerror: (() => void) | null = null
  src = ''
  play() { return Promise.resolve() }
  pause() {}
}

function carregar(plataforma: 'darwin' | 'linux', guardado: Record<string, string> = {}): Modulo {
  const module = { exports: {} }
  const storage = new Map(Object.entries(guardado))
  const registrar = (...a: unknown[]) => { logs.push(a.map(String).join(' ')) }
  const ditado = plataforma === 'darwin'
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, setTimeout, clearTimeout, Blob, AbortController, TextDecoder, btoa, atob, Float32Array, Int16Array,
    console: { log: registrar, info: registrar, warn: registrar, error: registrar, debug: registrar },
    localStorage: { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => storage.set(k, v) },
    window: {
      addEventListener() {},
      codync: { appVersion: '2.20.3', platform: plataforma, speech: { available: ditado, onEvent: () => () => {}, start() {}, stop() {}, locales: () => [] } },
    },
    navigator: { language: 'pt-BR', languages: ['pt-BR'], mediaDevices: { getUserMedia: async () => ({ getTracks: () => [] }) } },
    AudioContext: FakeAudioContext, WebSocket: FakeSocket, Audio: FakeAudio,
    URL: { createObjectURL: () => 'blob:x', revokeObjectURL() {} },
    speechSynthesis: { cancel() {}, speak() {}, getVoices: () => [] },
    SpeechSynthesisUtterance: class {},
  })
  return module.exports as Modulo
}

const mac = carregar('darwin')

/** Um Hub fictício: registra cada RPC e responde pelo `responder` do teste. */
function hub(responder: (method: string, body: Record<string, unknown>) => Promise<unknown>) {
  const pedidos: string[] = []
  const store = {
    bots: new Map(), allEntries: () => [], members: () => [],
    ready: async () => ({
      call: (method: string, body: Record<string, unknown>) => {
        pedidos.push(method)
        return responder(method, body)
      },
    }),
  }
  return { pedidos, store: store as never }
}

/** Fala alta até o limite de um trecho (20 s), que fecha a fala sem depender do relógio. */
function falar() {
  const alto = new Float32Array(2048).fill(0.5)
  for (let i = 0; i < 160; i++) mic.node!.onaudioprocess!({ inputBuffer: { getChannelData: () => alto } })
}

/** Procura o texto em qualquer campo alcançável do objeto (estado do motor, leitor, fila). */
function contem(raiz: unknown, texto: string, vistos = new Set<unknown>()): boolean {
  if (typeof raiz === 'string') return raiz.includes(texto)
  if (!raiz || typeof raiz !== 'object' || vistos.has(raiz)) return false
  vistos.add(raiz)
  if (raiz instanceof Error && raiz.message.includes(texto)) return true
  return Object.values(raiz).some((v) => contem(v, texto, vistos))
}

test('bloqueio 1 (D70): a chave no erro do provedor não fica no motor, no erro nem no log', async () => {
  logs.length = 0
  const recusa = () => Promise.reject(new Error(`Incorrect API key provided: ${CHAVE}`))
  // Ditado + leitura paga (o caminho da revisão) e Fala ouvindo e lendo pelo provedor.
  for (const plano of [{ tipo: 'fala-ditado', ler: 'openai' }, { tipo: 'fala', ouvir: 'openai', ler: 'openai' }] as const) {
    const { store } = hub(recusa)
    const motor = mac.criarMotor(plano, { store, botId: 'bot', send: () => {} })
    await motor.start()
    if (plano.tipo === 'fala-ditado') {
      ;(motor as unknown as { event(e: unknown): void }).event({ type: 'started' })
      motor.speak('Olá.')
    } else {
      falar()
    }
    for (let i = 0; i < 6; i++) await tick()
    assert.equal(motor.phase, 'failed', plano.tipo)
    assert.equal(JSON.stringify(motor.falha), JSON.stringify({ chave: mac.CHAVES_FALHA.chave }), plano.tipo)
    assert.equal(contem(motor, CHAVE.slice(8, 20)), false, `${plano.tipo}: nada da chave no estado`)
  }
  assert.equal(logs.some((l) => l.includes(CHAVE.slice(8, 20))), false, 'nada da chave no log')
  // A rede de segurança do `failure`: um texto com forma de chave vira só a categoria.
  const { store } = hub(recusa)
  const motor = mac.criarMotor({ tipo: 'local' }, { store, botId: 'bot', send: () => {} })
  motor.failure = `401 invalid api key ${CHAVE}`
  assert.equal(JSON.stringify(motor.falha), JSON.stringify({ chave: mac.CHAVES_FALHA.chave }))
})

test('bloqueio 2: encerrar a chamada com duas falas na fila não transcreve nem lê depois do fim', async () => {
  let liberar!: (v: { text: string }) => void
  const { pedidos, store } = hub((method) => method === 'voiceTranscribe' ? new Promise((r) => { liberar = r }) : Promise.resolve({ audio: '', format: 'aac' }))
  const enviados: string[] = []
  const motor = mac.criarMotor({ tipo: 'fala', ouvir: 'openai', ler: 'openai' }, { store, botId: 'bot', send: (t) => enviados.push(t) })
  await motor.start()
  falar()
  falar()
  for (let i = 0; i < 4; i++) await tick()
  assert.deepEqual(pedidos, ['voiceTranscribe'], 'a primeira está no Hub, a segunda espera na fila')
  motor.end() // o que CallView.finish faz no fim (Encerrar, limites ou falha sem queda)
  liberar({ text: 'oi' })
  motor.speak('Uma resposta que chegou atrasada.')
  for (let i = 0; i < 6; i++) await tick()
  assert.deepEqual(pedidos, ['voiceTranscribe'], 'nenhuma transcrição nem leitura começa depois do fim')
  assert.deepEqual(enviados, [], 'a transcrição atrasada não vira mensagem')
})

test('bloqueio 4: ouvir pela OpenAI e ler pelo Gemini propaga a recusa da leitura ao controle da chamada', async () => {
  const { store } = hub((method) => method === 'voiceSpeak' ? Promise.reject(new Error('402 insufficient_quota')) : Promise.resolve({ text: '' }))
  const motor = mac.criarMotor({ tipo: 'fala', ouvir: 'openai', ler: 'gemini' }, { store, botId: 'bot', send: () => {} })
  await motor.start()
  motor.speak('Olá.')
  for (let i = 0; i < 6; i++) await tick()
  assert.equal(motor.phase, 'failed')
  assert.equal(JSON.stringify(motor.falha), JSON.stringify({ chave: mac.CHAVES_FALHA.saldo }))
  assert.equal(motor.provider, 'gemini')
  // O controle da chamada: no Mac com Ditado, queda gratuita com aviso; no Windows/Linux, avisa e termina.
  assert.equal(mac.depoisDaFalha(true, true), 'queda')
  assert.equal(mac.depoisDaFalha(true, false), 'fim')
  assert.equal(mac.depoisDaFalha(false, true), 'nada')
})

async function geminiNoAr() {
  FakeSocket.todos = []
  const { pedidos, store } = hub(async () => ({ credential: 'token-curto' }))
  const motor = mac.criarMotor({ tipo: 'tempo-real', provider: 'gemini', fonte: 'hub' }, { store, botId: 'bot', send: () => {} })
  await motor.start()
  const ws = FakeSocket.todos[0]!
  ws.onopen!()
  ws.receber({ setupComplete: {} })
  ws.receber({ sessionResumptionUpdate: { resumable: true, newHandle: 'h1' } })
  for (let i = 0; i < 4; i++) await tick()
  assert.equal(motor.phase, 'listening')
  return { motor, ws, credenciais: () => pedidos.filter((p) => p === 'voiceSession').length }
}

test('bloqueio 5: o Gemini retoma uma vez por goAway, na mesma chamada', async () => {
  const { motor, ws, credenciais } = await geminiNoAr()
  ws.receber({ goAway: { timeLeft: '10s' } })
  for (let i = 0; i < 6; i++) await tick()
  assert.equal(credenciais(), 2)
  assert.equal(FakeSocket.todos.length, 2)
  const novo = FakeSocket.todos[1]!
  novo.onopen!()
  assert.match(novo.enviados[0]!, /"handle":"h1"/, 'retoma a mesma sessão')
  // O novo socket cai sem outro goAway: não reconecta de novo.
  novo.fechar('')
  for (let i = 0; i < 6; i++) await tick()
  assert.equal(FakeSocket.todos.length, 2)
  assert.equal(credenciais(), 2)
  assert.equal(motor.phase, 'failed')
})

test('bloqueio 5: fechamento sem goAway não abre socket nem pede credencial', async () => {
  const { motor, ws, credenciais } = await geminiNoAr()
  ws.fechar(`API key not valid: ${CHAVE}`)
  for (let i = 0; i < 6; i++) await tick()
  assert.equal(FakeSocket.todos.length, 1)
  assert.equal(credenciais(), 1)
  assert.equal(motor.phase, 'failed')
  assert.equal(JSON.stringify(motor.falha), JSON.stringify({ chave: mac.CHAVES_FALHA.chave }), 'o motivo do servidor vira categoria')
  // Depois do fim, um goAway atrasado não retoma.
  const outra = await geminiNoAr()
  outra.motor.end()
  outra.ws.receber({ goAway: {} })
  for (let i = 0; i < 6; i++) await tick()
  assert.equal(FakeSocket.todos.length, 1)
  assert.equal(outra.credenciais(), 1)
})

test('bloqueio 3: no Linux com as prefs novas, a entrada local é indisponível e o Fala não ouve pago sozinho', () => {
  const linux = carregar('linux')
  assert.equal(linux.onDeviceAvailable(), false)
  assert.equal(linux.chosenListen(), 'device')
  linux.engine.set('openai')
  linux.mode('openai').set('speech')
  const escolhas = () => ({ aoVivo: linux.VoiceSettings.provider(), fala: linux.VoiceSettings.isSpeechMode('openai'), ouvir: linux.chosenListen(), ditado: linux.onDeviceAvailable() })
  assert.equal(JSON.stringify(linux.planoDoMotor(escolhas())), '{"tipo":"sem-ouvir"}')
  const linha = linux.linhaOuvirLocal(false, linux.onDeviceUnavailableReason())
  assert.equal(linha.ok, false)
  assert.doesNotMatch(pt(linha.partes, false), /macOS/)
  assert.match(pt(linha.partes, false), /não está disponível no Linux/)
  // Quem já usava o Fala da OpenAI no Linux continua ouvindo por ela (a escolha que já valia).
  const antigo = carregar('linux', { callEngine: '"openai"', 'callMode.openai': '"speech"' })
  assert.equal(antigo.chosenListen(), 'openai')
  assert.equal(JSON.stringify(antigo.planoDoMotor({ aoVivo: 'openai', fala: true, ouvir: antigo.chosenListen(), ditado: false })), '{"tipo":"fala","ouvir":"openai","ler":"openai"}')
})

test('sugestão 6: o rodapé distingue o cofre do Hub do cofre do app no mesmo Mac', () => {
  const local = pt([mac.rodapeChavesNoHub(true)])
  assert.match(local, /^Ficam no cofre do computador principal, que roda neste Mac/)
  assert.match(local, /Uma basta: não precisa cadastrar a mesma chave duas vezes\./)
  assert.doesNotMatch(local, /não neste Mac/)
  assert.doesNotMatch(pt([mac.rodapeChavesNoHub(false)]), /\{hub\}|servidor|Estúdio/, 'nome do computador nunca aparece')
  assert.match(pt([mac.rodapeChavesNoHub(false)]), /^Ficam no cofre do computador principal, não neste Mac\./)
})

// MARK: voz-04 (D70) — chave deste aparelho

type Situacao = import('../../../../shared/fork-voz').SituacaoChave
const chaves = (openai: Situacao, gemini: Situacao = 'ausente', reserva = false) => ({ openai: { situacao: openai, reserva }, gemini: { situacao: gemini, reserva } })

/** O main fictício: registra cada pedido de credencial e responde pelo teste. */
function aparelho(resposta: { ok: true; credential: string } | { ok: false; categoria: 'cofre' | 'chave' | 'cancelado' }) {
  const pedidos: string[][] = []
  return { pedidos, credencialDoAparelho: async (p: string, m: string, v: string) => { pedidos.push([p, m, v]); return resposta } }
}

async function geminiTempoReal(situacao: Situacao, resposta: Parameters<typeof aparelho>[0], reserva = false) {
  FakeSocket.todos = []
  const plano = mac.planoDoMotor({ aoVivo: 'gemini', fala: false, ouvir: 'device', ditado: true, aparelho: chaves('ausente', situacao, reserva) })
  const h = hub(async () => ({ credential: 'token-do-hub' }))
  const a = aparelho(resposta)
  const motor = mac.criarMotor(plano as never, { store: h.store, botId: 'bot', send: () => {}, credencialDoAparelho: a.credencialDoAparelho as never })
  await motor.start()
  for (let i = 0; i < 6; i++) await tick()
  return { plano, motor, hub: h.pedidos, aparelho: a.pedidos }
}

test('voz-04: no Tempo real, a chave deste aparelho vence; sem ela, a reserva do Hub', async () => {
  const propria = await geminiTempoReal('guardada', { ok: true, credential: 'auth_tokens/do-aparelho' })
  assert.equal(JSON.stringify(propria.plano), '{"tipo":"tempo-real","provider":"gemini","fonte":"aparelho"}')
  assert.equal(propria.aparelho.length, 1)
  assert.equal(propria.aparelho[0]![0], 'gemini')
  assert.deepEqual(propria.hub, [], 'o Hub não é chamado com chave própria')
  assert.equal(FakeSocket.todos.length, 1, 'a sessão abre com a credencial do aparelho')
  propria.motor.end()
  const reserva = await geminiTempoReal('ausente', { ok: false, categoria: 'cofre' })
  assert.equal(JSON.stringify(reserva.plano), '{"tipo":"tempo-real","provider":"gemini","fonte":"hub"}')
  assert.deepEqual(reserva.aparelho, [])
  assert.deepEqual(reserva.hub, ['voiceSession'])
  reserva.motor.end()
})

test('voz-04: chave deste aparelho travada ou recusada cai para o local no Mac e para no Windows/Linux, nunca o Hub', async () => {
  for (const [situacao, categoria, texto] of [['travada', 'cofre', mac.CHAVES_FALHA.cofre], ['recusada', 'chave', mac.CHAVES_FALHA.chave]] as const) {
    const r = await geminiTempoReal(situacao, { ok: false, categoria })
    assert.equal(r.motor.phase, 'failed', situacao)
    assert.equal(JSON.stringify(r.motor.falha), JSON.stringify({ chave: texto }))
    assert.deepEqual(r.hub, [], `${situacao}: nenhuma credencial do Hub`)
    assert.equal(FakeSocket.todos.length, 0)
  }
  assert.equal(mac.depoisDaFalha(true, true), 'queda', 'Mac: Ditado + Voz do macOS, com aviso')
  assert.equal(mac.depoisDaFalha(true, false), 'fim', 'Windows/Linux: avisa e para')
  // Só a escolha explícita ("Usar a chave do Hub") leva à reserva.
  const escolhida = await geminiTempoReal('travada', { ok: false, categoria: 'cofre' }, true)
  assert.equal(JSON.stringify(escolhida.plano), '{"tipo":"tempo-real","provider":"gemini","fonte":"hub"}')
  assert.deepEqual(escolhida.hub, ['voiceSession'])
  assert.deepEqual(escolhida.aparelho, [])
  escolhida.motor.end()
})

test('voz-04 até o 05b: "ouvir" e "ler" separados — a parte com chave deste aparelho nunca vai ao Hub', async () => {
  const plano = (aoVivo: 'openai' | 'gemini' | null, ouvir: 'device' | 'openai' | 'gemini', ap: ReturnType<typeof chaves>, ditado = true) =>
    mac.planoDoMotor({ aoVivo, fala: aoVivo !== null, ouvir, ditado, aparelho: ap })
  // Ouvir pela OpenAI (chave daqui) e ler pelo Gemini (sem chave daqui): ouve pelo Ditado, lê pelo Hub.
  const so = plano('gemini', 'openai', chaves('guardada'))
  assert.equal(JSON.stringify(so), '{"tipo":"fala-ditado","ler":"gemini","transicao":"ouvir"}')
  {
    const h = hub((m) => Promise.resolve(m === 'voiceSpeak' ? { audio: '', format: 'aac' } : { text: '' }))
    const motor = mac.criarMotor(so as never, { store: h.store, botId: 'bot', send: () => {} })
    await motor.start()
    ;(motor as unknown as { event(e: unknown): void }).event({ type: 'started' })
    motor.speak('Olá.')
    for (let i = 0; i < 6; i++) await tick()
    assert.equal(h.pedidos.includes('voiceTranscribe'), false, 'ouvir: nada pelo Hub')
    assert.equal(h.pedidos.includes('voiceSpeak'), true, 'ler: o Gemini sem chave daqui segue pela reserva')
    motor.end()
  }
  // Ouvir pela OpenAI (sem chave daqui) e ler pelo Gemini (chave daqui): ouve pelo Hub, lê pela Voz do macOS.
  const le = plano('gemini', 'openai', chaves('ausente', 'travada'))
  assert.equal(JSON.stringify(le), '{"tipo":"local-ouvindo","ouvir":"openai","transicao":"ler"}')
  {
    const h = hub((m) => Promise.resolve(m === 'voiceTranscribe' ? { text: 'oi' } : { audio: '', format: 'aac' }))
    const motor = mac.criarMotor(le as never, { store: h.store, botId: 'bot', send: () => {} })
    await motor.start()
    falar()
    for (let i = 0; i < 6; i++) await tick()
    motor.speak('Resposta.')
    for (let i = 0; i < 6; i++) await tick()
    assert.equal(h.pedidos.includes('voiceTranscribe'), true, 'ouvir: a OpenAI sem chave daqui segue pela reserva')
    assert.equal(h.pedidos.includes('voiceSpeak'), false, 'ler: nada pelo Hub')
    motor.end()
  }
  assert.equal(JSON.stringify(plano('openai', 'openai', chaves('recusada'))), '{"tipo":"local","transicao":"ambos"}')
  assert.equal(JSON.stringify(plano('gemini', 'device', chaves('ausente', 'guardada'))), '{"tipo":"local","transicao":"ler"}')
  assert.equal(JSON.stringify(plano(null, 'openai', chaves('guardada'))), '{"tipo":"local","transicao":"ouvir"}')
  assert.equal(JSON.stringify(plano('gemini', 'openai', chaves('ausente', 'ausente'))), '{"tipo":"fala","ouvir":"openai","ler":"gemini"}', 'sem chave daqui, nada muda')
  // Sem Ditado (Windows/Linux): o Fala não liga, com o texto aprovado.
  assert.equal(JSON.stringify(plano('gemini', 'openai', chaves('guardada'), false)), '{"tipo":"so-tempo-real","provedores":["openai"]}')
  assert.equal(JSON.stringify(plano('openai', 'openai', chaves('guardada'), false)), '{"tipo":"so-tempo-real","provedores":["openai"]}')
  assert.equal(pt([mac.TEXTOS_CHAMADA.transicao('ouvir')]), 'A chave deste Mac ainda funciona só no Tempo real do Ao vivo. Seguindo com o Ditado do macOS.')
  assert.match(pt([mac.TEXTOS_CHAMADA.soTempoReal('servidor')], false), /use o Tempo real ou limpe a chave deste computador para usar a do computador principal\.$/)
})

test('disco incerto ou limpeza pendente: todos os modos caem para o Mac com aviso ou param sem local', () => {
  for (const situacao of ['incerta', 'limpeza-pendente'] as const) {
    for (const fala of [false, true]) {
      const escolha = { aoVivo: 'openai' as const, fala, ouvir: 'gemini' as const, aparelho: chaves(situacao) }
      assert.equal(JSON.stringify(mac.planoDoMotor({ ...escolha, ditado: true })), JSON.stringify({ tipo: 'local', chavePendente: true }))
      assert.equal(JSON.stringify(mac.planoDoMotor({ ...escolha, ditado: false })), JSON.stringify({ tipo: 'sem-voz', chavePendente: true }))
    }
  }
})

test('voz-04 rodada 5: credencial cancelada produz falha segura, sem socket nem reserva automática', async () => {
  const r = await geminiTempoReal('guardada', { ok: false, categoria: 'cancelado' })
  assert.equal(r.motor.phase, 'failed')
  assert.equal(JSON.stringify(r.motor.falha), JSON.stringify({ chave: mac.CHAVES_FALHA.cofre }))
  assert.deepEqual(r.hub, [])
  assert.equal(FakeSocket.todos.length, 0)
  r.motor.end()
})
