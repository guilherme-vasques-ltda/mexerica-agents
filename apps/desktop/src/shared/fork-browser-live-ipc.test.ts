import assert from 'node:assert/strict'
import test from 'node:test'
import { randomBytes } from 'node:crypto'
import { BrowserLiveSession, STALE_AFTER_MS, type BrowserLiveBridge } from './fork-browser-live.ts'
import type { ForkCloudCallResult } from './fork-cloud.ts'
import type { BrowserFrame } from './relay-protocol/browser-frames.ts'
import { loadMainModule, settle } from './test-support/main-module.ts'

/**
 * Ida e volta entre a tela e o processo principal: o código REAL de main/fork-cloud/service.ts e o REAL preload/index.ts
 * (carregados pelo harness loadMainModule), ligados por um Electron SIMULADO (ipcMain/ipcRenderer em memória) e por uma
 * conexão SIMULADA com o Hub. Não há processo Electron de verdade aqui. Os relógios do processo principal e da tela são
 * dois, com origens diferentes (como em dois processos Electron), e o atraso do IPC é tempo que passa nos dois sem que
 * a tela o veja.
 */

const scope = { hubId: 'hub1', spaceId: 'sp1', botId: 'bot1' }
const url = 'file:///fixture/index.html?window=chat'
const connectionId = 'A'.repeat(22)
const ATRASO = STALE_AFTER_MS + 500

const quadro = (sequence: number): BrowserFrame => ({
  session: 's1', target: 'aba1', sequence, targetGeneration: 0,
  image: { width: 960, height: 540 }, viewport: { width: 1280, height: 720 },
  jpeg: Uint8Array.from([0xff, 0xd8, sequence & 0xff, 0xff, 0xd9]),
})

type Listener = (...args: any[]) => void
type Recebido = { method: string; body: Record<string, any> }

function montar() {
  const main = { t: 50_000 }, tela = { t: 7_000_000 }
  const passa = (ms: number) => { main.t += ms; tela.t += ms }

  const handles = new Map<string, (...args: any[]) => any>(), ouvintesMain = new Map<string, Listener>()
  const ouvintesTela = new Map<string, Set<Listener>>()
  /** Milissegundos que o IPC leva: os quadros (processo principal → tela) e cada envio de clique (tela → processo principal), pela ordem do envio desde o começo do teste. */
  const atraso = { quadro: 0, clique: [] as number[] }
  const navegacao: Listener[] = []
  const frame = { url }
  const sender = {
    id: 1, mainFrame: frame, isDestroyed: () => false, once() {}, removeListener() {},
    on: (canal: string, cb: Listener) => { if (canal === 'did-start-navigation') navegacao.push(cb) },
    send: (canal: string, ...args: unknown[]) => {
      if (canal === 'forkCloud:frames:data') passa(atraso.quadro)
      for (const cb of ouvintesTela.get(canal) ?? []) cb({}, ...args)
    },
  }
  const evento = { sender, senderFrame: frame }
  const outroFrame = { url }
  const outroSender = { ...sender, id: 2, mainFrame: outroFrame }
  const outroEvento = { sender: outroSender, senderFrame: outroFrame }
  const ipcMain = {
    handle: (nome: string, cb: (...args: any[]) => any) => handles.set(nome, cb),
    on: (nome: string, cb: Listener) => ouvintesMain.set(nome, cb),
    removeHandler() {}, removeAllListeners() {},
  }

  const recebidoPeloHub: Recebido[] = []
  const hub: Record<string, (body: Record<string, any>) => ForkCloudCallResult> = {
    browserStreamSubscribe: (b) => ({ kind: 'ok', value: { subscribed: true, frames: true, session: b.session } }),
    browserStreamControl: () => ({ kind: 'ok', value: { epoch: 1 } }),
    browserStreamInput: (b) => ({ kind: 'ok', value: { accepted: true, sequence: b.sequence } }),
    browserStreamAck: () => ({ kind: 'ok', value: { acknowledged: true } }),
  }
  let sink: { onFrame: (f: BrowserFrame) => void; onEnd: (e: unknown) => void } | null = null
  const conexao = {
    framesNegotiated: true as boolean,
    getAttachment: () => ({ connectionId }),
    subscribeFrames: (s: NonNullable<typeof sink>) => { if (sink) return null; sink = s; return { id: 1, cancel() { sink = null } } },
    call: async (method: string, body: Record<string, any>) => { recebidoPeloHub.push({ method, body }); return hub[method]!(body) },
  }

  const { registerForkCloudService } = loadMainModule<typeof import('../main/fork-cloud/service.ts')>('fork-cloud/service', {
    './personal-runtime': {},
    'node:crypto': { randomBytes },
    electron: { app: { once() {}, on() {} }, ipcMain, BrowserWindow: { getAllWindows: () => [] }, powerMonitor: { on() {} } },
    './principal': {}, './account-api': {}, './device-keys': {}, './crypto': { unb64url: () => Buffer.alloc(16) }, './enroll': {}, './connection': {}, './pins': {},
  }, { Buffer, Object, URL, process, performance: { now: () => main.t } })
  const servico = registerForkCloudService({ sessionEpoch: 1, on() {}, off() {} } as never, {} as never, {} as never,
    () => [{ isDestroyed: () => false, webContents: sender }, { isDestroyed: () => false, webContents: outroSender }] as never, [url], () => false)
  ;(servico as unknown as { connection: unknown }).connection = conexao

  let cliques = 0
  const ipcRenderer = {
    invoke: async (nome: string, ...args: unknown[]) => {
      if (nome === 'forkCloud:call' && args[1] === 'browserStreamInput') passa(atraso.clique[cliques++] ?? 0)
      return handles.get(nome)!(evento, ...args)
    },
    send: (nome: string, ...args: unknown[]) => ouvintesMain.get(nome)!(evento, ...args),
    on: (nome: string, cb: Listener) => { (ouvintesTela.get(nome) ?? ouvintesTela.set(nome, new Set()).get(nome)!).add(cb) },
    removeListener: (nome: string, cb: Listener) => { ouvintesTela.get(nome)?.delete(cb) },
    sendSync: (nome: string) => (nome === 'app:hostPort' ? 0 : nome === 'app:openedAtLogin' || nome === 'speech:available' || nome === 'island:supported' ? false : nome === 'app:debugOpen' ? null : nome === 'speech:locales' ? [] : ''),
  }
  let bridge: any
  loadMainModule('../preload/index', {
    electron: { contextBridge: { exposeInMainWorld: (_: string, b: unknown) => { bridge = b } }, ipcRenderer },
    './fork-voz': { vozBridge: {} }, './fork-account-profile': { accountProfileBridge: {} }, './ssh': { sshBridge: {} },
  }, { crypto: globalThis.crypto, process: { platform: 'darwin', versions: {}, env: {} } })
  const ponte: BrowserLiveBridge = {
    call: (m, b, t) => bridge.forkCloud.call(connectionId, m, b, t),
    subscribeFrames: (f, e) => bridge.forkCloud.subscribeFrames(connectionId, f, e),
  }

  const sessao = new BrowserLiveSession(ponte, scope, 's1', 'aba1', () => tela.t)
  const vistos: BrowserFrame[] = []
  sessao.subscribe({ onState: () => {}, onFrame: (f) => vistos.push(f) })
  return {
    passa, atraso, sessao, ponte, hub, conexao, outraJanela: (m: string, b: unknown) => handles.get('forkCloud:call')!(outroEvento, connectionId, m, b, 5000) as Promise<ForkCloudCallResult>, hubManda: (f: BrowserFrame) => sink!.onFrame(f), assinado: () => sink !== null, navegar: (detalhes: Record<string, unknown>) => navegacao.forEach((cb) => cb(detalhes)),
    entradasNoHub: () => recebidoPeloHub.filter((c) => c.method === 'browserStreamInput'),
    /** O Hub manda o quadro, que só chega à tela se a fila do processo principal o despachar. */
    chega: (f: BrowserFrame) => sink!.onFrame(f),
    /** A tela termina de pintar o último quadro que recebeu e libera o próximo da fila. */
    pintaUltimo: () => sessao.frameShown(vistos.at(-1)!, true),
    /** O Hub manda o quadro n, ele atravessa o IPC e a tela o pinta. */
    quadro: async (n: number) => {
      sink!.onFrame(quadro(n))
      await settle()
      const visto = vistos.at(-1)!
      assert.equal(visto.sequence, n)
      sessao.frameShown(visto, true)
    },
  }
}

type Rig = ReturnType<typeof montar>

async function comPessoa() {
  const r = montar()
  await r.sessao.start()
  await settle()
  await r.quadro(1)
  assert.equal(await r.sessao.setController('person'), true)
  return r
}

test('controle positivo: quadro em dia, ida e volta pelo IPC, o clique chega ao Hub', async () => {
  const r = await comPessoa()
  assert.deepEqual(await r.sessao.click(5, 5, 100, 100), { ok: true })
  assert.equal(r.entradasNoHub().length, 1)
  assert.equal(r.entradasNoHub()[0]!.body.frameSequence, 1)
})

test('o quadro demora no IPC até a tela: a tela o acha novo, o processo principal o vê vencido e nada sai', async () => {
  const r = await comPessoa()
  r.atraso.quadro = ATRASO
  await r.quadro(2)
  assert.equal(r.sessao.isStale(), false, 'a tela, sozinha, acharia o quadro em dia: ela não vê o trânsito')
  assert.deepEqual(await r.sessao.click(5, 5, 100, 100), { ok: false, reason: 'refused' })
  assert.equal(r.entradasNoHub().length, 0)
  r.atraso.quadro = 0
  await r.quadro(3)
  assert.deepEqual(await r.sessao.click(5, 5, 100, 100), { ok: true }, 'controle: quadro novo, sem atraso, sai')
  assert.equal(r.entradasNoHub().length, 1)
})

test('o clique demora no IPC até o processo principal: o quadro vence no caminho e nada sai', async () => {
  const r = await comPessoa()
  r.atraso.clique = [ATRASO]
  assert.equal(r.sessao.isStale(), false)
  assert.deepEqual(await r.sessao.click(5, 5, 100, 100), { ok: false, reason: 'refused' })
  assert.equal(r.entradasNoHub().length, 0)
  await r.quadro(2)
  assert.deepEqual(await r.sessao.click(5, 5, 100, 100), { ok: true }, 'controle: o clique seguinte, sem atraso, sai')
  assert.equal(r.entradasNoHub().length, 1)
})

test('a nova tentativa após resultado incerto também passa pelo prazo do processo principal', async () => {
  const r = await comPessoa()
  r.hub.browserStreamInput = (b) => (r.entradasNoHub().length === 1 ? { kind: 'unknown', reason: 'timeout' } : { kind: 'ok', value: { accepted: b.sequence === 2, sequence: b.sequence } })
  assert.deepEqual(await r.sessao.click(5, 5, 100, 100), { ok: false, reason: 'unknown' })
  assert.equal(r.entradasNoHub().length, 1)

  r.atraso.clique = [0, 0, ATRASO]
  await r.quadro(2)
  assert.deepEqual(await r.sessao.click(5, 5, 100, 100), { ok: false, reason: 'refused' })
  assert.deepEqual(r.entradasNoHub().map((c) => c.body.sequence), [1, 1], 'o primeiro envio saiu; o reenvio vencido, não')

  await r.quadro(3)
  assert.deepEqual(await r.sessao.click(5, 5, 100, 100), { ok: true }, 'controle: sem atraso o reenvio sai com a sequência seguinte')
  assert.deepEqual(r.entradasNoHub().map((c) => c.body.sequence), [1, 1, 1, 2])
})

test('o prazo é decidido no processo principal: identidade do quadro e idade, no limite exato', async () => {
  const r = await comPessoa()
  const base = { session: 's1', target: 'aba1', targetGeneration: 0, frameSequence: 1, sequence: 1, epoch: 1, controller: 'person', x: 1, y: 1, displayWidth: 10, displayHeight: 10 }
  const manda = (over: Record<string, unknown>) => r.ponte.call('browserStreamInput', { ...base, ...over }, 5000)
  assert.equal((await manda({})).kind, 'ok', 'quadro entregue e em dia')
  assert.equal((await manda({ frameSequence: 99 })).kind, 'notSent', 'quadro que o processo principal nunca entregou')
  assert.equal((await manda({ session: 'outra' })).kind, 'notSent', 'outra sessão')
  assert.equal((await manda({ target: 'outra-aba' })).kind, 'notSent', 'outra aba')
  assert.equal((await manda({ targetGeneration: 7 })).kind, 'notSent', 'outra geração')
  assert.equal((await manda({ frameSequence: '1' })).kind, 'notSent', 'tipo errado')
  assert.equal(r.entradasNoHub().length, 1)
  r.passa(STALE_AFTER_MS)
  assert.equal((await manda({})).kind, 'ok', 'no limite de 6 s ainda vale')
  r.passa(1)
  assert.equal((await manda({})).kind, 'notSent', '1 ms depois está vencido')
  assert.equal(r.entradasNoHub().length, 2)
})

const entrada = { session: 's1', target: 'aba1', targetGeneration: 0, frameSequence: 1, sequence: 1, epoch: 1, controller: 'person', x: 1, y: 1, displayWidth: 10, displayHeight: 10 }
const cita = (r: Rig, over: Record<string, unknown>) => r.ponte.call('browserStreamInput', { ...entrada, ...over }, 5000)

test('quadro recebido mas retido na fila do processo principal não autoriza entrada; só o despachado à tela', async () => {
  const r = await comPessoa()
  r.chega(quadro(2))
  await settle()
  r.chega(quadro(3))
  await settle()
  assert.equal((await cita(r, { frameSequence: 3 })).kind, 'notSent', 'o 3 chegou ao main mas a fila o retém: ainda não saiu pelo IPC')
  assert.equal((await cita(r, { frameSequence: 2 })).kind, 'ok', 'controle: o 2 foi despachado')
  r.pintaUltimo()
  await settle()
  assert.equal((await cita(r, { frameSequence: 3 })).kind, 'ok', 'despachado depois, o 3 passa a valer')
})

test('o prazo de um quadro que esperou na fila conta desde a chegada, não desde o despacho', async () => {
  const r = await comPessoa()
  r.chega(quadro(2))
  await settle()
  r.chega(quadro(3))
  await settle()
  r.passa(STALE_AFTER_MS - 100)
  r.pintaUltimo()
  await settle()
  assert.equal((await cita(r, { frameSequence: 3 })).kind, 'ok', 'esperou 5,9 s na fila: ainda em dia')
  r.passa(101)
  assert.equal((await cita(r, { frameSequence: 3 })).kind, 'notSent', 'o despacho não reiniciou o relógio: 6,001 s desde a chegada')
})

test('quadro substituído na fila nunca foi despachado e não autoriza entrada, mesmo depois de o seguinte sair', async () => {
  const r = await comPessoa()
  r.chega(quadro(2))
  await settle()
  r.chega(quadro(3))
  r.chega(quadro(4))
  r.pintaUltimo()
  await settle()
  assert.equal((await cita(r, { frameSequence: 3 })).kind, 'notSent', 'o 3 foi descartado pela fila')
  assert.equal((await cita(r, { frameSequence: 4 })).kind, 'ok', 'controle: o 4 foi despachado')
})

test('a identidade do quadro é estrutural: sessão e alvo com quebra de linha não se confundem', async () => {
  const r = await comPessoa()
  r.chega({ ...quadro(5), session: 'a\nb', target: 'c' })
  await settle()
  assert.equal((await cita(r, { session: 'a', target: 'b\nc', frameSequence: 5 })).kind, 'notSent', 'outro par de identificadores')
  assert.equal((await cita(r, { session: 'a\nb', target: 'c', frameSequence: 5 })).kind, 'ok', 'controle: o par real')
})

test('consumidor de quadros já em uso é 409; capacidade não negociada é 501; conexão encerrada é 0', async () => {
  const r = await comPessoa()
  const fins: Array<{ status: number } | null> = []
  const segundo = r.ponte.subscribeFrames(() => {}, (e) => fins.push(e))
  assert.equal(await segundo.opened, false)
  assert.equal(fins.at(-1)?.status, 409, 'slot ocupado com a capacidade negociada: não pede para atualizar o app')

  r.conexao.framesNegotiated = false
  const sem = r.ponte.subscribeFrames(() => {}, (e) => fins.push(e))
  assert.equal(await sem.opened, false)
  assert.equal(fins.at(-1)?.status, 501, 'sem capacidade negociada')
})

test('âncora e subquadro não encerram os quadros; a troca de documento da janela principal encerra', async () => {
  const r = await comPessoa()
  r.navegar({ isMainFrame: true, isSameDocument: true })
  r.navegar({ isMainFrame: false, isSameDocument: false })
  await r.quadro(2)
  assert.equal(r.sessao.frame?.sequence, 2, 'seguem chegando')
  assert.equal(r.assinado(), true)
  r.navegar({ isMainFrame: true, isSameDocument: false })
  assert.equal(r.assinado(), false, 'a troca de documento cancela a assinatura de quadros')
})

test('a prova do quadro é da assinatura da janela: outra janela autorizada não usa o despacho feito à primeira', async () => {
  const r = await comPessoa()
  const entrada = { session: 's1', target: 'aba1', targetGeneration: 0, frameSequence: 1, sequence: 1, epoch: 1, controller: 'person', x: 1, y: 1, displayWidth: 10, displayHeight: 10 }
  assert.equal((await r.outraJanela('browserStreamInput', entrada)).kind, 'notSent', 'a outra janela não recebeu quadro algum')
  assert.equal(r.entradasNoHub().length, 0)
  assert.equal((await r.ponte.call('browserStreamInput', entrada, 5000)).kind, 'ok', 'controle: a janela que recebeu o quadro envia')
  assert.equal(r.entradasNoHub().length, 1)
})

test('trocar a inscrição no Hub invalida a prova: o mesmo par de identificadores não vale para o quadro novo', async () => {
  const r = await comPessoa()
  const entrada = { session: 's1', target: 'aba1', targetGeneration: 0, frameSequence: 1, sequence: 1, epoch: 1, controller: 'person', x: 1, y: 1, displayWidth: 10, displayHeight: 10 }
  assert.equal((await r.ponte.call('browserStreamInput', entrada, 5000)).kind, 'ok')
  assert.equal((await r.ponte.call('browserStreamSubscribe', { session: 's1', target: 'aba1' }, 5000)).kind, 'ok')
  assert.equal((await r.ponte.call('browserStreamInput', entrada, 5000)).kind, 'notSent', 'o Hub reiniciou geração e sequência: o quadro 1 antigo não vale mais')
  await r.quadro(2)
  assert.equal((await r.ponte.call('browserStreamInput', { ...entrada, frameSequence: 2 }, 5000)).kind, 'ok', 'controle: quadro entregue depois da troca vale')
})

test('o corpo da entrada que sai ao Hub é fechado: só os campos do contrato, com tipo e faixa conferidos', async () => {
  const r = await comPessoa()
  const entrada = { session: 's1', target: 'aba1', targetGeneration: 0, frameSequence: 1, sequence: 1, epoch: 1, controller: 'person', x: 1, y: 1, displayWidth: 10, displayHeight: 10 }
  const grande = { ...entrada, extra: 'x'.repeat(900_000), outro: { a: 1 } }
  assert.equal((await r.ponte.call('browserStreamInput', grande, 5000)).kind, 'ok')
  assert.deepEqual(Object.keys(r.entradasNoHub()[0]!.body).sort(), Object.keys(entrada).sort(), 'o que o Hub ignoraria não sai')
  assert.deepEqual(JSON.parse(JSON.stringify(r.entradasNoHub()[0]!.body)), entrada)
  for (const ruim of [{ controller: 'agent' }, { x: 'um' }, { displayWidth: 0 }, { sequence: -1 }, { epoch: 1.5 }, { session: '' }, { target: 'a'.repeat(257) }, { frameSequence: undefined }]) {
    assert.equal((await r.ponte.call('browserStreamInput', { ...entrada, ...ruim }, 5000)).kind, 'notSent', JSON.stringify(Object.keys(ruim)))
  }
  assert.equal((await r.ponte.call('browserStreamInput', [entrada], 5000)).kind, 'notSent')
  assert.equal(r.entradasNoHub().length, 1)
})
