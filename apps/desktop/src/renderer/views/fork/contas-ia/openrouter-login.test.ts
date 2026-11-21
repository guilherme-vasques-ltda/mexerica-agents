import assert from 'node:assert/strict'
import { test } from 'node:test'
import { CATALOGOS } from '../../../lib/fork-i18n/index.ts'
import type { HostClient } from '../../../client/host-client'
import { entrarComOpenRouter, fraseDaFalha, FalhaOpenRouter, type PonteDoOuvinte } from './openrouter-login.ts'
import type { Acesso } from './api-direta.ts'

const URL_DO_HUB = 'https://openrouter.ai/auth?callback_url=http%3A%2F%2F127.0.0.1%3A19424%2Foauth%2Fcallback&code_challenge=desafio&code_challenge_method=S256&state=estado-1&key_label=Mexerica'

/** Ouvinte de mentira: registra tudo, deixa o teste controlar o retorno do navegador e corta a espera quando desliga. */
function ponte(volta: () => Promise<string>) {
  const registro: string[] = []
  let recusarPendente: ((motivo: Error) => void) | null = null
  let avisarEspera: (() => void) | null = null
  const esperou = new Promise<void>((resolve) => { avisarEspera = resolve })
  const ouvinte: PonteDoOuvinte = {
    abrir: async () => ({ porta: 19424, tentativa: 7 }),
    esperar: (tentativa, url) => {
      registro.push(`esperar:${tentativa}:${url}`)
      avisarEspera?.()
      return new Promise<string>((resolve, reject) => {
        recusarPendente = reject
        volta().then(resolve, reject)
      })
    },
    responder: (tentativa, resultado) => registro.push(`responder:${tentativa}:${resultado.ok}:${resultado.nome ?? ''}`),
    fechar: (tentativa) => {
      registro.push(`fechar:${tentativa}`)
      recusarPendente?.(new Error('ouvinte desligado'))
      recusarPendente = null
    },
  }
  return { ouvinte, registro, esperou }
}

/** Hub de mentira: `openrouterEntrar` devolve a URL; `openrouterEntrarConcluir` devolve o `Acesso` e registra o corpo. */
function hub(responderConcluir: (corpo: unknown) => Acesso) {
  const corpos: { metodo: string; corpo: unknown }[] = []
  const client = {
    call: async (metodo: string, corpo: unknown) => {
      corpos.push({ metodo, corpo })
      if (metodo === 'openrouterEntrar') return { url: URL_DO_HUB }
      if (metodo === 'openrouterEntrarConcluir') return responderConcluir(corpo)
      throw new Error(`método inesperado: ${metodo}`)
    },
  } as unknown as HostClient
  return { client, corpos }
}

test('o login inteiro: pede a URL ao Hub com a porta do ouvinte, entrega o retorno e devolve o Acesso', async () => {
  const { client, corpos } = hub(() => ({ ok: true, modelos: [{ id: 'x/a', name: 'A' }] }))
  const { ouvinte, registro } = ponte(() => Promise.resolve('http://127.0.0.1:19424/oauth/callback?state=estado-1&code=codigo-ficticio'))
  const { fim } = entrarComOpenRouter(client, ouvinte)
  assert.deepEqual(await fim, { ok: true, modelos: [{ id: 'x/a', name: 'A' }] })
  assert.deepEqual(corpos, [
    { metodo: 'openrouterEntrar', corpo: { loopbackPort: 19424 } },
    { metodo: 'openrouterEntrarConcluir', corpo: { state: 'estado-1', code: 'codigo-ficticio' } },
  ])
  assert.ok(registro[0] === 'esperar:7:' + URL_DO_HUB, 'o ouvinte recebe a URL do Hub com a tentativa certa')
  assert.ok(registro.includes('responder:7:true:OpenRouter'), 'o navegador vê o desfecho de sucesso')
  assert.ok(registro.includes('fechar:7'), 'o ouvinte desliga no fim')
})

test('o retorno com erro do fornecedor vai ao Hub como {state, erro} e o Acesso recusado segue ao navegador', async () => {
  let recebido: unknown
  const { client } = hub((corpo) => { recebido = corpo; return { ok: false, erro: 'fornecedor_falhou' } })
  const { ouvinte, registro } = ponte(() => Promise.resolve('http://127.0.0.1:19424/oauth/callback?state=estado-1&error=access_denied'))
  const { fim } = entrarComOpenRouter(client, ouvinte)
  assert.deepEqual(await fim, { ok: false, erro: 'fornecedor_falhou' })
  assert.deepEqual(recebido, { state: 'estado-1', erro: 'access_denied' })
  assert.ok(registro.includes('responder:7:false:'), 'sem nome de conector no desfecho de erro')
})

test('o retorno que já estava em trânsito quando o Cancelar foi clicado não conclui o login', async () => {
  let cancelar: () => void = () => {}
  const corpos: unknown[] = []
  const client = {
    call: async (metodo: string, corpo: unknown) => {
      corpos.push(corpo)
      if (metodo === 'openrouterEntrar') return { url: URL_DO_HUB }
      throw new Error(`inesperado: ${metodo}`)
    },
  } as unknown as HostClient
  const { ouvinte, registro } = ponte(() => { cancelar(); return Promise.resolve('http://127.0.0.1:19424/oauth/callback?state=estado-1&code=codigo-ficticio') })
  const entrada = entrarComOpenRouter(client, ouvinte)
  cancelar = entrada.cancelar
  const desfecho = entrada.fim.then(() => null, (e: unknown) => e)
  const falha = await desfecho
  assert.ok(falha instanceof FalhaOpenRouter && falha.tipo === 'cancelado')
  assert.equal(corpos.length, 1, 'o Hub não recebe a conclusão depois do cancelamento')
  assert.ok(!registro.some((r) => r.startsWith('responder:7:')), 'o navegador não recebe desfecho de sucesso')
})

test('cancelar desliga o ouvinte e a espera termina como cancelado', async () => {
  const { client } = hub(() => ({ ok: true, modelos: [] }))
  const { ouvinte, registro, esperou } = ponte(() => new Promise<string>(() => {}))
  const { fim, cancelar } = entrarComOpenRouter(client, ouvinte)
  const desfecho = fim.catch((e: unknown) => e)
  await esperou
  cancelar()
  assert.ok(registro.includes('fechar:7'), 'o cancelamento encerra o ouvinte da tentativa certa')
  const falha = await desfecho
  assert.ok(falha instanceof FalhaOpenRouter)
  assert.equal(falha.tipo, 'cancelado')
})

test('sem retorno a tempo a espera termina como tempo e desliga o ouvinte', async () => {
  const { client } = hub(() => ({ ok: true, modelos: [] }))
  const { ouvinte, registro } = ponte(() => new Promise<string>(() => {}))
  const { fim } = entrarComOpenRouter(client, ouvinte, 20)
  const falha = await fim.then(() => null, (e: unknown) => e)
  assert.ok(falha instanceof FalhaOpenRouter && falha.tipo === 'tempo')
  assert.ok(registro.includes('fechar:7'))
})

test('sem porta livre para o ouvinte, ou Hub que não responde, a falha é a genérica', async () => {
  const { client, corpos } = hub(() => ({ ok: true, modelos: [] }))
  const semOuvinte: PonteDoOuvinte = {
    abrir: async () => null, esperar: () => Promise.reject(new Error('nunca')),
    responder() {}, fechar() {},
  }
  const { fim } = entrarComOpenRouter(client, semOuvinte)
  assert.equal(await fim.then(() => 'ok', (e: unknown) => (e as FalhaOpenRouter).tipo), 'falha')
  assert.deepEqual(corpos, [], 'sem ouvinte o Hub nem é chamado')

  const hubFora: HostClient = { call: async () => { throw new Error('sem resposta') } } as unknown as HostClient
  const { ouvinte, registro } = ponte(() => new Promise<string>(() => {}))
  const { fim: fim2 } = entrarComOpenRouter(hubFora as HostClient, ouvinte)
  assert.equal(await fim2.then(() => 'ok', (e: unknown) => (e as FalhaOpenRouter).tipo), 'falha')
  assert.ok(registro.includes('fechar:7'), 'o ouvinte não fica aberto quando o Hub falhou')
})

test('o Hub que falha ao concluir vira a falha genérica, sem deixar o navegador sem desfecho', async () => {
  const client = {
    call: async (metodo: string) => {
      if (metodo === 'openrouterEntrar') return { url: URL_DO_HUB }
      throw new Error('falha ao concluir')
    },
  } as unknown as HostClient
  const { ouvinte, registro } = ponte(() => Promise.resolve('http://127.0.0.1:19424/oauth/callback?state=estado-1&code=xyz'))
  const { fim } = entrarComOpenRouter(client, ouvinte)
  assert.equal(await fim.then(() => 'ok', (e: unknown) => (e as FalhaOpenRouter).tipo), 'falha')
  assert.ok(registro.includes('fechar:7'))
})

test('cada falha do login tem frase nos seis idiomas', () => {
  for (const tipo of ['cancelado', 'tempo', 'falha'] as const) {
    const chave = fraseDaFalha(tipo)
    for (const catalogo of Object.values(CATALOGOS)) assert.ok(catalogo[chave], `${tipo} sem texto`)
  }
})

test('o Hub respondendo «entrada negada» vira login cancelado, sem culpar o fornecedor', async () => {
  const { client } = hub(() => ({ ok: false, erro: 'entrada_negada' }) as unknown as Acesso)
  const { ouvinte } = ponte(() => Promise.resolve('http://127.0.0.1:19424/oauth/callback?state=estado-1&error=access_denied'))
  const { fim } = entrarComOpenRouter(client, ouvinte)
  await assert.rejects(fim, (e: unknown) => e instanceof FalhaOpenRouter && e.tipo === 'cancelado')
})

test('depois que o navegador voltou, cancelar não faz mais nada: o Hub já está gravando a chave', async () => {
  let liberar: (a: Acesso) => void = () => {}
  const gravando = new Promise<Acesso>((resolve) => { liberar = resolve })
  const chamadas: string[] = []
  const client = {
    call: async (metodo: string) => {
      chamadas.push(metodo)
      return metodo === 'openrouterEntrar' ? { url: URL_DO_HUB } : gravando
    },
  } as unknown as HostClient
  const { ouvinte, registro } = ponte(() => Promise.resolve('http://127.0.0.1:19424/oauth/callback?state=estado-1&code=codigo-ficticio'))
  let concluindo = 0
  const { fim, cancelar } = entrarComOpenRouter(client, ouvinte, undefined, () => { concluindo += 1 })
  while (!chamadas.includes('openrouterEntrarConcluir')) await new Promise((r) => setTimeout(r, 5))
  cancelar()
  liberar({ ok: true, modelos: [] })
  assert.deepEqual(await fim, { ok: true, modelos: [] })
  assert.equal(concluindo, 1)
  assert.ok(!registro.some((linha) => linha === 'fechar:7') || registro.indexOf('fechar:7') > registro.indexOf('responder:7:true:OpenRouter'), 'o ouvinte só fecha depois do desfecho')
})
