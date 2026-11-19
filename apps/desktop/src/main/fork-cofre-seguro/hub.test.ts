import assert from 'node:assert/strict'
import test from 'node:test'
import { chamarHub, resolverHub, type CapturaHub, type FontesDeHub } from './hub.ts'

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
  assert.equal(resolverHub('pc-1', fontes({ local: { computerId: 'pc-1', baseURL: 'https://hub.exemplo.com', token: 'x' } }), viva), null)
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
  local = { ...LOCAL, baseURL: 'https://exemplo.com' }
  assert.equal(c.current(), false, 'fora do loopback invalida')
})

test('chamarHub: guarda de posse e validade imediatamente antes do POST; redirect:error', async () => {
  const SENHA = 'senha-ficticia-do-teste'
  const inits: RequestInit[] = []
  const http = (async (_u: any, i: any) => { inits.push(i); return new Response('{}') }) as unknown as typeof fetch
  // Sem captura viva ou sem posse do listener: nenhum envio.
  for (const morta of [captura({ current: () => false }), captura({ listener: async () => false })]) {
    await assert.rejects(chamarHub(morta, 'm', { value: SENHA }, http), (e) => JSON.stringify(e) === '{"status":0}')
  }
  assert.equal(inits.length, 0)
  // Viva: um POST só, com redirect:'error' e o Bearer da captura.
  const ok = await chamarHub(captura(), 'connectorRequestFinish', { value: SENHA }, http)
  assert.deepEqual(ok, {})
  assert.equal(inits.length, 1)
  assert.equal(inits[0]!.redirect, 'error')
  assert.equal((inits[0]!.headers as Record<string, string>).Authorization, 'Bearer tk')
})

test('chamarHub: captura invalidada DURANTE a espera do listener não envia nada', async () => {
  const SENHA = 'senha-ficticia-do-teste'
  let liberar!: (v: boolean) => void
  const espera = new Promise<boolean>((r) => { liberar = r })
  let vivo = true
  let envios = 0
  const http = (async () => { envios++; return new Response('{}') }) as unknown as typeof fetch
  const c = captura({ current: () => vivo, listener: () => espera })
  const p = chamarHub(c, 'connectorRequestFinish', { value: SENHA }, http)
  // current() inicial é true e o listener fica pendente (lsof/proc); a janela fecha / o túnel cai aqui.
  vivo = false
  liberar(true) // o listener responde true sobre o que observou — não vale mais
  await assert.rejects(p, (e) => JSON.stringify(e) === '{"status":0}')
  assert.equal(envios, 0, 'current() reconferido depois do await: zero envios')
})

test('chamarHub: 307/308 não gera segundo envio; o corpo nunca segue adiante', async () => {
  const SENHA = 'senha-ficticia-do-teste'
  let chamadas = 0
  // Nem origem externa nem outro listener local recebem o corpo: o transporte recusa redirect
  // (redirect:'error' já provado acima; aqui a resposta de redirect é um erro, sem reenvio).
  for (const status of [307, 308]) {
    const http = (async () => { chamadas++; return new Response(null, { status }) }) as unknown as typeof fetch
    await assert.rejects(chamarHub(captura(), 'm', { value: SENHA }, http), (e) => (e as { status: number }).status === status)
  }
  assert.equal(chamadas, 2, 'uma tentativa por chamada; nunca um segundo POST')
})

test('chamarHub: captura que morre durante o envio falha fechado; erros não levam corpo nem token', async () => {
  const SENHA = 'senha-ficticia-do-teste'
  let vivo = true
  const morrendo = captura({ current: () => vivo })
  const http = (async () => { vivo = false; return new Response('{}') }) as unknown as typeof fetch
  await assert.rejects(chamarHub(morrendo, 'm', { value: SENHA }, http), (e) => JSON.stringify(e) === '{"status":0}')
  const destino = captura()
  await assert.rejects(chamarHub(destino, 'm', { value: SENHA }, async () => new Response(`{"error":"${SENHA}"}`, { status: 500 })), (e) => JSON.stringify(e) === '{"status":500}')
  await assert.rejects(chamarHub(destino, 'm', { value: SENHA }, async () => { throw new Error(`falha ${SENHA} tk`) }), (e) => JSON.stringify(e) === '{"status":0}')
})
