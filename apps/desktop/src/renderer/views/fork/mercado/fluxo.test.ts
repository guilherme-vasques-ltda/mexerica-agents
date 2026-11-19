import assert from 'node:assert/strict'
import { test } from 'node:test'
import { FluxoConectar, valorDoCabecalho, type Dependencias } from './fluxo.ts'
import { conta, grupo, servico, tipo } from './dados-de-teste.ts'

type Chamada = { metodo: string; corpo: Record<string, unknown> }

/** Um Hub de mentira: guarda cada chamada e responde pela função dada; nada de rede. */
function montar(responder: (c: Chamada) => unknown, extra: Partial<Dependencias> = {}) {
  const chamadas: Chamada[] = []
  const cliente = { call: async (metodo: string, corpo: Record<string, unknown>) => { const c = { metodo, corpo }; chamadas.push(c); return responder(c) } }
  const eventos: string[] = []
  const deps: Dependencias = {
    cliente: async () => cliente as never,
    entrar: async (id) => { eventos.push(`entrar:${id}`) },
    verificar: async (id) => { eventos.push(`verificar:${id}`); return { status: 'ready' } },
    guardarChave: async (id, campos) => { eventos.push(`chave:${id}:${Object.keys(campos)[0]}`) },
    reler: async () => { eventos.push('reler') },
    ...extra,
  }
  return { chamadas, eventos, deps }
}

const ficha = (contas: { id: string; name: string; identidade: null }[]) =>
  servico('stripe', 'Stripe', { contas, tipos: [tipo([grupo('ler', { padrao: true }), grupo('faturas')])] })

test('1ª conta: não pede nome, manda o padrão seguro e segue do login à conferência até ficar pronta', async () => {
  const s = ficha([])
  const { chamadas, eventos, deps } = montar(() => ({ connector: conta('stripe-1', 'Stripe', 'stripe', { auth: 'signedOut' }), proximoPasso: 'entrar' }))
  const f = new FluxoConectar(s, s.tipos[0], deps)
  assert.equal(f.podeContinuar, true)
  await f.confirmar()
  assert.equal(chamadas.length, 1)
  assert.deepEqual(chamadas[0].corpo, { servico: 'stripe', tipo: 'x.oficial_oauth', liberacoes: ['ler'] })
  assert.deepEqual(eventos, ['reler', 'entrar:stripe-1', 'verificar:stripe-1', 'reler'])
  assert.equal(f.passo, 'pronto')
  assert.equal(f.conta?.id, 'stripe-1')
})

test('2ª conta: o campo do nome novo já vem com "Stripe 2" e o nome sugerido é o que vai ao Hub se a pessoa não mexer', async () => {
  const s = ficha([{ id: 'a', name: 'Stripe – Loja', identidade: null }])
  const { chamadas, deps } = montar(() => ({ connector: conta('stripe-2', 'Stripe 2', 'stripe'), proximoPasso: 'entrar' }))
  const f = new FluxoConectar(s, s.tipos[0], deps)
  assert.equal(f.nomeDaNova, 'Stripe 2')
  assert.equal(f.podeContinuar, true)
  await f.confirmar()
  assert.equal(chamadas[0].corpo.nome, 'Stripe 2')
  const editavel = new FluxoConectar(s, s.tipos[0], deps)
  editavel.nomear('nova', 'Stripe – Consultoria')
  assert.equal(editavel.nomeDaNova, 'Stripe – Consultoria')
})

test('2ª conta: sem os dois nomes não continua; com eles, manda nome e nomePrimeira', async () => {
  const s = ficha([{ id: 'a', name: 'Stripe', identidade: null }])
  const { chamadas, deps } = montar(() => ({ connector: conta('stripe-2', 'Stripe – Consultoria', 'stripe'), proximoPasso: 'entrar' }))
  const f = new FluxoConectar(s, s.tipos[0], deps)
  assert.equal(f.podeContinuar, false)
  f.nomear('nova', 'Stripe – Consultoria')
  assert.equal(f.podeContinuar, false)
  f.nomear('primeira', 'Stripe – Loja')
  assert.equal(f.podeContinuar, true)
  await f.confirmar()
  assert.deepEqual(chamadas[0].corpo, { servico: 'stripe', tipo: 'x.oficial_oauth', liberacoes: ['ler'], nome: 'Stripe – Consultoria', nomePrimeira: 'Stripe – Loja' })
})

test('nome repetido: a recusa do Hub cai no campo do nome novo e nada avança', async () => {
  const s = ficha([{ id: 'a', name: 'Stripe – Loja', identidade: null }])
  const { eventos, deps } = montar(() => { throw new Error('nome_repetido: já existe "Stripe – Loja" neste Space') })
  const f = new FluxoConectar(s, s.tipos[0], deps)
  f.nomear('nova', 'Stripe – Loja')
  await f.confirmar()
  assert.equal(f.passo, 'configurar')
  assert.equal(f.erro?.codigo, 'nome_repetido')
  assert.equal(f.erro?.campo, 'nova')
  assert.deepEqual(eventos, [])
  f.nomear('nova', 'Stripe – Loja 2')
  assert.equal(f.erro, null)
})

test('sem nenhuma liberação marcada não continua; "voltar ao padrão seguro" devolve o padrão', async () => {
  const s = ficha([])
  const { deps } = montar(() => ({}))
  const f = new FluxoConectar(s, s.tipos[0], deps)
  f.alternar('ler')
  assert.equal(f.podeContinuar, false)
  f.voltarAoPadrao()
  assert.deepEqual(f.escolhidos, ['ler'])
  assert.equal(f.podeContinuar, true)
})

test('login que falha volta ao passo de entrar com o motivo, e Tentar de novo refaz; cancelar apaga a conta pela metade', async () => {
  const s = ficha([])
  let tentativas = 0
  const { chamadas, eventos, deps } = montar(
    (c) => (c.metodo === 'marketConectar' ? { connector: conta('stripe-1', 'Stripe', 'stripe', { auth: 'signedOut' }), proximoPasso: 'entrar' } : { removed: true, bots: [], revogadoNoFornecedor: false }),
    { entrar: async () => { tentativas++; if (tentativas === 1) throw new Error('login recusado') } },
  )
  const f = new FluxoConectar(s, s.tipos[0], deps)
  await f.confirmar()
  assert.equal(f.passo, 'entrando')
  assert.equal(f.falhaDeEntrada, 'login recusado')
  await f.entrar()
  assert.equal(f.passo, 'pronto')
  // Cancelar depois de pronto não apaga nada.
  await f.cancelar()
  assert.equal(chamadas.some((c) => c.metodo === 'marketContaDesconectar'), false)
  assert.ok(eventos.includes('verificar:stripe-1'))

  const g = montar(
    (c) => (c.metodo === 'marketConectar' ? { connector: conta('stripe-9', 'Stripe', 'stripe'), proximoPasso: 'entrar' } : { removed: true, bots: [], revogadoNoFornecedor: false }),
    { entrar: async () => { throw new Error('fechou') } },
  )
  const f2 = new FluxoConectar(s, s.tipos[0], g.deps)
  await f2.confirmar()
  await f2.cancelar()
  assert.deepEqual(g.chamadas.map((c) => c.metodo), ['marketConectar', 'marketContaDesconectar'])
  assert.deepEqual(g.chamadas[1].corpo, { id: 'stripe-9' })
})

test('jeito por chave: pede o valor, guarda pelo cofre do Hub com o nome do campo da ficha e confere', async () => {
  const t = tipo([grupo('ler', { padrao: true })], { tipo: 'oficial_chave', auth: { modo: 'chave', destino: 'header', nome: 'Authorization', comoCriarLeitura: 'Crie uma chave só de leitura.' } })
  const s = servico('stripe', 'Stripe', { contas: [], tipos: [t] })
  const { eventos, deps } = montar(() => ({ connector: conta('stripe-1', 'Stripe', 'stripe', { auth: 'none' }), proximoPasso: 'chave' }))
  const f = new FluxoConectar(s, t, deps)
  await f.confirmar()
  assert.equal(f.passo, 'chave')
  await f.enviarChave('   ')
  assert.equal(f.passo, 'chave')
  await f.enviarChave('segredo-de-teste')
  assert.deepEqual(eventos.filter((e) => e.startsWith('chave')), ['chave:stripe-1:Authorization'])
  assert.equal(f.passo, 'pronto')
})

test('chave recusada pela conferência volta ao passo da chave com o motivo', async () => {
  const t = tipo([grupo('ler', { padrao: true })], { tipo: 'oficial_chave', auth: { modo: 'chave', destino: 'header', nome: 'Authorization', comoCriarLeitura: '' } })
  const s = servico('stripe', 'Stripe', { contas: [], tipos: [t] })
  const { deps } = montar(() => ({ connector: conta('stripe-1', 'Stripe', 'stripe', { auth: 'none' }), proximoPasso: 'chave' }), { verificar: async () => { throw new Error('401') } })
  const f = new FluxoConectar(s, t, deps)
  await f.confirmar()
  await f.enviarChave('errada')
  assert.equal(f.passo, 'chave')
  assert.equal(f.falhaDeEntrada, '401')
})

test('cancelar tenta apagar a conta pela metade mais uma vez se o Hub estiver ocupado', async () => {
  const s = ficha([])
  let desconexoes = 0
  const { deps } = montar((c) => {
    if (c.metodo === 'marketConectar') return { connector: conta('stripe-7', 'Stripe', 'stripe', { auth: 'signedOut' }), proximoPasso: 'entrar' }
    desconexoes++
    if (desconexoes === 1) throw new Error('core_StoreBusy')
    return { removed: true, bots: [], revogadoNoFornecedor: false }
  }, { entrar: async () => { throw new Error('fechou') } })
  const f = new FluxoConectar(s, s.tipos[0], deps)
  await f.confirmar()
  await f.cancelar()
  assert.equal(desconexoes, 2)
})

test('o valor do cabeçalho leva o prefixo da ficha, sem repeti-lo e sem mexer em chave sem prefixo', () => {
  assert.equal(valorDoCabecalho('Bearer ', '  ghp_abc  '), 'Bearer ghp_abc')
  assert.equal(valorDoCabecalho('Bearer ', 'Bearer ghp_abc'), 'Bearer ghp_abc')
  assert.equal(valorDoCabecalho('Bearer ', 'bearer ghp_abc'), 'bearer ghp_abc')
  assert.equal(valorDoCabecalho(null, ' sk_abc '), 'sk_abc')
  assert.equal(valorDoCabecalho(undefined, 'sk_abc'), 'sk_abc')
})
