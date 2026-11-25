import assert from 'node:assert/strict'
import test from 'node:test'
import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto'
import * as path from 'node:path'
import type { WebContents } from 'electron'
import { deferred, loadMainModule, settle } from '../../shared/test-support/main-module.ts'
import * as contexts from './contexts.ts'
import { environmentProfile } from '../../shared/environment.ts'

/** Objetos do contexto vm têm outro protótipo; compara a forma. */
const same = (actual: unknown, expected: unknown) => assert.equal(JSON.stringify(actual), JSON.stringify(expected))

type Listener = (...args: any[]) => void
type Handler = (event: { sender: unknown; senderFrame: unknown }, ...args: any[]) => any

/** BrowserWindow, ipcMain, observadores e relógio substituídos; registra a ordem real de openPrivateBox. */
function electron(options: { load?: () => Promise<void>; platform?: string; observers?: { observerSignals: () => Promise<string[]> } } = {}) {
  const windows: any[] = []
  const steps: string[] = []
  const handlers = new Map<string, Handler>()
  const timers: (() => void)[] = []
  const observers = { signals: [] as string[], checks: 0, fail: false }
  const hub = { calls: 0, call: async () => { hub.calls++; throw new Error('envio inesperado ao Hub') } }
  class BrowserWindow {
    options: any
    destroyed = false
    shown = false
    url = ''
    nativeId = 'window:42:0'
    idReads = 0
    listeners = new Map<string, Listener[]>()
    webContents: any
    constructor(options: any) {
      this.options = options
      const self = this
      const listeners = new Map<string, Listener[]>()
      const sent: unknown[][] = []
      this.webContents = {
        sent, mainFrame: { frame: windows.length },
        isDestroyed: () => this.destroyed,
        getURL: () => self.url,
        send: (...args: unknown[]) => sent.push(args),
        setWindowOpenHandler: (handler: () => unknown) => steps.push(`openHandler:${JSON.stringify(handler())}`),
        on: (name: string, listener: Listener) => listeners.set(name, [...(listeners.get(name) ?? []), listener]),
        emit: (name: string, ...args: unknown[]) => (listeners.get(name) ?? []).forEach(listener => listener(...args)),
      }
      windows.push(this)
      steps.push('new')
    }
    on(name: string, listener: Listener) { this.listeners.set(name, [...(this.listeners.get(name) ?? []), listener]) }
    isDestroyed() { return this.destroyed }
    destroy() { this.destroyed = true; steps.push('destroy') }
    setContentProtection(enabled: boolean) { steps.push(`protection:${enabled}`) }
    getMediaSourceId() { this.idReads++; return this.nativeId }
    async loadURL(url: string) {
      steps.push(`load:${contexts.isGeneralContext(this.webContents) ? 'geral' : 'privado'}:${url.startsWith('data:text/html')}`)
      await (options.load ?? (async () => {}))()
      this.url = url
    }
    show() { this.shown = true; steps.push(`show:${contexts.isGeneralContext(this.webContents) ? 'geral' : 'privado'}`) }
  }
  const module = loadMainModule<typeof import('./window.ts')>('fork-secret-box/window', {
    electron: { BrowserWindow, ipcMain: { handle: (channel: string, handler: Handler) => handlers.set(channel, handler) } },
    'node:crypto': { randomUUID: () => `nonce-${windows.length}` },
    'node:path': path,
    './contexts': contexts,
    './observers': options.observers ?? { observerSignals: async () => {
      observers.checks++
      steps.push('observadores')
      if (observers.fail) throw new Error('falha inesperada')
      return observers.signals
    } },
  }, {
    encodeURIComponent, __dirname: '/app/out/main', process: { platform: options.platform ?? 'darwin' },
    setInterval: (callback: () => void) => { timers.push(callback); return timers.length },
    clearInterval: (id: number) => { timers[id - 1] = () => {} },
  })
  const gesture = (sender: any, nonce: unknown, requestRef: unknown = 'pedido-1', senderFrame = sender?.mainFrame) =>
    handlers.get(module.gestureChannel)!({ sender, senderFrame }, nonce, requestRef)
  const tick = async () => { for (const timer of timers) timer(); await settle() }
  return { module, windows, steps, handlers, observers, hub, gesture, tick }
}

test('confere observadores, marca antes de carregar e mostrar; preload próprio, nunca o geral; fora de broadcasts', async () => {
  const f = electron()
  const general = { webContents: { isDestroyed: () => false, send: (...args: unknown[]) => generalSent.push(args) } as unknown as WebContents }
  const generalSent: unknown[][] = []
  const window = await f.module.openPrivateBox(f.hub)
  assert.deepEqual(f.steps, ['observadores', 'new', 'protection:true', 'openHandler:{"action":"deny"}', 'load:privado:true', 'observadores', 'show:privado'])
  const prefs = f.windows[0].options.webPreferences
  assert.equal(prefs.preload, '/app/out/preload/fork-secret-box.js')
  assert.equal(JSON.stringify(prefs.additionalArguments), '["--donna-box-nonce=nonce-0"]')
  assert.equal(prefs.sandbox, true)
  assert.equal(prefs.contextIsolation, true)
  const generalWindows = contexts.generalWindowProvider(() => [general, window as any])
  assert.deepEqual(generalWindows(), [general])
  for (const w of generalWindows()) w.webContents.send('host:change', { state: 'fixture' })
  assert.equal(generalSent.length, 1)
  assert.deepEqual(f.windows[0].webContents.sent, [])
})

test('Linux e Windows: indisponível, sem janela e sem conferência (D331 B)', async () => {
  for (const platform of ['linux', 'win32']) {
    const f = electron({ platform })
    await assert.rejects(f.module.openPrivateBox(f.hub), /caixa-indisponivel: sistema/)
    assert.deepEqual(f.windows, [])
    assert.equal(f.handlers.size, 0)
  }
})

test('ajudante presente, registrado, rodando ou conferência com erro: a caixa não abre (D331 C)', async () => {
  for (const signals of [['binário'], ['agente:enabled'], ['processo:sim'], ['processo:incerto']]) {
    const f = electron()
    f.observers.signals = signals
    await assert.rejects(f.module.openPrivateBox(f.hub), /caixa-indisponivel: observador/)
    assert.deepEqual(f.windows, [])
  }
  const f = electron()
  f.observers.fail = true
  await assert.rejects(f.module.openPrivateBox(f.hub))
  assert.deepEqual(f.windows, [])
})

test('ajudante aparece com a caixa aberta: fecha na hora; id nativo lido a cada conferência', async () => {
  const f = electron()
  await f.module.openPrivateBox(f.hub)
  const [box] = f.windows
  assert.equal(box.idReads, 1)
  await f.tick()
  assert.equal(box.idReads, 2)
  assert.equal(box.destroyed, false)
  f.observers.signals = ['processo:sim']
  await f.tick()
  assert.equal(box.destroyed, true)
  const g = electron()
  await g.module.openPrivateBox(g.hub)
  g.windows[0].nativeId = ''
  await g.tick()
  assert.equal(g.windows[0].destroyed, true)
})

test('gesto: só a instância viva com seu nonce; geral, nonce velho, recriada e navegada são recusadas; nada vai ao Hub', async () => {
  const f = electron()
  await f.module.openPrivateBox(f.hub)
  const [first] = f.windows
  const unavailable = { ok: false, reason: 'indisponível' }
  const refused = { ok: false, reason: 'recusado' }
  same(await f.gesture(first.webContents, 'nonce-0'), unavailable)
  assert.equal(first.idReads, 2)
  const general = { isDestroyed: () => false, getURL: () => first.url, mainFrame: {} }
  same(await f.gesture(general, 'nonce-0'), refused)
  same(await f.gesture(first.webContents, 'nonce-0', 'pedido-1', {}), refused)
  same(await f.gesture(first.webContents, 'outro'), refused)
  await f.module.openPrivateBox(f.hub)
  const second = f.windows[1]
  assert.equal(first.destroyed, true)
  same(await f.gesture(first.webContents, 'nonce-0'), refused)
  same(await f.gesture(second.webContents, 'nonce-0'), refused)
  same(await f.gesture(second.webContents, 'nonce-1'), unavailable)
  second.webContents.emit('did-start-navigation', { url: 'https://exemplo.invalid' })
  assert.equal(second.destroyed, true)
  same(await f.gesture(second.webContents, 'nonce-1'), refused)
  assert.equal(f.hub.calls, 0)
})

test('observador surge entre abrir e o gesto: recusa e fecha', async () => {
  const f = electron()
  await f.module.openPrivateBox(f.hub)
  f.observers.signals = ['agente:requires-approval']
  same(await f.gesture(f.windows[0].webContents, 'nonce-0'), { ok: false, reason: 'recusado' })
  assert.equal(f.windows[0].destroyed, true)
})

test('falha ao carregar destrói, nunca mostra e não vira janela geral', async () => {
  const f = electron({ load: async () => { throw new Error('falha de carga') } })
  await assert.rejects(f.module.openPrivateBox(f.hub), /falha de carga/)
  const [window] = f.windows
  assert.equal(window.shown, false)
  assert.equal(window.destroyed, true)
  assert.deepEqual(f.steps.slice(-2), ['load:privado:true', 'destroy'])
  assert.deepEqual(contexts.generalWindowProvider(() => [window])(), [])
})

test('navegação, queda do renderer ou recriação: fecha a instância; a nova sai marcada', async () => {
  const f = electron()
  await f.module.openPrivateBox(f.hub)
  const [first] = f.windows
  first.webContents.emit('did-start-navigation', { url: 'https://exemplo.invalid' })
  assert.equal(first.destroyed, true)
  await f.module.openPrivateBox(f.hub)
  const second = f.windows[1]
  assert.notEqual(second.webContents, first.webContents)
  assert.equal(contexts.isGeneralContext(second.webContents), false)
  let prevented = false
  second.webContents.emit('will-navigate', { preventDefault: () => { prevented = true } })
  assert.equal(prevented, true)
  second.webContents.emit('render-process-gone', {}, { reason: 'crashed' })
  assert.equal(second.destroyed, true)
})

test('host-proxy recusa host:call vindo da janela privada, sem fetch', async () => {
  const f = electron()
  await f.module.openPrivateBox(f.hub)
  const handlers = new Map<string, Handler>()
  let fetches = 0
  const proxy = loadMainModule<typeof import('../host-proxy.ts')>('host-proxy', {
    electron: { ipcMain: { handle: (channel: string, h: Handler) => handlers.set(channel, h), on: (channel: string, h: Handler) => handlers.set(channel, h) } },
    './analytics': { noteAnalyticsSetting: () => {} },
    './fork-secret-box/contexts': contexts,
    'node:crypto': { createHmac, randomBytes, timingSafeEqual },
  }, { URL, AbortController, setTimeout, clearTimeout, TextDecoder, Buffer })
  proxy.registerHostProxy()
  const result = await handlers.get('host:call')!({ sender: f.windows[0].webContents, senderFrame: null }, 'http://localhost:1', 'fixture', 'ping', {}, 1000)
  assert.equal(JSON.stringify(result), JSON.stringify({ ok: false, error: { status: 0, message: 'Contexto indisponível.' } }))
  assert.equal(fetches, 0)
})

test('preload geral não expõe o canal do gesto', async () => {
  const { readdir, readFile } = await import('node:fs/promises')
  const dir = new URL('../../preload/', import.meta.url)
  const general = (await readdir(dir)).filter(file => file !== 'fork-secret-box.ts')
  assert.ok(general.includes('index.ts'))
  for (const file of general) assert.equal((await readFile(new URL(file, dir), 'utf8')).includes('fork-secret-box'), false, file)
})

/**
 * observers.ts e screen.ts reais, como a sonda da revisão: app main em /Applications/Donnabot.app, Dev no
 * bundle separado; só FS, login item, launchctl e pgrep são substituídos. `files` = caminhos existentes.
 */
function realObservers(mac: { running: Set<string>; loaded: Set<string>; files: Set<string>; pgrep?: () => Promise<void> }) {
  const fs = {
    lstatSync: (file: string) => { if (mac.files.has(file)) return {}; throw Object.assign(new Error('ENOENT'), { code: 'ENOENT' }) },
    existsSync: (file: string) => mac.files.has(file),
    statSync: () => ({ mtimeMs: 0 }),
  }
  return loadMainModule<typeof import('./observers.ts')>('fork-secret-box/observers', {
    'node:child_process': { execFile: (bin: string, args: string[], _options: unknown, done: (error: unknown) => void) => {
      if (bin === '/bin/launchctl') return done([...mac.loaded].some(label => args[1].endsWith(`/${label}`)) ? null : { code: 113 })
      void (mac.pgrep ?? (async () => {}))().then(() => done([...mac.running].some(name => args[2].includes(`/${name}.app/`)) ? null : { code: 1 }))
    } },
    'node:fs': fs,
    'node:os': { homedir: () => '/Users/dono' },
    'node:path': path,
    electron: { app: { isPackaged: true, getLoginItemSettings: () => ({ status: 'not-registered' }) }, ipcMain: {} },
    '../host-controller': { prefs: { get: () => null, set: () => {} } },
    '../environment': { identity: environmentProfile('main'), localIdentity: environmentProfile('main'), isRc: false },
  }, { process: { platform: 'darwin', resourcesPath: '/Applications/Donnabot.app/Contents/Resources', getuid: () => 501 } })
}

const devHelper = '/Applications/Donnabot Dev.app/Contents/Library/LoginItems/DonnabotDevScreen.app/Contents/MacOS/DonnabotDevScreen'
const mac = (over: Partial<Parameters<typeof realObservers>[0]> = {}) =>
  ({ running: new Set<string>(), loaded: new Set<string>(), files: new Set<string>(), ...over })

test('R1-01 na caixa: ajudante Dev (ou main) rodando impede abrir; surgindo com a caixa aberta, fecha', async () => {
  for (const name of ['DonnabotDevScreen', 'DonnabotScreen']) {
    const m = mac({ running: new Set([name]) })
    const f = electron({ observers: realObservers(m) })
    await assert.rejects(f.module.openPrivateBox(f.hub), /caixa-indisponivel: observador/)
    assert.equal(f.windows.length, 0)
    m.running.clear()
    await f.module.openPrivateBox(f.hub)
    assert.equal(f.windows[0].shown, true)
    m.running.add(name)
    await f.tick()
    await settle()
    assert.equal(f.windows[0].destroyed, true)
  }
})

test('R1-01 sonda da revisão: binário Dev no bundle separado, sem registro nem processo, não abre', async () => {
  const f = electron({ observers: realObservers(mac({ files: new Set([devHelper]) })) })
  await assert.rejects(f.module.openPrivateBox(f.hub), /caixa-indisponivel: observador/)
  assert.equal(f.windows.length, 0)
})

test('R1-03 sonda da revisão: Dev instalado e registrado durante a última espera do pgrep: shown fica false', async () => {
  const wait = deferred()
  let waiting = false
  const m = mac({ pgrep: async () => { if (waiting) await wait.promise } })
  const f = electron({ observers: realObservers(m), load: async () => { waiting = true } })
  const opening = f.module.openPrivateBox(f.hub)
  await settle()
  m.files.add(devHelper)
  m.loaded.add('app.donnabot.dev.screen')
  wait.resolve()
  await assert.rejects(opening, /caixa-indisponivel: observador/)
  assert.equal(f.windows[0].shown, false)
  assert.equal(f.windows[0].destroyed, true)
})
