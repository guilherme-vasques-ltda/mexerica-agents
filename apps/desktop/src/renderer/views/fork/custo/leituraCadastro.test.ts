// MX-158 (R2, P3-2): a máquina de estados do cadastro de contas (o núcleo do `useCadastroContas`). O repo não tem
// harness de hook (sem react-test-renderer/jsdom), então o hook é só a ligação e as regras são testadas aqui:
// mudança de Hub/época/client, resposta atrasada ou fora de ordem, e offline → online com o mesmo client.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { LeituraCadastro } from './leituraCadastro.ts'
import { chaveDoCadastro, type ContaCadastro } from './modelo.ts'

const conta = (id: string): ContaCadastro => ({ id, label: id, backend: 'claude', billing: 'subscription' })
const pendente = () => {
  let ok!: (c: ContaCadastro[]) => void, erro!: (e: unknown) => void
  const promessa = new Promise<ContaCadastro[]>((a, b) => { ok = a; erro = b })
  return { promessa, ok, erro }
}
const solta = () => new Promise<void>((r) => setImmediate(r))
const ids = (m: LeituraCadastro, chave: string, client: unknown) => {
  const e = m.estado(chave, client)
  return e.kind === 'pronto' ? e.contas.map((c) => c.id) : e.kind
}

test('lê e mostra só na identidade em que leu; antes da resposta é "lendo"', async () => {
  const m = new LeituraCadastro(), client = {}, chave = chaveDoCadastro('hub-a', 1)
  const p = pendente()
  m.ler(chave, client, () => p.promessa)
  assert.equal(m.estado(chave, client).kind, 'lendo')
  p.ok([conta('a')])
  await solta()
  assert.deepEqual(ids(m, chave, client), ['a'])
})

test('mudou o Hub, a época ou o client: o cadastro velho some no render (lendo), sem esperar efeito', async () => {
  const m = new LeituraCadastro(), client = {}, chave = chaveDoCadastro('hub-a', 1)
  m.ler(chave, client, async () => [conta('a')])
  await solta()
  assert.equal(m.estado(chaveDoCadastro('hub-b', 1), client).kind, 'lendo')
  assert.equal(m.estado(chaveDoCadastro('hub-a', 2), client).kind, 'lendo') // resync
  assert.equal(m.estado(chave, {}).kind, 'lendo') // reconexão: outro client
  assert.deepEqual(ids(m, chave, client), ['a']) // a identidade original continua válida
})

test('resposta atrasada de uma identidade anterior é descartada (o cleanup do efeito chama soltar)', async () => {
  const m = new LeituraCadastro(), client = {}, chaveA = chaveDoCadastro('hub-a', 1), chaveB = chaveDoCadastro('hub-b', 1)
  const velha = pendente(), nova = pendente()
  m.ler(chaveA, client, () => velha.promessa)
  m.soltar() // trocou de Hub: o efeito limpa e relê
  m.ler(chaveB, client, () => nova.promessa)
  velha.ok([conta('do-hub-a')])
  await solta()
  assert.equal(m.estado(chaveB, client).kind, 'lendo')
  assert.equal(m.estado(chaveA, client).kind, 'lendo')
  nova.ok([conta('do-hub-b')])
  await solta()
  assert.deepEqual(ids(m, chaveB, client), ['do-hub-b'])
})

test('fora de ordem: a leitura mais antiga que chega depois não sobrescreve a mais nova (Atualizar)', async () => {
  const m = new LeituraCadastro(), client = {}, chave = chaveDoCadastro('hub', 1)
  const primeira = pendente(), segunda = pendente()
  m.ler(chave, client, () => primeira.promessa)
  m.ler(chave, client, () => segunda.promessa) // Atualizar na mesma identidade
  segunda.ok([conta('nova')])
  await solta()
  primeira.ok([conta('velha')])
  await solta()
  assert.deepEqual(ids(m, chave, client), ['nova'])
})

test('falha vira "falha" (nunca lista vazia); falha atrasada de leitura descartada não aparece', async () => {
  const m = new LeituraCadastro(), client = {}, chave = chaveDoCadastro('hub', 1)
  m.ler(chave, client, () => Promise.reject(new Error('x')))
  await solta()
  assert.equal(m.estado(chave, client).kind, 'falha')
  const velha = pendente()
  m.ler(chave, client, () => velha.promessa)
  m.soltar()
  velha.erro(new Error('tarde'))
  await solta()
  assert.equal(m.estado(chave, client).kind, 'falha') // continua a anterior; a descartada não escreveu
})

test('offline → online com o MESMO client: o cadastro antigo foi esquecido e não ressuscita', async () => {
  const m = new LeituraCadastro(), client = {}, chave = chaveDoCadastro('hub', 1)
  m.ler(chave, client, async () => [conta('antigo')])
  await solta()
  assert.deepEqual(ids(m, chave, client), ['antigo'])
  assert.equal(m.estado(chave, null).kind, 'lendo') // offline: client null
  m.esquecer() // o efeito de "sem client"
  // De volta online, o mesmo objeto de client: só a nova leitura vale.
  assert.equal(m.estado(chave, client).kind, 'lendo')
  const nova = pendente()
  m.ler(chave, client, () => nova.promessa)
  assert.equal(m.estado(chave, client).kind, 'lendo')
  nova.ok([conta('novo')])
  await solta()
  assert.deepEqual(ids(m, chave, client), ['novo'])
})

test('resposta em voo quando cai a conexão e o efeito solta: não é guardada', async () => {
  const m = new LeituraCadastro(), client = {}, chave = chaveDoCadastro('hub', 1)
  const p = pendente()
  m.ler(chave, client, () => p.promessa)
  m.soltar()
  m.esquecer()
  p.ok([conta('tarde')])
  await solta()
  assert.equal(m.estado(chave, client).kind, 'lendo')
})

test('assinar avisa a cada troca do guardado e cancela', async () => {
  const m = new LeituraCadastro(), client = {}, chave = chaveDoCadastro('hub', 1)
  let avisos = 0
  const cancelar = m.assinar(() => { avisos++ })
  m.ler(chave, client, async () => [conta('a')])
  await solta()
  assert.equal(avisos, 1)
  m.esquecer()
  assert.equal(avisos, 2)
  m.esquecer() // nada a esquecer: não avisa
  assert.equal(avisos, 2)
  cancelar()
  m.ler(chave, client, async () => [conta('b')])
  await solta()
  assert.equal(avisos, 2)
})
