import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { createWebBridge, WEB_UNAVAILABLE } from './platform.ts'
import type { CodyncBridge } from '../shared/ipc.ts'

test('createWebBridge marca web e carrega os padrões sem nativo', async () => {
  const ponte = createWebBridge()
  assert.equal(ponte.web, true)
  assert.equal(ponte.appScheme, 'codync')
  assert.equal(ponte.hostPort, 19422)
  assert.equal(ponte.computerName, 'Navegador')
  // O app web local sobrepõe tudo isso pelas partes.
  const sobrescrita = createWebBridge({ hostPort: 19423, computerName: 'Casa', appScheme: 'donnabot' })
  assert.equal(sobrescrita.hostPort, 19423)
  assert.equal(sobrescrita.computerName, 'Casa')
  assert.equal(sobrescrita.appScheme, 'donnabot')
})

test('sem partes, o host é o stub desabilitado e os arquivos recusam', async () => {
  const ponte = createWebBridge()
  const instantaneo = await ponte.host.snapshot()
  assert.equal(instantaneo.state.kind, 'disabled')
  assert.equal(instantaneo.local, null)
  await assert.rejects(ponte.host.call('http://127.0.0.1:1', '', 'bots.list', {}, 1000), (e: Error) => e.message === WEB_UNAVAILABLE)
  await assert.rejects(ponte.files.begin({ id: 'f', name: 'a.pdf', size: 1, sha256: 'a'.repeat(64) }), (e: Error) => e.message === WEB_UNAVAILABLE)
})

test('partes de host e files entram na ponte', async () => {
  const host: CodyncBridge['host'] = {
    snapshot: async () => ({ state: { kind: 'running' }, local: null, dev: false, logPath: '' }),
    onChange: () => () => {},
    install() {},
    restart() {},
    call: async () => ({ ok: true }),
    stream: () => () => {},
  }
  let salvo = ''
  const files: CodyncBridge['files'] = {
    begin: async () => 'id-1',
    write: async (_id, _offset, dados) => { salvo = String(dados.length) },
    finish: async () => {},
    cancel: async () => {},
  }
  const ponte = createWebBridge({ host, files })
  assert.equal((await ponte.host.snapshot()).state.kind, 'running')
  assert.deepEqual(await ponte.host.call('', '', 'x', {}, 1), { ok: true })
  await ponte.files.write('id-1', 0, new Uint8Array([1, 2]))
  assert.equal(salvo, '2')
})

test('degradação explícita dos recursos nativos: nada finge suporte', async () => {
  const ponte = createWebBridge()
  assert.equal(ponte.island.supported, false)
  assert.equal(await ponte.island.send('b', 'oi', undefined, ''), 'sem-confirmacao')
  assert.equal(ponte.speech.available, false)
  assert.deepEqual(await ponte.speech.locales(), [])
  assert.deepEqual(await ponte.forkCofre.abrir({ computerId: 'c1', entryId: 'e1' }), { ok: false, motivo: 'semSuporte' })
  assert.deepEqual(await ponte.forkCofre.abrirNovo({ computerId: 'c1' } as never), { ok: false, motivo: 'semSuporte' })
  assert.equal((await ponte.updates.state()).supported, false)
  assert.deepEqual(await ponte.app.screenAgentState(), { available: false, needsApproval: false, registered: false })
  assert.deepEqual(await ponte.app.syncScreenAgent(true), { needsApproval: false })
  // No-ops de bandeja e janela não lançam.
  ponte.app.setTraySummary({} as never)
  ponte.app.setWindowControls({} as never)
  ponte.app.openMemoryFile('/qualquer/caminho')
  ponte.host.install()
  ponte.host.restart()
})

test('openExternal é fail-closed: só https, sem opener, e o aviso nunca traz a URL', async () => {
  const abertas: unknown[] = []
  const avisos: string[] = []
  const janela = { open: (...args: unknown[]) => { abertas.push(args); return null } }
  const globalAntes = (globalThis as Record<string, unknown>).window
  const avisoAntes = console.warn
  ;(globalThis as Record<string, unknown>).window = janela
  console.warn = (...mensagens: unknown[]) => { avisos.push(mensagens.join(' ')) }
  try {
    const ponte = createWebBridge()
    ponte.app.openExternal('https://exemplo.invalid/pagina')
    assert.equal(abertas.length, 1)
    assert.deepEqual(abertas[0], ['https://exemplo.invalid/pagina', '_blank', 'noopener,noreferrer'])

    for (const url of ['http://exemplo.invalid/', 'javascript:alert(1)', 'ftp://exemplo.invalid/', 'isto não é URL']) {
      ponte.app.openExternal(url)
    }
    // Só o https passou; os avisos são genéricos — a URL recusada não vai para o console.
    assert.equal(abertas.length, 1)
    assert.ok(avisos.length > 0)
    for (const aviso of avisos) assert.ok(!aviso.includes('exemplo.invalid'), aviso)
  } finally {
    ;(globalThis as Record<string, unknown>).window = globalAntes
    console.warn = avisoAntes
  }
})

test('copy na web tenta a área de transferência e engole a falha', () => {
  const ponte = createWebBridge()
  assert.doesNotThrow(() => ponte.app.copy('texto'))
})

test('o index.html da web carrega CSP sem script inline, sem unsafe-eval e com connect-src só da origem', () => {
  const html = readFileSync(join(import.meta.dirname, 'index.html'), 'utf8')
  const csp = /<meta http-equiv="Content-Security-Policy" content="([^"]+)" \/>/.exec(html)?.[1]
  assert.ok(csp, 'CSP ausente')
  const diretiva = (nome: string) => new RegExp(`${nome}[^;]*`).exec(csp!)?.[0] ?? ''
  assert.ok(diretiva('script-src').includes("'self'"), 'script-src precisa de self')
  assert.ok(!diretiva('script-src').includes('unsafe-inline'), 'script-src não pode ter unsafe-inline')
  assert.ok(!diretiva('script-src').includes('unsafe-eval'), 'script-src não pode ter unsafe-eval')
  assert.ok(!diretiva('script-src').includes('https:'), 'script-src não pode abrir https:')
  assert.ok(diretiva('connect-src').includes("'self'"), 'connect-src precisa de self')
  assert.ok(!diretiva('connect-src').includes('https:'), 'connect-src não pode abrir https:')
  assert.ok(diretiva('object-src').includes("'none'"), 'object-src precisa de none')
  assert.ok(diretiva('base-uri').includes("'none'"), 'base-uri precisa de none')
  // Sem script inline (nem hash): só módulos com src da própria origem.
  assert.equal(/<script(?![^>]*\bsrc=)[^>]*>/i.test(html), false, 'script inline no HTML')
  assert.ok(html.includes('<script type="module" src="./main.ts"></script>'))
  // Manifest PWA e cor do chão Creme.
  assert.ok(html.includes('<link rel="manifest" href="/manifest.webmanifest" />'))
  assert.ok(html.includes('<meta name="theme-color" content="#fcfaf6" />'))
  for (const favicon of ['favicon.ico', 'favicon-16.png', 'favicon-32.png', 'favicon-192.png', 'favicon-180.png']) {
    assert.ok(html.includes(`assets/brand/favicons/${favicon}`), favicon)
  }
})
