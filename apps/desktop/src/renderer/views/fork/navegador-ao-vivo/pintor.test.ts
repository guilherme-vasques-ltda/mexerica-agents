import test from 'node:test'
import assert from 'node:assert/strict'
import { BrowserLiveSession, type BrowserLiveBridge } from '../../../../shared/fork-browser-live.ts'
import type { ForkCloudCallResult, ForkCloudStreamError } from '../../../../shared/fork-cloud.ts'
import type { BrowserFrame } from '../../../../shared/relay-protocol/browser-frames.ts'
import { jpegSintetico } from '../../../../shared/relay-protocol/browser-frames-fixtures.ts'
import { desenhar } from './desenhar.ts'
import { ligarPintor } from './pintor.ts'

const scope = { hubId: 'h', spaceId: 'sp', botId: 'b' }

const quadro = (sequence: number, jpeg = jpegSintetico(960, 540)): BrowserFrame => ({ session: 's1', target: 'aba1', sequence, targetGeneration: 0, image: { width: 960, height: 540 }, viewport: { width: 1280, height: 720 }, jpeg })

const tela = () => {
  const chamadas: string[] = []
  return { width: 100, height: 100, chamadas, getContext: () => ({ drawImage: () => chamadas.push('desenhou') }) }
}

/** Bitmap falso com decodificação controlada pelo teste: cada `criar` fica pendente até `liberar`. */
function bitmaps(largura = 960, altura = 540) {
  const pendentes: ((ok: boolean) => void)[] = []
  const fechados: number[] = []
  let criados = 0
  return {
    fechados,
    get criados() {
      return criados
    },
    pendentes,
    criar: () =>
      new Promise<{ width: number; height: number; close(): void }>((resolve, reject) => {
        criados += 1
        const n = criados
        pendentes.push((ok) => (ok ? resolve({ width: largura, height: altura, close: () => fechados.push(n) }) : reject(new Error('decodificação falhou'))))
      }),
    liberar: (ok = true) => pendentes.shift()!(ok),
  }
}

function sessaoComCanal() {
  let onFrame: (f: BrowserFrame, waitedMs: number) => void = () => {}
  let onEnd: (e: ForkCloudStreamError | null) => void = () => {}
  const estado = { consumidos: 0, cancelados: 0 }
  const bridge: BrowserLiveBridge = {
    async call(method, body): Promise<ForkCloudCallResult> {
      if (method === 'browserStreamSubscribe') return { kind: 'ok', value: { subscribed: true, frames: true, session: (body as { session: string }).session } }
      if (method === 'browserStreamControl') return { kind: 'ok', value: { epoch: 1 } }
      return { kind: 'ok', value: {} }
    },
    subscribeFrames(f, e) {
      onFrame = f
      onEnd = e
      return { cancel: () => (estado.cancelados += 1), consumed: () => (estado.consumidos += 1), opened: Promise.resolve(true) }
    },
  }
  const sessao = new BrowserLiveSession(bridge, scope, 's1', 'aba1', () => 1_000)
  return { sessao, estado, frame: (f: BrowserFrame) => onFrame(f, 0), fim: (e: ForkCloudStreamError | null) => onEnd(e) }
}

const settle = () => new Promise((r) => setImmediate(r))

test('quadro normal é desenhado e o bitmap sempre é fechado, também quando o desenho falha', async () => {
  const ok = bitmaps()
  const canvas = tela()
  const p = desenhar(canvas, quadro(1), () => true, ok.criar)
  ok.liberar(true)
  assert.equal(await p, true)
  assert.deepEqual([canvas.width, canvas.height, canvas.chamadas, ok.fechados], [960, 540, ['desenhou'], [1]])

  const falha = bitmaps()
  const quebrada = { width: 5, height: 5, getContext: () => ({ drawImage: () => { throw new Error('canvas quebrou') } }) }
  const q = desenhar(quebrada, quadro(2), () => true, falha.criar)
  falha.liberar(true)
  assert.equal(await q, false)
  assert.deepEqual(falha.fechados, [1], 'fechou o bitmap mesmo com erro no desenho')
})

test('imagem acima do teto de pixels não chega nem a ser decodificada', async () => {
  const enorme = bitmaps()
  assert.equal(await desenhar(tela(), quadro(1, jpegSintetico(8192, 8192)), () => true, enorme.criar), false)
  assert.equal(enorme.criados, 0, 'recusado antes de decodificar')
  const mente = bitmaps(20_000, 20_000)
  const canvas = tela()
  const p = desenhar(canvas, quadro(2), () => true, mente.criar)
  mente.liberar(true)
  assert.equal(await p, false, 'o bitmap decodificado maior que o teto também é recusado')
  assert.deepEqual([canvas.width, canvas.chamadas, mente.fechados], [100, [], [1]])
})

test('bitmap com outra proporção que o cabeçalho anunciou não é pintado: o clique desviaria', async () => {
  const outra = bitmaps(960, 720)
  const canvas = tela()
  const p = desenhar(canvas, quadro(1), () => true, outra.criar)
  outra.liberar(true)
  assert.equal(await p, false)
  assert.deepEqual([canvas.width, canvas.chamadas, outra.fechados], [100, [], [1]], 'canvas intocado e bitmap fechado')
  const certa = bitmaps(960, 540)
  const q = desenhar(canvas, quadro(2), () => true, certa.criar)
  certa.liberar(true)
  assert.equal(await q, true, 'controle: mesma proporção pinta')
})

test('desenho que termina depois de a tela ser desfeita não toca o canvas e fecha o bitmap', async () => {
  const b = bitmaps()
  const canvas = tela()
  let vale = true
  const p = desenhar(canvas, quadro(1), () => vale, b.criar)
  vale = false
  b.liberar(true)
  assert.equal(await p, false)
  assert.deepEqual([canvas.width, canvas.chamadas, b.fechados], [100, [], [1]])
})

test('revogação: o canvas é limpo na hora e o desenho em andamento é descartado, sem confirmar o quadro', async () => {
  const c = sessaoComCanal()
  const canvas = tela()
  const b = bitmaps()
  let pintados = 0
  ligarPintor(c.sessao, () => canvas, () => (pintados += 1), b.criar)
  await settle()
  c.frame(quadro(1))
  b.liberar(true)
  await settle()
  assert.deepEqual([canvas.width, canvas.height, pintados, c.sessao.frame?.sequence], [960, 540, 1, 1], 'o primeiro quadro foi pintado')

  c.frame(quadro(2))
  assert.equal(b.pendentes.length, 1, 'o segundo desenho está em andamento')
  c.fim({ status: 403, message: '' })
  assert.deepEqual([c.sessao.state.reason, canvas.width, canvas.height], ['denied', 0, 0], 'pixels apagados na hora')
  b.liberar(true)
  await settle()
  assert.deepEqual([canvas.width, canvas.chamadas.length, pintados, c.sessao.frame, b.fechados], [0, 1, 1, null, [1, 2]], 'o desenho pendente não voltou à tela')
})

test('desenho que falha limpa o canvas e deixa a sessão sem quadro para clicar', async () => {
  const c = sessaoComCanal()
  const canvas = tela()
  const b = bitmaps()
  ligarPintor(c.sessao, () => canvas, () => {}, b.criar)
  await settle()
  c.frame(quadro(1))
  b.liberar(true)
  await settle()
  assert.equal(c.sessao.frame?.sequence, 1)
  c.frame(quadro(2))
  b.liberar(false)
  await settle()
  assert.deepEqual([c.sessao.frame, canvas.width, c.estado.consumidos], [null, 0, 2], 'sem quadro, tela limpa, e a fila andou')
})

test('trocar de painel: desligar libera o consumidor e um desenho em andamento não confirma nada depois', async () => {
  const c = sessaoComCanal()
  const canvas = tela()
  const b = bitmaps()
  const desligar = ligarPintor(c.sessao, () => canvas, () => {}, b.criar)
  await settle()
  c.frame(quadro(1))
  desligar()
  assert.equal(c.estado.cancelados, 1, 'o consumidor local saiu')
  b.liberar(true)
  await settle()
  assert.deepEqual([canvas.chamadas.length, c.sessao.frame, c.estado.consumidos], [0, null, 0])
})
