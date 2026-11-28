import assert from 'node:assert/strict'
import { request } from 'node:http'
import { createServer } from 'node:net'
import { networkInterfaces } from 'node:os'
import { test } from 'node:test'
import { CAMINHO, CAMINHO_AUTH, LoginEncerrado, PORTA_DEV, PORTA_PRINCIPAL, redirectDoApp, RetornoDoLogin, lerPedido, type Opcoes } from './fork-oauth-loopback.ts'

const pedido = (porta: number, extra = '') => `https://login.exemplo.test/authorize?response_type=code&client_id=c&redirect_uri=${encodeURIComponent(redirectDoApp(porta))}&state=estado-1${extra}`

/** Um app de mentira: porta efêmera, navegador de mentira que só registra a URL aberta. */
function montar(extra: Partial<Opcoes> = {}) {
  const abertas: string[] = []
  const retorno = new RetornoDoLogin({ porta: 0, abrir: async (u) => { abertas.push(u); return true }, idioma: () => 'pt-BR', ...extra })
  return { retorno, abertas }
}

const chamar = (porta: number, caminho: string, opcoes: { host?: string; metodo?: string } = {}) =>
  new Promise<{ status: number; corpo: string; cabecalhos: Record<string, unknown> }>((resolve, reject) => {
    const req = request({ host: '127.0.0.1', port: porta, path: caminho, method: opcoes.metodo ?? 'GET', headers: { Host: opcoes.host ?? `127.0.0.1:${porta}` } }, (res) => {
      let corpo = ''
      res.on('data', (d: Buffer) => (corpo += d))
      res.on('end', () => resolve({ status: res.statusCode ?? 0, corpo, cabecalhos: res.headers }))
    })
    req.on('error', reject)
    req.end()
  })

const recusada = (porta: number) => new Promise<boolean>((resolve) => {
  const req = request({ host: '127.0.0.1', port: porta, path: '/', timeout: 1500 }, () => resolve(false))
  req.on('error', () => resolve(true))
  req.end()
})

test('as portas do app são as duas da lista do Hub e nunca as do Hub nem do Codync', () => {
  assert.deepEqual([PORTA_PRINCIPAL, PORTA_DEV], [19424, 19425])
  assert.equal(CAMINHO, '/oauth/callback')
})

test('o caminho exigido pelo login do ChatGPT é o segundo que o ouvinte atende', () => {
  assert.equal(CAMINHO_AUTH, '/auth/callback')
})

test('o pedido do Hub só vale com redirect_uri exato do app, https e um único state', () => {
  assert.deepEqual(lerPedido(pedido(19424), 19424), { state: 'estado-1' })
  assert.deepEqual(lerPedido(`http://127.0.0.1:9/authorize?redirect_uri=${encodeURIComponent(redirectDoApp(19424))}&state=s`, 19424), { state: 's' })
  assert.equal(lerPedido(pedido(19424), 19425), null, 'porta diferente')
  assert.equal(lerPedido(pedido(19424, '&redirect_uri=http%3A%2F%2Foutro'), 19424), null, 'redirect repetido')
  assert.equal(lerPedido(pedido(19424, '&state=outro'), 19424), null, 'state repetido')
  assert.equal(lerPedido('https://x.test/a?redirect_uri=http%3A%2F%2F127.0.0.1%3A19422%2Foauth%2Fcallback&state=s', 19424), null, 'porta do Hub')
  assert.equal(lerPedido(`https://x.test/a?redirect_uri=${encodeURIComponent(redirectDoApp(19424))}`, 19424), null, 'sem state')
  assert.equal(lerPedido(`https://x.test/a?redirect_uri=${encodeURIComponent(redirectDoApp(19424))}&state=`, 19424), null, 'state vazio')
  assert.equal(lerPedido(pedido(19424).replace('https://', 'http://'), 19424), null, 'http fora do loopback')
  assert.equal(lerPedido(pedido(19424).replace('https://', 'https://u:p@'), 19424), null, 'credencial na URL')
  assert.equal(lerPedido(`${pedido(19424)}#frag`, 19424), null, 'fragmento')
  assert.equal(lerPedido('javascript:alert(1)', 19424), null)
  assert.equal(lerPedido(null, 19424), null)
  assert.equal(lerPedido(42, 19424), null)
})

test('o pedido do Hub também vale com callback_url (OpenRouter), sozinho e exato do app; com redirect_uri junto não passa', () => {
  const auth = (porta: number, extra = '') => `https://openrouter.ai/auth?callback_url=${encodeURIComponent(redirectDoApp(porta))}&state=estado-1${extra}`
  assert.deepEqual(lerPedido(auth(19424), 19424), { state: 'estado-1' }, 'callback_url aceito no lugar do redirect_uri')
  assert.equal(lerPedido(auth(19424), 19425), null, 'porta diferente')
  assert.equal(lerPedido(auth(19424, `&redirect_uri=${encodeURIComponent(redirectDoApp(19424))}`), 19424), null, 'os dois juntos recusam')
  assert.equal(lerPedido(auth(19424).replace('https://openrouter.ai', 'https://falso.exemplo.test'), 19424), null, 'callback_url de outra origem não passa')
  assert.equal(lerPedido(auth(19424).replace('https://openrouter.ai', 'https://openrouter.ai:8443'), 19424), null, 'porta alternativa na origem não passa')
  // O redirect_uri comum (OAuth de conector do mercado) segue aceito em qualquer fornecedor https.
  assert.deepEqual(lerPedido(`https://fornecedor.exemplo.test/authorize?redirect_uri=${encodeURIComponent(redirectDoApp(19424))}&state=s`, 19424), { state: 's' })
  assert.equal(lerPedido(auth(19424, `&callback_url=${encodeURIComponent(redirectDoApp(19424))}`), 19424), null, 'callback repetido')
  assert.equal(lerPedido(`https://x.test/a?callback_url=${encodeURIComponent(redirectDoApp(19425))}&state=s`, 19424), null, 'callback de outra porta do app')
  assert.equal(lerPedido(auth(19424, '&state=outro'), 19424), null, 'state repetido')
  assert.equal(lerPedido(`https://x.test/a?callback_url=${encodeURIComponent(redirectDoApp(19424))}`, 19424), null, 'sem state')
  assert.equal(lerPedido(`https://x.test/a?callback_url=http%3A%2F%2Fatacante.test%2Foauth%2Fcallback&state=s`, 19424), null, 'callback que não é o do app')
})

test('o pedido pode pedir o caminho /auth/callback (login do ChatGPT), exato na porta do app; outro caminho, host ou esquema recusa', () => {
  const auth = (porta: number, extra = '') => `https://auth.exemplo.test/authorize?redirect_uri=${encodeURIComponent(redirectDoApp(porta, CAMINHO_AUTH))}&state=estado-1${extra}`
  assert.deepEqual(lerPedido(auth(19424), 19424), { state: 'estado-1' }, 'caminho do ChatGPT aceito no redirect_uri')
  assert.deepEqual(lerPedido(auth(19425), 19425), { state: 'estado-1' }, 'e na outra porta do app')
  // O `callback_url` é só do login do OpenRouter (origem oficial): o retorno do ChatGPT vem por `redirect_uri`.
  assert.equal(lerPedido(`https://auth.exemplo.test/authorize?callback_url=${encodeURIComponent(redirectDoApp(19424, CAMINHO_AUTH))}&state=estado-1`, 19424), null, 'callback_url de outra origem não passa')
  assert.equal(lerPedido(auth(19424), 19425), null, 'porta diferente')
  assert.equal(lerPedido(`https://x.test/a?redirect_uri=${encodeURIComponent(`http://127.0.0.1:19424${CAMINHO_AUTH}`)}&redirect_uri=${encodeURIComponent(redirectDoApp(19424, CAMINHO_AUTH))}&state=s`, 19424), null, 'redirect repetido')
  assert.equal(lerPedido(`https://x.test/a?redirect_uri=${encodeURIComponent(`http://localhost:19424${CAMINHO_AUTH}`)}&state=s`, 19424), null, 'host localhost não é o do app')
  assert.equal(lerPedido(`https://x.test/a?redirect_uri=${encodeURIComponent(`https://127.0.0.1:19424${CAMINHO_AUTH}`)}&state=s`, 19424), null, 'esquema https no retorno não é o do app')
  for (const ruim of ['/callback', '/auth/callback/', '/auth/callbackx', '/oauth/callback/auth/callback']) {
    assert.equal(lerPedido(`https://x.test/a?redirect_uri=${encodeURIComponent(`http://127.0.0.1:19424${ruim}`)}&state=s`, 19424), null, `caminho ${ruim} não é um dos dois do app`)
  }
  assert.equal(lerPedido(`https://x.test/a?redirect_uri=${encodeURIComponent(redirectDoApp(19424, CAMINHO_AUTH))}&state=`, 19424), null, 'state vazio')
  assert.equal(lerPedido(auth(19424).replace('https://', 'http://'), 19424), null, 'http fora do loopback')
})

test('o navegador tem de voltar pelo caminho que o pedido pediu; o outro caminho do app não passa', async () => {
  const { retorno } = montar()
  const { porta, tentativa: t } = (await retorno.abrir())!
  const pedidoAuth = `https://auth.exemplo.test/authorize?redirect_uri=${encodeURIComponent(redirectDoApp(porta, CAMINHO_AUTH))}&state=estado-1`
  const voltou = retorno.esperar(t, pedidoAuth)
  // O caminho comum do app também é atendido, mas não para este pedido.
  assert.equal((await chamar(porta, `${CAMINHO}?state=estado-1&code=x`)).status, 404)
  const certo = chamar(porta, `${CAMINHO_AUTH}?state=estado-1&code=x`)
  assert.match(await voltou, /code=x/)
  retorno.responder(t, { ok: true, nome: 'ChatGPT' })
  assert.equal((await certo).status, 200)

  const { retorno: outro } = montar()
  const { porta: p, tentativa: t2 } = (await outro.abrir())!
  const pedidoComum = `https://login.exemplo.test/authorize?redirect_uri=${encodeURIComponent(redirectDoApp(p))}&state=estado-1`
  const espera = outro.esperar(t2, pedidoComum)
  espera.catch(() => {})
  assert.equal((await chamar(p, `${CAMINHO_AUTH}?state=estado-1&code=x`)).status, 404, 'pedido no caminho comum não aceita retorno no do ChatGPT')
  outro.encerrar(t2)
  await assert.rejects(espera, LoginEncerrado)
})

test('retorno certo: chega ao app, o navegador espera o desfecho e vê a página em português; depois o ouvinte some', async () => {
  const { retorno, abertas } = montar()
  const aberto = await retorno.abrir()
  assert.ok(aberto && aberto.porta > 0)
  const { porta, tentativa } = aberto!
  const url = pedido(porta)
  const voltou = retorno.esperar(tentativa, url)
  assert.deepEqual(abertas, [url])
  const pagina = chamar(porta, `${CAMINHO}?state=estado-1&code=abc&iss=https%3A%2F%2Femissor.test`)
  assert.equal(await voltou, `http://127.0.0.1:${porta}${CAMINHO}?state=estado-1&code=abc&iss=https%3A%2F%2Femissor.test`)
  retorno.responder(tentativa, { ok: true, nome: 'Linear' })
  const r = await pagina
  assert.equal(r.status, 200)
  assert.match(r.corpo, /Linear conectado/)
  assert.match(r.corpo, /Pode fechar esta aba e voltar para a Mexerica\./)
  assert.match(r.corpo, /lang="pt-BR"/)
  assert.match(String(r.cabecalhos['content-security-policy']), /default-src 'none'/)
  assert.equal(r.cabecalhos['cache-control'], 'no-store')
  assert.equal(r.cabecalhos['referrer-policy'], 'no-referrer')
  assert.ok(await recusada(porta), 'depois do desfecho a porta fecha')
})

test('a página de falha mostra a frase recusada, escapada, e o retorno de erro do fornecedor também chega ao app', async () => {
  const { retorno } = montar()
  const { porta, tentativa: t } = (await retorno.abrir())!
  const voltou = retorno.esperar(t, pedido(porta))
  const pagina = chamar(porta, `${CAMINHO}?state=estado-1&error=access_denied`)
  assert.match(await voltou, /error=access_denied/)
  retorno.responder(t, { ok: false, detalhe: '<script>alert(1)</script> & "x"' })
  const r = await pagina
  assert.match(r.corpo, /O login não foi concluído/)
  assert.match(r.corpo, /&lt;script&gt;alert\(1\)&lt;\/script&gt; &amp; &quot;x&quot;/)
  assert.doesNotMatch(r.corpo, /<script>/)
})

test('state de outra tentativa, repetido ou ausente não passa e o login segue esperando o certo', async () => {
  const { retorno } = montar()
  const { porta, tentativa: t } = (await retorno.abrir())!
  const voltou = retorno.esperar(t, pedido(porta))
  for (const q of ['state=outro&code=x', 'code=x', 'state=estado-1&state=estado-1&code=x', 'state=&code=x']) {
    const r = await chamar(porta, `${CAMINHO}?${q}`)
    assert.equal(r.status, 400, q)
    assert.match(r.corpo, /Este não é o login em andamento/)
  }
  let resolvido = false
  void voltou.then(() => { resolvido = true })
  await new Promise((r) => setTimeout(r, 30))
  assert.equal(resolvido, false)
  const certo = chamar(porta, `${CAMINHO}?state=estado-1&code=ok`)
  assert.match(await voltou, /code=ok/)
  retorno.responder(t, { ok: true, nome: 'X' })
  assert.equal((await certo).status, 200)
})

test('só GET no caminho certo e no endereço exato do ouvinte; Host de outro nome (DNS rebinding) é recusado', async () => {
  const { retorno } = montar()
  const { porta, tentativa: t } = (await retorno.abrir())!
  const voltou = retorno.esperar(t, pedido(porta))
  voltou.catch(() => {})
  assert.equal((await chamar(porta, `${CAMINHO}?state=estado-1&code=x`, { host: 'atacante.test' })).status, 404)
  assert.equal((await chamar(porta, `${CAMINHO}?state=estado-1&code=x`, { host: `127.0.0.1:${porta + 1}` })).status, 404)
  assert.equal((await chamar(porta, `${CAMINHO}?state=estado-1&code=x`, { metodo: 'POST' })).status, 404)
  assert.equal((await chamar(porta, `/outro?state=estado-1&code=x`)).status, 404)
  assert.equal((await chamar(porta, `/`)).status, 404)
  retorno.encerrar(t)
  await assert.rejects(voltou, LoginEncerrado)
})

test('sem login em andamento o ouvinte aberto não aceita nada, e um pedido que não é do app fecha tudo', async () => {
  const { retorno, abertas } = montar()
  const { porta, tentativa: t } = (await retorno.abrir())!
  assert.equal((await chamar(porta, `${CAMINHO}?state=estado-1&code=x`)).status, 404)
  await assert.rejects(retorno.esperar(t, pedido(porta + 1)), /não é o que este app espera/)
  assert.deepEqual(abertas, [], 'o navegador nem chega a abrir')
  assert.ok(await recusada(porta), 'pedido recusado desliga o ouvinte')
  await assert.rejects(retorno.esperar(t, pedido(porta)), /não é o que este app espera/, 'sem ouvinte ligado não há espera')
})

test('porta ocupada: não abre e devolve null (o login segue pelo caminho de antes)', async () => {
  const ocupante = createServer()
  const porta = await new Promise<number>((r) => ocupante.listen(0, '127.0.0.1', () => r((ocupante.address() as { port: number }).port)))
  try {
    const { retorno } = montar({ porta })
    assert.equal(await retorno.abrir(), null)
  } finally {
    ocupante.close()
  }
})

test('o ouvinte só existe em 127.0.0.1: nenhum outro endereço do computador atende', async () => {
  const { retorno } = montar()
  const { porta, tentativa: t } = (await retorno.abrir())!
  try {
    const externos = Object.values(networkInterfaces()).flat().filter((i) => i && i.family === 'IPv4' && !i.internal)
    for (const i of externos) {
      const falhou = await new Promise<boolean>((resolve) => {
        const req = request({ host: i!.address, port: porta, path: '/', timeout: 1500 }, () => resolve(false))
        req.on('error', () => resolve(true))
        req.on('timeout', () => { req.destroy(); resolve(true) })
        req.end()
      })
      assert.ok(falhou, `${i!.address} não pode atender`)
    }
  } finally {
    retorno.encerrar(t)
  }
})

test('sem retorno a tempo: o login termina como "tempo" e o ouvinte fecha; um segundo login cancela o primeiro', async () => {
  const lento = montar({ esperaMs: 40 })
  const { porta, tentativa: t } = (await lento.retorno.abrir())!
  await assert.rejects(lento.retorno.esperar(t, pedido(porta)), (e: unknown) => e instanceof LoginEncerrado && e.motivo === 'tempo')
  assert.ok(await recusada(porta))

  const { retorno } = montar()
  const { porta: p, tentativa: t1 } = (await retorno.abrir())!
  const primeiro = retorno.esperar(t1, pedido(p))
  primeiro.catch(() => {})
  const segundo = retorno.esperar(t1, pedido(p).replace('estado-1', 'estado-2'))
  await assert.rejects(primeiro, (e: unknown) => e instanceof LoginEncerrado && e.motivo === 'cancelado')
  const pagina = chamar(p, `${CAMINHO}?state=estado-2&code=z`)
  assert.match(await segundo, /state=estado-2/)
  retorno.responder(t1, { ok: true, nome: 'Y' })
  await pagina
})

test('retorno recebido mas o app não responde: o navegador não fica pendurado', async () => {
  const { retorno } = montar({ respostaMs: 40 })
  const { porta, tentativa: t } = (await retorno.abrir())!
  const voltou = retorno.esperar(t, pedido(porta))
  const pagina = chamar(porta, `${CAMINHO}?state=estado-1&code=x`)
  await voltou
  const r = await pagina
  assert.match(r.corpo, /Retorno recebido/)
  assert.ok(await recusada(porta))
})

test('não abre o navegador: o login falha e o ouvinte fecha', async () => {
  const { retorno } = montar({ abrir: async () => false })
  const { porta, tentativa: t } = (await retorno.abrir())!
  await assert.rejects(retorno.esperar(t, pedido(porta)), /Não foi possível abrir/)
  assert.ok(await recusada(porta))
})

test('os textos saem no idioma do app (inglês: a mesma página da Mexerica, não a do Hub)', async () => {
  const { retorno } = montar({ idioma: () => 'en' })
  const { porta, tentativa: t } = (await retorno.abrir())!
  const voltou = retorno.esperar(t, pedido(porta))
  const pagina = chamar(porta, `${CAMINHO}?state=estado-1&code=x`)
  await voltou
  retorno.responder(t, { ok: true, nome: 'Linear' })
  const r = await pagina
  assert.match(r.corpo, /Linear is connected/)
  assert.match(r.corpo, /lang="en"/)
})

test('duas tentativas seguidas: a antiga nunca desliga a nova (o fim de uma não fecha o ouvinte da outra)', async () => {
  const { retorno } = montar()
  const [a, b] = await Promise.all([retorno.abrir(), retorno.abrir()])
  const atual = b ?? a
  assert.ok(atual)
  if (a && b) assert.notEqual(a.tentativa, b.tentativa)
  // A tentativa velha, se chegou a abrir, foi cancelada e o fim dela não mexe na atual.
  await assert.rejects(retorno.esperar((atual!.tentativa) - 1, pedido(atual!.porta)), LoginEncerrado)
  retorno.encerrar(atual!.tentativa - 1)
  const voltou = retorno.esperar(atual!.tentativa, pedido(atual!.porta))
  const pagina = chamar(atual!.porta, `${CAMINHO}?state=estado-1&code=z`)
  assert.match(await voltou, /code=z/)
  retorno.responder(atual!.tentativa - 1, { ok: false, detalhe: 'de outra tentativa' })
  retorno.responder(atual!.tentativa, { ok: true, nome: 'Z' })
  assert.match((await pagina).corpo, /Z conectado/)
})

test('abrir de novo com um login esperando cancela a espera antiga, e o encerrar antigo não derruba o ouvinte novo', async () => {
  const { retorno } = montar()
  const velho = (await retorno.abrir())!
  const esperaVelha = retorno.esperar(velho.tentativa, pedido(velho.porta))
  esperaVelha.catch(() => {})
  const novo = (await retorno.abrir())!
  await assert.rejects(esperaVelha, (e: unknown) => e instanceof LoginEncerrado && e.motivo === 'cancelado')
  // O que o renderer da tentativa velha faz ao ver o cancelamento:
  retorno.encerrar(velho.tentativa)
  const espera = retorno.esperar(novo.tentativa, pedido(novo.porta))
  const pagina = chamar(novo.porta, `${CAMINHO}?state=estado-1&code=n`)
  assert.match(await espera, /code=n/)
  retorno.responder(novo.tentativa, { ok: true, nome: 'N' })
  assert.equal((await pagina).status, 200)
})

test('com autorização do Hub, só abre a URL que o Hub devolveu para a tentativa; outra URL do renderer é recusada sem abrir nada', async () => {
  let plano: string | null = null
  const { retorno, abertas } = montar({ autorizar: (url) => { const igual = plano === url; plano = null; return igual } })
  const ouvinte = (await retorno.abrir())!
  const doHub = pedido(ouvinte.porta)
  plano = doHub
  const outra = pedido(ouvinte.porta, '&extra=1')
  await assert.rejects(retorno.esperar(ouvinte.tentativa, outra), /não é o que este app espera/)
  assert.deepEqual(abertas, [])
  assert.equal(await recusada(ouvinte.porta), true, 'a recusa fecha o ouvinte')
})

test('o ouvinte entrega à autorização o identificador da tentativa viva e avisa quando deixa de valer', async () => {
  const vistos: Array<string | null> = []
  const fechados: Array<string | null> = []
  const { retorno } = montar({ autorizar: (_url, nonce) => { vistos.push(nonce); return false }, aoFechar: (nonce) => fechados.push(nonce) })
  const ouvinte = (await retorno.abrir('tentativa-do-hub-0001'))!
  assert.equal(retorno.nonceVivo(), 'tentativa-do-hub-0001')
  await assert.rejects(retorno.esperar(ouvinte.tentativa, pedido(ouvinte.porta)))
  assert.deepEqual(vistos, ['tentativa-do-hub-0001'])
  assert.equal(retorno.nonceVivo(), null, 'a recusa fecha o ouvinte e a tentativa deixa de valer')
  const outro = (await retorno.abrir('tentativa-do-hub-0002'))!
  assert.equal(retorno.nonceVivo(), 'tentativa-do-hub-0002')
  retorno.encerrar(outro.tentativa)
  assert.equal(retorno.nonceVivo(), null)
  assert.ok(fechados.includes('tentativa-do-hub-0001') && fechados.includes('tentativa-do-hub-0002'))
})

test('com autorização do Hub, a URL do Hub abre uma única vez', async () => {
  let plano: string | null = null
  const { retorno, abertas } = montar({ autorizar: (url) => { const igual = plano === url; plano = null; return igual } })
  const ouvinte = (await retorno.abrir())!
  const doHub = pedido(ouvinte.porta)
  plano = doHub
  const espera = retorno.esperar(ouvinte.tentativa, doHub)
  assert.deepEqual(abertas, [doHub])
  retorno.fechar()
  await assert.rejects(espera, LoginEncerrado)
  const novo = (await retorno.abrir())!
  await assert.rejects(retorno.esperar(novo.tentativa, doHub), /não é o que este app espera/)
  assert.deepEqual(abertas, [doHub], 'a mesma URL não abre de novo')
})

