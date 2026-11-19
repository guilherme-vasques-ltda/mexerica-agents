// MX-214 (rodadas 4 e 5): o resultado do envio da Ilha volta pela janela principal (registro do processo principal).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { isWindowSender } from '../main/fork-chat-sender.ts'
import { pedidoDaIlha, RespostasDaIlha } from '../main/fork-island-resposta.ts'

test('a conversa responde "bloqueado" ou "aceito": a Ilha recebe exatamente isso', async () => {
  const r = new RespostasDaIlha()
  const a = r.abrir(1000)
  assert.equal(r.responder(a.id, 'bloqueado'), true)
  assert.equal(await a.resultado, 'bloqueado')
  const b = r.abrir(1000)
  r.responder(b.id, 'aceito')
  assert.equal(await b.resultado, 'aceito')
})

test('sem confirmação NÃO é aceite: o prazo resolve "sem-confirmacao"', async () => {
  const r = new RespostasDaIlha()
  const muda = r.abrir(20)
  assert.equal(await muda.resultado, 'sem-confirmacao')
})

test('resposta atrasada (depois do prazo), repetida ou de outro envio nunca resolve nada', async () => {
  const r = new RespostasDaIlha()
  const velha = r.abrir(20, 'envio-velho-0001')
  assert.equal(await velha.resultado, 'sem-confirmacao')
  assert.equal(r.responder('envio-velho-0001', 'aceito'), false, 'resposta atrasada é ignorada')
  const a = r.abrir(1000, 'envio-novo-0001')
  assert.equal(r.responder('envio-velho-0001', 'aceito'), false, 'id de outro envio não resolve este')
  assert.equal(r.responder('nao-existe', 'bloqueado'), false)
  assert.equal(r.responder(42, 'bloqueado'), false)
  assert.equal(r.responder(a.id, 'bloqueado'), true)
  assert.equal(r.responder(a.id, 'aceito'), false, 'a segunda resposta não muda nada')
  assert.equal(await a.resultado, 'bloqueado')
})

test('resultado fora do formato é ignorado (não consome o pedido): o prazo decide', async () => {
  const r = new RespostasDaIlha()
  const a = r.abrir(30)
  for (const ruim of ['sem-confirmacao', 'ok', '', null, undefined, 1, { r: 'aceito' }, ['aceito']]) assert.equal(r.responder(a.id, ruim), false, String(ruim))
  assert.equal(await a.resultado, 'sem-confirmacao')
})

test('o pedido da Ilha valida o formato antes de chegar à conversa', () => {
  const ok = pedidoDaIlha('mac/byte', 'oi', undefined, 'a1b2c3d4-e5f6')
  assert.deepEqual(ok, { ref: 'mac/byte', text: 'oi', contentOrigin: undefined, nonce: 'a1b2c3d4-e5f6' })
  assert.ok(pedidoDaIlha('mac/byte', 'oi', { segments: [] }, 'a1b2c3d4-e5f6'))
  for (const [ref, text, origem, nonce] of [
    [1, 'oi', undefined, 'a1b2c3d4-e5f6'], ['', 'oi', undefined, 'a1b2c3d4-e5f6'], ['r', 42, undefined, 'a1b2c3d4-e5f6'], ['r', '   ', undefined, 'a1b2c3d4-e5f6'],
    ['r', 'oi', 'texto', 'a1b2c3d4-e5f6'], ['r', 'oi', [1], 'a1b2c3d4-e5f6'], ['r', 'oi', undefined, ''], ['r', 'oi', undefined, 'curto'], ['r', 'oi', undefined, 'tem espaço e símbolos!!'], ['r', 'oi', undefined, undefined],
  ] as const) assert.equal(pedidoDaIlha(ref, text, origem, nonce), null, JSON.stringify([ref, text, origem, nonce]))
})

test('só o frame principal da janela conhecida, na URL local esperada, é remetente válido', () => {
  const principal = { url: 'file:///app/index.html?window=island#x' }
  const web = { mainFrame: principal }
  const janela = { isDestroyed: () => false, webContents: web } as never
  const url = 'file:///app/index.html?window=island'
  assert.equal(isWindowSender({ sender: web, senderFrame: principal } as never, janela, url), true)
  assert.equal(isWindowSender({ sender: web, senderFrame: { url: principal.url } } as never, janela, url), false, 'subframe não vale')
  assert.equal(isWindowSender({ sender: { mainFrame: principal }, senderFrame: principal } as never, janela, url), false, 'outro webContents não vale')
  assert.equal(isWindowSender({ sender: web, senderFrame: principal } as never, null, url), false, 'sem janela')
  assert.equal(isWindowSender({ sender: web, senderFrame: principal } as never, { isDestroyed: () => true, webContents: web } as never, url), false, 'janela destruída')
  assert.equal(isWindowSender({ sender: web, senderFrame: { ...principal, url: 'file:///outra.html' } } as never, { isDestroyed: () => false, webContents: { mainFrame: { url: 'file:///outra.html' } } } as never, url), false, 'URL diferente da esperada')
  assert.equal(isWindowSender({ sender: web, senderFrame: principal } as never, janela, 'https://exemplo.com/index.html'), false, 'URL esperada não local')
})
