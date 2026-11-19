// Fronteira de segurança: o que o main process deixa o renderer abrir fora do app.
// Roda o main/navigation.ts real, com electron simulado.
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'

type Handler = (...args: any[]) => any

function fixture(platform = 'linux') {
  const opened: string[] = []
  const warnings: unknown[][] = []
  let falhaAoAbrir = false
  const appHandlers = new Map<string, Handler>()
  const electron = {
    app: { on: (name: string, fn: Handler) => appHandlers.set(name, fn) },
    shell: { openExternal: async (url: string) => { if (falhaAoAbrir) throw new Error(`falha ao abrir ${url}`); opened.push(url) } },
  }
  const mod = loadMainModule<typeof import('../main/navigation.ts')>('navigation', { electron }, {
    URL,
    process: { platform },
    console: { warn: (...a: unknown[]) => warnings.push(a), log() {}, error() {} },
  })
  return { mod, opened, warnings, appHandlers, falhar: () => { falhaAoAbrir = true } }
}

test('openExternalURL só abre http, https e mailto', async () => {
  const f = fixture()
  for (const ok of ['https://example.test/a?b=1', 'http://example.test/', 'mailto:alguem@example.test']) {
    assert.equal(await f.mod.openExternalURL(ok), true, ok)
  }
  assert.deepEqual(f.opened, ['https://example.test/a?b=1', 'http://example.test/', 'mailto:alguem@example.test'])
})

test('openExternalURL recusa esquemas perigosos e entrada que não é texto, sem chamar o SO', async () => {
  const f = fixture()
  const ruins: unknown[] = [
    'javascript:alert(1)', 'file:///etc/passwd', 'data:text/html,<script>1</script>', 'vbscript:x', 'ssh://host/', 'smb://host/share',
    'x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture', 'não é url', '', '//example.test/', 'https:',
    null, undefined, 42, {}, ['https://example.test/'], { href: 'https://example.test/' },
  ]
  for (const ruim of ruins) assert.equal(await f.mod.openExternalURL(ruim), false, String(ruim))
  assert.deepEqual(f.opened, [])
})

test('openExternalURL: falha do SO vira false e o log só guarda o protocolo (nunca o endereço)', async () => {
  const f = fixture()
  f.falhar()
  assert.equal(await f.mod.openExternalURL('https://user:senha@example.test/segredo?token=abc'), false)
  const texto = JSON.stringify(f.warnings)
  assert.ok(!/senha|segredo|token|example\.test/.test(texto), texto)
  assert.ok(texto.includes('https:'))
})

test('um URL recusado também não vaza endereço no log', async () => {
  const f = fixture()
  await f.mod.openExternalURL('file:///var/dados/confidencial-xyz.txt')
  assert.ok(!/confidencial|dados/.test(JSON.stringify(f.warnings)))
})

test('openSystemSettingsURL aceita só os painéis fixos e recusa qualquer outro', async () => {
  const f = fixture()
  const painel = 'x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture'
  assert.equal(await f.mod.openSystemSettingsURL(painel), true)
  for (const ruim of [
    'x-apple.systempreferences:com.apple.preference.security?Privacy_Camera',
    painel + '&extra=1', painel.toUpperCase(), 'https://example.test/', 'file:///', 'x-apple.systempreferences:', null, 7,
  ]) assert.equal(await f.mod.openSystemSettingsURL(ruim), false, String(ruim))
  assert.deepEqual(f.opened, [painel])
})

test('o painel de teclado do Spotlight só é aceito no macOS', async () => {
  const { spotlightKeyboardSettings } = await import('./fork-berga-shortcut.ts')
  assert.equal(await fixture('darwin').mod.openSystemSettingsURL(spotlightKeyboardSettings), true)
  assert.equal(await fixture('linux').mod.openSystemSettingsURL(spotlightKeyboardSettings), false)
})

function janela(f: ReturnType<typeof fixture>) {
  const ouvintes = new Map<string, Handler>()
  let abertura: Handler | undefined
  const contents = {
    getType: () => 'window',
    on: (n: string, fn: Handler) => ouvintes.set(n, fn),
    setWindowOpenHandler: (fn: Handler) => { abertura = fn },
  }
  f.mod.registerWindowNavigation('https://app.example.test/index.html')
  f.appHandlers.get('web-contents-created')!({}, contents)
  return { contents, ouvintes, abertura: () => abertura! }
}

test('navegação do app para fora é cancelada e vai para o navegador; a própria página do app passa', async () => {
  const f = fixture()
  const j = janela(f)
  const evento = (url: string) => { const e = { url, prevented: false, preventDefault() { e.prevented = true } }; return e }
  const dentro = evento('https://app.example.test/index.html?aba=2#x')
  j.ouvintes.get('will-navigate')!(dentro)
  assert.equal(dentro.prevented, false)
  const fora = evento('https://outro.example.test/')
  j.ouvintes.get('will-redirect')!(fora)
  assert.equal(fora.prevented, true)
  const perigoso = evento('javascript:alert(1)')
  j.ouvintes.get('will-navigate')!(perigoso)
  assert.equal(perigoso.prevented, true)
  await Promise.resolve()
  assert.deepEqual(f.opened, ['https://outro.example.test/'])
})

test('página do app com credenciais no URL não é tratada como a página do app', () => {
  const f = fixture()
  const j = janela(f)
  const e = { url: 'https://u:p@app.example.test/index.html', prevented: false, preventDefault() { e.prevented = true } }
  j.ouvintes.get('will-navigate')!(e)
  assert.equal(e.prevented, true)
})

test('window.open é sempre negado; só endereço externo vai para o navegador', async () => {
  const f = fixture()
  const j = janela(f)
  assert.equal(j.abertura()({ url: 'https://example.test/popup' }).action, 'deny')
  assert.equal(j.abertura()({ url: 'file:///etc/passwd' }).action, 'deny')
  await Promise.resolve()
  assert.deepEqual(f.opened, ['https://example.test/popup'])
})

test('janela filha criada por brecha é destruída e o endereço vai ao navegador', async () => {
  const f = fixture()
  const j = janela(f)
  const protegidos: Handler[] = []
  let destruida = false
  const filho = { webContents: { getType: () => 'window', on: (_n: string, fn: Handler) => protegidos.push(fn), setWindowOpenHandler() {} }, destroy: () => { destruida = true } }
  j.ouvintes.get('did-create-window')!(filho, { url: 'https://example.test/filha' })
  assert.equal(destruida, true)
  assert.ok(protegidos.length >= 3, 'a filha também recebe os mesmos guardas')
  await Promise.resolve()
  assert.deepEqual(f.opened, ['https://example.test/filha'])
})

test('só contents do tipo window são protegidos e nunca em duplicidade', () => {
  const f = fixture()
  f.mod.registerWindowNavigation('https://app.example.test/index.html')
  let ouvintes = 0
  const contents = (tipo: string) => ({ getType: () => tipo, on: () => { ouvintes++ }, setWindowOpenHandler() {} })
  f.appHandlers.get('web-contents-created')!({}, contents('webview'))
  assert.equal(ouvintes, 0)
  const c = contents('window')
  f.appHandlers.get('web-contents-created')!({}, c)
  const depois = ouvintes
  f.appHandlers.get('web-contents-created')!({}, c)
  assert.equal(ouvintes, depois)
})
