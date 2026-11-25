import assert from 'node:assert/strict'
import test from 'node:test'
import * as path from 'node:path'
import { Readable } from 'node:stream'
import { loadMainModule, settle } from '../../shared/test-support/main-module.ts'
import * as contexts from '../fork-secret-box/contexts.ts'

// MX-209: a janela do cartão seguro. Valor de teste fictício; os testes provam que ele só chega ao Hub,
// e que o renderer só passa a referência do pedido — os textos da janela vêm do Hub.
const SENHA = 'senha-ficticia-do-teste-9f3'
const same = (actual: unknown, expected: unknown) => assert.equal(JSON.stringify(actual), JSON.stringify(expected))

type Handler = (event: { sender: unknown; senderFrame: unknown }, ...args: any[]) => any
type Listener = (...args: any[]) => void

const ENTRADA = {
  entryId: 'e-1', botId: 'bot-1', botName: 'Berga', kind: 'login', status: 'pending',
  title: 'github.com', site: 'github.com', field: null, location: null,
  spaceId: 'sp-1', conversationId: 'conv-1',
}

function ambiente(opcoes: {
  http?: (url: string) => Promise<Response>
  local?: boolean
  load?: () => Promise<void>
  /** Respostas do GET por chamada (na ordem); o padrão repete ENTRADA. */
  gets?: (unknown | { falha: number })[]
  /** Captura SSH viva/morta para pc-ssh. */
  sshViva?: () => { current: boolean; listener: () => Promise<boolean> }
} = {}) {
  const windows: any[] = []
  const passos: string[] = []
  const handlers = new Map<string, Handler>()
  const chamadas: { url: string; token: string; body: string }[] = []
  class BrowserWindow {
    options: any
    destroyed = false
    shown = false
    focused = 0
    url = ''
    listeners = new Map<string, Listener[]>()
    webContents: any
    constructor(options: any) {
      this.options = options
      const listeners = new Map<string, Listener[]>()
      this.webContents = {
        mainFrame: { frame: windows.length }, listeners,
        isDestroyed: () => this.destroyed,
        getURL: () => this.url,
        send: () => {},
        setWindowOpenHandler: (handler: () => unknown) => passos.push(`openHandler:${JSON.stringify(handler())}`),
        on: (name: string, listener: Listener) => listeners.set(name, [...(listeners.get(name) ?? []), listener]),
        emit: (name: string, ...args: unknown[]) => (listeners.get(name) ?? []).forEach((l) => l(...args)),
      }
      windows.push(this)
      passos.push('new')
    }
    on(name: string, listener: Listener) { this.listeners.set(name, [...(this.listeners.get(name) ?? []), listener]) }
    isDestroyed() { return this.destroyed }
    isMinimized() { return false }
    restore() {}
    focus() { this.focused++ }
    destroy() { this.destroyed = true; this.listeners.get('closed')?.forEach((l) => l()); passos.push('destroy') }
    setContentProtection(ativo: boolean) { passos.push(`protecao:${ativo}`) }
    async loadURL(url: string) {
      passos.push(`carrega:${contexts.isGeneralContext(this.webContents) ? 'geral' : 'privada'}`)
      await (opcoes.load ?? (async () => {}))()
      this.url = url
    }
    show() { this.shown = true }
  }
  const avisos: unknown[][] = []
  const geral = { isDestroyed: () => false, webContents: { mainFrame: { frame: 'chat' }, isDestroyed: () => false, send: (...a: unknown[]) => avisos.push(a) } }
  const gets = [...(opcoes.gets ?? [])]
  const http = opcoes.http ?? (async (u: string) => {
    if (u.endsWith('/api/connectorRequestGet')) {
      const proxima = gets.length > 1 ? gets.shift() : gets[0]
      if (proxima && typeof proxima === 'object' && 'falha' in proxima) {
        return new Response(JSON.stringify({ error: 'negado' }), { status: (proxima as { falha: number }).falha })
      }
      return new Response(JSON.stringify(proxima ?? ENTRADA), { status: 200 })
    }
    return new Response('{}', { status: 200 })
  })
  const modulo = loadMainModule<typeof import('./janela.ts')>('fork-cofre-seguro/janela', {
    electron: { BrowserWindow, ipcMain: { handle: (c: string, h: Handler) => handlers.set(c, h), on: (c: string, h: Handler) => handlers.set(c, h) } },
    'node:crypto': { randomUUID: () => `nonce-${windows.length}` },
    'node:path': path,
    '../fork-secret-box/contexts': contexts,
    '../../renderer/lib/fork-i18n/index.ts': { traduzir: () => 'Cartão seguro' },
    '../fork-proved-loopback': {
      postProvado: async (baseURL: string, rota: string, token: string, body: string, _timeout: number, options: { atual: () => boolean }) => {
        if (!options.atual()) throw new Error('captura indisponível')
        const url = baseURL + rota
        chamadas.push({ url, token, body })
        const response = await http(url)
        return Object.assign(Readable.from([await response.text()]), { statusCode: response.status })
      },
    },
    '../fork-idioma': { idiomaDoApp: () => 'pt-BR' },
  }, { __dirname: '/app/out/main', URL, AbortSignal, JSON, setImmediate })
  const local = opcoes.local === false ? null : { computerId: 'pc-1', baseURL: 'http://127.0.0.1:4567', token: 'token-do-hub' }
  const sshViva = opcoes.sshViva ?? (() => ({ current: true, listener: async () => true }))
  modulo.registrarCofreSeguro({
    pai: () => ({ isDestroyed: () => false } as any), gerais: () => [geral as any],
    hubs: {
      local: () => local,
      ssh: (computerId: string, current: () => boolean) => computerId === 'pc-ssh'
        ? { baseURL: 'http://127.0.0.1:5555', token: 't-ssh', current: () => current() && sshViva().current, listener: sshViva().listener }
        : null,
    },
    url: 'file:///app/out/renderer/index.html?window=cofre-seguro',
  })
  const chamar = (canal: string, sender: any, ...args: unknown[]) => handlers.get(canal)!({ sender, senderFrame: sender?.mainFrame }, ...args)
  /** Chamada vinda de um quadro que não é o principal do remetente. */
  const chamarQuadro = (canal: string, sender: any, ...args: unknown[]) => handlers.get(canal)!({ sender, senderFrame: { frame: 'filho' } }, ...args)
  const abrir = async (extra: object = {}) => {
    const r = await chamar('fork-cofre-seguro:abrir', geral.webContents, { computerId: 'pc-1', entryId: 'e-1', ...extra })
    await settle()
    return r
  }
  const abrirNovo = async (pedido: unknown = { computerId: 'pc-1' }, sender: any = geral.webContents) => {
    const r = await chamar('fork-cofre-seguro:abrir-novo', sender, pedido)
    await settle()
    return r
  }
  const salvaLogin = () => chamadas.filter((c) => c.url.endsWith('/api/credentialSaveLogin'))
  const finishes = () => chamadas.filter((c) => c.url.endsWith('/api/connectorRequestFinish'))
  const getsChamadas = () => chamadas.filter((c) => c.url.endsWith('/api/connectorRequestGet'))
  return { windows, passos, avisos, chamadas, finishes, salvaLogin, getsChamadas, geral, chamar, chamarQuadro, abrir, abrirNovo }
}

test('abre: o main busca a entrada no Hub e deriva dela todos os textos da janela', async () => {
  const a = ambiente()
  const r = await a.abrir()
  same(r, { ok: true, foco: false })
  assert.deepEqual(a.passos, ['new', 'protecao:true', 'openHandler:{"action":"deny"}', 'carrega:privada'])
  const { options } = a.windows[0]
  const prefs = options.webPreferences
  assert.equal(prefs.sandbox, true)
  assert.equal(prefs.contextIsolation, true)
  assert.equal(prefs.nodeIntegration, false)
  assert.equal(prefs.devTools, false)
  assert.equal(prefs.preload, '/app/out/preload/fork-cofre-seguro.js')
  assert.equal(JSON.stringify(prefs.additionalArguments), '["--mx-cofre-nonce=nonce-0"]')
  assert.equal(options.modal, true)
  assert.equal(a.windows[0].shown, true)
  assert.equal(contexts.isGeneralContext(a.windows[0].webContents), false)
  same(a.avisos, [['fork-cofre-seguro:estado', { entryId: 'e-1', estado: 'aberta' }]])
  // O GET pede só a referência; a resposta vem do Hub, não do renderer.
  assert.equal(a.getsChamadas().length, 1)
  assert.deepEqual(JSON.parse(a.getsChamadas()[0].body), { entryId: 'e-1' })
  const dados = await a.chamar('fork-cofre-seguro:dados', a.windows[0].webContents, 'nonce-0')
  same(dados.pedido, { tipo: 'login', titulo: 'github.com', botNome: 'Berga', site: 'github.com', campo: null, local: null })
})

test('abrir de novo o mesmo pedido foca a janela existente; não cria outra nem repete o GET', async () => {
  const a = ambiente()
  await a.abrir()
  same(await a.abrir(), { ok: true, foco: true })
  assert.equal(a.windows.length, 1)
  assert.equal(a.windows[0].focused, 1)
  assert.equal(a.getsChamadas().length, 1)
})

test('abrir: só o chat geral, no quadro principal; o resto é recusado sem criar janela', async () => {
  const a = ambiente()
  await a.abrir()
  const privada = a.windows[0].webContents
  const outro = { computerId: 'pc-1', entryId: 'e-2' }
  // A própria janela segura não é contexto geral: não pode abrir outras.
  same(await a.chamar('fork-cofre-seguro:abrir', privada, outro), { ok: false, motivo: 'invalido' })
  same(await a.chamarQuadro('fork-cofre-seguro:abrir', a.geral.webContents, outro), { ok: false, motivo: 'invalido' })
  same(await a.chamar('fork-cofre-seguro:abrir', a.geral.webContents, 'lixo'), { ok: false, motivo: 'invalido' })
  assert.equal(a.windows.length, 1)
})

test('aberturas concorrentes do mesmo pedido coalescem; reabrir durante loadURL não duplica nem destrói', async () => {
  let liberarGet!: () => void
  const portaGet = new Promise<void>((r) => { liberarGet = r })
  let liberarLoad!: () => void
  const a = ambiente({
    load: () => new Promise<void>((r) => { liberarLoad = r }),
    http: async (url: string) => {
      if (String(url).endsWith('/api/connectorRequestGet')) await portaGet
      return new Response(JSON.stringify(ENTRADA))
    },
  })
  const ref = { computerId: 'pc-1', entryId: 'e-1' }
  // Duas chamadas com o GET atrasado: a reserva por chave faz a segunda esperar a mesma abertura.
  const p1 = a.chamar('fork-cofre-seguro:abrir', a.geral.webContents, ref)
  const p2 = a.chamar('fork-cofre-seguro:abrir', a.geral.webContents, ref)
  liberarGet()
  same(await p1, { ok: true, foco: false })
  same(await p2, { ok: true, foco: false })
  assert.equal(a.windows.length, 1, 'uma BrowserWindow só')
  // Janela criada mas ainda carregando: reabrir não destrói a instância nem cria outra.
  same(await a.chamar('fork-cofre-seguro:abrir', a.geral.webContents, ref), { ok: true, foco: false })
  assert.equal(a.windows.length, 1)
  assert.equal(a.windows[0].destroyed, false)
  liberarLoad()
  await settle()
  // Carregada: reabrir foca a mesma instância (nonce único, sem órfã).
  same(await a.abrir(), { ok: true, foco: true })
  assert.equal(a.windows.length, 1)
  assert.equal(a.windows[0].focused, 1)
  assert.equal(JSON.stringify(a.windows[0].options.webPreferences.additionalArguments), '["--mx-cofre-nonce=nonce-0"]')
})

test('entryId inexistente, concluído, cancelado ou de tipo errado: o main recusa e não abre janela', async () => {
  for (const [gets, motivo] of [
    [[{ falha: 404 }], 'invalido'],
    [[{ ...ENTRADA, status: 'ready' }], 'invalido'],
    [[{ ...ENTRADA, status: 'cancelled' }], 'invalido'],
    [[{ ...ENTRADA, kind: 'agent' }], 'invalido'],
    [[{ ...ENTRADA, kind: 'link' }], 'invalido'],
    [[{ ...ENTRADA, entryId: 'e-outro' }], 'invalido'],
    [[{ ...ENTRADA, title: '  ' }], 'invalido'],
  ] as const) {
    const a = ambiente({ gets: [...gets] })
    same(await a.abrir(), { ok: false, motivo })
    assert.equal(a.windows.length, 0)
  }
})

test('contexto errado (Hub nega o espaço/conversa do pedido): recusa fechada, sem janela', async () => {
  const a = ambiente({ gets: [{ falha: 403 }] })
  same(await a.abrir({ spaceId: 'sp-errada', conversationId: 'conv-errada' }), { ok: false, motivo: 'invalido' })
  assert.equal(a.windows.length, 0)
  // O corpo do GET leva a referência de contexto; o Hub decide se ela bate com a entrada.
  const corpo = JSON.parse(a.getsChamadas()[0].body)
  assert.deepEqual(corpo, { entryId: 'e-1', spaceId: 'sp-errada', conversationId: 'conv-errada' })
})

test('computador sem caminho seguro (nuvem/relay, desconhecido): falha fechada, sem janela', async () => {
  const a = ambiente()
  same(await a.abrir({ computerId: 'nuvem-1' }), { ok: false, motivo: 'semSuporte' })
  assert.equal(a.windows.length, 0)
  const b = ambiente({ local: false })
  same(await b.abrir(), { ok: false, motivo: 'semSuporte' })
  // O computador por túnel SSH em loopback é conhecido do main.
  same(await a.abrir({ computerId: 'pc-ssh' }), { ok: true, foco: false })
})

test('túnel SSH encerrado com anexo ainda presente, ou porta reocupada: nenhuma chamada sai', async () => {
  // A captura do cofre tem as guardas do ssh.ts: current() e listener() conferidos antes de cada POST.
  for (const estado of [{ current: false, listener: async () => true }, { current: true, listener: async () => false }]) {
    const a = ambiente({ sshViva: () => estado })
    same(await a.abrir({ computerId: 'pc-ssh' }), { ok: false, motivo: 'semConexao' })
    assert.equal(a.windows.length, 0)
    assert.equal(a.chamadas.length, 0)
  }
})

test('troca de captura com a janela aberta: salvar reconfere e falha fechado sem enviar o valor', async () => {
  let capturaViva = true
  const a = ambiente({ sshViva: () => ({ current: capturaViva, listener: async () => true }) })
  same(await a.abrir({ computerId: 'pc-ssh' }), { ok: true, foco: false })
  const w = a.windows[0]
  capturaViva = false // o túnel caiu ou foi trocado depois da abertura
  const r = await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { valor: SENHA, usuario: '', rotulo: '' })
  same(r, { ok: false, erro: 'conexao' })
  assert.equal(a.finishes().length, 0)
  assert.equal(JSON.stringify(r).includes(SENHA), false)
})

test('o pedido vindo do chat nunca carrega valor nem texto: campos extras são descartados', async () => {
  const a = ambiente()
  await a.abrir({ valor: SENHA, value: SENHA, password: SENHA,
    pedido: { tipo: 'login', titulo: 'site-falso.com', botNome: 'Invasor', valor: SENHA } })
  const dados = await a.chamar('fork-cofre-seguro:dados', a.windows[0].webContents, 'nonce-0')
  assert.equal(JSON.stringify(dados).includes(SENHA), false)
  assert.equal(JSON.stringify(dados).includes('site-falso'), false)
  assert.equal(JSON.stringify(dados).includes('Invasor'), false)
  assert.equal(dados.pedido.titulo, 'github.com')
  assert.equal(JSON.stringify(a.avisos).includes(SENHA), false)
})

test('dados: remetente errado, nonce errado, quadro errado ou janela morta recebem null', async () => {
  const a = ambiente()
  await a.abrir()
  const w = a.windows[0]
  assert.equal((await a.chamar('fork-cofre-seguro:dados', w.webContents, 'nonce-0')).pedido.botNome, 'Berga')
  assert.equal(await a.chamar('fork-cofre-seguro:dados', w.webContents, 'nonce-errado'), null)
  assert.equal(await a.chamar('fork-cofre-seguro:dados', w.webContents, undefined), null)
  assert.equal(await a.chamar('fork-cofre-seguro:dados', a.geral.webContents, 'nonce-0'), null)
  assert.equal(await a.chamarQuadro('fork-cofre-seguro:dados', w.webContents, 'nonce-0'), null)
  const clone = { ...w.webContents }
  assert.equal(await a.chamar('fork-cofre-seguro:dados', clone, 'nonce-0'), null)
  w.destroy()
  assert.equal(await a.chamar('fork-cofre-seguro:dados', w.webContents, 'nonce-0'), null)
})

test('salvar: reconfere a entrada e a captura, e o valor vai só ao Hub; nada volta com ele', async () => {
  const a = ambiente()
  await a.abrir({ spaceId: 'sp-1', hubId: 'hub-1' })
  const w = a.windows[0]
  const r = await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { valor: SENHA, usuario: 'ana@exemplo.com', rotulo: 'Conta da Ana' })
  same(r, { ok: true })
  assert.equal(JSON.stringify(r).includes(SENHA), false)
  // Um GET de abertura + um GET de reconferência + um POST finish.
  assert.equal(a.getsChamadas().length, 2)
  assert.equal(a.finishes().length, 1)
  const f = a.finishes()[0]
  assert.equal(f.url, 'http://127.0.0.1:4567/api/connectorRequestFinish')
  assert.equal(f.token, 'token-do-hub')
  const corpo = JSON.parse(f.body)
  assert.equal(corpo.value, SENHA)
  assert.equal(corpo.entryId, 'e-1')
  assert.equal(corpo.username, 'ana@exemplo.com')
  assert.equal(corpo.spaceId, 'sp-1')
  assert.equal(corpo.conversationId, 'conv-1')
  assert.equal(corpo.hubId, 'hub-1')
  await settle()
  assert.equal(w.destroyed, true)
  same(a.avisos.at(-1), ['fork-cofre-seguro:estado', { entryId: 'e-1', estado: 'salva' }])
  assert.equal(JSON.stringify(a.avisos).includes(SENHA), false)
})

test('salvar: se a entrada mudou ou já concluiu entre abrir e salvar, nada é enviado', async () => {
  for (const [depois, esperado] of [
    [{ ...ENTRADA, title: 'outro-site.com' }, 'recusado'],
    [{ ...ENTRADA, status: 'ready' }, 'recusado'],
    [{ ...ENTRADA, botName: 'OutroBot' }, 'recusado'],
    [{ falha: 500 }, 'conexao'],
  ] as const) {
    const a = ambiente({ gets: [ENTRADA, depois] })
    await a.abrir()
    const w = a.windows[0]
    const r = await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { valor: SENHA, usuario: '', rotulo: '' })
    same(r, { ok: false, erro: esperado })
    assert.equal(a.finishes().length, 0, `sem POST finish para ${JSON.stringify(depois).slice(0, 60)}`)
    assert.equal(JSON.stringify(r).includes(SENHA), false)
  }
})

test('salvar chave (secret): sem usuário nem rótulo no corpo', async () => {
  const a = ambiente({ gets: [{ ...ENTRADA, kind: 'secret', field: 'API_KEY', location: 'env', site: null }] })
  await a.abrir()
  await a.chamar('fork-cofre-seguro:salvar', a.windows[0].webContents, 'nonce-0', { valor: SENHA, usuario: 'x', rotulo: 'y' })
  const corpo = JSON.parse(a.finishes()[0].body)
  assert.equal(corpo.username, null)
  assert.equal(corpo.label, null)
  assert.equal(corpo.value, SENHA)
})

test('salvar: remetente errado, nonce errado, quadro errado ou janela morta não chegam ao Hub', async () => {
  const a = ambiente()
  await a.abrir()
  const w = a.windows[0]
  const envio = { valor: SENHA, usuario: '', rotulo: '' }
  same(await a.chamar('fork-cofre-seguro:salvar', a.geral.webContents, 'nonce-0', envio), { ok: false, erro: 'fechada' })
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-errado', envio), { ok: false, erro: 'fechada' })
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, undefined, envio), { ok: false, erro: 'fechada' })
  same(await a.chamarQuadro('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', envio), { ok: false, erro: 'fechada' })
  same(await a.chamar('fork-cofre-seguro:salvar', { ...w.webContents }, 'nonce-0', envio), { ok: false, erro: 'fechada' })
  assert.equal(a.getsChamadas().length, 1)
  assert.equal(a.finishes().length, 0)
  w.destroy()
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', envio), { ok: false, erro: 'fechada' })
  assert.equal(a.finishes().length, 0)
})

test('salvar: valor vazio ou grande demais é recusado antes do Hub', async () => {
  const a = ambiente()
  await a.abrir()
  const w = a.windows[0]
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { valor: '', usuario: '', rotulo: '' }), { ok: false, erro: 'vazio' })
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { valor: 'x'.repeat(5000), usuario: '', rotulo: '' }), { ok: false, erro: 'vazio' })
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', 'texto'), { ok: false, erro: 'vazio' })
  assert.equal(a.finishes().length, 0)
})

test('Hub recusa ou cai no finish: a resposta não leva o valor nem o texto do erro', async () => {
  // O GET sempre responde a entrada; só o POST finish varia.
  const comGet = (finish: () => Promise<Response>) =>
    async (url: string) => url.endsWith('/api/connectorRequestGet') ? new Response(JSON.stringify(ENTRADA)) : finish()
  for (const [http, esperado] of [
    [comGet(async () => new Response(JSON.stringify({ error: `falhou com ${SENHA}` }), { status: 400 })), 'recusado'],
    [comGet(async () => Promise.reject(new Error(`rede caiu com ${SENHA}`))), 'conexao'],
  ] as const) {
    const a = ambiente({ http })
    await a.abrir()
    const w = a.windows[0]
    const r = await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { valor: SENHA, usuario: '', rotulo: '' })
    same(r, { ok: false, erro: esperado })
    assert.equal(JSON.stringify(r).includes(SENHA), false)
    assert.equal(w.destroyed, false)
    assert.equal(JSON.stringify(a.avisos).includes(SENHA), false)
  }
})

test('cancelar fecha só com nonce certo; render-process-gone, navegação e fechar encerram a instância', async () => {
  const a = ambiente()
  await a.abrir()
  const w = a.windows[0]
  await a.chamar('fork-cofre-seguro:cancelar', w.webContents, 'nonce-errado')
  assert.equal(w.destroyed, false)
  await a.chamar('fork-cofre-seguro:cancelar', w.webContents, 'nonce-0')
  assert.equal(w.destroyed, true)
  same(a.avisos.at(-1), ['fork-cofre-seguro:estado', { entryId: 'e-1', estado: 'fechada' }])

  const b = ambiente()
  await b.abrir()
  b.windows[0].webContents.emit('render-process-gone')
  assert.equal(b.windows[0].destroyed, true)

  const c = ambiente()
  await c.abrir()
  const prevent = { n: 0 }
  c.windows[0].webContents.emit('will-navigate', { preventDefault: () => prevent.n++ })
  assert.equal(prevent.n, 1)
  c.windows[0].webContents.emit('did-start-navigation', {}, 'file:///outro', false, true)
  assert.equal(c.windows[0].destroyed, true)
  // Depois de fechada, abrir de novo cria uma instância nova, com nonce novo.
  same(await c.abrir(), { ok: true, foco: false })
  assert.equal(c.windows.length, 2)
  assert.equal(await c.chamar('fork-cofre-seguro:dados', c.windows[1].webContents, 'nonce-0'), null)
  assert.notEqual(await c.chamar('fork-cofre-seguro:dados', c.windows[1].webContents, 'nonce-1'), null)
})

test('página não carregada ainda: dados e salvar recusam (instância não está viva)', async () => {
  let liberar!: () => void
  const a = ambiente({ load: () => new Promise<void>((r) => { liberar = r }) })
  await a.chamar('fork-cofre-seguro:abrir', a.geral.webContents, { computerId: 'pc-1', entryId: 'e-1' })
  const w = a.windows[0]
  assert.equal(await a.chamar('fork-cofre-seguro:dados', w.webContents, 'nonce-0'), null)
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { valor: SENHA }), { ok: false, erro: 'fechada' })
  liberar()
  await settle()
  assert.equal(a.finishes().length, 0)
})

// ---- MX-277: "Mover para o cofre" abre a janela segura VAZIA para guardar um login novo (sem pedido do bot) ----

const ENVIO_NOVO = { valor: SENHA, usuario: 'ana@exemplo.com', rotulo: 'Painel', site: 'painel.exemplo.com' }

test('login novo: a janela abre VAZIA, sem consultar nenhum pedido do Hub e sem valor em canto nenhum', async () => {
  const a = ambiente()
  same(await a.abrirNovo(), { ok: true, foco: false })
  assert.equal(a.windows.length, 1)
  assert.equal(a.windows[0].shown, true)
  assert.equal(a.getsChamadas().length, 0, 'não há entrada no Hub para ler')
  assert.equal(a.chamadas.filter((c) => c.url.endsWith('/api/hello')).length, 1, 'só uma leitura sem valor e sem cofre confere a captura')
  const dados = await a.chamar('fork-cofre-seguro:dados', a.windows[0].webContents, 'nonce-0')
  same(dados, { pedido: { tipo: 'loginNovo', titulo: '', site: null, campo: null, local: null, botNome: '' } })
  same(a.avisos.at(-1), ['fork-cofre-seguro:estado', { entryId: 'login-novo', estado: 'aberta' }])
  assert.equal(JSON.stringify([a.chamadas, a.avisos, a.passos]).includes(SENHA), false)
})

test('login novo: nada do trecho escondido vai no IPC — qualquer campo além do computador recusa e nada abre', async () => {
  const a = ambiente()
  for (const ruim of [{ computerId: 'pc-1', valor: SENHA }, { computerId: 'pc-1', password: SENHA }, { computerId: 'pc-1', value: SENHA }, { computerId: 'pc-1', site: 'x.com' }, SENHA, null, {}]) {
    same(await a.abrirNovo(ruim), { ok: false, motivo: 'invalido' })
  }
  assert.equal(a.windows.length, 0)
  assert.equal(a.chamadas.length, 0, 'o Hub nem foi consultado')
  assert.equal(JSON.stringify(a.chamadas).includes(SENHA), false)
})

test('login novo: salvar é feito PELO main com as guardas — só o status volta, o valor só chega ao Hub', async () => {
  const a = ambiente()
  await a.abrirNovo()
  const w = a.windows[0]
  const r = await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', ENVIO_NOVO)
  same(r, { ok: true })
  assert.equal(JSON.stringify(r).includes(SENHA), false)
  assert.equal(a.salvaLogin().length, 1)
  const f = a.salvaLogin()[0]
  assert.equal(f.url, 'http://127.0.0.1:4567/api/credentialSaveLogin')
  assert.equal(f.token, 'token-do-hub')
  same(JSON.parse(f.body), { site: 'painel.exemplo.com', username: 'ana@exemplo.com', password: SENHA, label: 'Painel', createOnly: true })
  assert.equal(JSON.parse(f.body).createOnly, true, 'criação exclusiva: o Hub nunca substitui um login existente')
  assert.equal(a.finishes().length, 0, 'não passa pelo caminho do pedido do bot')
  await settle()
  assert.equal(w.destroyed, true, 'a janela se fecha')
  same(a.avisos.at(-1), ['fork-cofre-seguro:estado', { entryId: 'login-novo', estado: 'salva' }])
  assert.equal(JSON.stringify(a.avisos).includes(SENHA), false, 'o chat só sabe "salva", nunca o valor')
  // O valor só existe no corpo da chamada ao Hub.
  assert.equal(a.chamadas.filter((c) => c.body.includes(SENHA)).length, 1)
})

test('login novo: relay (nuvem) e computador desconhecido são recusados antes de abrir qualquer coisa', async () => {
  const a = ambiente()
  same(await a.abrirNovo({ computerId: 'pc-relay' }), { ok: false, motivo: 'semSuporte' })
  same(await a.abrirNovo({ computerId: 'outro-pc' }), { ok: false, motivo: 'semSuporte' })
  assert.equal(a.windows.length, 0)
  assert.equal(a.chamadas.length, 0)
})

test('login novo: cancelar fecha sem salvar nada, e o chat é avisado só de "fechada"', async () => {
  const a = ambiente()
  await a.abrirNovo()
  const w = a.windows[0]
  a.chamar('fork-cofre-seguro:cancelar', w.webContents, 'nonce-0')
  await settle()
  assert.equal(w.destroyed, true)
  assert.equal(a.salvaLogin().length, 0)
  same(a.avisos.at(-1), ['fork-cofre-seguro:estado', { entryId: 'login-novo', estado: 'fechada' }])
})

test('login novo: remetente errado, quadro errado, nonce errado e entrada falsa pelo canal do pedido não abrem nem salvam', async () => {
  const a = ambiente()
  // Remetente que não é a janela geral: a própria janela segura aberta (privada) não abre outra.
  await a.abrirNovo()
  const w = a.windows[0]
  same(await a.abrirNovo({ computerId: 'pc-1' }, w.webContents), { ok: false, motivo: 'invalido' })
  same(await a.chamarQuadro('fork-cofre-seguro:abrir-novo', a.geral.webContents, { computerId: 'pc-1' }), { ok: false, motivo: 'invalido' })
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-errado', ENVIO_NOVO), { ok: false, erro: 'fechada' })
  same(await a.chamar('fork-cofre-seguro:salvar', a.geral.webContents, 'nonce-0', ENVIO_NOVO), { ok: false, erro: 'fechada' })
  same(await a.chamarQuadro('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', ENVIO_NOVO), { ok: false, erro: 'fechada' })
  assert.equal(a.salvaLogin().length, 0)
  // O id reservado do login novo não abre pelo canal do pedido do bot.
  same(await a.abrir({ entryId: 'login-novo' }), { ok: false, motivo: 'invalido' })
  assert.equal(a.getsChamadas().length, 0)
})

test('login novo: uma janela por abertura — abrir de novo só traz a mesma para frente', async () => {
  const a = ambiente()
  same(await a.abrirNovo(), { ok: true, foco: false })
  same(await a.abrirNovo(), { ok: true, foco: true })
  assert.equal(a.windows.length, 1)
  assert.equal(a.windows[0].focused, 1)
  // Duas chamadas concorrentes dividem a mesma abertura.
  const b = ambiente()
  const [x, y] = await Promise.all([b.abrirNovo(), b.abrirNovo()])
  assert.equal(b.windows.length, 1)
  assert.ok(x.ok && y.ok)
})

test('login novo: sem site, valor vazio ou Hub recusando — nada é guardado, a janela fica e nada do valor volta', async () => {
  const a = ambiente()
  await a.abrirNovo()
  const w = a.windows[0]
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { ...ENVIO_NOVO, site: '' }), { ok: false, erro: 'vazio' })
  same(await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { ...ENVIO_NOVO, valor: '' }), { ok: false, erro: 'vazio' })
  assert.equal(a.salvaLogin().length, 0)
  const recusa = ambiente({ http: async (url: string) => {
    if (String(url).endsWith('/api/credentialSaveLogin')) return new Response(JSON.stringify({ error: SENHA }), { status: 400 })
    return new Response('{}', { status: 200 })
  } })
  await recusa.abrirNovo()
  const r = await recusa.chamar('fork-cofre-seguro:salvar', recusa.windows[0].webContents, 'nonce-0', ENVIO_NOVO)
  same(r, { ok: false, erro: 'recusado' })
  assert.equal(JSON.stringify(r).includes(SENHA), false, 'o corpo do erro do Hub nunca volta')
  assert.equal(recusa.windows[0].destroyed, false)
})

test('login novo: a conexão (túnel SSH) que morre antes de salvar não envia o valor', async () => {
  let viva = true
  const a = ambiente({ sshViva: () => ({ get current() { return viva }, listener: async () => true }) })
  same(await a.abrirNovo({ computerId: 'pc-ssh' }), { ok: true, foco: false })
  viva = false
  const r = await a.chamar('fork-cofre-seguro:salvar', a.windows[0].webContents, 'nonce-0', ENVIO_NOVO)
  same(r, { ok: false, erro: 'conexao' })
  assert.equal(a.salvaLogin().length, 0, 'o valor não saiu')
  // Hub SSH que não responde na abertura: não abre.
  const b = ambiente({ sshViva: () => ({ current: false, listener: async () => true }) })
  same(await b.abrirNovo({ computerId: 'pc-ssh' }), { ok: false, motivo: 'semConexao' })
  assert.equal(b.windows.length, 0)
})

test('login novo: o Hub recusa com 409 (já existe login para o site e usuário) — a janela mostra "jaExiste", fica aberta e nada do valor volta', async () => {
  const a = ambiente({ http: async (url: string) => {
    if (String(url).endsWith('/api/credentialSaveLogin')) return new Response(JSON.stringify({ error: `login_exists ${SENHA}` }), { status: 409 })
    return new Response('{}', { status: 200 })
  } })
  await a.abrirNovo()
  const w = a.windows[0]
  const r = await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', ENVIO_NOVO)
  same(r, { ok: false, erro: 'jaExiste' })
  assert.equal(JSON.stringify(r).includes(SENHA), false, 'o corpo do erro do Hub nunca volta')
  assert.equal(w.destroyed, false, 'a janela fica aberta (a pessoa pode corrigir o usuário)')
  assert.equal(a.avisos.some((x) => JSON.stringify(x).includes('salva')), false, 'o chat não é avisado de "salva"')
  // Pode tentar de novo na mesma janela (salvando volta a false).
  const outra = await a.chamar('fork-cofre-seguro:salvar', w.webContents, 'nonce-0', { ...ENVIO_NOVO, usuario: 'outra@exemplo.com' })
  same(outra, { ok: false, erro: 'jaExiste' })
})

test('login novo: outras recusas do Hub (cofre bloqueado, rótulo) viram "recusado" e erro de rede vira "conexao"', async () => {
  for (const [status, esperado] of [[400, 'recusado'], [403, 'recusado'], [500, 'recusado']] as const) {
    const a = ambiente({ http: async (url: string) => (String(url).endsWith('/api/credentialSaveLogin') ? new Response('{}', { status }) : new Response('{}', { status: 200 })) })
    await a.abrirNovo()
    same(await a.chamar('fork-cofre-seguro:salvar', a.windows[0].webContents, 'nonce-0', ENVIO_NOVO), { ok: false, erro: esperado })
    assert.equal(a.windows[0].destroyed, false)
  }
})

// ---- MX-286: o código de login usa a mesma janela segura (kind "code") ----

const ENTRADA_CODIGO = {
  entryId: 'e-1', botId: 'bot-1', botName: 'Berga', kind: 'code', status: 'pending',
  title: 'GitHub conta A', site: 'github.com', field: null, location: null,
  spaceId: 'sp-1', conversationId: 'conv-1',
}
const CODIGO = '482913'

test('código de login: o main deriva o pedido do Hub (título do login e site) e a janela abre pelo mesmo caminho', async () => {
  const a = ambiente({ gets: [ENTRADA_CODIGO] })
  same(await a.abrir({ spaceId: 'sp-1' }), { ok: true, foco: false })
  assert.equal(a.windows.length, 1)
  const dados = await a.chamar('fork-cofre-seguro:dados', a.windows[0].webContents, 'nonce-0')
  assert.deepEqual(JSON.parse(JSON.stringify(dados)), { pedido: { tipo: 'code', titulo: 'GitHub conta A', site: 'github.com', campo: null, local: null, botNome: 'Berga' } })
  same(a.avisos.at(-1), ['fork-cofre-seguro:estado', { entryId: 'e-1', estado: 'aberta' }])
})

test('código de login: nada do código vai no IPC do chat (só ids) e o Hub só recebe o código no finish', async () => {
  const a = ambiente({ gets: [ENTRADA_CODIGO] })
  await a.abrir({ valor: CODIGO, code: CODIGO, value: CODIGO, codigo: CODIGO })
  assert.equal(JSON.stringify(a.chamadas).includes(CODIGO), false, 'o código nem existe antes de a pessoa digitar na janela')
  const r = await a.chamar('fork-cofre-seguro:salvar', a.windows[0].webContents, 'nonce-0', { valor: CODIGO, usuario: 'x', rotulo: 'y' })
  same(r, { ok: true })
  assert.equal(JSON.stringify(r).includes(CODIGO), false)
  assert.equal(a.finishes().length, 1)
  const f = a.finishes()[0]
  assert.equal(f.url, 'http://127.0.0.1:4567/api/connectorRequestFinish')
  assert.equal(f.token, 'token-do-hub')
  const corpo = JSON.parse(f.body)
  assert.equal(corpo.value, CODIGO)
  assert.equal(corpo.entryId, 'e-1')
  assert.equal(corpo.username, null, 'código não leva usuário')
  assert.equal(corpo.label, null, 'nem rótulo')
  assert.equal(a.chamadas.filter((c) => c.body.includes(CODIGO)).length, 1, 'o código só está no corpo do finish')
  await settle()
  assert.equal(a.windows[0].destroyed, true)
  same(a.avisos.at(-1), ['fork-cofre-seguro:estado', { entryId: 'e-1', estado: 'salva' }])
  assert.equal(JSON.stringify(a.avisos).includes(CODIGO), false)
})

test('código de login: tamanho e formato fora de 4–10 dígitos são recusados antes do Hub', async () => {
  const a = ambiente({ gets: [ENTRADA_CODIGO] })
  await a.abrir()
  for (const ruim of ['', '123', '12345678901', '12 34', 'abcdef', '123-456']) {
    same(await a.chamar('fork-cofre-seguro:salvar', a.windows[0].webContents, 'nonce-0', { valor: ruim }), { ok: false, erro: 'vazio' })
  }
  assert.equal(a.finishes().length, 0)
  // 4 e 10 dígitos valem.
  for (const bom of ['4829', '4829135067']) {
    const b = ambiente({ gets: [ENTRADA_CODIGO] })
    await b.abrir()
    same(await b.chamar('fork-cofre-seguro:salvar', b.windows[0].webContents, 'nonce-0', { valor: bom }), { ok: true })
    assert.equal(JSON.parse(b.finishes()[0].body).value, bom)
  }
})

test('código de login: relay/computador sem caminho seguro e pedido que mudou antes de salvar não enviam o código', async () => {
  const relay = ambiente({ gets: [ENTRADA_CODIGO] })
  same(await relay.abrir({ computerId: 'pc-relay' }), { ok: false, motivo: 'semSuporte' })
  assert.equal(relay.windows.length, 0)
  assert.equal(relay.chamadas.length, 0)
  for (const depois of [{ ...ENTRADA_CODIGO, status: 'ready' }, { ...ENTRADA_CODIGO, title: 'Outro login' }, { ...ENTRADA_CODIGO, kind: 'login' }]) {
    const a = ambiente({ gets: [ENTRADA_CODIGO, depois] })
    await a.abrir()
    same(await a.chamar('fork-cofre-seguro:salvar', a.windows[0].webContents, 'nonce-0', { valor: CODIGO }), { ok: false, erro: 'recusado' })
    assert.equal(a.finishes().length, 0)
  }
})

test('código de login: cancelar não envia nada e fecha', async () => {
  const a = ambiente({ gets: [ENTRADA_CODIGO] })
  await a.abrir()
  a.chamar('fork-cofre-seguro:cancelar', a.windows[0].webContents, 'nonce-0')
  await settle()
  assert.equal(a.windows[0].destroyed, true)
  assert.equal(a.finishes().length, 0)
  same(a.avisos.at(-1), ['fork-cofre-seguro:estado', { entryId: 'e-1', estado: 'fechada' }])
})
