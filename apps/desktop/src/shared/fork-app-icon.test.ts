// MX-9: o seletor de ícone é só dados + main seguro. O renderer manda o `id`; o main resolve o PNG do manifesto
// empacotado, valida o PNG pronto e aplica por `app.dock.setIcon` (macOS) ou `BrowserWindow.setIcon` (demais).
// O padrão é o ícone do pacote: sem escolha, nenhum setIcon; a preferência antiga `app-icon-v1.json` segue ignorada.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import * as path from 'node:path'
import { APP_ICON_CHOICES, APP_ICON_DEFAULT_ID, APP_ICON_OPTIONS, appIconAsset } from './app-icons.ts'
import { loadMainModule } from './test-support/main-module.ts'

const desktop = new URL('../../', import.meta.url)
const iconsDir = new URL('resources/app-icons/', desktop)

function png(data: Buffer): [number, number] {
  assert.equal(data.subarray(0, 8).toString('hex'), '89504e470d0a1a0a')
  return [data.readUInt32BE(16), data.readUInt32BE(20)]
}

// SHA256 dos `icon_256x256.png` do pacote final (pacote-opcoes-r2-r3-20261110 @ e91caed5, SHA256SUMS).
// A arte aplicada é o arquivo do pacote byte a byte — sem quantizar, sem recomprimir.
const PACKAGE_SHA256: Readonly<Record<string, string>> = {
  'icone-01-laranja-close.png': '126d6263b2a6caa11d4c8236754e3ed98292ba09c58dda6db0cb6cd0480d8a4b',
  'icone-02-laranja-inteiro.png': 'b6892bdfee0138c6790be5ceb3f5fd76f8a6295960aec15bc46b7bc8a7cf4472',
  'icone-03-laranja-degrade-feliz.png': '7a453da5d38299a8680ab6f12cd45ecb6b0290504b5b0c3a2866033997a8bed3',
  'icone-04-verde-close.png': 'd9e0dd37816d4320fbc8828261d7e333b6d66e252ea098ddd0b43a11f49e9562',
  'icone-05-verde-piscando.png': '4f2e74f460f814bdc0d9ad47119f9db1dd30c362952a2ee52b5269f1cd75d835',
  'icone-06-creme-inteiro.png': '8d44c6a928b5c5255fb26f6197fdf6a616181d03bbd0ccf944be78361c3b4a0b',
  'icone-07-branco-piscando.png': '942c1b36899ddd497fd04055d813fcd197fd16d0855278696ddb5bdc9c68d8be',
  'icone-08-branco-close.png': '9c7d1b825ff175b86bd7894215e5a0eadaffac6651bd8bff8bcbed585a8ef254',
  'icone-09-escuro-inteiro.png': '02f0231bba6c81b2c3686ce5bd573029a4632d988812cdf1086a8736adf53e4e',
  'icone-10-creme-close-feliz.png': '5cb02835ad2f5d187dc7c2add9bb109fabcc88595fc8ac3e9b4f05c4003fe361',
  'icone-11-gomo-laranja.png': '90b78cc1911b37ea11d7ec963a27dda4fec7ab9caef1d80c724c69e0741021e4',
  'icone-12-gomo-creme.png': 'e0d2a23eb94855bf2287723deb5e9bc37a3f51e824e657c0fe56024d7780267d',
  'icone-13-lima-close.png': '07c403fcbab7ac215030fd4034f0170e1fc26aadd430f2506b6448986bb2f957',
  'icone-14-limao-inteiro.png': '6e316009b9b84498de3550c920e1ad2b26fc8ac2ccab7af4c6347103feb98799',
  'icone-15-toranja-close.png': 'b9a2902ca6ad407b21844292f60a7b851f766ccba21f2ca0b641f6261c9d02c2',
  'icone-16-designer.png': 'bd5345e69f090c3de51b22ddc0caaa86ceb1929fa5148b7402ae8c27db626d91',
  'icone-17-pesquisa.png': '8c5c069ccd8c0e81da7c3494b79ca63a570370209abd673222a76641aa090dd8',
  'icone-18-financeiro.png': '21d82320639a2684fbb42e3c59d324b32dfab5ad30efc046efe39604083e65d2',
  'icone-19-berga.png': 'bc02b03d872bf568bd5a42b9d9f6283805c002c7ac6719e8283b1f0fda9e6b18',
}

test('MX-9: o manifesto tem o padrão primeiro, ids únicos e só ids conhecidos resolvem arquivo', { skip: 'arte própria fora do recorte (pendência de direitos); reativar ao liberar a arte' }, () => {
  assert.equal(APP_ICON_CHOICES.length, 1 + APP_ICON_OPTIONS.length)
  assert.equal(APP_ICON_CHOICES[0]!.id, APP_ICON_DEFAULT_ID)
  assert.equal(new Set(APP_ICON_CHOICES.map((choice) => choice.id)).size, APP_ICON_CHOICES.length)
  for (const choice of APP_ICON_CHOICES) {
    assert.equal(appIconAsset(choice.id)?.file, choice.file)
    const file = new URL(choice.file, iconsDir)
    assert.ok(existsSync(file), `${choice.id}: falta ${choice.file}`)
    const data = readFileSync(file)
    const [width, height] = png(data)
    assert.deepEqual([width, height], [256, 256], `${choice.id}: ${choice.file}`)
    if (choice.id !== APP_ICON_DEFAULT_ID) {
      assert.equal(createHash('sha256').update(data).digest('hex'), PACKAGE_SHA256[choice.file], `${choice.id}: arte diferente do pacote final`)
    }
  }
  for (const invalid of ['../resources/icon.png', 'padrao.png', 'icone-01-laranja-close.png', 'laranja-close', 'icone-99-inexistente', 'file:///x.png', '', null, undefined, 42, { id: 'icone-01-laranja-close' }]) {
    assert.equal(appIconAsset(invalid), null, String(invalid))
  }
})

interface FakeWindow { isDestroyed(): boolean; setIcon(image: { png: string }): void }
interface ControllerModule {
  createAppIconController(store: { get(): unknown; set(id: string): void }, followDock?: (watcher: (change: string) => void) => unknown): {
    selected(): string
    set(id: unknown): string
    reapply(): void
  }
  registerAppIconIPC(controller: unknown, chat: () => unknown, rendererURL: string): void
}

function load(platform: 'darwin' | 'linux' | 'win32', failFile?: string) {
  const dockCalls: string[] = []
  const windowCalls: string[] = []
  const windows: FakeWindow[] = []
  const errors: string[] = []
  const handlers = new Map<string, (event: unknown, ...args: unknown[]) => unknown>()
  const listeners = new Map<string, ((...args: unknown[]) => unknown)[]>()
  const electron = {
    app: {
      isPackaged: true,
      dock: platform === 'darwin'
        ? { setIcon(image: { png: string }) { dockCalls.push(image.png) }, isVisible: () => true }
        : undefined,
      on(event: string, listener: (...args: unknown[]) => unknown) { listeners.set(event, [...(listeners.get(event) ?? []), listener]) },
    },
    BrowserWindow: { getAllWindows: () => windows },
    ipcMain: { handle: (channel: string, handler: (event: unknown, ...args: unknown[]) => unknown) => handlers.set(channel, handler) },
    nativeImage: {
      createEmpty() { return { png: 'reset', isEmpty: () => true, getSize: () => ({ width: 0, height: 0 }) } },
      createFromPath(file: string) {
        const name = path.basename(file)
        if (name === failFile) return { isEmpty: () => true, getSize: () => ({ width: 0, height: 0 }) }
        // icon.png é o ícone real do pacote (1024 px); as opções são os PNGs 256 do manifesto.
        return { png: name, isEmpty: () => false, getSize: () => (name === 'icon.png' ? { width: 1024, height: 1024 } : { width: 256, height: 256 }) }
      },
    },
  }
  const mod = loadMainModule<ControllerModule>('fork-app-icon-controller', { electron, 'node:path': path }, {
    process: { platform, resourcesPath: '/res', env: {} },
    __dirname: '/app/out/main',
    URL,
    console: { error: (line: unknown) => errors.push(String(line)), warn() {}, log() {} },
  })
  return {
    mod, dockCalls, windowCalls, windows, errors, handlers,
    newWindow() {
      const win: FakeWindow = { isDestroyed: () => false, setIcon(image) { windowCalls.push(image.png) } }
      windows.push(win)
      return win
    },
    emit(event: string, ...args: unknown[]) { for (const listener of listeners.get(event) ?? []) listener({}, ...args) },
  }
}

function store(initial?: unknown) {
  let value = initial
  const writes: unknown[] = []
  return {
    writes,
    get value() { return value },
    get() { return value },
    set(id: string) { value = id; writes.push(id) },
  }
}

test('MX-9 macOS: a opção chama app.dock.setIcon; o padrão volta pelo mesmo caminho; janela não é tocada', () => {
  const r = load('darwin')
  const prefs = store()
  const icon = r.mod.createAppIconController(prefs)
  assert.equal(icon.selected(), 'padrao')
  assert.deepEqual(r.dockCalls, [])
  assert.equal(icon.set('icone-01-laranja-close'), 'icone-01-laranja-close')
  assert.deepEqual(r.dockCalls, ['icone-01-laranja-close.png'])
  icon.reapply()
  assert.deepEqual(r.dockCalls, ['icone-01-laranja-close.png', 'icone-01-laranja-close.png'])
  assert.equal(icon.set('padrao'), 'padrao')
  // No mac o padrão volta com imagem vazia (reset para o ícone do bundle/Assets.car), não com PNG.
  assert.deepEqual(r.dockCalls, ['icone-01-laranja-close.png', 'icone-01-laranja-close.png', 'reset'], 'voltar ao padrão devolve o ícone do pacote')
  assert.equal(prefs.value, 'padrao')
  assert.equal(icon.set('../resources/icon.png'), 'padrao')
  assert.equal(icon.set({ file: 'x.png' }), 'padrao')
  assert.equal(prefs.writes.filter((id) => id === 'icone-01-laranja-close').length, 1)
})

test('MX-9: a preferência versionada {version:1,id} resolve; versão ou id inválidos caem no padrão', () => {
  const ok = load('darwin')
  const icon = ok.mod.createAppIconController(store({ version: 1, id: 'icone-04-verde-close' }))
  assert.equal(icon.selected(), 'icone-04-verde-close')
  assert.deepEqual(ok.dockCalls, ['icone-04-verde-close.png'])
  const versao = load('darwin')
  const antiga = versao.mod.createAppIconController(store({ version: 2, id: 'icone-04-verde-close' }))
  assert.equal(antiga.selected(), 'padrao')
  assert.deepEqual(versao.dockCalls, [])
  const idRuim = load('darwin')
  const invalido = idRuim.mod.createAppIconController(store({ version: 1, id: '../resources/icon.png' }))
  assert.equal(invalido.selected(), 'padrao')
  assert.deepEqual(idRuim.dockCalls, [])
  // Preferência 'padrao' gravada: o pacote já traz o ícone certo, nenhum setIcon na partida.
  for (const inicial of ['padrao', { version: 1, id: 'padrao' }]) {
    const partida = load('darwin')
    const icon = partida.mod.createAppIconController(store(inicial))
    assert.equal(icon.selected(), 'padrao')
    assert.deepEqual(partida.dockCalls, [], `inicial ${JSON.stringify(inicial)} não aplica nada`)
  }
})

test('MX-9 Linux/Windows: BrowserWindow.setIcon vale para as janelas, inclusive as que nascem depois', () => {
  const r = load('linux')
  const prefs = store()
  const icon = r.mod.createAppIconController(prefs)
  r.newWindow()
  assert.equal(icon.set('icone-05-verde-piscando'), 'icone-05-verde-piscando')
  assert.deepEqual(r.windowCalls, ['icone-05-verde-piscando.png'])
  const depois = r.newWindow()
  r.emit('browser-window-created', depois)
  assert.deepEqual(r.windowCalls, ['icone-05-verde-piscando.png', 'icone-05-verde-piscando.png'])
  assert.equal(icon.set('padrao'), 'padrao')
  assert.deepEqual(r.windowCalls, ['icone-05-verde-piscando.png', 'icone-05-verde-piscando.png', 'icon.png', 'icon.png'], 'voltar ao padrão reaplica o ícone real do pacote em todas as janelas')
  r.emit('browser-window-created', r.newWindow())
  assert.deepEqual(r.windowCalls, ['icone-05-verde-piscando.png', 'icone-05-verde-piscando.png', 'icon.png', 'icon.png'], 'depois do padrão, janela nova fica com o ícone do pacote')
})

test('MX-9: PNG que não valida não grava nem aplica; o erro fica no console', () => {
  const r = load('darwin', 'icone-09-escuro-inteiro.png')
  const prefs = store()
  const icon = r.mod.createAppIconController(prefs)
  assert.equal(icon.set('icone-09-escuro-inteiro'), 'padrao')
  assert.equal(prefs.value, undefined)
  assert.deepEqual(r.dockCalls, [])
  assert.equal(r.errors.length, 1)
  assert.equal(icon.set('icone-01-laranja-close'), 'icone-01-laranja-close')
  assert.deepEqual(r.dockCalls, ['icone-01-laranja-close.png'])
})

test('MX-9: o IPC só aceita o frame principal da janela de chat local conhecida', () => {
  const r = load('darwin')
  const prefs = store()
  const icon = r.mod.createAppIconController(prefs)
  const wc = { mainFrame: { url: 'file:///chat.html' } }
  const chat = { isDestroyed: () => false, webContents: wc }
  r.mod.registerAppIconIPC(icon, () => chat as never, 'file:///chat.html')
  const selected = r.handlers.get('appIcon:selected')!
  const set = r.handlers.get('appIcon:set')!
  const bom = { sender: wc, senderFrame: wc.mainFrame }
  assert.equal(selected(bom), 'padrao')
  assert.equal(set(bom, 'icone-01-laranja-close'), 'icone-01-laranja-close')
  assert.equal(set(bom, '../resources/icon.png'), 'icone-01-laranja-close')
  assert.equal(selected({ sender: wc, senderFrame: { url: 'file:///outra.html' } }), 'padrao')
  assert.equal(set({ sender: wc, senderFrame: { url: 'file:///outra.html' } }, 'icone-04-verde-close'), 'padrao')
})
