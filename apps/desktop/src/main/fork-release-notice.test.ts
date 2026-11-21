import assert from 'node:assert/strict'
import { lstatSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import * as fs from 'node:fs'
import * as path from 'node:path'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { GITHUB_RELEASES_API_URL, RELEASE_TAG_URL_PREFIX, type ReleaseNoticeState } from '../shared/fork-release-notice.ts'
import { loadMainModule } from '../shared/test-support/main-module.ts'
import {
  createFileReleaseNoticeStore,
  createReleaseNoticeController,
  fetchLatestRelease,
  type ReleaseNoticeStore,
  type ReleaseNoticeStoreData,
} from './fork-release-notice-core.ts'

const TAG_URL = `${RELEASE_TAG_URL_PREFIX}v0.2.0`
const PAYLOAD_NOVO = JSON.stringify({ tag_name: 'v0.2.0', html_url: TAG_URL })
const PAYLOAD_IGUAL = JSON.stringify({ tag_name: 'v0.1.0', html_url: `${RELEASE_TAG_URL_PREFIX}v0.1.0` })

/** Resposta falsa: corpo lido só por `arrayBuffer`, com marca para provar que não foi lido. */
const resposta = (status: number, headers: Record<string, string> = {}, corpo = '') => {
  const marca = { lido: false }
  return {
    resposta: { status, headers: new Headers(headers), body: null, arrayBuffer: async () => { marca.lido = true; return new TextEncoder().encode(corpo).buffer } } as unknown as Response,
    marca,
  }
}

test('fetch: 200 com release nova devolve available; 404 devolve up-to-date', async () => {
  const sucesso = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => resposta(200, {}, PAYLOAD_NOVO).resposta })
  assert.deepEqual(sucesso, { ok: true, result: { kind: 'available', version: '0.2.0', tag: 'v0.2.0', url: TAG_URL } })
  const semRelease = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => resposta(404, {}, '').resposta })
  assert.deepEqual(semRelease, { ok: true, result: { kind: 'up-to-date' } })
  const igual = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => resposta(200, {}, PAYLOAD_IGUAL).resposta })
  assert.deepEqual(igual, { ok: true, result: { kind: 'up-to-date' } })
})

test('fetch: segue redirect permitido (api.github.com → github.com) e chega na resposta', async () => {
  const pedidas: string[] = []
  const fetcher = (async (url: string | URL | Request) => {
    const destino = String(url)
    pedidas.push(destino)
    if (destino === GITHUB_RELEASES_API_URL) {
      return new Response(null, { status: 302, headers: { location: 'https://api.github.com/repos/example/example/releases/99' } })
    }
    return resposta(200, {}, PAYLOAD_NOVO).resposta
  }) as typeof fetch
  const resultado = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher })
  assert.deepEqual(resultado, { ok: true, result: { kind: 'available', version: '0.2.0', tag: 'v0.2.0', url: TAG_URL } })
  assert.deepEqual(pedidas, [GITHUB_RELEASES_API_URL, 'https://api.github.com/repos/example/example/releases/99'])
})

test('fetch: redirect para fora dos hosts permitidos, HTTP ou acima do limite de 3 falha fechado', async () => {
  const praFora = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => new Response(null, { status: 302, headers: { location: 'https://evil.example.invalid/latest' } }) })
  assert.deepEqual(praFora, { ok: false })
  const praHttp = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => new Response(null, { status: 301, headers: { location: 'http://github.com/x' } }) })
  assert.deepEqual(praHttp, { ok: false })
  let saltos = 0
  const semFim = (async () => {
    saltos += 1
    return new Response(null, { status: 302, headers: { location: 'https://api.github.com/hop' + saltos } })
  }) as typeof fetch
  assert.deepEqual(await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: semFim }), { ok: false })
  assert.equal(saltos, 4, 'a quarta resposta de redirect já é recusada')
})

test('fetch: tempo esgotado derruba a busca (o sinal de aborto chega ao fetcher)', async () => {
  const fetcher = (async (_url: string | URL | Request, init?: RequestInit) => {
    return await new Promise<Response>((_resolve, reject) => {
      init?.signal?.addEventListener('abort', () => reject(new Error('timeout')), { once: true })
    })
  }) as typeof fetch
  assert.deepEqual(await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher, timeoutMs: 50 }), { ok: false })
})

test('fetch: o prazo vale para a busca inteira — o mesmo sinal corta o segundo salto', async () => {
  const fetcher = (async (url: string | URL | Request, init?: RequestInit) => {
    if (String(url) === GITHUB_RELEASES_API_URL) {
      return new Response(null, { status: 302, headers: { location: 'https://api.github.com/repos/example/example/releases/99' } })
    }
    return await new Promise<Response>((_resolve, reject) => {
      init?.signal?.addEventListener('abort', () => reject(new Error('timeout')), { once: true })
    })
  }) as typeof fetch
  assert.deepEqual(await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher, timeoutMs: 50 }), { ok: false })
})

test('fetch: 404 e redirect cancelam o corpo que não vai ser lido', async () => {
  const corpo = () => {
    const marca = { cancelado: false }
    return { marca, body: { cancel: async () => { marca.cancelado = true } } }
  }
  const semRelease = corpo()
  const quarentaEquatro = await fetchLatestRelease({
    currentVersion: 'v0.1.0',
    fetcher: async () => ({ status: 404, headers: new Headers(), body: semRelease.body, arrayBuffer: async () => new ArrayBuffer(0) }) as unknown as Response,
  })
  assert.deepEqual(quarentaEquatro, { ok: true, result: { kind: 'up-to-date' } })
  assert.equal(semRelease.marca.cancelado, true)

  const redirecionado = corpo()
  const praFora = await fetchLatestRelease({
    currentVersion: 'v0.1.0',
    fetcher: async () => ({ status: 302, headers: new Headers({ location: 'https://evil.example.invalid/x' }), body: redirecionado.body, arrayBuffer: async () => new ArrayBuffer(0) }) as unknown as Response,
  })
  assert.deepEqual(praFora, { ok: false })
  assert.equal(redirecionado.marca.cancelado, true)
})

test('fetch: corpo maior que o limite é recusado pelo Content-Length sem ler um byte', async () => {
  const grande = resposta(200, { 'content-length': String(262_144 + 1) }, PAYLOAD_NOVO)
  const resultado = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => grande.resposta })
  assert.deepEqual(resultado, { ok: false })
  assert.equal(grande.marca.lido, false)
})

test('fetch: corpo sem Content-Length maior que o limite é cortado no meio do stream', async () => {
  const fluxo = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(PAYLOAD_NOVO.slice(0, 100)))
      controller.enqueue(new TextEncoder().encode(PAYLOAD_NOVO.slice(100) + 'x'.repeat(200)))
      controller.close()
    },
  })
  const resultado = await fetchLatestRelease({ currentVersion: 'v0.1.0', maxBytes: 64, fetcher: async () => new Response(fluxo) })
  assert.deepEqual(resultado, { ok: false })
})

test('fetch: erro de rede, status inesperado e payload ilegível falham fechado', async () => {
  const rede = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => { throw new Error('sem rede') } })
  assert.deepEqual(rede, { ok: false })
  const status = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => resposta(500, {}, 'erro').resposta })
  assert.deepEqual(status, { ok: false })
  const lixo = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => resposta(200, {}, 'não é json').resposta })
  assert.deepEqual(lixo, { ok: false })
  const tagRuim = await fetchLatestRelease({ currentVersion: 'v0.1.0', fetcher: async () => resposta(200, {}, JSON.stringify({ tag_name: 'v0.2.0-beta.1', html_url: TAG_URL })).resposta })
  assert.deepEqual(tagRuim, { ok: false })
})

/** Store em memória para os testes do controlador. */
const storeDeMemoria = (inicial: ReleaseNoticeStoreData = { dismissedVersion: null, lastCheckAt: null, cachedRelease: null }): ReleaseNoticeStore => {
  let atual = inicial
  return { load: () => atual, save: (dados) => { atual = { ...dados } } }
}

const fetchDe = (respostas: string[]) => {
  let chamadas = 0
  return {
    chamadas: () => chamadas,
    fetcher: (async () => { chamadas += 1; return resposta(200, {}, respostas[Math.min(chamadas - 1, respostas.length - 1)]).resposta }) as typeof fetch,
  }
}

const fetchQueFalha = (async () => resposta(500, {}, 'erro').resposta) as typeof fetch

const controlador = (extras: Partial<Parameters<typeof createReleaseNoticeController>[0]> = {}) => createReleaseNoticeController({
  currentVersion: 'v0.1.0',
  store: storeDeMemoria(),
  openExternal: async () => true,
  ...extras,
})

test('checagem automática falha em silêncio: sem cache fica idle, com cache mantém available', async () => {
  const semCache = controlador({ fetcher: fetchQueFalha })
  await semCache.check({ manual: false })
  assert.equal(semCache.getState().status, 'idle')
  assert.equal(semCache.getState().availableVersion, null)

  const comCache = controlador({
    fetcher: fetchQueFalha,
    store: storeDeMemoria({ dismissedVersion: null, lastCheckAt: null, cachedRelease: { version: '0.2.0', tag: 'v0.2.0', url: TAG_URL } }),
  })
  assert.equal(comCache.getState().status, 'available')
  await comCache.check({ manual: false })
  assert.equal(comCache.getState().status, 'available', 'falha automática não apaga o cache válido')
})

test('checagem manual mostra erro; automática que já rodava é promovida quando chega uma manual', async () => {
  const manual = controlador({ fetcher: fetchQueFalha })
  await manual.check({ manual: true })
  assert.equal(manual.getState().status, 'error')

  let soltar: (() => void) | null = null
  const fetcher = (async () => {
    return await new Promise<Response>((resolve) => { soltar = () => resolve(resposta(500, {}, 'erro').resposta) })
  }) as typeof fetch
  const estados: string[] = []
  const mista = controlador({ fetcher, onChange: (s) => estados.push(s.status) })
  const automatica = mista.check({ manual: false })
  assert.deepEqual(estados, [], 'a automática não anuncia checking')
  const manualNoMeio = mista.check({ manual: true })
  assert.deepEqual(estados, ['checking'], 'a manual que chegou por cima anuncia checking')
  soltar!()
  await automatica
  const depois = await manualNoMeio
  assert.equal(depois.status, 'error', 'a manual que chegou por cima promove o tratamento de erro')
  assert.deepEqual(estados, ['checking', 'error'])
})

test('cooldown: automático respeita 6 h e manual 60 s, mas manual com erro retenta', async () => {
  let agora = 1_000_000
  const agoraFn = () => agora
  const falhas = fetchDe([PAYLOAD_NOVO])

  const automático = controlador({ fetcher: falhas.fetcher, now: agoraFn, store: storeDeMemoria({ dismissedVersion: null, lastCheckAt: agora - 1000, cachedRelease: null }) })
  await automático.check({ manual: false })
  assert.equal(falhas.chamadas(), 0, 'última checagem há 1 s: não bate na rede')

  const manual = controlador({ fetcher: falhas.fetcher, now: agoraFn })
  await manual.check({ manual: true })
  assert.equal(falhas.chamadas(), 1)
  agora += 5_000
  await manual.check({ manual: true })
  assert.equal(falhas.chamadas(), 1, '5 s depois, dentro dos 60 s: não bate de novo')
  assert.equal(manual.getState().status, 'available')

  const comErro = controlador({ fetcher: fetchQueFalha, now: agoraFn })
  await comErro.check({ manual: true })
  assert.equal(comErro.getState().status, 'error')
  agora += 5_000
  await comErro.check({ manual: true })
  assert.equal(comErro.getState().status, 'error', 'com erro, a manual dentro do cooldown ainda retenta na rede')
})

test('cooldown manual: dentro da janela, o status volta ao cache sem rede', async () => {
  let agora = 1_000_000
  const agoraFn = () => agora
  const rede = fetchDe([PAYLOAD_IGUAL])
  const manual = controlador({ fetcher: rede.fetcher, now: agoraFn })
  await manual.check({ manual: true })
  assert.equal(manual.getState().status, 'up-to-date')
  agora += 5_000
  await manual.check({ manual: true })
  assert.equal(rede.chamadas(), 1)
  assert.equal(manual.getState().status, 'up-to-date')
})

test('relógio do sistema recuado não prende o cooldown: automática e manual voltam à rede', async () => {
  let agora = 2_000_000
  const agoraFn = () => agora
  let chamadas = 0
  const fetcher = (async () => {
    chamadas += 1
    return chamadas === 1 ? resposta(500, {}, 'erro').resposta : resposta(200, {}, PAYLOAD_NOVO).resposta
  }) as typeof fetch
  const aviso = controlador({
    fetcher, now: agoraFn,
    store: storeDeMemoria({ dismissedVersion: null, lastCheckAt: agora + 500_000, cachedRelease: null }),
  })
  await aviso.check({ manual: false })
  assert.equal(chamadas, 1, 'lastCheckAt no futuro não bloqueia a checagem automática')
  assert.equal(aviso.getState().status, 'idle', 'falha automática segue silenciosa')
  await aviso.check({ manual: true })
  assert.equal(chamadas, 2, 'a manual consulta em vez de ficar presa no cooldown')
  assert.equal(aviso.getState().status, 'available')
})

test('checadas ao mesmo tempo são deduplicadas: uma só batida na rede', async () => {
  const rede = fetchDe([PAYLOAD_NOVO])
  const um = controlador({ fetcher: rede.fetcher })
  const [a, b] = await Promise.all([um.check({ manual: true }), um.check({ manual: true })])
  assert.equal(rede.chamadas(), 1)
  assert.deepEqual(a, b)
})

test('dispensa persiste por versão; versão maior volta a avisar; versão igual limpa o cache', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'release-notice-'))
  const arquivo = join(dir, 'release-notice.json')
  const relógio = { agora: 1_000_000 }
  const agoraFn = () => relógio.agora
  try {
    const v02 = fetchDe([PAYLOAD_NOVO])
    const aviso = controlador({ fetcher: v02.fetcher, store: createFileReleaseNoticeStore(arquivo), now: agoraFn })
    await aviso.check({ manual: true })
    assert.equal(aviso.getState().status, 'available')
    aviso.dismiss()
    assert.equal(aviso.getState().dismissedVersion, '0.2.0')

    const disco = JSON.parse(readFileSync(arquivo, 'utf8')) as ReleaseNoticeStoreData
    assert.deepEqual(disco.cachedRelease, { version: '0.2.0', tag: 'v0.2.0', url: TAG_URL })
    if (process.platform !== 'win32') {
      const meta = lstatSync(arquivo)
      assert.equal(meta.mode & 0o777, 0o600, 'arquivo regular 0600 no POSIX')
      assert.ok(meta.isFile())
    }

    const reaberto = controlador({ fetcher: fetchQueFalha, store: createFileReleaseNoticeStore(arquivo), now: agoraFn })
    assert.equal(reaberto.getState().availableVersion, '0.2.0', 'cache restaurado do disco')
    assert.equal(reaberto.getState().dismissedVersion, '0.2.0', 'dispensa restaurada do disco')

    relógio.agora += 100_000
    const v03 = fetchDe([JSON.stringify({ tag_name: 'v0.3.0', html_url: `${RELEASE_TAG_URL_PREFIX}v0.3.0` })])
    const proxima = controlador({ fetcher: v03.fetcher, store: createFileReleaseNoticeStore(arquivo), now: agoraFn })
    await proxima.check({ manual: true })
    assert.equal(proxima.getState().availableVersion, '0.3.0')
    assert.equal(proxima.getState().dismissedVersion, '0.2.0', 'a dispensa antiga não cala a versão nova')

    relógio.agora += 100_000
    const atualizada = controlador({ fetcher: fetchDe([PAYLOAD_IGUAL]).fetcher, store: createFileReleaseNoticeStore(arquivo), now: agoraFn })
    await atualizada.check({ manual: true })
    assert.equal(atualizada.getState().status, 'up-to-date')
    assert.equal(atualizada.getState().availableVersion, null)
    const noDisco = JSON.parse(readFileSync(arquivo, 'utf8')) as ReleaseNoticeStoreData
    assert.equal(noDisco.cachedRelease, null, 'versão já atual limpa o cache do disco')
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('arquivo de estado corrompido, com URL inválida ou substituído por symlink é recusado', () => {
  const dir = mkdtempSync(join(tmpdir(), 'release-notice-'))
  const arquivo = join(dir, 'release-notice.json')
  try {
    writeFileSync(arquivo, 'não é json')
    assert.deepEqual(createFileReleaseNoticeStore(arquivo).load(), { dismissedVersion: null, lastCheckAt: null, cachedRelease: null })

    writeFileSync(arquivo, JSON.stringify({ dismissedVersion: '0.2.0', lastCheckAt: 12, cachedRelease: { version: '0.2.0', tag: 'v0.2.0', url: 'https://evil.example.invalid/v0.2.0' } }))
    const lido = createFileReleaseNoticeStore(arquivo).load()
    assert.equal(lido.dismissedVersion, '0.2.0')
    assert.equal(lido.cachedRelease, null, 'URL que não passa na validação não volta do disco')

    // Symlink é coisa de POSIX: no Windows a criação exige privilégio e o teste fica pulado.
    if (process.platform !== 'win32') {
      const alvo = join(dir, 'alvo.json')
      writeFileSync(alvo, '{}')
      rmSync(arquivo)
      symlinkSync(alvo, arquivo)
      assert.deepEqual(createFileReleaseNoticeStore(arquivo).load(), { dismissedVersion: null, lastCheckAt: null, cachedRelease: null }, 'leitura não segue symlink')
      assert.throws(() => createFileReleaseNoticeStore(arquivo).save({ dismissedVersion: null, lastCheckAt: null, cachedRelease: null }), 'escrita não segue symlink')
    }
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('cache do disco só conta se ainda for mais novo que a versão atual', () => {
  const antigo = controlador({
    fetcher: fetchQueFalha,
    store: storeDeMemoria({ dismissedVersion: null, lastCheckAt: null, cachedRelease: { version: '0.0.9', tag: 'v0.0.9', url: `${RELEASE_TAG_URL_PREFIX}v0.0.9` } }),
  })
  assert.equal(antigo.getState().availableVersion, null)
  assert.equal(antigo.getState().status, 'idle')
})

test('openDownload: abre só URL revalidada; sem URL ou inválida, não chama o navegador', async () => {
  const abertas: string[] = []
  const abre = async (url: string) => { abertas.push(url); return true }
  const comRelease = controlador({
    fetcher: fetchDe([PAYLOAD_NOVO]).fetcher,
    openExternal: abre,
  })
  await comRelease.check({ manual: true })
  assert.equal(await comRelease.openDownload(), true)
  assert.deepEqual(abertas, [TAG_URL])

  const semRelease = controlador({ openExternal: abre })
  assert.equal(await semRelease.openDownload(), false)
  const quebrado = controlador({
    store: storeDeMemoria({ dismissedVersion: null, lastCheckAt: null, cachedRelease: { version: '0.2.0', tag: 'v0.2.0', url: 'https://evil.example.invalid/v0.2.0' } }),
    openExternal: abre,
  })
  assert.equal(await quebrado.openDownload(), false)
  const queLanca = controlador({ fetcher: fetchDe([PAYLOAD_NOVO]).fetcher, openExternal: async () => { throw new Error('sem navegador') } })
  await queLanca.check({ manual: true })
  assert.equal(await queLanca.openDownload(), false)
  assert.deepEqual(abertas, [TAG_URL])
})

test('start agenda a checagem automática e stop cancela; dentro do cooldown não agenda', async () => {
  const agendadas: { callback: () => void; atraso: number }[] = []
  const setTimer = (callback: () => void, atraso: number) => { agendadas.push({ callback, atraso }); return agendadas.length }
  const clearTimer = (handle: unknown) => { if (typeof handle === 'number') agendadas.splice(handle - 1, 1) }
  const rede = fetchDe([PAYLOAD_NOVO])

  const aviso = controlador({ fetcher: rede.fetcher, setTimer, clearTimer, autoDelayMs: 50 })
  aviso.start()
  assert.equal(agendadas.length, 1)
  assert.equal(agendadas[0]!.atraso, 50)
  aviso.stop()
  assert.equal(agendadas.length, 0)

  const recente = controlador({
    fetcher: rede.fetcher, setTimer, clearTimer,
    store: storeDeMemoria({ dismissedVersion: null, lastCheckAt: Date.now() - 1000, cachedRelease: null }),
  })
  recente.start()
  assert.equal(agendadas.length, 0, 'checado há 1 s: nada a agendar')
})

test('cache do disco rejeita versão/tag adulteradas e normaliza a dispensa para o canônico', () => {
  const dir = mkdtempSync(join(tmpdir(), 'release-notice-'))
  const arquivo = join(dir, 'release-notice.json')
  try {
    const comCache = (mudanca: Record<string, unknown>) =>
      JSON.stringify({ dismissedVersion: null, lastCheckAt: 12, cachedRelease: { version: '0.2.0', tag: 'v0.2.0', url: TAG_URL, ...mudanca } })
    writeFileSync(arquivo, comCache({ tag: 'v9.9.9' }))
    assert.equal(createFileReleaseNoticeStore(arquivo).load().cachedRelease, null, 'tag que não casa com a versão')
    writeFileSync(arquivo, comCache({ version: 'v0.2.0' }))
    assert.equal(createFileReleaseNoticeStore(arquivo).load().cachedRelease, null, 'versão não canônica')
    writeFileSync(arquivo, comCache({ url: `${RELEASE_TAG_URL_PREFIX}v0.3.0` }))
    assert.equal(createFileReleaseNoticeStore(arquivo).load().cachedRelease, null, 'URL que aponta para outra versão')

    writeFileSync(arquivo, JSON.stringify({ dismissedVersion: 'v0.2.0', lastCheckAt: null, cachedRelease: null }))
    assert.equal(createFileReleaseNoticeStore(arquivo).load().dismissedVersion, '0.2.0', 'dispensa normalizada para o canônico')
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('adaptador: remetente negado não recebe o estado real e nenhum handler age', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'release-notice-'))
  try {
    writeFileSync(join(dir, 'release-notice.json'), JSON.stringify({ dismissedVersion: null, lastCheckAt: null, cachedRelease: { version: '0.2.0', tag: 'v0.2.0', url: TAG_URL } }))
    const handlers = new Map<string, (event: unknown) => unknown>()
    const ouvidas: string[] = []
    const abertas: string[] = []
    const enviadas: unknown[] = []
    const avisos: unknown[][] = []
    const janela = { isDestroyed: () => false, webContents: { send: (_canal: string, estado: unknown) => { enviadas.push(estado) } } }
    const modulo = loadMainModule<{ registerReleaseNotice: (options: { windows: () => unknown[]; userDataDir: string; currentVersion: string; allowed: (event: unknown) => boolean }) => void }>('fork-release-notice', {
      electron: {
        app: { on: (evento: string) => { ouvidas.push(evento) } },
        ipcMain: { handle: (canal: string, handler: (event: unknown) => unknown) => { handlers.set(canal, handler) } },
        shell: { openExternal: async (url: string) => { abertas.push(url) } },
      },
      'node:fs': fs, 'node:path': path,
    }, { process: { platform: 'linux' }, setTimeout: () => 0, clearTimeout: () => {}, URL, console: { log() {}, error() {}, warn: (...args: unknown[]) => { avisos.push(args) } } })
    modulo.registerReleaseNotice({ windows: () => [janela], userDataDir: dir, currentVersion: 'v0.1.0', allowed: (event) => (event as { ok: boolean }).ok })
    assert.deepEqual([...handlers.keys()].sort(), ['releaseNotice:check', 'releaseNotice:dismiss', 'releaseNotice:openDownload', 'releaseNotice:state'])
    assert.deepEqual(ouvidas, ['before-quit'])

    const negado = { ok: false }
    const permitido = { ok: true }
    const estadoNegado = handlers.get('releaseNotice:state')!(negado) as ReleaseNoticeState
    assert.equal(estadoNegado.supported, false, 'negado não recebe o estado real')
    assert.equal(estadoNegado.availableVersion, null)
    assert.equal(estadoNegado.currentVersion, 'v0.1.0')
    const estadoReal = handlers.get('releaseNotice:state')!(permitido) as ReleaseNoticeState
    assert.equal(estadoReal.supported, true)
    assert.equal(estadoReal.availableVersion, '0.2.0', 'o cache do disco voltou')

    const checkNegado = await handlers.get('releaseNotice:check')!(negado) as ReleaseNoticeState
    assert.equal(checkNegado.supported, false, 'check negado devolve o estado negado, sem checar nada')
    assert.equal(checkNegado.availableVersion, null)
    assert.equal(checkNegado.status, 'idle')
    const dispensaNegada = await handlers.get('releaseNotice:dismiss')!(negado) as ReleaseNoticeState
    assert.equal(dispensaNegada.supported, false, 'dispensa negada devolve o estado negado')
    assert.equal(dispensaNegada.dismissedVersion, null)
    const depoisDaNegacao = handlers.get('releaseNotice:state')!(permitido) as ReleaseNoticeState
    assert.equal(depoisDaNegacao.dismissedVersion, null, 'nenhuma dispensa foi gravada')
    assert.equal(depoisDaNegacao.availableVersion, '0.2.0')

    assert.equal(await handlers.get('releaseNotice:openDownload')!(negado), false)
    assert.deepEqual(abertas, [], 'negado não abre nada')
    assert.equal(await handlers.get('releaseNotice:openDownload')!(permitido), true)
    assert.deepEqual(abertas, [TAG_URL])

    assert.equal(avisos.length, 4, 'cada canal negado registra o motivo uma vez')
    assert.deepEqual(avisos.map(([, detalhe]) => (detalhe as { channel: string }).channel), ['state', 'check', 'dismiss', 'openDownload'])
    for (const args of avisos) {
      assert.equal(args.length, 2, 'só a mensagem e o canal; nada do remetente nem do evento')
      assert.equal(args[0], '[release-notice] denied: unauthorized sender')
      assert.deepEqual(Object.keys(args[1] as object), ['channel'])
    }
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})
