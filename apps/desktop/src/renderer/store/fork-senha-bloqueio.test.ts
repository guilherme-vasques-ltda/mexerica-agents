// MX-214 (rodada 3): com o aviso de senha colada aberto, NENHUMA entrada envia ao bot: o BotStore REAL recusa o envio
// (chamada de voz, painel, Ilha…), e volta a enviar quando o aviso fecha. Só o transporte e a ponte do Electron são fictícios.
import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Computer } from '../../shared/models'
import type { HostTransport } from '../client/host-client'

const bundle = buildSync({
  stdin: {
    contents: `export { BotStore } from './store/bot-store'; export { normalizeBot } from '../shared/models'; export { bloqueioSenha, destinoSenha } from './store/fork-senha-bloqueio'; export { responderOf } from './views/fork/torre/responder'; export { IslandModel } from './views/island/island-model'`,
    resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, alias: { '@shared': new URL('../../shared', import.meta.url).pathname }, loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
})
const settle = async () => { for (let i = 0; i < 12; i++) await tick() }
const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })


/** O BotStore real com um Hub fictício; `falhar.envio` faz o próximo `send` do Hub falhar (a entrada local vira "failed"). */
async function abrir(janela: Record<string, unknown> = {}) {
  const calls: { method: string; body: Record<string, unknown> }[] = []
  const falhar = { envio: false }
  let seq = 0
  const transport: HostTransport = {
    async call(method, raw) {
      const body = raw as Record<string, unknown>
      calls.push({ method, body })
      switch (method) {
        case 'hello': return { version: '2.20.3', minApp: '2.20.1', name: 'Fixture', backends: [] }
        case 'send': {
          if (falhar.envio) throw new Error('Hub fora do ar')
          return { entry: { id: `e-${++seq}`, seq, botId: body.botId ?? 'byte', rev: 10 + seq, kind: 'user', turn: 1, createdAt: 1, updatedAt: 1, data: { text: body.text, status: 'sent', clientNonce: body.clientNonce } } }
        }
        default: return {}
      }
    },
    stream(_path, onData) {
      queueMicrotask(() => onData(JSON.stringify({ type: 'hello', hostId: 'hub', rev: 1, usage: { providers: [] } })))
      return () => {}
    },
  }
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, URLSearchParams, Date, setTimeout, clearTimeout, crypto, btoa, Uint8Array,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    document: { documentElement: {}, body: { setAttribute() {} } },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3', platform: 'darwin' }, janela), location: { search: '' } },
  })
  const exp = module.exports as {
    BotStore: typeof import('./bot-store').BotStore; normalizeBot: typeof import('../../shared/models').normalizeBot
    bloqueioSenha: typeof import('./fork-senha-bloqueio').bloqueioSenha; destinoSenha: typeof import('./fork-senha-bloqueio').destinoSenha
    responderOf: typeof import('../views/fork/torre/responder').responderOf; IslandModel: typeof import('../views/island/island-model').IslandModel
  }
  const store = new exp.BotStore({ id: 'mac', name: 'Mac', signKey: '', urls: [] } as Computer, true, () => transport)
  store.start()
  await settle()
  store.bots.set('byte', exp.normalizeBot({ id: 'byte', name: 'Byte', backend: 'codex' }))
  store.bots.set('prosa', exp.normalizeBot({ id: 'prosa', name: 'Prosa', backend: 'codex' }))
  const enviados = () => calls.filter((c) => c.method === 'send').length
  const bloquear = () => exp.bloqueioSenha.abrir('rascunho-byte', exp.destinoSenha('mac', 'byte'))
  const liberar = () => exp.bloqueioSenha.fechar('rascunho-byte')
  const erro = () => (store as unknown as { lastError: string | null }).lastError
  const barrado = () => exp.bloqueioSenha.foiBarrado(exp.destinoSenha('mac', 'byte'))
  const falhas = () => store.allEntries('byte').filter((e) => e.data.status === 'failed')
  return { store, calls, falhar, enviados, bloquear, liberar, erro, barrado, falhas, ...exp }
}

test('com o aviso aberto o BotStore não envia (voz incluída), avisa o motivo e devolve "bloqueado"; fechado, envia; outro bot não é afetado', async () => {
  const { store, enviados, bloquear, liberar, erro, barrado } = await abrir()
  try {
    assert.equal(store.send('antes do aviso', 'byte'), 'aceito')
    await settle()
    assert.equal(enviados(), 1, 'controle: sem aviso o envio chega ao Hub')

    bloquear()
    assert.equal(store.send('fala da chamada', 'byte', { sourceKind: 'voice' }), 'bloqueado')
    assert.equal(store.send('outra entrada do painel', 'byte'), 'bloqueado')
    await settle()
    assert.equal(enviados(), 1, 'com o aviso aberto nada sai')
    assert.equal(store.allEntries('byte').length, 1, 'nem aparece bolha local nova')
    assert.equal(barrado(), true, 'o cartão do aviso mostra o motivo (nunca silencioso)')
    assert.equal(erro(), null, 'e não é um erro genérico')

    assert.equal(store.send('para outro bot', 'prosa'), 'aceito', 'o aviso é do destino, não de todos')
    await settle()
    assert.equal(enviados(), 2)

    liberar()
    assert.equal(store.send('depois da decisão', 'byte'), 'aceito')
    await settle()
    assert.equal(enviados(), 3, 'fechado o aviso, voltou a enviar')
  } finally { store.retire() }
})

test('reenvio real: com o aviso aberto a entrada falha PERMANECE (com Reenviar); resolvido o aviso, o reenvio funciona com o mesmo nonce', async () => {
  const { store, calls, falhar, enviados, bloquear, liberar, barrado, erro, falhas } = await abrir()
  try {
    falhar.envio = true
    store.send('mensagem que falhou', 'byte')
    await settle()
    const falha = falhas()[0]!
    assert.ok(falha, 'a mensagem ficou como falha')
    const nonce = falha.data.clientNonce
    falhar.envio = false
    const antes = enviados()

    bloquear()
    assert.equal(store.retry(falha), 'bloqueado')
    await settle()
    assert.equal(enviados(), antes, 'nada foi enviado')
    assert.equal(falhas().length, 1, 'a entrada falha continua na conversa')
    assert.equal(falhas()[0]!.data.text, 'mensagem que falhou')
    assert.equal(barrado(), true)
    assert.equal(erro(), null, 'aviso inline, sem diálogo "Algo deu errado"')

    liberar()
    assert.equal(barrado(), false, 'resolvido o aviso, o motivo some junto')
    assert.equal(store.retry(falhas()[0]!), 'aceito')
    await settle()
    assert.equal(enviados(), antes + 1)
    assert.equal(calls.filter((c) => c.method === 'send').at(-1)!.body.clientNonce, nonce, 'mesmo nonce')
    assert.equal(falhas().length, 0)
  } finally { store.retire() }
})

test('Torre: com o aviso aberto a resposta por texto não descarta a falha, não registra nonce e mostra o motivo; resolvido, volta a enviar', async () => {
  const { store, responderOf, falhar, enviados, bloquear, liberar, barrado, falhas } = await abrir()
  try {
    const responder = responderOf(store as never)
    falhar.envio = true
    const nonce = responder.answer('byte', 'minha resposta', undefined)
    assert.ok(nonce)
    await settle()
    assert.equal(falhas().length, 1)
    falhar.envio = false
    const antes = enviados()

    bloquear()
    assert.equal(responder.envioBloqueado?.('byte'), true, 'a Torre sabe que está bloqueada (mostra o motivo e desliga Enviar)')
    assert.equal(responder.answer('byte', 'minha resposta', undefined, nonce!), null, 'sem nonce de tentativa aceita')
    await settle()
    assert.equal(enviados(), antes)
    assert.equal(falhas().length, 1, 'a falha não foi descartada')
    assert.equal(barrado(), true, 'o motivo aparece')
    assert.equal(responder.sendState('byte', nonce!), 'failed', 'continua falha, pode tentar de novo')

    liberar()
    assert.equal(responder.envioBloqueado?.('byte'), false)
    assert.equal(responder.answer('byte', 'minha resposta', undefined, nonce!), nonce)
    await settle()
    assert.equal(enviados(), antes + 1)
  } finally { store.retire() }
})

const bots = [
  { ref: 'mac/byte', name: 'Byte', shape: 'blob', color: 'blue', status: 'idle', working: false, backend: 'Codex', folder: '', detail: '', steps: [], error: null },
  { ref: 'mac/prosa', name: 'Prosa', shape: 'blob', color: 'red', status: 'idle', working: false, backend: 'Codex', folder: '', detail: '', steps: [], error: null },
] as never

/** Uma Ilha com `island.send` controlado pelo teste: cada chamada fica pendente até `responder`. */
async function ilha() {
  const chamadas: { ref: string; text: string; nonce: string; responder: (r: string | Error) => void }[] = []
  const send = (ref: string, text: string, _o: unknown, nonce: string) => new Promise<string>((resolve, reject) => {
    chamadas.push({ ref, text, nonce, responder: (r) => (r instanceof Error ? reject(r) : resolve(r)) })
  })
  const { IslandModel } = await abrir({ island: { send, setShape() {}, setFocusable() {}, setKeys() {} } })
  const modelo = new IslandModel()
  modelo.summary.bots = bots
  let fechou = 0
  modelo.close = () => { fechou++ }
  return { modelo, chamadas, fechou: () => fechou }
}

test('Ilha: barrada, mantém o texto (não fecha) e mostra o motivo; aceita, fecha', async () => {
  const { modelo, chamadas, fechou } = await ilha()
  const p = modelo.send('texto da Ilha', undefined)
  chamadas[0]!.responder('bloqueado')
  assert.equal(await p, false, 'barrado: a visão não limpa o campo')
  assert.equal(fechou(), 0)
  assert.equal(modelo.sendState, 'bloqueado')
  const q = modelo.send('texto da Ilha', undefined)
  chamadas[1]!.responder('aceito')
  assert.equal(await q, true, 'resolvido o aviso, o envio volta a funcionar')
  assert.equal(fechou(), 1)
  assert.equal(modelo.sendState, 'idle')
})

test('Ilha: sem confirmação (prazo, erro) NÃO é aceite: texto mantido, motivo à vista, e tentar de novo reusa o nonce', async () => {
  const { modelo, chamadas, fechou } = await ilha()
  const p = modelo.send('minha mensagem', undefined)
  chamadas[0]!.responder('sem-confirmacao')
  assert.equal(await p, false)
  assert.equal(fechou(), 0, 'a Ilha não fechou')
  assert.equal(modelo.sendState, 'semConfirmacao')
  // Erro do IPC também é "não confirmado".
  const q = modelo.send('minha mensagem', undefined)
  chamadas[1]!.responder(new Error('ipc caiu'))
  assert.equal(await q, false)
  assert.equal(modelo.sendState, 'semConfirmacao')
  assert.equal(chamadas[1]!.nonce, chamadas[0]!.nonce, 'mesmo texto e mesmo bot: o mesmo nonce (o Hub não duplica)')
  // Texto diferente: nonce novo.
  const r = modelo.send('outra mensagem', undefined)
  chamadas[2]!.responder('sem-confirmacao')
  await r
  assert.notEqual(chamadas[2]!.nonce, chamadas[0]!.nonce)
})

test('Ilha: enquanto espera a confirmação fica ocupada — sem segundo envio, sem trocar de bot; depois do aceite libera', async () => {
  const { modelo, chamadas, fechou } = await ilha()
  const p = modelo.send('primeira', undefined)
  assert.equal(modelo.sending, true)
  assert.equal(await modelo.send('primeira', undefined), false, 'segundo Enter/clique durante a espera não envia')
  assert.equal(await modelo.send('segunda', undefined), false)
  const antes = modelo.messageBot?.ref
  modelo.nextMessageBot()
  assert.equal(modelo.messageBot?.ref, antes, 'não troca de bot durante a espera')
  assert.equal(chamadas.length, 1, 'um só pedido saiu')
  chamadas[0]!.responder('aceito')
  assert.equal(await p, true)
  assert.equal(modelo.sending, false)
  assert.equal(fechou(), 1)
  modelo.nextMessageBot()
  assert.notEqual(modelo.messageBot?.ref, antes, 'liberada, troca de bot de novo')
})
