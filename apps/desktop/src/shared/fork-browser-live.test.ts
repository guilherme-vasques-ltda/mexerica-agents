import test from 'node:test'
import assert from 'node:assert/strict'
import { BrowserHubSlot, BrowserLiveSession, createFrameGate, STALE_AFTER_MS, subscribeBody, type BrowserLiveBridge, type LiveState } from './fork-browser-live.ts'
import type { ForkCloudCallResult, ForkCloudStreamError } from './fork-cloud.ts'
import type { BrowserFrame } from './relay-protocol/browser-frames.ts'

const scope = { hubId: 'hub1', spaceId: 'sp1', botId: 'bot1' }

const quadro = (sequence: number, over: Partial<BrowserFrame> = {}): BrowserFrame => ({
  session: 's1',
  target: 'aba1',
  sequence,
  targetGeneration: 0,
  image: { width: 960, height: 540 },
  viewport: { width: 1280, height: 720 },
  jpeg: Uint8Array.from([0xff, 0xd8, sequence & 0xff, 0xff, 0xd9]),
  ...over,
})

type Chamada = { method: string; body: Record<string, unknown> }

/** O canal real aceita um consumidor de quadros por vez: o fake também, e liberar o slot é cancelar. */
function canalFalso(aceita = true) {
  const canal = {
    ocupado: false,
    cancelado: 0,
    consumidos: 0,
    abertos: 0,
    onFrame: (_f: BrowserFrame, _waitedMs: number) => {},
    onEnd: (_e: ForkCloudStreamError | null) => {},
    subscribeFrames(f: (frame: BrowserFrame, waitedMs: number) => void, e: (error: ForkCloudStreamError | null) => void, abrir?: Promise<boolean>) {
      canal.abertos += 1
      if (!aceita || canal.ocupado) {
        queueMicrotask(() => e({ status: 501, message: '' }))
        return { cancel: () => {}, consumed: () => {}, opened: Promise.resolve(false) }
      }
      canal.ocupado = true
      canal.onFrame = f
      canal.onEnd = e
      let ativo = true
      return {
        cancel: () => {
          if (!ativo) return
          ativo = false
          canal.ocupado = false
          canal.cancelado += 1
        },
        consumed: () => (canal.consumidos += 1),
        opened: abrir ?? Promise.resolve(true),
      }
    },
  }
  return canal
}

function montar(respostas: Record<string, (body: Record<string, unknown>, n: number) => ForkCloudCallResult | Promise<ForkCloudCallResult>> = {}, canal = canalFalso(), sessaoId = 's1', abrir?: Promise<boolean>, slot?: BrowserHubSlot) {
  const chamadas: Chamada[] = []
  const contagem: Record<string, number> = {}
  const padrao: Record<string, (body: Record<string, unknown>, n: number) => ForkCloudCallResult> = {
    browserStreamSubscribe: (b) => ({ kind: 'ok', value: { subscribed: true, frames: true, session: b.session } }),
    browserStreamControl: () => ({ kind: 'ok', value: { epoch: 1 } }),
    browserStreamTarget: () => ({ kind: 'ok', value: { targetGeneration: 1 } }),
    browserStreamInput: (b) => ({ kind: 'ok', value: { accepted: true, sequence: b.sequence } }),
    browserStreamAck: () => ({ kind: 'ok', value: { acknowledged: true } }),
  }
  const bridge: BrowserLiveBridge = {
    async call(method, body) {
      const b = body as Record<string, unknown>
      chamadas.push({ method, body: b })
      contagem[method] = (contagem[method] ?? 0) + 1
      return (respostas[method] ?? padrao[method]!)(b, contagem[method]!)
    },
    subscribeFrames: (f, e) => canal.subscribeFrames(f, e, abrir),
  }
  let agora = 1_000
  const sessao = new BrowserLiveSession(bridge, scope, sessaoId, 'aba1', () => agora, slot)
  const estados: LiveState[] = []
  const vistos: BrowserFrame[] = []
  sessao.subscribe({ onState: (s) => estados.push(s), onFrame: (f) => vistos.push(f) })
  const frame = (f: BrowserFrame, esperou = 0) => canal.onFrame(f, esperou)
  return {
    sessao,
    chamadas,
    estado: canal,
    estados,
    vistos,
    frame,
    /** O quadro chega e a tela termina de pintá-lo. */
    mostrar: (f: BrowserFrame, esperou = 0) => {
      frame(f, esperou)
      sessao.frameShown(f, true)
    },
    fim: (e: ForkCloudStreamError | null) => canal.onEnd(e),
    tempo: (ms: number) => (agora += ms),
    tick: () => new Promise((r) => setImmediate(r)),
  }
}

const aoVivoComPessoa = async (m: ReturnType<typeof montar>) => {
  await m.sessao.start()
  m.mostrar(quadro(1))
  assert.equal(await m.sessao.setController('person'), true)
}

test('inscreve a sessão com escopo, sessão e alvo e só fica ao vivo depois da resposta do Hub', async () => {
  const m = montar()
  assert.deepEqual(subscribeBody(scope, 's1', 'aba1'), { hubId: 'hub1', spaceId: 'sp1', botId: 'bot1', session: 's1', target: 'aba1' })
  assert.throws(() => subscribeBody({ ...scope, botId: '' }, 's1', 'aba1'), /browser_context_required/)
  const inicio = m.sessao.start()
  assert.equal(m.sessao.state.status, 'connecting')
  await inicio
  assert.equal(m.sessao.state.status, 'live')
  assert.deepEqual(m.chamadas.map((c) => c.method), ['browserStreamSubscribe'])
  assert.deepEqual(m.chamadas[0]!.body, { hubId: 'hub1', spaceId: 'sp1', botId: 'bot1', session: 's1', target: 'aba1' })
  assert.equal(m.estado.abertos, 1, 'a assinatura dos quadros abre antes do pedido ao Hub')
})

test('recusa do Hub, falta de capacidade e canal incerto fecham a tela, sem nada pendurado', async () => {
  const negado = montar({ browserStreamSubscribe: () => ({ kind: 'err', status: 403, message: 'x', code: 'browser_scope_unavailable' }) })
  await negado.sessao.start()
  assert.deepEqual([negado.sessao.state.status, negado.sessao.state.reason, negado.estado.cancelado], ['closed', 'denied', 1])

  const semCaps = montar({ browserStreamSubscribe: () => ({ kind: 'ok', value: { subscribed: true, frames: false, session: 's1' } }) })
  await semCaps.sessao.start()
  assert.deepEqual([semCaps.sessao.state.reason, semCaps.estado.cancelado], ['unsupported', 1])

  const semSuporte = montar({ browserStreamSubscribe: () => new Promise(() => {}) as never })
  const p = semSuporte.sessao.start()
  semSuporte.fim({ status: 501, message: '' })
  assert.equal(semSuporte.sessao.state.reason, 'unsupported')
  void p

  const incerto = montar({ browserStreamSubscribe: () => ({ kind: 'unknown', reason: 'timeout' }) })
  await incerto.sessao.start()
  assert.equal(incerto.sessao.state.reason, 'connectionLost')

  const outro = montar({ browserStreamSubscribe: () => ({ kind: 'err', status: 400, message: 'x' }) })
  await outro.sessao.start()
  assert.equal(outro.sessao.state.reason, 'unavailable')
})

test('revogação, troca de conta ou queda do canal fecham a tela na hora e cancelam a assinatura', async () => {
  const m = montar()
  await m.sessao.start()
  m.mostrar(quadro(1))
  m.fim({ status: 0, message: '' })
  assert.deepEqual([m.sessao.state.status, m.sessao.state.reason], ['closed', 'connectionLost'])
  assert.equal(m.sessao.frame, null, 'o último quadro não fica na memória')
  assert.deepEqual(await m.sessao.click(10, 10, 100, 100), { ok: false, reason: 'notLive' })
  const ultimo = m.estados.length
  m.sessao.close('closed')
  assert.equal(m.estados.length, ultimo, 'fechar de novo não repete')
})

test('quadro de outra sessão, outro alvo, outra geração ou sequência velha nunca chega à tela', async () => {
  const m = montar()
  await m.sessao.start()
  m.frame(quadro(5))
  m.frame(quadro(5))
  m.frame(quadro(4))
  m.frame(quadro(6, { session: 'outra' }))
  m.frame(quadro(7, { target: 'aba2' }))
  m.frame(quadro(8, { targetGeneration: 9 }))
  m.frame(quadro(9))
  assert.deepEqual(m.vistos.map((f) => f.sequence), [5, 9])
  assert.equal(m.estado.consumidos, 5, 'o que foi descartado libera a fila para o próximo')
})

test('cliente lento: a fila nunca passa de um quadro e só o mais recente é entregue', () => {
  const entregues: number[] = []
  const gate = createFrameGate<number>((n) => entregues.push(n))
  for (let n = 1; n <= 50; n += 1) gate.offer(n)
  assert.deepEqual(entregues, [1], 'enquanto a tela não confirma, nada mais sai')
  assert.equal(gate.queued, 1)
  assert.equal(gate.dropped, 48)
  gate.consumed()
  assert.deepEqual(entregues, [1, 50])
  gate.consumed()
  gate.offer(51)
  assert.deepEqual(entregues, [1, 50, 51])
  gate.clear()
  gate.offer(52)
  assert.deepEqual(entregues, [1, 50, 51, 52])
})

test('confirmar o quadro mostrado libera o próximo e manda um aviso por vez ao Hub', async () => {
  let solta: (r: ForkCloudCallResult) => void = () => {}
  const m = montar({ browserStreamAck: (_b, n) => (n === 1 ? (new Promise((r) => (solta = r)) as never) : { kind: 'ok', value: {} }) })
  await m.sessao.start()
  m.frame(quadro(1))
  m.sessao.frameShown(quadro(1), true)
  m.sessao.frameShown(quadro(1), true)
  m.sessao.frameShown(quadro(1), true)
  assert.equal(m.estado.consumidos, 3)
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamAck').length, 1, 'um aviso em voo; os outros se juntam')
  solta({ kind: 'ok', value: {} })
  await m.tick()
  await m.tick()
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamAck').length, 2)
})

test('a pessoa só envia entrada depois do Hub confirmar a época; o corpo leva a sequência do quadro mostrado', async () => {
  const m = montar()
  await m.sessao.start()
  m.mostrar(quadro(3))
  assert.deepEqual(await m.sessao.click(50, 40, 200, 100), { ok: false, reason: 'notPerson' })
  assert.equal(await m.sessao.setController('person'), true)
  assert.deepEqual(m.sessao.state.controller, 'person')
  assert.equal(m.sessao.state.epoch, 1)
  assert.deepEqual(await m.sessao.click(50, 40, 200, 100), { ok: true })
  const entrada = m.chamadas.find((c) => c.method === 'browserStreamInput')!
  assert.deepEqual(entrada.body, { session: 's1', target: 'aba1', frameSequence: 3, targetGeneration: 0, sequence: 1, epoch: 1, controller: 'person', x: 50, y: 40, displayWidth: 200, displayHeight: 100 })
  assert.deepEqual(await m.sessao.click(1, 1, 200, 100), { ok: true })
  assert.deepEqual(m.chamadas.filter((c) => c.method === 'browserStreamInput').map((c) => c.body.sequence), [1, 2])
})

test('o Hub recusando o controle fecha a tela; resposta incerta também; a pessoa não assume sem confirmação', async () => {
  const negado = montar({ browserStreamControl: () => ({ kind: 'err', status: 403, message: 'x' }) })
  await negado.sessao.start()
  assert.equal(await negado.sessao.setController('person'), false)
  assert.equal(negado.sessao.state.reason, 'denied')

  const incerto = montar({ browserStreamControl: () => ({ kind: 'unknown', reason: 'timeout' }) })
  await incerto.sessao.start()
  assert.equal(await incerto.sessao.setController('person'), false)
  assert.equal(incerto.sessao.state.controller, 'agent')
  assert.equal(incerto.sessao.state.status, 'closed')

  const recusa = montar({ browserStreamControl: () => ({ kind: 'err', status: 400, message: 'x' }) })
  await recusa.sessao.start()
  assert.equal(await recusa.sessao.setController('person'), false)
  assert.deepEqual([recusa.sessao.state.status, recusa.sessao.state.controller, recusa.sessao.state.switching], ['live', 'agent', false])
})

test('sem quadro ou com quadro de mais de 6 s a entrada fica bloqueada; coordenada inválida e fila cheia também', async () => {
  const m = montar()
  await m.sessao.start()
  assert.equal(await m.sessao.setController('person'), true)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'noFrame' })
  m.mostrar(quadro(1))
  m.tempo(STALE_AFTER_MS + 1)
  assert.equal(m.sessao.isStale(), true)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'stale' })
  m.mostrar(quadro(2))
  assert.equal(m.sessao.isStale(), false)
  assert.deepEqual(await m.sessao.click(Number.NaN, 5, 100, 100), { ok: false, reason: 'invalid' })
  assert.deepEqual(await m.sessao.click(5, 5, 0, 100), { ok: false, reason: 'invalid' })
  const lento = montar({ browserStreamInput: () => new Promise(() => {}) as never })
  await aoVivoComPessoa(lento)
  const envios = Array.from({ length: 4 }, () => lento.sessao.click(5, 5, 100, 100))
  assert.deepEqual(await lento.sessao.click(5, 5, 100, 100), { ok: false, reason: 'busy' })
  void envios
})

test('entrada recusada pelo Hub não avança a sequência; revogação no meio fecha tudo', async () => {
  let n = 0
  const m = montar({ browserStreamInput: (b) => (++n === 1 ? { kind: 'ok', value: { accepted: false, sequence: b.sequence } } : n === 3 ? { kind: 'err', status: 403, message: 'x' } : { kind: 'ok', value: { accepted: true, sequence: b.sequence } }) })
  await aoVivoComPessoa(m)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'refused' })
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: true })
  assert.deepEqual(m.chamadas.filter((c) => c.method === 'browserStreamInput').map((c) => c.body.sequence), [1, 1])
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'refused' })
  assert.deepEqual([m.sessao.state.status, m.sessao.state.reason, m.estado.cancelado], ['closed', 'denied', 1])
})

test('resultado incerto da entrada não repete sozinho; a próxima tenta a sequência seguinte uma vez', async () => {
  let n = 0
  const m = montar({ browserStreamInput: (b) => (++n === 1 ? { kind: 'unknown', reason: 'timeout' } : n === 2 ? { kind: 'ok', value: { accepted: false, sequence: b.sequence } } : { kind: 'ok', value: { accepted: true, sequence: b.sequence } }) })
  await aoVivoComPessoa(m)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'unknown' })
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: true })
  assert.deepEqual(m.chamadas.filter((c) => c.method === 'browserStreamInput').map((c) => c.body.sequence), [1, 1, 2])
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: true })
  assert.deepEqual(m.chamadas.filter((c) => c.method === 'browserStreamInput').map((c) => c.body.sequence), [1, 1, 2, 3])
})

test('incerteza também na nova tentativa fecha a tela: repetir as mesmas sequências nunca alcançaria o Hub', async () => {
  let n = 0
  const m = montar({ browserStreamInput: (b) => (++n === 1 || n === 3 ? { kind: 'unknown', reason: 'timeout' } : { kind: 'ok', value: { accepted: false, sequence: b.sequence } }) })
  await aoVivoComPessoa(m)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'unknown' })
  assert.equal(m.sessao.state.status, 'live', 'uma incerteza só: a tela segue e a próxima tenta a sequência seguinte')
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'unknown' })
  assert.deepEqual(m.chamadas.filter((c) => c.method === 'browserStreamInput').map((c) => c.body.sequence), [1, 1, 2])
  assert.deepEqual(m.sessao.state, { ...m.sessao.state, status: 'closed', reason: 'connectionLost' })
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'notLive' }, 'nada mais sai depois de fechar')
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamInput').length, 3)
})

test('painel fechado e reaberto durante a abertura local não deixa a sessão sem inscrição', async () => {
  const canal = canalFalso()
  let abre: (ok: boolean) => void = () => {}
  const m = montar({}, canal, 's1', new Promise<boolean>((r) => (abre = r)))
  const inicio = m.sessao.start()
  m.sessao.pause()
  const volta = m.sessao.resume()
  abre(true)
  await inicio
  await volta
  await m.tick()
  assert.deepEqual([m.sessao.state.status, m.sessao.state.reason], ['live', null])
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamSubscribe').length, 1)
  assert.equal(canal.ocupado, true, 'o consumidor local voltou')
  m.mostrar(quadro(1))
  assert.deepEqual(m.vistos.map((f) => f.sequence), [1])
})

test('painel fechado durante a abertura local e reaberto depois também inscreve', async () => {
  const canal = canalFalso()
  let abre: (ok: boolean) => void = () => {}
  const m = montar({}, canal, 's1', new Promise<boolean>((r) => (abre = r)))
  const inicio = m.sessao.start()
  m.sessao.pause()
  abre(true)
  await inicio
  assert.equal(m.sessao.state.status, 'connecting')
  assert.equal(m.chamadas.length, 0, 'pausada: nada vai ao Hub')
  await m.sessao.resume()
  await m.tick()
  assert.equal(m.sessao.state.status, 'live')
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamSubscribe').length, 1)
})

test('trocar de aba avança a geração, descarta quadro e entrada pendente da anterior', async () => {
  const m = montar()
  await aoVivoComPessoa(m)
  assert.equal(await m.sessao.setTarget('aba2'), true)
  assert.deepEqual([m.sessao.state.target, m.sessao.state.targetGeneration, m.sessao.frame], ['aba2', 1, null])
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'noFrame' })
  m.frame(quadro(1, { target: 'aba1' }))
  m.frame(quadro(2, { target: 'aba2', targetGeneration: 0 }))
  assert.equal(m.vistos.length, 1, 'só o quadro anterior à troca; os velhos não passam')
  m.mostrar(quadro(1, { target: 'aba2', targetGeneration: 1 }))
  assert.equal(m.vistos.length, 2)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: true })
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamInput')[0]!.body.targetGeneration, 1)
  assert.equal(await m.sessao.setTarget(''), false)
})

test('entrada na fila é descartada se o controle ou a aba mudarem antes de ela sair', async () => {
  let libera: (r: ForkCloudCallResult) => void = () => {}
  const m = montar({ browserStreamInput: (_b, n) => (n === 1 ? (new Promise((r) => (libera = r)) as never) : { kind: 'ok', value: { accepted: true } }) })
  await aoVivoComPessoa(m)
  const primeira = m.sessao.click(5, 5, 100, 100)
  const segunda = m.sessao.click(6, 6, 100, 100)
  await m.tick()
  assert.equal(await m.sessao.setController('agent'), true)
  libera({ kind: 'ok', value: { accepted: true } })
  assert.deepEqual(await primeira, { ok: true })
  assert.deepEqual(await segunda, { ok: false, reason: 'refused' })
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamInput').length, 1)
})

test('um clique só vale para o quadro realmente pintado: recebido mas não desenhado, ou desenho que falhou, não libera entrada', async () => {
  const m = montar()
  await aoVivoComPessoa(m)
  m.frame(quadro(2))
  assert.equal(m.sessao.frame?.sequence, 1, 'o quadro 2 chegou, mas a tela ainda mostra o 1')
  m.sessao.frameShown(quadro(2), false)
  assert.equal(m.sessao.frame, null, 'o desenho falhou: a tela é limpa e não sobra quadro para clicar')
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'noFrame' })
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamInput').length, 0)
  m.mostrar(quadro(3))
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: true })
  assert.equal(m.chamadas.find((c) => c.method === 'browserStreamInput')!.body.frameSequence, 3)
})

test('o prazo de 6 s conta desde a chegada do quadro: pintura atrasada não o reinicia', async () => {
  const m = montar()
  await aoVivoComPessoa(m)
  const lento = quadro(2)
  m.frame(lento)
  m.tempo(STALE_AFTER_MS + 1)
  m.sessao.frameShown(lento, true)
  assert.equal(m.sessao.frame, lento, 'o quadro foi pintado')
  assert.equal(m.sessao.isStale(), true, 'mas já chegou vencido: o tempo de decodificar conta')
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'stale' })

  const meio = quadro(3)
  m.frame(meio)
  m.tempo(3_000)
  m.sessao.frameShown(meio, true)
  assert.equal(m.sessao.isStale(), false, '3 s de pintura ainda cabem no prazo')
  m.tempo(3_500)
  assert.equal(m.sessao.isStale(), true, 'o prazo seguiu contando desde a chegada, não desde a pintura')
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'stale' })

  const rapido = quadro(4)
  m.frame(rapido)
  m.tempo(200)
  m.sessao.frameShown(rapido, true)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: true })

  m.sessao.frameShown(quadro(9), true)
  assert.equal(m.sessao.frame, rapido, 'quadro que a sessão nunca recebeu não vira referência de entrada')
})

test('a entrada é revalidada no envio: o quadro vence enquanto ela espera na fila e ela não sai', async () => {
  let libera: (r: ForkCloudCallResult) => void = () => {}
  const m = montar({ browserStreamInput: (_b, n) => (n === 1 ? (new Promise((r) => (libera = r)) as never) : { kind: 'ok', value: { accepted: true } }) })
  await aoVivoComPessoa(m)
  const primeira = m.sessao.click(5, 5, 100, 100)
  const segunda = m.sessao.click(6, 6, 100, 100)
  await m.tick()
  m.tempo(STALE_AFTER_MS + 1)
  libera({ kind: 'ok', value: { accepted: true } })
  assert.deepEqual(await primeira, { ok: true })
  assert.deepEqual(await segunda, { ok: false, reason: 'stale' })
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamInput').length, 1)
})

test('a entrada na fila não sai com um quadro mais novo na tela: as coordenadas eram do anterior', async () => {
  let libera: (r: ForkCloudCallResult) => void = () => {}
  const m = montar({ browserStreamInput: (_b, n) => (n === 1 ? (new Promise((r) => (libera = r)) as never) : { kind: 'ok', value: { accepted: true } }) })
  await aoVivoComPessoa(m)
  const primeira = m.sessao.click(5, 5, 100, 100)
  const segunda = m.sessao.click(6, 6, 100, 100)
  await m.tick()
  m.mostrar(quadro(2))
  libera({ kind: 'ok', value: { accepted: true } })
  assert.deepEqual(await primeira, { ok: true })
  assert.deepEqual(await segunda, { ok: false, reason: 'refused' })
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamInput').length, 1)
})

test('a nova tentativa depois de um resultado incerto também revalida o quadro', async () => {
  let n = 0
  const m = montar({ browserStreamInput: (b) => (++n === 1 ? { kind: 'unknown', reason: 'timeout' } : (m.tempo(STALE_AFTER_MS + 1), { kind: 'ok', value: { accepted: false, sequence: b.sequence } })) })
  await aoVivoComPessoa(m)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'unknown' })
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'stale' })
  assert.equal(m.chamadas.filter((c) => c.method === 'browserStreamInput').length, 2, 'a segunda tentativa velha não sai')
})

test('a assinatura local abre antes do pedido ao Hub; recusada, nada é inscrito do outro lado', async () => {
  let abre: (ok: boolean) => void = () => {}
  const espera = montar({}, canalFalso(), 's1', new Promise<boolean>((r) => (abre = r)))
  const inicio = espera.sessao.start()
  await espera.tick()
  assert.equal(espera.chamadas.length, 0, 'o Hub só é chamado depois de a assinatura local abrir')
  abre(true)
  await inicio
  assert.deepEqual(espera.chamadas.map((c) => c.method), ['browserStreamSubscribe'])

  const semCapacidade = montar({}, canalFalso(false))
  await semCapacidade.sessao.start()
  assert.deepEqual([semCapacidade.sessao.state.status, semCapacidade.sessao.state.reason], ['closed', 'unsupported'])
  assert.equal(semCapacidade.chamadas.length, 0, 'sem assinatura local, o Hub não recebe inscrição')

  const recusada = montar({}, canalFalso(), 's1', Promise.resolve(false))
  await recusada.sessao.start()
  assert.equal(recusada.sessao.state.status, 'closed')
  assert.equal(recusada.chamadas.length, 0)
})

test('painel fechado libera o slot de quadros da conexão; outra sessão abre e a primeira volta sem inscrever de novo', async () => {
  const canal = canalFalso()
  const a = montar({}, canal, 's1')
  await a.sessao.start()
  a.mostrar(quadro(1))
  a.sessao.pause()
  assert.equal(canal.ocupado, false, 'desmontar libera o consumidor')
  assert.equal(a.sessao.frame, null, 'sem quadro guardado depois de fechar')
  const b = montar({}, canal, 's2')
  await b.sessao.start()
  assert.deepEqual([b.sessao.state.status, b.sessao.state.reason], ['live', null], 'outro navegador na mesma conexão abre')
  b.sessao.pause()
  await a.sessao.resume()
  assert.equal(canal.ocupado, true)
  assert.deepEqual(a.chamadas.map((c) => c.method).filter((m) => m === 'browserStreamSubscribe'), ['browserStreamSubscribe'], 'a sessão do Hub continua; só o consumidor local volta')
  assert.equal(a.chamadas.filter((c) => c.method === 'browserStreamAck').length, 1 + 1, 'pede o próximo quadro ao voltar')
  a.mostrar(quadro(2))
  assert.deepEqual(a.vistos.map((f) => f.sequence), [1, 2])
  a.frame(quadro(2))
  a.frame(quadro(1, { session: 'outra' }))
  assert.equal(a.vistos.length, 2)
})

test('painel fechado e aberto de novo antes da inscrição terminar não perde o consumidor', async () => {
  const canal = canalFalso()
  let abre: (r: ForkCloudCallResult) => void = () => {}
  const m = montar({ browserStreamSubscribe: (b) => new Promise((r) => (abre = () => r({ kind: 'ok', value: { subscribed: true, frames: true, session: b.session } }))) as never }, canal)
  const inicio = m.sessao.start()
  await m.tick()
  m.sessao.pause()
  await m.sessao.resume()
  abre({ kind: 'ok', value: {} })
  await inicio
  await m.tick()
  assert.equal(m.sessao.state.status, 'live')
  assert.equal(canal.ocupado, true, 'o consumidor voltou quando a inscrição ficou ao vivo')
  m.mostrar(quadro(1))
  assert.deepEqual(m.vistos.map((f) => f.sequence), [1])
})

test('revogação do acesso fecha como negado, não como queda, e não deixa quadro nem entrada', async () => {
  const m = montar()
  await aoVivoComPessoa(m)
  const entrada = m.sessao.click(5, 5, 100, 100)
  m.fim({ status: 403, message: '' })
  assert.deepEqual([m.sessao.state.status, m.sessao.state.reason], ['closed', 'denied'])
  assert.equal(m.sessao.frame, null)
  assert.deepEqual(await entrada, { ok: false, reason: 'refused' })
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'notLive' })
})

/**
 * Hub com uma inscrição de navegador por canal (host/src/remote/channel.rs, stream/canal.rs): inscrever outra sessão derruba a
 * anterior, a sessão ainda inscrita recusa uma segunda inscrição (403) e confirmação, controle, aba e entrada valem para a
 * inscrição ativa, seja qual for a sessão do painel que mandou. O controle e a geração recomeçam a cada inscrição.
 */
function hubFalso() {
  const hub = {
    ativa: null as string | null,
    registradas: new Set<string>(),
    inscricoes: [] as string[],
    comandos: [] as { method: string; sessao: string | null; corpo: Record<string, unknown> }[],
    epocas: new Map<string, number>(),
    recusarInscricao: false,
    responder(method: string, b: Record<string, unknown>): ForkCloudCallResult {
      if (method === 'browserStreamSubscribe') {
        const sessao = b.session as string
        hub.inscricoes.push(sessao)
        if (hub.recusarInscricao || hub.registradas.has(sessao)) return { kind: 'err', status: 403, message: 'x', code: 'browser_scope_unavailable' }
        if (hub.ativa) hub.registradas.delete(hub.ativa)
        hub.registradas.add(sessao)
        hub.ativa = sessao
        hub.epocas.set(sessao, 0)
        return { kind: 'ok', value: { subscribed: true, frames: true, session: sessao } }
      }
      hub.comandos.push({ method, sessao: hub.ativa, corpo: b })
      if (!hub.ativa) return { kind: 'err', status: 400, message: 'x' }
      if (method === 'browserStreamControl') {
        const epoca = (hub.epocas.get(hub.ativa) ?? 0) + 1
        hub.epocas.set(hub.ativa, epoca)
        return { kind: 'ok', value: { epoch: epoca } }
      }
      if (method === 'browserStreamTarget') return { kind: 'ok', value: { targetGeneration: 1 } }
      if (method === 'browserStreamInput') return { kind: 'ok', value: { accepted: hub.ativa === b.session, sequence: b.sequence } }
      return { kind: 'ok', value: { acknowledged: true } }
    },
  }
  const viaHub = Object.fromEntries(['browserStreamSubscribe', 'browserStreamControl', 'browserStreamTarget', 'browserStreamInput', 'browserStreamAck'].map((m) => [m, (b: Record<string, unknown>) => hub.responder(m, b)]))
  return { hub, viaHub }
}

test('trocar de painel na mesma conexão: ao reabrir, a sessão se inscreve de novo e só então libera controle e entrada', async () => {
  const canal = canalFalso()
  const slot = new BrowserHubSlot()
  const { hub, viaHub } = hubFalso()
  const a = montar(viaHub, canal, 'sA', undefined, slot)
  const b = montar(viaHub, canal, 'sB', undefined, slot)
  await a.sessao.start()
  a.mostrar(quadro(1, { session: 'sA' }))
  assert.equal(await a.sessao.setController('person'), true)
  a.sessao.pause()
  await b.sessao.start()
  b.sessao.pause()
  assert.equal(hub.ativa, 'sB', 'o painel B tomou a inscrição única do Hub')

  const comandosAntes = hub.comandos.length
  const reabrir = a.sessao.resume()
  await reabrir
  assert.equal(hub.ativa, 'sA', 'o Hub inscreveu a sessão do painel A de novo')
  assert.deepEqual(hub.inscricoes, ['sA', 'sB', 'sA'])
  assert.ok(a.estados.some((e) => e.status === 'connecting'), 'enquanto reinscreve, nada de controle')
  assert.deepEqual([a.sessao.state.status, a.sessao.state.controller, a.sessao.state.epoch], ['live', 'agent', 0], 'o Hub recomeçou: o bot controla de novo')
  assert.equal(a.sessao.frame, null)
  assert.ok(hub.comandos.slice(comandosAntes).every((c) => c.sessao === 'sA'), 'nenhum comando do painel A foi para a inscrição do B')

  a.mostrar(quadro(1, { session: 'sA' }))
  assert.deepEqual(await a.sessao.click(5, 5, 100, 100), { ok: false, reason: 'notPerson' })
  assert.equal(await a.sessao.setController('person'), true)
  assert.equal(hub.comandos.at(-1)!.sessao, 'sA')
  assert.deepEqual(await a.sessao.click(5, 5, 100, 100), { ok: true })
  assert.deepEqual([hub.comandos.at(-1)!.method, hub.comandos.at(-1)!.sessao], ['browserStreamInput', 'sA'])
})

test('reabrir sem outra sessão ter assumido a conexão não inscreve de novo (o Hub recusaria a repetida)', async () => {
  const canal = canalFalso()
  const { hub, viaHub } = hubFalso()
  const a = montar(viaHub, canal, 'sA', undefined, new BrowserHubSlot())
  await a.sessao.start()
  a.sessao.pause()
  await a.sessao.resume()
  assert.deepEqual(hub.inscricoes, ['sA'])
  assert.equal(a.sessao.state.status, 'live')
  assert.equal(await a.sessao.setController('person'), true)
})

test('painel aberto e derrubado por outra sessão da conexão fecha e não manda mais comando ao Hub', async () => {
  const canal = canalFalso()
  const slot = new BrowserHubSlot()
  const { hub, viaHub } = hubFalso()
  const a = montar(viaHub, canal, 'sA', undefined, slot)
  await aoVivoComPessoa(a)
  const b = montar(viaHub, canalFalso(), 'sB', undefined, slot)
  await b.sessao.start()
  assert.deepEqual([a.sessao.state.status, a.sessao.state.reason], ['closed', 'unavailable'])
  const antes = hub.comandos.length
  assert.equal(await a.sessao.setController('agent'), false)
  assert.deepEqual(await a.sessao.click(5, 5, 100, 100), { ok: false, reason: 'notLive' })
  assert.equal(hub.comandos.length, antes)
})

test('reinscrever ao reabrir e o Hub recusar: a tela fecha, nada fica pela metade', async () => {
  const canal = canalFalso()
  const slot = new BrowserHubSlot()
  const { hub, viaHub } = hubFalso()
  const a = montar(viaHub, canal, 'sA', undefined, slot)
  const b = montar(viaHub, canal, 'sB', undefined, slot)
  await a.sessao.start()
  a.sessao.pause()
  await b.sessao.start()
  b.sessao.pause()
  hub.recusarInscricao = true
  await a.sessao.resume()
  assert.deepEqual([a.sessao.state.status, a.sessao.state.reason], ['closed', 'denied'])
  assert.equal(canal.ocupado, false, 'o consumidor local também sai')
})

test('a fila do cliente mede a espera desde a chegada e entrega a idade junto', () => {
  let agora = 100
  const entregas: [number, number][] = []
  const fila = createFrameGate<number>((n, esperou) => entregas.push([n, esperou]), () => agora)
  fila.offer(1)
  agora += 50
  fila.offer(2)
  agora += 6_500
  fila.offer(3)
  agora += 800
  fila.consumed()
  assert.deepEqual(entregas, [[1, 0], [3, 800]], 'o quadro 3 esperou 800 ms; o 2 foi descartado')
  fila.offer(4)
  agora += 7_000
  fila.consumed()
  assert.deepEqual(entregas.at(-1), [4, 7_000], 'quadro que ficou 7 s na fila sai com 7 s de idade')
  fila.consumed()
  fila.offer(5)
  assert.deepEqual(entregas.at(-1), [5, 0], 'controle: sem fila a espera é zero')
})

test('quadro que envelheceu na fila do cliente chega vencido e não libera clique; o relógio não reinicia ao entregar', async () => {
  const m = montar()
  await aoVivoComPessoa(m)
  const velho = quadro(2)
  m.mostrar(velho, STALE_AFTER_MS + 500)
  assert.equal(m.sessao.frame, velho)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'stale' })

  const novo = quadro(3)
  m.mostrar(novo, 5_000)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: true }, 'controle: 5 s de fila ainda cabem')
  m.tempo(1_500)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'stale' }, 'a idade seguiu contando até o envio')

  for (const invalido of [Number.NaN, -1, Number.POSITIVE_INFINITY]) {
    const q = quadro(10 + (Number.isNaN(invalido) ? 0 : invalido < 0 ? 1 : 2))
    m.mostrar(q, invalido)
    assert.equal(m.sessao.isStale(), true, 'idade inválida conta como vencida: ' + invalido)
  }
})

test('fila e relógio de ponta a ponta: o quadro que esperou 6,5 s na fila do desktop chega à sessão já vencido', async () => {
  const m = montar()
  await aoVivoComPessoa(m)
  let agora = 0
  const fila = createFrameGate<BrowserFrame>((f, esperou) => m.frame(f, esperou), () => agora)
  fila.offer(quadro(2))
  const preso = quadro(3)
  fila.offer(preso)
  agora += 6_500
  fila.consumed()
  m.sessao.frameShown(preso, true)
  assert.equal(m.sessao.frame, preso)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: false, reason: 'stale' })
  agora += 1
  const rapido = quadro(4)
  fila.offer(rapido)
  fila.consumed()
  m.sessao.frameShown(rapido, true)
  assert.deepEqual(await m.sessao.click(5, 5, 100, 100), { ok: true }, 'controle: sem espera na fila o clique sai')
})

/** Inscrição cuja resposta o teste segura: o Hub já trocou a inscrição ao receber, a resposta chega depois. */
function inscricaoControlada(hub: ReturnType<typeof hubFalso>['hub']) {
  const pendentes: { sessao: string; responder: (r?: ForkCloudCallResult) => void }[] = []
  return {
    pendentes,
    responder: (b: Record<string, unknown>) =>
      new Promise<ForkCloudCallResult>((resolve) => {
        const aplicada = hub.responder('browserStreamSubscribe', b)
        pendentes.push({ sessao: b.session as string, responder: (r) => resolve(r ?? aplicada) })
      }),
  }
}

const aoVivoA = async (a: ReturnType<typeof montar>) => {
  await a.sessao.start()
  a.mostrar(quadro(1, { session: 'sA' }))
  assert.equal(await a.sessao.setController('person'), true)
}

test('durante a troca de inscrição nenhum painel manda comando; só a confirmação positiva da tentativa vigente devolve a autoridade', async () => {
  const slot = new BrowserHubSlot()
  const { hub, viaHub } = hubFalso()
  const a = montar(viaHub, canalFalso(), 'sA', undefined, slot)
  await aoVivoA(a)
  const inscricao = inscricaoControlada(hub)
  const b = montar({ ...viaHub, browserStreamSubscribe: inscricao.responder }, canalFalso(), 'sB', undefined, slot)
  const inicio = b.sessao.start()
  await b.tick()
  assert.equal(hub.ativa, 'sB', 'o Hub já trocou a inscrição, e a resposta ainda não chegou')
  assert.equal(slot.active, null, 'a autoridade da conexão inteira está suspensa')

  const antes = hub.comandos.length
  assert.equal(await a.sessao.setController('agent'), false)
  assert.equal(await a.sessao.setTarget('aba2'), false)
  assert.deepEqual(await a.sessao.click(5, 5, 100, 100), { ok: false, reason: 'notLive' })
  a.sessao.frameShown(quadro(2, { session: 'sA' }), true)
  assert.equal(await b.sessao.setController('person'), false, 'o painel novo também espera a confirmação')
  assert.equal(hub.comandos.length, antes, 'nenhum comando saiu durante a troca')

  inscricao.pendentes[0]!.responder()
  await inicio
  assert.deepEqual([b.sessao.state.status, slot.active === b.sessao], ['live', true])
  assert.deepEqual([a.sessao.state.status, a.sessao.state.reason], ['closed', 'unavailable'], 'o painel anterior perde a sessão, sem comando')
  assert.deepEqual(slot.log.filter((e) => e.event !== 'release').map((e) => e.event + ':' + e.session), ['begin:sA', 'confirm:sA', 'begin:sB', 'confirm:sB'])
  assert.equal(await b.sessao.setController('person'), true)
  assert.equal(hub.comandos.at(-1)!.sessao, 'sB')
})

test('resposta incerta da inscrição não devolve a autoridade ao painel anterior', async () => {
  const slot = new BrowserHubSlot()
  const { hub, viaHub } = hubFalso()
  const a = montar(viaHub, canalFalso(), 'sA', undefined, slot)
  await aoVivoA(a)
  const inscricao = inscricaoControlada(hub)
  const b = montar({ ...viaHub, browserStreamSubscribe: inscricao.responder }, canalFalso(), 'sB', undefined, slot)
  const inicio = b.sessao.start()
  await b.tick()
  inscricao.pendentes[0]!.responder({ kind: 'unknown', reason: 'timeout' })
  await inicio
  assert.deepEqual([b.sessao.state.status, b.sessao.state.reason], ['closed', 'connectionLost'])
  assert.equal(slot.active, null, 'ninguém tem autoridade')
  assert.deepEqual([a.sessao.state.status, a.sessao.state.reason], ['closed', 'unavailable'])
  const antes = hub.comandos.length
  assert.equal(await a.sessao.setController('agent'), false)
  assert.equal(hub.comandos.length, antes)
  assert.ok(slot.log.some((e) => e.event === 'abandon' && e.session === 'sB'))
})

test('recusa explícita do Hub, que não trocou a inscrição, devolve a autoridade ao painel anterior (controle positivo)', async () => {
  const slot = new BrowserHubSlot()
  const { hub, viaHub } = hubFalso()
  const a = montar(viaHub, canalFalso(), 'sA', undefined, slot)
  await aoVivoA(a)
  const b = montar({ ...viaHub, browserStreamSubscribe: () => ({ kind: 'err', status: 403, message: 'x', code: 'browser_scope_unavailable' }) }, canalFalso(), 'sB', undefined, slot)
  await b.sessao.start()
  assert.deepEqual([b.sessao.state.status, b.sessao.state.reason], ['closed', 'denied'])
  assert.equal(slot.active, a.sessao, 'o Hub não trocou nada: A continua inscrita')
  assert.equal(a.sessao.state.status, 'live')
  assert.equal(await a.sessao.setController('agent'), true)
  assert.equal(hub.comandos.at(-1)!.sessao, 'sA')
  assert.ok(slot.log.some((e) => e.event === 'reject' && e.session === 'sB'))
})

test('confirmação atrasada de uma tentativa já substituída não dá autoridade a ninguém', async () => {
  const slot = new BrowserHubSlot()
  const { hub, viaHub } = hubFalso()
  const inscricao = inscricaoControlada(hub)
  const resp = { ...viaHub, browserStreamSubscribe: inscricao.responder }
  const b = montar(resp, canalFalso(), 'sB', undefined, slot)
  const c = montar(resp, canalFalso(), 'sC', undefined, slot)
  const iniciaB = b.sessao.start()
  await b.tick()
  const iniciaC = c.sessao.start()
  await c.tick()
  assert.deepEqual(inscricao.pendentes.map((p) => p.sessao), ['sB', 'sC'])
  inscricao.pendentes[0]!.responder()
  await iniciaB
  assert.deepEqual([b.sessao.state.status, b.sessao.state.reason], ['closed', 'unavailable'], 'a confirmação de B chegou atrasada')
  assert.equal(slot.active, null, 'a confirmação atrasada não restabelece autoridade')
  assert.equal(await b.sessao.setController('person'), false)
  assert.equal(await c.sessao.setController('person'), false, 'C ainda espera a própria confirmação')
  inscricao.pendentes[1]!.responder()
  await iniciaC
  assert.equal(slot.active, c.sessao)
  assert.equal(await c.sessao.setController('person'), true)
  assert.equal(hub.comandos.at(-1)!.sessao, 'sC')
  assert.ok(slot.log.some((e) => e.event === 'superseded' && e.session === 'sB'))
})
