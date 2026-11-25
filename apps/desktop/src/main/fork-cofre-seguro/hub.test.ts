import assert from 'node:assert/strict'
import test from 'node:test'
import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto'
import { hubProva, TOKEN_PROVA } from '../../shared/test-support/hub-prova.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
import type { CapturaHub, FontesDeHub } from './hub.ts'

const { chamarHub, resolverHub } = loadMainModule<typeof import('./hub.ts')>('fork-cofre-seguro/hub', {
  'node:crypto': { createHmac, randomBytes, timingSafeEqual },
}, { URL, Buffer })

const LOCAL = { computerId: 'pc-1', baseURL: 'http://127.0.0.1:1', token: 'tk' }
const captura = (o: Partial<CapturaHub> & { baseURL?: string; token?: string } = {}): CapturaHub => ({
  baseURL: 'http://127.0.0.1:1', token: 'tk', current: () => true, listener: async () => true, ...o,
})

const fontes = (o: { local?: typeof LOCAL | null; ssh?: ((computerId: string, current: () => boolean) => CapturaHub | null) | null } = {}): FontesDeHub => ({
  local: () => (o.local === undefined ? LOCAL : o.local),
  ssh: o.ssh === undefined
    ? (computerId: string) => computerId === 'pc-ssh' ? captura({ baseURL: 'http://127.0.0.1:2', token: 'ts' }) : null
    : (o.ssh ?? (() => null)),
})

test('resolve só o Hub local e o túnel SSH em loopback; o resto falha fechado', () => {
  const viva = () => true
  assert.equal(resolverHub('pc-1', fontes(), viva)?.baseURL, 'http://127.0.0.1:1')
  assert.equal(resolverHub('pc-1', fontes(), viva)?.token, 'tk')
  assert.equal(resolverHub('pc-ssh', fontes(), viva)?.baseURL, 'http://127.0.0.1:2')
  assert.equal(resolverHub('nuvem', fontes(), viva), null)
  assert.equal(resolverHub('pc-1', fontes({ local: null }), viva), null)
  assert.equal(resolverHub('pc-1', fontes({ local: { computerId: 'pc-1', baseURL: 'https://hub.example.invalid', token: 'x' } }), viva), null)
  assert.equal(resolverHub('pc-ssh', fontes({ ssh: () => null }), viva), null)
})

test('a captura local morre com a janela, com a troca do host local ou com a saída do loopback', () => {
  let vivo = true
  let local: typeof LOCAL | null = LOCAL
  const c = resolverHub('pc-1', { local: () => local, ssh: () => null }, () => vivo)!
  assert.equal(c.current(), true)
  vivo = false
  assert.equal(c.current(), false, 'janela fechada invalida')
  vivo = true
  local = { ...LOCAL, token: 'outro' }
  assert.equal(c.current(), false, 'host trocado invalida')
  local = { ...LOCAL, baseURL: 'https://example.invalid' }
  assert.equal(c.current(), false, 'fora do loopback invalida')
})

const SENHA = 'senha-ficticia-do-teste'
const falha = (status: number) => (erro: unknown) => JSON.stringify(erro) === JSON.stringify({ status })

test('chamarHub: bearer e valor só saem pelo socket que provou o token', async (t) => {
  const recebidos: string[] = []
  const hub = await hubProva((request, response) => {
    assert.equal(request.method, 'POST')
    let corpo = ''
    request.setEncoding('utf8').on('data', (parte) => { corpo += parte })
    request.on('end', () => {
      recebidos.push(corpo)
      response.end('{}')
    })
    return true
  })
  t.after(() => hub.close())
  const viva = captura({ baseURL: hub.url(), token: TOKEN_PROVA })
  for (const morta of [{ ...viva, current: () => false }, { ...viva, listener: async () => false }]) {
    await assert.rejects(chamarHub(morta, 'm', { value: SENHA }), falha(0))
  }
  assert.equal(hub.seen.length, 0)

  await assert.rejects(chamarHub({ ...viva, token: 'token-sem-prova' }, 'm', { value: SENHA }), falha(0))
  assert.equal(hub.seen.length, 1)
  assert.match(hub.seen[0]!.url, /^\/health\?/)
  assert.equal(hub.seen[0]!.authorization, undefined)
  assert.deepEqual(recebidos, [])

  assert.equal(JSON.stringify(await chamarHub(viva, 'connectorRequestFinish', { value: SENHA })), '{}')
  assert.deepEqual(hub.seen.slice(1).map((pedido) => pedido.authorization), [undefined, `Bearer ${TOKEN_PROVA}`])
  assert.deepEqual(recebidos, [JSON.stringify({ value: SENHA })])
  assert.deepEqual(hub.violacoes, [])
})

test('chamarHub: captura invalidada durante a espera do listener não envia nada', async (t) => {
  const hub = await hubProva(() => false)
  t.after(() => hub.close())
  let liberar!: (v: boolean) => void
  const espera = new Promise<boolean>((r) => { liberar = r })
  let vivo = true
  const c = captura({ baseURL: hub.url(), token: TOKEN_PROVA, current: () => vivo, listener: () => espera })
  const p = chamarHub(c, 'connectorRequestFinish', { value: SENHA })
  vivo = false
  liberar(true)
  await assert.rejects(p, falha(0))
  assert.deepEqual(hub.seen, [])
})

test('chamarHub: 307/308 não gera segundo envio; o corpo nunca segue adiante', async (t) => {
  const hub = await hubProva((request, response) => {
    response.writeHead(Number(request.url?.split('/').at(-1)), { Location: '/nao-seguir' }).end()
    return true
  })
  t.after(() => hub.close())
  const viva = captura({ baseURL: hub.url(), token: TOKEN_PROVA })
  for (const status of [307, 308]) {
    await assert.rejects(chamarHub(viva, String(status), { value: SENHA }), falha(status))
  }
  assert.deepEqual(hub.seen.filter((r) => !r.url.startsWith('/health')).map((r) => r.url), ['/api/307', '/api/308'])
  assert.deepEqual(hub.violacoes, [])
})

test('chamarHub: captura que morre durante o envio falha fechado; erros não levam corpo nem token', async (t) => {
  let vivo = true
  const hub = await hubProva((request, response) => {
    if (request.url === '/api/morre') {
      vivo = false
      response.end('{}')
    } else if (request.url === '/api/rede') {
      response.destroy()
    } else {
      response.writeHead(500).end(JSON.stringify({ error: SENHA }))
    }
    return true
  })
  t.after(() => hub.close())
  const destino = captura({ baseURL: hub.url(), token: TOKEN_PROVA })
  await assert.rejects(chamarHub({ ...destino, current: () => vivo }, 'morre', { value: SENHA }), falha(0))
  await assert.rejects(chamarHub(destino, 'negado', { value: SENHA }), falha(500))
  await assert.rejects(chamarHub(destino, 'rede', { value: SENHA }), falha(0))
})
