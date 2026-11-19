import assert from 'node:assert/strict'
import { test } from 'node:test'
import { computadorDaPagina, contar, falhaDe, falhaSemModelo, LIMITE_ITENS, lerLista, NuncaFazerStore, rascunhosOrfaos, spaceDaPagina, textoValido, type Link } from './modelo.ts'

const item = (id: string, origem: 'space' | 'bot', ordem = 0) => ({ id, spaceId: 's1', botId: origem === 'bot' ? 'b1' : null, texto: `nunca ${id}`, ordem, criadoEm: 1, atualizadoEm: 1, origem })
const erro = (status: number, message: string) => Object.assign(new Error(message), { status })

/** Um Hub em memória com o contrato do #700: CAS por lista, 20 itens, 200 caracteres. */
function hub() {
  const state = { space: [item('s-a', 'space')], bot: [item('b-a', 'bot'), item('b-b', 'bot', 1)], revision: 3, spaceRevision: 7 }
  const calls: [string, Record<string, unknown>][] = []
  let proxima: unknown = null
  const link: Link = {
    call: async <T,>(method: string, body?: unknown) => {
      const b = body as Record<string, unknown>
      calls.push([method, b])
      if (proxima) { const e = proxima; proxima = null; throw e }
      if (method !== 'nuncaFazer') {
        if (b.expectedRevision !== state.revision) throw erro(409, 'nunca_fazer_revision_conflict')
        if (method === 'nuncaFazerAdicionar') state.bot.push(item(`n${state.bot.length}`, 'bot', state.bot.length))
        if (method === 'nuncaFazerReordenar') state.bot = (b.ids as string[]).map((id, i) => ({ ...state.bot.find((x) => x.id === id)!, ordem: i }))
        if (method === 'nuncaFazerRemover') state.bot = state.bot.filter((x) => x.id !== b.id)
        state.revision++
      }
      return structuredClone(state) as T
    },
  }
  return { state, calls, link, falhar: (e: unknown) => { proxima = e } }
}

test('MX-111 lê só a resposta do contrato', () => {
  const ok = lerLista({ space: [item('a', 'space')], bot: [], revision: 0, spaceRevision: 2 })
  assert.deepEqual(ok, { space: [{ id: 'a', texto: 'nunca a', ordem: 0, origem: 'space' }], bot: [], revision: 0, spaceRevision: 2 })
  for (const bad of [null, { space: [], bot: [], revision: -1, spaceRevision: 0 }, { space: [item('a', 'bot')], bot: [], revision: 0, spaceRevision: 0 }, { space: [], revision: 0, spaceRevision: 0 }]) assert.throws(() => lerLista(bad))
})

test('MX-111 limites iguais aos do Hub: 1 a 200 caracteres Unicode, aparados', () => {
  assert.equal(contar('  nunca  '), 5)
  assert.equal(textoValido('   '), false)
  assert.equal(textoValido('😀'.repeat(200)), true)
  assert.equal(textoValido('a'.repeat(201)), false)
})

test('MX-111 falhas pelo status e código do Hub', () => {
  assert.equal(falhaDe(erro(404, 'unknown method')), 'hubAntigo')
  assert.equal(falhaDe(erro(409, 'nunca_fazer_revision_conflict')), 'conflito')
  assert.equal(falhaDe(erro(403, 'forbidden')), 'soDono')
  assert.equal(falhaDe(erro(0, 'unreachable')), 'semConexao')
  assert.equal(falhaDe(erro(400, 'limit_exceeded: máximo de 20 itens')), 'limite')
  assert.equal(falhaDe(erro(400, 'invalid_field: texto deve ter de 1 a 200 caracteres')), 'invalido')
  assert.equal(falhaDe(erro(400, 'item_not_found')), 'itemSumiu')
  assert.equal(falhaDe(erro(400, 'space_not_found')), 'spaceSumiu')
  assert.equal(falhaDe(erro(400, 'bot_not_found')), 'botSumiu')
  assert.equal(falhaDe(new Error('timeout')), 'envio')
})

test('MX-111 no bot: lê Space e bot, grava só a do bot com a revisão lida', async () => {
  const h = hub()
  const store = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1' })
  await store.carregar()
  assert.deepEqual(h.calls[0], ['nuncaFazer', { spaceId: 's1', botId: 'b1' }])
  assert.deepEqual(store.lista?.space.map((i) => i.id), ['s-a'])
  assert.deepEqual(store.editaveis.map((i) => i.id), ['b-a', 'b-b'])
  assert.equal(await store.adicionar('  nunca postar  '), true)
  assert.deepEqual(h.calls[1], ['nuncaFazerAdicionar', { spaceId: 's1', botId: 'b1', texto: 'nunca postar', expectedRevision: 3 }])
  assert.equal(store.lista?.revision, 4)
  assert.equal(await store.mover('b-b', -1), true)
  assert.deepEqual(h.calls[2]![1], { spaceId: 's1', botId: 'b1', ids: ['b-b', 'b-a', 'n2'], expectedRevision: 4 })
  assert.equal(await store.mover('b-b', -1), false, 'o primeiro não sobe')
  assert.equal(h.calls.length, 3)
})

test('MX-111 em Ajustes: a lista do Space, sem botId', async () => {
  const h = hub()
  const store = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: null })
  await store.carregar()
  assert.deepEqual(h.calls[0], ['nuncaFazer', { spaceId: 's1' }])
  assert.deepEqual(store.editaveis.map((i) => i.id), ['s-a'])
})

test('MX-111 conflito 409: relê a lista, não grava e avisa (o texto fica com a tela)', async () => {
  const h = hub()
  const store = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1' })
  await store.carregar()
  h.state.revision = 9 // outra janela gravou
  h.state.bot.push(item('outro', 'bot', 2))
  assert.equal(await store.adicionar('nunca X'), false)
  assert.equal(store.falha, 'conflito')
  assert.equal(store.lista?.revision, 9, 'relida do Hub')
  assert.deepEqual(store.editaveis.map((i) => i.id), ['b-a', 'b-b', 'outro'])
  assert.equal(await store.adicionar('nunca X'), true, 'tentar de novo usa a revisão nova')
  assert.equal(store.falha, null)
})

test('MX-111 sem conexão: trava sem apagar o que estava lido', async () => {
  const h = hub()
  let online = true
  const store = new NuncaFazerStore(() => online ? h.link : null, { spaceId: 's1', botId: 'b1' })
  await store.carregar()
  online = false
  assert.equal(store.online, false)
  assert.equal(await store.adicionar('nunca X'), false)
  assert.equal(store.falha, 'semConexao')
  await store.carregar()
  assert.equal(store.editaveis.length, 2, 'a lista lida continua à vista')
  assert.equal(h.calls.length, 1)
})

test('MX-111 Hub antigo sem as RPCs: aviso, sem lista e sem quebrar', async () => {
  const h = hub()
  h.falhar(erro(404, 'unknown method'))
  const store = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1' })
  await store.carregar()
  assert.equal(store.falha, 'hubAntigo')
  assert.equal(store.lista, null)
  assert.equal(await store.adicionar('nunca X'), false)
})

test('MX-111 lista cheia e texto longo não chegam ao Hub', async () => {
  const h = hub()
  h.state.bot = Array.from({ length: LIMITE_ITENS }, (_, i) => item(`x${i}`, 'bot', i))
  const store = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1' })
  await store.carregar()
  assert.equal(store.cheia, true)
  assert.equal(await store.adicionar('nunca X'), false)
  assert.equal(await store.editar('x0', 'a'.repeat(201)), false)
  assert.equal(h.calls.length, 1)
})

test('TEC706-01 rascunho de item removido em outro lugar continua à vista (não morre com a linha)', async () => {
  const h = hub()
  const store = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1' })
  await store.carregar()
  const rascunhos = { 'b-a': 'nunca X editado' }
  assert.deepEqual(rascunhosOrfaos(rascunhos, store.editaveis, store.lendo), [], 'com o item na lista, o rascunho fica na linha')
  h.state.bot = h.state.bot.filter((x) => x.id !== 'b-a') // outra janela removeu
  h.state.revision++
  h.falhar(erro(400, 'item_not_found'))
  assert.equal(await store.editar('b-a', 'nunca X editado'), false)
  assert.equal(store.falha, 'itemSumiu')
  assert.deepEqual(store.editaveis.map((i) => i.id), ['b-b'], 'relida do Hub')
  assert.deepEqual(rascunhosOrfaos(rascunhos, store.editaveis, store.lendo), [['b-a', 'nunca X editado']])
  assert.deepEqual(rascunhosOrfaos(rascunhos, store.editaveis, true), [], 'durante a leitura nada é órfão')
  assert.equal(store.travado, false, 'dá para adicionar o rascunho como novo')
  assert.equal(await store.adicionar('nunca X editado'), true)
})

test('TEC706-02 falha com status 0: relê; sem releitura, nada grava com a revisão velha', async () => {
  const h = hub()
  const store = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1' })
  await store.carregar()
  let cai = 2 // a escrita e a releitura caem
  const call = h.link.call
  h.link.call = async (m, b) => { if (cai > 0) { cai--; h.calls.push([m, b as Record<string, unknown>]); throw erro(0, 'unreachable') } return call(m, b) }
  assert.equal(await store.adicionar('nunca X'), false)
  assert.equal(store.desatualizada, true)
  assert.equal(store.falha, 'semConexao')
  assert.equal(store.travado, true, 'a tela trava, como o aviso anuncia')
  const antes = h.calls.length
  assert.equal(await store.adicionar('nunca X'), false)
  assert.equal(await store.remover('b-a'), false)
  assert.equal(await store.mover('b-b', -1), false)
  assert.equal(h.calls.length, antes, 'nenhuma escrita sai com a revisão velha')
  await store.carregar()
  assert.equal(store.desatualizada, false)
  assert.equal(store.travado, false)
  assert.equal(await store.adicionar('nunca X'), true)
})

test('TEC706-02 status 0 com releitura boa: destrava e não anuncia trava', async () => {
  const h = hub()
  const store = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1' })
  await store.carregar()
  h.falhar(erro(0, 'unreachable'))
  assert.equal(await store.adicionar('nunca X'), false)
  assert.equal(store.desatualizada, false)
  assert.equal(store.travado, false)
  assert.equal(store.falha, 'envio', 'não diz "sem conexão / travada" com a lista já relida')
  assert.deepEqual(h.calls.map(([m]) => m), ['nuncaFazer', 'nuncaFazerAdicionar', 'nuncaFazer'])
})

test('UI706-01 gravar a lista do Space relê o editor do bot montado (o Hub não avisa)', async () => {
  const h = hub()
  const call = h.link.call
  // O Hub em memória grava a lista do bot; aqui a escrita do Space muda `space` e a revisão do Space.
  h.link.call = async (m, b) => {
    const corpo = b as Record<string, unknown>
    if (m === 'nuncaFazerAdicionar' && !corpo.botId) { h.calls.push([m, corpo]); h.state.space.push(item('s-novo', 'space', 1)); h.state.spaceRevision++; return structuredClone({ ...h.state, revision: h.state.spaceRevision }) as never }
    if (m === 'nuncaFazer' && !corpo.botId) { h.calls.push([m, corpo]); return structuredClone({ ...h.state, revision: h.state.spaceRevision }) as never }
    return call(m, b)
  }
  const doBot = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1', hub: 'mac' })
  const doSpace = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: null, hub: 'mac' })
  const outroHub = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1', hub: 'outro' })
  const desligar = [doBot.ativar(), doSpace.ativar(), outroHub.ativar()]
  await Promise.all([doBot.carregar(), doSpace.carregar(), outroHub.carregar()])
  assert.equal(await doSpace.adicionar('nunca Y'), true)
  await new Promise((r) => setTimeout(r, 0))
  assert.deepEqual(doBot.lista?.space.map((i) => i.id), ['s-a', 's-novo'], 'o bot mostra o herdado novo sem reabrir')
  assert.deepEqual(outroHub.lista?.space.map((i) => i.id), ['s-a'], 'outro Hub não é tocado')
  for (const d of desligar) d()
  assert.equal(await doSpace.adicionar('nunca Z'), true)
  await new Promise((r) => setTimeout(r, 0))
  assert.deepEqual(doBot.lista?.space.map((i) => i.id), ['s-a', 's-novo'], 'desligado, não relê')
})

test('UI706-01 invalidar durante uma escrita relê depois dela, sem cortar o resultado', async () => {
  const h = hub()
  const store = new NuncaFazerStore(() => h.link, { spaceId: 's1', botId: 'b1' })
  await store.carregar()
  const gravando = store.adicionar('nunca X')
  store.invalidar()
  assert.equal(await gravando, true, 'a escrita não é descartada')
  await new Promise((r) => setTimeout(r, 0))
  assert.deepEqual(h.calls.map(([m]) => m), ['nuncaFazer', 'nuncaFazerAdicionar', 'nuncaFazer'])
})

test('sem modelo (Space ainda não carregou) nunca mostra "Atualize o Hub"', () => {
  assert.equal(falhaSemModelo(true), null, 'online: fica lendo')
  assert.equal(falhaSemModelo(false), 'semConexao')
})

test('Ajustes › Nunca fazer: só o Space ativo, no computador dele; nunca o Pessoal de outro Hub', () => {
  const ativo = { computerId: 'c1', hostId: 'h1', spaceId: 'space-a' }
  assert.equal(spaceDaPagina(ativo, 'c1', 'h1', 'pessoal'), 'space-a')
  assert.equal(spaceDaPagina(null, 'c1', 'h1', 'pessoal'), 'pessoal', 'Pessoal aberto')
  assert.equal(spaceDaPagina(ativo, 'c2', 'h1', 'pessoal-c2'), null, 'outro computador: não cai no Pessoal')
  assert.equal(spaceDaPagina(ativo, 'c1', 'h2', 'pessoal'), null, 'Hub diferente: aguarda')
  assert.equal(spaceDaPagina(ativo, 'c1', undefined, 'pessoal'), null, 'Hub ainda sem identidade: aguarda')
  assert.equal(spaceDaPagina(null, 'c1', 'h1', undefined), null, 'Spaces ainda não chegaram')
})

test('o computador da página é o do Space ativo; o escolhido só vale com o Pessoal aberto', () => {
  assert.equal(computadorDaPagina({ computerId: 'A', hostId: 'hub-A', spaceId: 'equipe-A' }, 'B'), 'A')
  assert.equal(computadorDaPagina(null, 'B'), 'B')
})

test('troca de Space (e de computador): nada de A aparece nem é gravado em B; resposta tardia de A é descartada', async () => {
  const hubs = { A: hub(), B: hub() }
  const ativoA = { computerId: 'A', hostId: 'hub-A', spaceId: 'equipe-A' }
  const ativoA2 = { computerId: 'A', hostId: 'hub-A', spaceId: 'equipe-A2' }
  const ativoB = { computerId: 'B', hostId: 'hub-B', spaceId: 'equipe-B' }
  // O que a página faz: escopo derivado do ativo; um modelo por escopo.
  const abrir = (ativo: typeof ativoA, escolhido: string) => {
    const c = computadorDaPagina(ativo, escolhido) as 'A' | 'B'
    const space = spaceDaPagina(ativo, c, `hub-${c}`, `pessoal-${c}`)!
    return new NuncaFazerStore(() => hubs[c].link, { spaceId: space, botId: null, hub: c })
  }
  const emA = abrir(ativoA, 'B') // computador escolhido B é ignorado: vale o do ativo
  assert.deepEqual(emA.escopo, { spaceId: 'equipe-A', botId: null, hub: 'A' })
  await emA.carregar()
  // resposta de A ainda em voo quando o topo muda para outro Space do mesmo Hub
  let soltar!: () => void
  const lenta: Link = { call: () => new Promise((r) => { soltar = () => r({ space: [item('so-de-A', 'space')], bot: [], revision: 1, spaceRevision: 1 }) }) as never }
  const velho = new NuncaFazerStore(() => lenta, { spaceId: 'equipe-A', botId: null, hub: 'A' })
  const pendente = velho.carregar()
  const emA2 = abrir(ativoA2, 'B')
  assert.equal(emA2.lista, null, 'começa sem a lista de A')
  assert.equal(emA2.escopo.spaceId, 'equipe-A2')
  const emB = abrir(ativoB, 'A') // troca de computador
  assert.deepEqual(emB.escopo, { spaceId: 'equipe-B', botId: null, hub: 'B' })
  soltar(); await pendente
  assert.equal(emA2.lista, null); assert.equal(emB.lista, null, 'resposta tardia de A não entra em B')
  await emB.carregar(); assert.equal(await emB.adicionar('nunca B'), true)
  assert.ok(hubs.B.calls.every(([, b]) => b.spaceId === 'equipe-B'), 'toda chamada de B usa o escopo de B')
  assert.ok(hubs.A.calls.every(([, b]) => b.spaceId === 'equipe-A'), 'A só vê o seu')
  assert.equal(hubs.A.calls.filter(([m]) => m === 'nuncaFazerAdicionar').length, 0, 'nenhuma escrita em A')
})
