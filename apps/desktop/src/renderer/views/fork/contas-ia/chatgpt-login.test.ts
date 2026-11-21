import assert from 'node:assert/strict'
import { test } from 'node:test'
import { CATALOGOS } from '../../../lib/fork-i18n/index.ts'
import type { HostClient } from '../../../client/host-client'
import { entrarComChatGPT, fraseDaFalha, FalhaChatGPT, lerEstadoChatGPT, sairDoChatGPT, type EstadoChatGPT, type PonteDoOuvinte } from './chatgpt-login.ts'

const URL_DO_HUB = 'https://auth.exemplo.invalid/api/accounts/authorize?client_id=cliente-ficticio&redirect_uri=http%3A%2F%2F127.0.0.1%3A19424%2Fauth%2Fcallback&response_type=code&scope=openid&state=estado-1'

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

/** Hub de mentira: `chatgptEntrar` devolve a URL; `chatgptEntrarConcluir` devolve o desfecho e registra o corpo. */
function hub(responderConcluir: (corpo: unknown) => { ok: boolean; conta?: { rotulo: string | null; planoHabilitado: boolean }; erro?: string; avisoAnteriorSemConfirmacao?: boolean }) {
  const corpos: { metodo: string; corpo: unknown }[] = []
  const client = {
    call: async (metodo: string, corpo: unknown) => {
      corpos.push({ metodo, corpo })
      if (metodo === 'chatgptEntrar') return { url: URL_DO_HUB }
      if (metodo === 'chatgptEntrarConcluir') return responderConcluir(corpo)
      throw new Error(`método inesperado: ${metodo}`)
    },
  } as unknown as HostClient
  return { client, corpos }
}

const VOLTA_OK = 'http://127.0.0.1:19424/auth/callback?state=estado-1&code=codigo-ficticio'

test('conta anterior encerrada sem confirmação do emissor: o desfecho carrega o aviso', async () => {
  const { client } = hub(() => ({ ok: true, conta: { rotulo: 'pessoa@example.invalid', planoHabilitado: true }, avisoAnteriorSemConfirmacao: true }))
  const { ouvinte } = ponte(() => Promise.resolve(VOLTA_OK))
  const { fim } = entrarComChatGPT(client, ouvinte, false, 'pessoal-1', true)
  assert.deepEqual(await fim, { ok: true, rotulo: 'pessoa@example.invalid', planoHabilitado: true, avisoAnteriorSemConfirmacao: true })
})

test('o login inteiro: pede a URL ao Hub com a porta do ouvinte, entrega o retorno e devolve a conta', async () => {
  const { client, corpos } = hub(() => ({ ok: true, conta: { rotulo: 'pessoa@example.invalid', planoHabilitado: true } }))
  const { ouvinte, registro } = ponte(() => Promise.resolve(VOLTA_OK))
  const { fim } = entrarComChatGPT(client, ouvinte, false, 'pessoal-1')
  assert.deepEqual(await fim, { ok: true, rotulo: 'pessoa@example.invalid', planoHabilitado: true })
  assert.deepEqual(corpos, [
    { metodo: 'chatgptEntrar', corpo: { loopbackPort: 19424, spaceId: 'pessoal-1' } },
    { metodo: 'chatgptEntrarConcluir', corpo: { state: 'estado-1', code: 'codigo-ficticio', spaceId: 'pessoal-1' } },
  ])
  assert.ok(registro[0] === 'esperar:7:' + URL_DO_HUB, 'o ouvinte recebe a URL do Hub com a tentativa certa')
  assert.ok(registro.includes('responder:7:true:ChatGPT'), 'o navegador vê o desfecho de sucesso')
  assert.ok(registro.includes('fechar:7'), 'o ouvinte desliga no fim')
})

test('o cliente emitido no retorno do primeiro registro segue ao Hub; repetido ou vazio, o retorno é recusado', async () => {
  const { client, corpos } = hub(() => ({ ok: true, conta: { rotulo: null, planoHabilitado: true } }))
  const { ouvinte } = ponte(() => Promise.resolve(VOLTA_OK + '&client_id=oaiapp_ficticio'))
  await entrarComChatGPT(client, ouvinte, false, 'pessoal-1').fim
  assert.deepEqual(corpos[1], { metodo: 'chatgptEntrarConcluir', corpo: { state: 'estado-1', code: 'codigo-ficticio', spaceId: 'pessoal-1', client_id: 'oaiapp_ficticio' } })
  for (const volta of [VOLTA_OK + '&client_id=a&client_id=b', VOLTA_OK + '&client_id=']) {
    const { client: c, corpos: cs } = hub(() => ({ ok: true }))
    const { ouvinte: o } = ponte(() => Promise.resolve(volta))
    const falha = await entrarComChatGPT(c, o, false, 'pessoal-1').fim.then(() => null, (e: unknown) => e)
    assert.ok(falha instanceof FalhaChatGPT && falha.tipo === 'falha', volta)
    assert.equal(cs.length, 1, 'o Hub não recebe o retorno recusado')
  }
})

test('«entrar com outra conta» pede o registro dinâmico de novo', async () => {
  const { client, corpos } = hub(() => ({ ok: true, conta: { rotulo: null, planoHabilitado: true } }))
  const { ouvinte } = ponte(() => Promise.resolve(VOLTA_OK))
  await entrarComChatGPT(client, ouvinte, false, 'pessoal-1', true).fim
  assert.deepEqual(corpos[0], { metodo: 'chatgptEntrar', corpo: { loopbackPort: 19424, spaceId: 'pessoal-1', novaConta: true } })
})

test('depois do retorno o cancelar não faz mais nada e o login conclui', async () => {
  let cancelar: () => void = () => {}
  let avisou = false
  const { client } = hub(() => { cancelar(); return { ok: true, conta: { rotulo: null, planoHabilitado: true } } })
  const { ouvinte, registro } = ponte(() => Promise.resolve(VOLTA_OK))
  const entrada = entrarComChatGPT(client, ouvinte, false, 'pessoal-1', false, undefined, () => { avisou = true })
  cancelar = entrada.cancelar
  assert.deepEqual(await entrada.fim, { ok: true, rotulo: null, planoHabilitado: true })
  assert.ok(avisou, 'a tela é avisada de que já está concluindo')
  assert.equal(registro.filter((r) => r === 'fechar:7').length, 1, 'só o fim fecha o ouvinte')
})

test('o erro do fornecedor vale antes do código no mesmo retorno', async () => {
  const { client, corpos } = hub(() => ({ ok: false, erro: 'entrada_negada' }))
  const { ouvinte } = ponte(() => Promise.resolve(VOLTA_OK + '&error=access_denied'))
  await entrarComChatGPT(client, ouvinte, false, 'pessoal-1').fim.catch(() => null)
  assert.deepEqual(corpos[1].corpo, { state: 'estado-1', erro: 'access_denied', spaceId: 'pessoal-1' })
})

test('ativar o uso do plano refaz o login pedindo consentimento; o escopo concedido decide o plano', async () => {
  const { client, corpos } = hub(() => ({ ok: true, conta: { rotulo: null, planoHabilitado: false } }))
  const { ouvinte } = ponte(() => Promise.resolve(VOLTA_OK))
  const { fim } = entrarComChatGPT(client, ouvinte, true, 'pessoal-1')
  assert.deepEqual(await fim, { ok: true, rotulo: null, planoHabilitado: false }, 'sem o escopo do plano, a conta fica com plano desabilitado')
  assert.deepEqual(corpos[0], { metodo: 'chatgptEntrar', corpo: { loopbackPort: 19424, spaceId: 'pessoal-1', consentir: true } })
})

test('a descrição que acompanha o erro não mascara o código: «access_denied» segue ao Hub como código', async () => {
  let recebido: unknown
  const { client } = hub((corpo) => { recebido = corpo; return { ok: false, erro: 'entrada_negada' } })
  const { ouvinte } = ponte(() => Promise.resolve('http://127.0.0.1:19424/auth/callback?state=estado-1&error=access_denied&error_description=a+pessoa+desistiu'))
  await entrarComChatGPT(client, ouvinte, false, 'pessoal-1').fim.then(() => null, () => null)
  assert.deepEqual(recebido, { state: 'estado-1', erro: 'access_denied', spaceId: 'pessoal-1' })
})

test('o retorno negado pela pessoa vai ao Hub como {state, erro} e o desfecho recusado segue ao navegador', async () => {
  let recebido: unknown
  const { client } = hub((corpo) => { recebido = corpo; return { ok: false, erro: 'entrada_negada' } })
  const { ouvinte, registro } = ponte(() => Promise.resolve('http://127.0.0.1:19424/auth/callback?state=estado-1&error=access_denied'))
  const { fim } = entrarComChatGPT(client, ouvinte, false, 'pessoal-1')
  const desfecho = await fim.then(() => null, (e: unknown) => e)
  assert.ok(desfecho instanceof FalhaChatGPT && desfecho.tipo === 'cancelado', 'a negação da pessoa vira cancelamento')
  assert.deepEqual(recebido, { state: 'estado-1', erro: 'access_denied', spaceId: 'pessoal-1' })
  assert.ok(registro.includes('responder:7:false:'), 'sem nome no desfecho de erro')
})

test('a recusa do fornecedor numa troca com código não vira cancelamento da pessoa', async () => {
  const { client, corpos } = hub(() => ({ ok: false, erro: 'fornecedor_falhou' }))
  const { ouvinte } = ponte(() => Promise.resolve(VOLTA_OK))
  const { fim } = entrarComChatGPT(client, ouvinte, false, 'pessoal-1')
  assert.deepEqual(await fim, { ok: false, erro: 'fornecedor_falhou' })
  assert.ok(corpos[1].corpo && (corpos[1].corpo as Record<string, unknown>).code !== undefined, 'a recusa veio depois de entregar o código')
})

test('o Hub respondendo «entrada negada» vira login cancelado, sem culpar o fornecedor', async () => {
  const { client } = hub(() => ({ ok: false, erro: 'entrada_negada' }))
  const { ouvinte } = ponte(() => Promise.resolve('http://127.0.0.1:19424/auth/callback?state=estado-1&error=access_denied'))
  const { fim } = entrarComChatGPT(client, ouvinte, false, 'pessoal-1')
  await assert.rejects(fim, (e: unknown) => e instanceof FalhaChatGPT && e.tipo === 'cancelado')
})

test('cancelar desliga o ouvinte e a espera termina como cancelado', async () => {
  const { client } = hub(() => ({ ok: true, conta: { rotulo: null, planoHabilitado: false } }))
  const { ouvinte, registro, esperou } = ponte(() => new Promise<string>(() => {}))
  const { fim, cancelar } = entrarComChatGPT(client, ouvinte, false, 'pessoal-1')
  const desfecho = fim.catch((e: unknown) => e)
  await esperou
  cancelar()
  assert.ok(registro.includes('fechar:7'), 'o cancelamento encerra o ouvinte da tentativa certa')
  const falha = await desfecho
  assert.ok(falha instanceof FalhaChatGPT)
  assert.equal(falha.tipo, 'cancelado')
})

test('sem retorno a tempo a espera termina como tempo e desliga o ouvinte', async () => {
  const { client } = hub(() => ({ ok: true, conta: { rotulo: null, planoHabilitado: false } }))
  const { ouvinte, registro } = ponte(() => new Promise<string>(() => {}))
  const { fim } = entrarComChatGPT(client, ouvinte, false, 'pessoal-1', false, 20)
  const falha = await fim.then(() => null, (e: unknown) => e)
  assert.ok(falha instanceof FalhaChatGPT && falha.tipo === 'tempo')
  assert.ok(registro.includes('fechar:7'))
})

test('sem porta livre para o ouvinte, Hub que não responde ou concluir que falha: a falha é a genérica', async () => {
  const { client, corpos } = hub(() => ({ ok: true, conta: { rotulo: null, planoHabilitado: false } }))
  const semOuvinte: PonteDoOuvinte = {
    abrir: async () => null, esperar: () => Promise.reject(new Error('nunca')),
    responder() {}, fechar() {},
  }
  const { fim } = entrarComChatGPT(client, semOuvinte, false, 'pessoal-1')
  assert.equal(await fim.then(() => 'ok', (e: unknown) => (e as FalhaChatGPT).tipo), 'falha')
  assert.deepEqual(corpos, [], 'sem ouvinte o Hub nem é chamado')

  const hubFora: HostClient = { call: async () => { throw new Error('sem resposta') } } as unknown as HostClient
  const { ouvinte, registro } = ponte(() => new Promise<string>(() => {}))
  const { fim: fim2 } = entrarComChatGPT(hubFora as HostClient, ouvinte, false, 'pessoal-1')
  assert.equal(await fim2.then(() => 'ok', (e: unknown) => (e as FalhaChatGPT).tipo), 'falha')
  assert.ok(registro.includes('fechar:7'), 'o ouvinte não fica aberto quando o Hub falhou')

  const client2 = {
    call: async (metodo: string) => {
      if (metodo === 'chatgptEntrar') return { url: URL_DO_HUB }
      throw new Error('falha ao concluir')
    },
  } as unknown as HostClient
  const { ouvinte: ouvinte2, registro: registro2 } = ponte(() => Promise.resolve(VOLTA_OK))
  const { fim: fim3 } = entrarComChatGPT(client2, ouvinte2, false, 'pessoal-1')
  assert.equal(await fim3.then(() => 'ok', (e: unknown) => (e as FalhaChatGPT).tipo), 'falha')
  assert.ok(registro2.includes('fechar:7'))
})

test('cada falha do login tem frase nos seis idiomas', () => {
  for (const tipo of ['cancelado', 'tempo', 'falha'] as const) {
    const chave = fraseDaFalha(tipo)
    for (const catalogo of Object.values(CATALOGOS)) assert.ok(catalogo[chave], `${tipo} sem texto`)
  }
})

test('ler estado e sair chamam os métodos do Hub com o corpo esperado', async () => {
  const corpos: { metodo: string; corpo: unknown }[] = []
  const estado: EstadoChatGPT = { conectado: true, rotulo: 'pessoa@example.invalid', planoHabilitado: true, precisaEntrar: false }
  const client = {
    call: async (metodo: string, corpo: unknown) => {
      corpos.push({ metodo, corpo })
      if (metodo === 'chatgptEstado') return estado
      if (metodo === 'chatgptSair') return { ok: true, revogado: false }
      throw new Error(`método inesperado: ${metodo}`)
    },
  } as unknown as HostClient
  assert.deepEqual(await lerEstadoChatGPT(client), estado)
  assert.deepEqual(await sairDoChatGPT(client), { ok: true, revogado: false })
  assert.deepEqual(corpos, [
    { metodo: 'chatgptEstado', corpo: {} },
    { metodo: 'chatgptSair', corpo: {} },
  ])
})
